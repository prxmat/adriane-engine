//! Graph-runtime integration: run a [`ReActAgent`] as a node handler — the Rust
//! port of `@adriane/graph-sdk`'s `agent-node.ts` pattern.
//!
//! When the agent needs human approval and `suspend_for_approval` is set, the
//! handler returns [`NodeOutput::interrupt`] carrying the pending result into the
//! output channel — the run suspends cleanly. The control plane then patches the
//! [`APPROVED_TOOLS_CHANNEL`] (`GraphRuntime::update_state`) and resumes: the node
//! re-runs, the agent sees the granted tools, and execution proceeds.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use adriane_graph_core::GraphState;
use adriane_graph_runtime::{NodeHandler, NodeOutput};
use serde_json::Value;

use crate::react::ReActAgent;

/// Channel holding the names of tools whose human approval has been granted. The
/// control plane writes it before resuming a run that suspended for approval.
pub const APPROVED_TOOLS_CHANNEL: &str = "__approvedTools";

/// Reason carried by the interrupt an agent node raises when it needs approval.
pub const AGENT_APPROVAL_INTERRUPT: &str = "agent-approval-required";

/// Default channel an agent node writes its [`crate::react::AgentResult`] into.
pub const DEFAULT_AGENT_OUTPUT_CHANNEL: &str = "agentResult";

/// Build a [`NodeHandler`] that runs `agent` over the current state and writes its
/// result to `output_channel`.
///
/// - Approved tools are read from [`APPROVED_TOOLS_CHANNEL`] (a JSON array of
///   names; absence or `null` means none).
/// - With `suspend_for_approval`, a result flagged `requires_human_review`
///   suspends the run via [`NodeOutput::interrupt`], persisting the pending
///   result (including its `approvalRequests`) into the output channel.
/// - A gateway error is written to the output channel as `{ "error": "<msg>" }`
///   instead of failing the node: the runtime has no node-failure status or
///   retries yet, and surfacing the error as channel data keeps the run
///   deterministic and lets the graph route on it (e.g. into an alert path).
pub fn agent_node_handler(
    agent: Arc<ReActAgent>,
    output_channel: String,
    suspend_for_approval: bool,
) -> NodeHandler {
    Box::new(move |state: GraphState| {
        let agent = Arc::clone(&agent);
        let output_channel = output_channel.clone();
        Box::pin(async move {
            let approved = approved_tool_names(&state.channels);
            match agent.run(&Value::Null, &state.channels, &approved).await {
                Ok(result) => {
                    let requires_review = result.requires_human_review;
                    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
                    let mut patch = BTreeMap::new();
                    patch.insert(output_channel, value);
                    if suspend_for_approval && requires_review {
                        NodeOutput::interrupt(AGENT_APPROVAL_INTERRUPT, patch)
                    } else {
                        NodeOutput::update(patch)
                    }
                }
                Err(error) => {
                    let mut patch = BTreeMap::new();
                    patch.insert(
                        output_channel,
                        serde_json::json!({ "error": error.to_string() }),
                    );
                    NodeOutput::update(patch)
                }
            }
        })
    })
}

/// Read the granted tool names from the channels — tolerant of an absent, `null`,
/// or non-string-array channel (all mean "nothing granted").
fn approved_tool_names(channels: &BTreeMap<String, Value>) -> HashSet<String> {
    match channels.get(APPROVED_TOOLS_CHANNEL) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use adriane_graph_core::{
        ChannelDefinition, ChannelReducer, GraphDefinition, GraphId, GraphStatus, NodeDefinition,
        NodeId, NodeType, RunId,
    };
    use adriane_graph_runtime::{
        GraphRuntime, InMemoryConditionRegistry, InMemoryNodeRegistry, NodeRegistry,
    };
    use adriane_llm_gateway::{
        DefaultLlmGateway, LlmProvider, LlmResponse, LlmToolCall, LlmUsage, MockAdapter,
    };
    use serde_json::json;

    use super::*;
    use crate::tools::{sync_tool, InMemoryToolRegistry, ToolDefinition};

    fn replace_channel() -> ChannelDefinition {
        ChannelDefinition {
            channel_type: "json".to_owned(),
            reducer: ChannelReducer::Replace,
            default: None,
        }
    }

    fn agent_graph() -> GraphDefinition {
        GraphDefinition {
            id: GraphId::from("g-agent"),
            version: "0.0.0".to_owned(),
            name: "agent graph".to_owned(),
            recursion_limit: None,
            channels: [
                (DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel()),
                (APPROVED_TOOLS_CHANNEL.to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![NodeDefinition {
                id: NodeId::from("assistant"),
                node_type: NodeType::Agent,
                label: "assistant".to_owned(),
                subgraph_id: None,
                input_mapping: None,
                output_mapping: None,
                fan_out: None,
                retry_policy: None,
                metadata: None,
            }],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        }
    }

    fn tool_use(name: &str) -> LlmResponse {
        LlmResponse {
            content: String::new(),
            tool_calls: Some(vec![LlmToolCall {
                id: "tu1".to_owned(),
                name: name.to_owned(),
                input: json!({}),
            }]),
            stop_reason: Some("tool_use".to_owned()),
            usage: LlmUsage::default(),
            model: "mock".to_owned(),
            provider: LlmProvider::Anthropic,
        }
    }

    fn text(content: &str) -> LlmResponse {
        LlmResponse {
            content: content.to_owned(),
            tool_calls: None,
            stop_reason: Some("end_turn".to_owned()),
            usage: LlmUsage::default(),
            model: "mock".to_owned(),
            provider: LlmProvider::Anthropic,
        }
    }

    /// Parity proof with the TS SDK test "suspends the run for approval, then
    /// executes the tool once granted on resume". The second scripted `tool_use`
    /// matters: after approval the node re-runs, so the agent must ask again to
    /// actually execute the now-granted tool.
    #[tokio::test]
    async fn suspends_for_approval_then_executes_after_grant() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);

        let mut tools = InMemoryToolRegistry::new();
        tools.register(
            ToolDefinition {
                name: "refund".to_owned(),
                description: "Issues a refund.".to_owned(),
                requires_approval: true,
                input_schema: Some(json!({ "type": "object" })),
            },
            sync_tool(move |_input| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!({ "ok": true }))
            }),
        );

        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(MockAdapter::new(
            LlmProvider::Anthropic,
            vec![
                tool_use("refund"),
                tool_use("refund"),
                text("FINAL: refunded"),
            ],
        )));

        let agent = ReActAgent::new("assistant", "refund agent", Arc::new(gateway))
            .with_tools(Arc::new(tools));

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("assistant"),
            agent_node_handler(
                Arc::new(agent),
                DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(),
                true,
            ),
        );

        let runtime = GraphRuntime::new(agent_graph(), nodes, InMemoryConditionRegistry::new());
        let run_id = RunId::from("run-approval");

        let suspended = runtime
            .start(run_id.clone(), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        let pending = suspended
            .channels
            .get(DEFAULT_AGENT_OUTPUT_CHANNEL)
            .expect("pending result persisted");
        assert_eq!(pending.get("requiresHumanReview"), Some(&json!(true)));
        assert_eq!(calls.load(Ordering::SeqCst), 0); // gated before execution

        runtime
            .update_state(
                &run_id,
                [(APPROVED_TOOLS_CHANNEL.to_owned(), json!(["refund"]))]
                    .into_iter()
                    .collect(),
            )
            .unwrap();

        let done = runtime.resume(&run_id).await.unwrap();
        assert_eq!(done.status, GraphStatus::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1); // ran once approval was granted
    }

    #[tokio::test]
    async fn surfaces_a_gateway_error_into_the_output_channel() {
        // No adapter registered: the agent's first complete() fails.
        let gateway = DefaultLlmGateway::new();
        let agent = ReActAgent::new("assistant", "agent", Arc::new(gateway));

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("assistant"),
            agent_node_handler(
                Arc::new(agent),
                DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(),
                false,
            ),
        );

        let runtime = GraphRuntime::new(agent_graph(), nodes, InMemoryConditionRegistry::new());
        let done = runtime
            .start(RunId::from("run-err"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(done.status, GraphStatus::Completed);
        let result = done
            .channels
            .get(DEFAULT_AGENT_OUTPUT_CHANNEL)
            .expect("error surfaced");
        let message = result
            .get("error")
            .and_then(Value::as_str)
            .expect("error message");
        assert!(!message.is_empty());
    }
}
