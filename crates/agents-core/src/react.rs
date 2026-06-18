//! The ReAct loop — the Rust port of `@adriane-ai/agents-core`'s `ReActAgent`.
//!
//! Mirrors the TS behavioural contract:
//! - native `tool_calls` on a response take precedence over the text protocol;
//! - the text protocol understands `FINAL: <answer>` (stop) and
//!   `ACTION: <tool> <optional json>` (call a tool);
//! - a `requires_approval` tool is **never** executed by the agent itself: unless
//!   its name is in `approved_tool_names`, an approval request is recorded and the
//!   loop stops — no self-approval, ever.
//!
//! One adaptation: the Rust `LlmMessage` is text-only (no content blocks yet), so
//! tool results are fed back as `observation:<json>` user messages on both paths,
//! the way the TS text protocol does.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use adriane_llm_gateway::{LlmError, LlmGateway, LlmMessage, LlmProvider, LlmRequest, LlmToolDef};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::InMemoryToolRegistry;

/// Default model, matching the TS `DEFAULT_MODEL`.
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";

/// Default iteration budget, matching the TS default.
pub const DEFAULT_MAX_ITERATIONS: usize = 6;

/// One pending approval. `subject` is `"tool:<name>"`, exactly like the TS shape
/// (`{ description: "tool:<name>" }`) flattened to its description string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestItem {
    pub subject: String,
    pub reason: String,
}

/// What an agent run produces — wire-compatible (camelCase) with the TS
/// `AgentResult` subset: `reasoning`, `approvalRequests`, `requiresHumanReview`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResult {
    pub reasoning: String,
    pub approval_requests: Vec<ApprovalRequestItem>,
    pub requires_human_review: bool,
}

/// Outcome of the shared tool-execution path (native and `ACTION:` calls).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolOutcome {
    /// The tool ran; its observation went into the trace and conversation.
    Executed,
    /// The tool is approval-gated and not granted — the run must stop here.
    Approval,
    /// No such tool; recorded in the trace, the loop moves on.
    NotFound,
}

/// A ReAct agent over the async LLM gateway: think → act (tool) → observe, until
/// `FINAL:` or the iteration budget. Cheap to share behind an `Arc` — see
/// [`crate::node::agent_node_handler`] for the graph-runtime integration.
pub struct ReActAgent {
    pub name: String,
    pub description: String,
    gateway: Arc<dyn LlmGateway>,
    tools: Option<Arc<InMemoryToolRegistry>>,
    provider: LlmProvider,
    model: String,
    system: Option<String>,
    max_iterations: usize,
}

impl ReActAgent {
    /// An agent with the TS defaults: Anthropic, [`DEFAULT_MODEL`], no tools, no
    /// system prompt, [`DEFAULT_MAX_ITERATIONS`] iterations.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        gateway: Arc<dyn LlmGateway>,
    ) -> Self {
        ReActAgent {
            name: name.into(),
            description: description.into(),
            gateway,
            tools: None,
            provider: LlmProvider::Anthropic,
            model: DEFAULT_MODEL.to_owned(),
            system: None,
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }

    pub fn with_tools(mut self, tools: Arc<InMemoryToolRegistry>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn with_provider(mut self, provider: LlmProvider) -> Self {
        self.provider = provider;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Run the ReAct loop against the given input and channel snapshot.
    /// `approved_tool_names` are the `requires_approval` tools a human has already
    /// granted (e.g. injected into state on resume): they execute instead of being
    /// gated again.
    pub async fn run(
        &self,
        input: &Value,
        channels: &BTreeMap<String, Value>,
        approved_tool_names: &HashSet<String>,
    ) -> Result<AgentResult, LlmError> {
        let mut trace: Vec<String> = Vec::new();
        let mut approval_requests: Vec<ApprovalRequestItem> = Vec::new();
        let tool_defs = self.build_tool_defs();

        // `Value`'s Display is compact JSON — same output as `serde_json::to_string`.
        let state_value = Value::Object(
            channels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let mut conversation = vec![LlmMessage {
            role: "user".to_owned(),
            content: format!("Input: {input}\nState: {state_value}"),
        }];

        'iterations: for _ in 0..self.max_iterations {
            let response = self
                .gateway
                .complete(LlmRequest {
                    provider: self.provider,
                    model: self.model.clone(),
                    messages: conversation.clone(),
                    system: self.system.clone(),
                    tools: tool_defs.clone(),
                    max_tokens: None,
                    temperature: None,
                })
                .await?;

            let content = response.content.trim().to_owned();
            trace.push(format!("thought:{content}"));

            // Native tool-calling takes precedence over the text protocol.
            let tool_calls = response.tool_calls.unwrap_or_default();
            if !tool_calls.is_empty() {
                if !content.is_empty() {
                    conversation.push(LlmMessage {
                        role: "assistant".to_owned(),
                        content: content.clone(),
                    });
                }
                for call in &tool_calls {
                    let outcome = self
                        .execute_tool_call(
                            &call.name,
                            call.input.clone(),
                            approved_tool_names,
                            &mut trace,
                            &mut approval_requests,
                            &mut conversation,
                        )
                        .await;
                    if outcome == ToolOutcome::Approval {
                        break 'iterations;
                    }
                }
                continue;
            }

            // `ACTION: <tool> <json>` text-protocol call — execute, feed the
            // observation back, loop. Checked before the final-answer fallthrough.
            if let Some(rest) = content.strip_prefix("ACTION: ") {
                conversation.push(LlmMessage {
                    role: "assistant".to_owned(),
                    content: content.clone(),
                });
                let (tool_name, payload) = parse_action(rest);
                let outcome = self
                    .execute_tool_call(
                        &tool_name,
                        payload,
                        approved_tool_names,
                        &mut trace,
                        &mut approval_requests,
                        &mut conversation,
                    )
                    .await;
                if outcome == ToolOutcome::Approval {
                    break;
                }
                continue;
            }

            // Any tool-call-free, action-free text turn is the final answer.
            // Honour an explicit `FINAL:` marker wherever it appears (models
            // rarely put it first), otherwise take the whole text. We must NOT
            // loop here: re-querying would append an assistant-terminated
            // conversation, which strict OpenAI-compatible providers (e.g.
            // Mistral) reject with a 400.
            let answer = match content.find("FINAL:") {
                Some(index) => content[index + "FINAL:".len()..].trim(),
                None => content.as_str(),
            };
            trace.push(format!("final:{answer}"));
            break;
        }

        Ok(AgentResult {
            reasoning: trace.join("\n"),
            requires_human_review: !approval_requests.is_empty(),
            approval_requests,
        })
    }

    /// Resolve a tool by name and either execute it or gate it. Shared by the
    /// native tool-call path and the `ACTION:` text protocol so both honour the
    /// approval rule identically: a `requires_approval` tool is never self-executed.
    async fn execute_tool_call(
        &self,
        name: &str,
        input: Value,
        approved_tool_names: &HashSet<String>,
        trace: &mut Vec<String>,
        approval_requests: &mut Vec<ApprovalRequestItem>,
        conversation: &mut Vec<LlmMessage>,
    ) -> ToolOutcome {
        let resolved = self.tools.as_ref().and_then(|tools| tools.resolve(name));
        let Some((definition, handler)) = resolved else {
            trace.push(format!("observation:tool_not_found:{name}"));
            return ToolOutcome::NotFound;
        };

        // Gate sensitive tools — unless this exact tool was already approved by a
        // human (e.g. granted on resume), in which case it runs.
        if definition.requires_approval && !approved_tool_names.contains(&definition.name) {
            approval_requests.push(ApprovalRequestItem {
                subject: format!("tool:{}", definition.name),
                reason: format!(
                    "Tool '{}' requires human approval before execution.",
                    definition.name
                ),
            });
            trace.push(format!("observation:approval_required:{name}"));
            return ToolOutcome::Approval;
        }

        // A handler error is data, not a crash: it goes back to the model as an
        // observation so the loop can recover or finalize.
        let output = match handler(input).await {
            Ok(value) => value.to_string(),
            Err(message) => Value::String(format!("tool_error:{message}")).to_string(),
        };
        let observation = format!("observation:{output}");
        trace.push(observation.clone());
        conversation.push(LlmMessage {
            role: "user".to_owned(),
            content: observation,
        });
        ToolOutcome::Executed
    }

    /// Advertise tools to the LLM: only those carrying a JSON Schema, with their
    /// description — the shape the provider needs to emit native tool calls.
    fn build_tool_defs(&self) -> Option<Vec<LlmToolDef>> {
        let tools = self.tools.as_ref()?;
        let defs: Vec<LlmToolDef> = tools
            .list()
            .into_iter()
            .filter_map(|definition| {
                definition.input_schema.clone().map(|schema| LlmToolDef {
                    name: definition.name.clone(),
                    description: Some(definition.description.clone()),
                    input_schema: schema,
                })
            })
            .collect();
        if defs.is_empty() {
            None
        } else {
            Some(defs)
        }
    }
}

/// Parse the remainder of an `ACTION: <tool> <optional json>` line. A missing or
/// unparsable payload defaults to `{}` (the TS protocol's default input).
fn parse_action(rest: &str) -> (String, Value) {
    let rest = rest.trim_start();
    let (name, payload_text) = match rest.split_once(char::is_whitespace) {
        Some((name, payload)) => (name.to_owned(), payload.trim().to_owned()),
        None => (rest.to_owned(), String::new()),
    };
    let payload = if payload_text.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(&payload_text)
            .unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
    };
    (name, payload)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use adriane_llm_gateway::{DefaultLlmGateway, LlmResponse, LlmToolCall, LlmUsage, MockAdapter};
    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::tools::{sync_tool, ToolDefinition};

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

    fn gateway_with(responses: Vec<LlmResponse>) -> Arc<DefaultLlmGateway> {
        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(MockAdapter::new(
            LlmProvider::Anthropic,
            responses,
        )));
        Arc::new(gateway)
    }

    fn counting_tool(
        name: &str,
        requires_approval: bool,
        calls: &Arc<AtomicUsize>,
    ) -> (ToolDefinition, crate::tools::ToolHandler) {
        let counter = Arc::clone(calls);
        (
            ToolDefinition {
                name: name.to_owned(),
                description: format!("The {name} tool."),
                requires_approval,
                input_schema: Some(json!({ "type": "object" })),
            },
            sync_tool(move |_input| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!({ "ok": true }))
            }),
        )
    }

    #[tokio::test]
    async fn final_stops_the_loop() {
        let agent = ReActAgent::new("a", "test agent", gateway_with(vec![text("FINAL: done")]));
        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert!(result.reasoning.contains("final:done"));
        assert!(!result.requires_human_review);
        assert!(result.approval_requests.is_empty());
    }

    #[tokio::test]
    async fn native_tool_use_executes_then_finalizes() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = InMemoryToolRegistry::new();
        let weather = counting_tool("weather", false, &calls);
        registry.register(weather.0, weather.1);

        let agent = ReActAgent::new(
            "a",
            "weather agent",
            gateway_with(vec![tool_use("weather"), text("FINAL: 21C")]),
        )
        .with_tools(Arc::new(registry));

        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(result.reasoning.contains("observation:{\"ok\":true}"));
        assert!(result.reasoning.contains("final:21C"));
        assert!(!result.requires_human_review);
    }

    #[tokio::test]
    async fn requires_approval_gates_the_tool() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = InMemoryToolRegistry::new();
        let deploy = counting_tool("deploy", true, &calls);
        registry.register(deploy.0, deploy.1);

        let agent = ReActAgent::new(
            "a",
            "deploy agent",
            gateway_with(vec![tool_use("deploy"), text("FINAL: deployed")]),
        )
        .with_tools(Arc::new(registry));

        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(result.requires_human_review);
        assert_eq!(result.approval_requests.len(), 1);
        assert_eq!(result.approval_requests[0].subject, "tool:deploy");
        assert!(result
            .reasoning
            .contains("observation:approval_required:deploy"));
    }

    #[tokio::test]
    async fn approved_set_unlocks_the_gated_tool() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = InMemoryToolRegistry::new();
        let deploy = counting_tool("deploy", true, &calls);
        registry.register(deploy.0, deploy.1);

        let agent = ReActAgent::new(
            "a",
            "deploy agent",
            gateway_with(vec![tool_use("deploy"), text("FINAL: deployed")]),
        )
        .with_tools(Arc::new(registry));

        let approved: HashSet<String> = ["deploy".to_owned()].into_iter().collect();
        let result = agent
            .run(&json!({}), &BTreeMap::new(), &approved)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!result.requires_human_review);
        assert!(result.approval_requests.is_empty());
        assert!(result.reasoning.contains("final:deployed"));
    }

    #[tokio::test]
    async fn action_text_protocol_executes_with_parsed_input() {
        let seen: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let capture = Arc::clone(&seen);
        let mut registry = InMemoryToolRegistry::new();
        registry.register(
            ToolDefinition {
                name: "echo".to_owned(),
                description: "Echoes its input.".to_owned(),
                requires_approval: false,
                input_schema: Some(json!({ "type": "object" })),
            },
            sync_tool(move |input| {
                *capture.lock().expect("lock") = Some(input.clone());
                Ok(input)
            }),
        );

        let agent = ReActAgent::new(
            "a",
            "echo agent",
            gateway_with(vec![text("ACTION: echo {\"x\":1}"), text("FINAL: ok")]),
        )
        .with_tools(Arc::new(registry));

        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert_eq!(*seen.lock().expect("lock"), Some(json!({ "x": 1 })));
        assert!(result.reasoning.contains("observation:{\"x\":1}"));
        assert!(result.reasoning.contains("final:ok"));
    }

    #[tokio::test]
    async fn unknown_tool_is_traced_and_the_loop_continues() {
        let agent = ReActAgent::new(
            "a",
            "toolless agent",
            gateway_with(vec![tool_use("missing"), text("FINAL: ok")]),
        );
        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert!(result
            .reasoning
            .contains("observation:tool_not_found:missing"));
        assert!(result.reasoning.contains("final:ok"));
        assert!(!result.requires_human_review);
    }

    /// A gateway that counts `complete()` calls, to prove the loop does not
    /// re-query after an assistant-terminated text turn (the Mistral 400 bug).
    struct CountingGateway {
        responses: Vec<LlmResponse>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmGateway for CountingGateway {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, LlmError> {
            let next = self.calls.fetch_add(1, Ordering::SeqCst);
            let index = next.min(self.responses.len() - 1);
            Ok(self.responses[index].clone())
        }
    }

    #[tokio::test]
    async fn final_marker_anywhere_in_the_text_terminates_with_the_suffix() {
        let agent = ReActAgent::new(
            "a",
            "test agent",
            gateway_with(vec![text("Here is the answer.\nFINAL: 42")]),
        );
        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        // The suffix after `FINAL:` is the answer; the prefix is dropped.
        assert!(result.reasoning.contains("final:42"));
        assert!(!result.reasoning.contains("final:Here is the answer."));
        assert!(!result.requires_human_review);
    }

    #[tokio::test]
    async fn plain_text_without_final_terminates_as_final_with_a_single_llm_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gateway = Arc::new(CountingGateway {
            // Two responses scripted, but the loop must stop after the first:
            // a plain text turn is the final answer, never re-queried.
            responses: vec![
                text("The answer is plainly stated."),
                text("FINAL: should-not-run"),
            ],
            calls: Arc::clone(&calls),
        });
        let agent = ReActAgent::new("a", "test agent", gateway);

        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();

        // Exactly one completion — no assistant-terminated re-query.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // The whole text becomes the final answer when no `FINAL:` is present.
        assert!(result
            .reasoning
            .contains("final:The answer is plainly stated."));
        assert!(!result.requires_human_review);
    }

    #[test]
    fn agent_result_serializes_camel_case() {
        let result = AgentResult {
            reasoning: "thought:x".to_owned(),
            approval_requests: vec![ApprovalRequestItem {
                subject: "tool:deploy".to_owned(),
                reason: "Tool 'deploy' requires human approval before execution.".to_owned(),
            }],
            requires_human_review: true,
        };
        let wire = serde_json::to_string(&result).expect("serializes");
        assert!(wire.contains("\"approvalRequests\""));
        assert!(wire.contains("\"requiresHumanReview\":true"));
        let back: AgentResult = serde_json::from_str(&wire).expect("round-trips");
        assert_eq!(back, result);
    }
}
