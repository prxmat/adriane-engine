//! The wire spec the SDK sends across the napi boundary, plus the outcome it
//! gets back. All camelCase, matching the TS SDK and `@adriane/graph-core`.

use std::collections::BTreeMap;

use adriane_agents_core::ApprovalRequestItem;
use adriane_graph_core::{GraphDefinition, GraphState};
use adriane_llm_gateway::ModelTier;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for one agent node, keyed in [`EngineSpec::agents`] by node id.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    /// `"openai" | "anthropic" | "mistral"` — drives the provider slot used in the
    /// [`adriane_llm_gateway::LlmRequest`]. The actual adapter is chosen from env
    /// (see `bridge::build_gateway`); this only sets the request's `provider`.
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Optional capability tier (`"frontier" | "balanced" | "fast" | "creative"`).
    /// When set and no explicit `model` is given, the concrete model is resolved by
    /// the [`adriane_llm_gateway::ModelPolicy`] against the providers available in the
    /// process environment (see `bridge::resolve_agent_model`). An explicit `model`
    /// always wins over `tier`.
    #[serde(default)]
    pub tier: Option<ModelTier>,
    #[serde(default)]
    pub system: Option<String>,
    /// Names of tools this agent may call. A name in [`EngineSpec::js_tool_names`]
    /// is backed by a JS `execute` fn; otherwise it is a no-op stub tool.
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    /// When true, a tool that needs approval suspends the run (the agent-node
    /// dynamic-interrupt). When false, gated tools simply stop the agent loop.
    #[serde(default)]
    pub suspend_for_approval: bool,
    /// Tools (by name) that require human approval before the agent may run them.
    #[serde(default)]
    pub approval_tool_names: Vec<String>,
    /// The channel the agent writes its `AgentResult` into. Defaults to the
    /// agents-core `DEFAULT_AGENT_OUTPUT_CHANNEL` (`agentResult`).
    #[serde(default)]
    pub output_channel: Option<String>,
}

/// A graph node backed by a native Rust component, keyed in
/// [`EngineSpec::component_nodes`] by node id. The bridge builds the runtime handler
/// from [`adriane_components::ComponentRegistry::build_handler`] using `kind` +
/// `params`, so the component runs natively on Rust (it is **not** routed to the JS
/// `on_node` seam). `kind` is one of `ComponentRegistry::kinds()` (e.g.
/// `"promptBuilder"`, `"router"`, `"retriever"`); `params` is the component's
/// configuration object, validated up front at build time.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentNodeSpec {
    pub kind: String,
    #[serde(default)]
    pub params: Value,
}

/// The full spec for a run/resume/approve call.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSpec {
    pub graph: GraphDefinition,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub initial_data: BTreeMap<String, Value>,
    /// Serialized [`GraphState`] of a suspended run — required by resume/approve,
    /// ignored by start.
    #[serde(default)]
    pub state: Option<GraphState>,
    /// Tool names a human has granted (approve path): written into the
    /// `__approvedTools` channel before resuming.
    #[serde(default)]
    pub approved_tools: Vec<String>,
    /// Per-node agent configuration, keyed by node id.
    #[serde(default)]
    pub agents: BTreeMap<String, AgentSpec>,
    /// Per-node native component configuration, keyed by node id. Such a node runs a
    /// Rust [`adriane_components`] handler (built at assemble time) instead of the JS
    /// seam, even if its id also appears in [`Self::js_node_ids`].
    #[serde(default)]
    pub component_nodes: BTreeMap<String, ComponentNodeSpec>,
    /// Node ids whose handler is a JS closure (action / tool / custom nodes).
    #[serde(default)]
    pub js_node_ids: Vec<String>,
    /// Tool names whose `execute` is a JS closure.
    #[serde(default)]
    pub js_tool_names: Vec<String>,
}

/// What a run/resume/approve call returns to JS: the final state plus, when
/// suspended, the pending approvals and the state to feed back into approve.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    /// The final (or suspended) [`GraphState`].
    pub state: GraphState,
    /// `"running" | "suspended" | "completed" | "failed"` etc. — the state's
    /// status, surfaced at top level for convenience.
    pub status: String,
    /// Pending tool approvals gathered from the agent output channels when the run
    /// suspended for approval. Empty when not suspended on an approval gate.
    pub pending_approvals: Vec<ApprovalRequestItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_graph_json() -> Value {
        json!({
            "id": "g",
            "version": "0.0.0",
            "name": "g",
            "channels": {},
            "nodes": [{ "id": "a", "type": "action", "label": "a" }],
            "edges": [],
            "entryNodeId": "a"
        })
    }

    #[test]
    fn deserializes_a_minimal_start_spec() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "runId": "run-1",
            "jsNodeIds": ["a"]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        assert_eq!(spec.run_id.as_deref(), Some("run-1"));
        assert_eq!(spec.js_node_ids, vec!["a".to_owned()]);
        assert!(spec.agents.is_empty());
        assert!(spec.state.is_none());
        assert!(spec.initial_data.is_empty());
    }

    #[test]
    fn deserializes_an_agent_spec_with_camel_case_keys() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": {
                    "provider": "anthropic",
                    "model": "claude-x",
                    "system": "be helpful",
                    "toolNames": ["refund"],
                    "maxIterations": 3,
                    "suspendForApproval": true,
                    "approvalToolNames": ["refund"]
                }
            },
            "jsToolNames": ["refund"]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let agent = spec.agents.get("assistant").expect("agent present");
        assert_eq!(agent.provider, "anthropic");
        assert_eq!(agent.model.as_deref(), Some("claude-x"));
        assert_eq!(agent.tool_names, vec!["refund".to_owned()]);
        assert_eq!(agent.max_iterations, Some(3));
        assert!(agent.suspend_for_approval);
        assert_eq!(agent.approval_tool_names, vec!["refund".to_owned()]);
        assert_eq!(spec.js_tool_names, vec!["refund".to_owned()]);
        assert!(agent.tier.is_none());
        assert!(spec.component_nodes.is_empty());
    }

    #[test]
    fn deserializes_an_agent_tier_from_camel_case() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": { "provider": "mistral", "tier": "fast" }
            }
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let agent = spec.agents.get("assistant").expect("agent present");
        assert_eq!(agent.tier, Some(ModelTier::Fast));
        assert!(agent.model.is_none());
    }

    #[test]
    fn deserializes_a_component_node_spec() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "componentNodes": {
                "a": {
                    "kind": "promptBuilder",
                    "params": { "template": "Hi {{name}}", "into": "prompt" }
                }
            }
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let component = spec.component_nodes.get("a").expect("component present");
        assert_eq!(component.kind, "promptBuilder");
        assert_eq!(
            component.params.get("into").and_then(Value::as_str),
            Some("prompt")
        );
    }

    #[test]
    fn run_outcome_serializes_camel_case() {
        let state = GraphState {
            run_id: adriane_graph_core::RunId::from("r"),
            graph_id: adriane_graph_core::GraphId::from("g"),
            current_node_id: adriane_graph_core::NodeId::from("a"),
            status: adriane_graph_core::GraphStatus::Suspended,
            channels: BTreeMap::new(),
            version: 1,
            checkpoint_id: Some("r:0".to_owned()),
            created_at: "0".to_owned(),
            updated_at: "0".to_owned(),
        };
        let outcome = RunOutcome {
            state,
            status: "suspended".to_owned(),
            pending_approvals: vec![ApprovalRequestItem {
                subject: "tool:refund".to_owned(),
                reason: "needs approval".to_owned(),
            }],
        };
        let wire = serde_json::to_string(&outcome).expect("serializes");
        assert!(wire.contains("\"pendingApprovals\""));
        assert!(wire.contains("\"tool:refund\""));
        assert!(wire.contains("\"currentNodeId\":\"a\""));
    }
}
