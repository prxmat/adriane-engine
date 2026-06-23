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

use crate::middleware::{Flow, MiddlewareStack, RunCtx, ToolCallCtx, ToolControl};
use crate::todos::{TodoItem, WRITE_TODOS_TOOL};
use crate::tools::InMemoryToolRegistry;

/// Default model, matching the TS `DEFAULT_MODEL`.
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";

/// Default iteration budget, matching the TS default.
pub const DEFAULT_MAX_ITERATIONS: usize = 6;

/// One pending approval. `subject` is `"tool:<name>"`, exactly like the TS shape
/// (`{ description: "tool:<name>" }`) flattened to its description string.
///
/// For a **content-scoped** tool (ADR 0024 phase 2c — the guarded fs writes), the
/// approval is pinned to the exact call: `approval_key` is the composite
/// `"<name>#<sha256(input)>"` that must be granted to unlock THIS write (a different
/// path/content hashes differently and re-gates — no over-grant), and `input` carries
/// the gated tool input so a reviewer sees the path + content. Both are `None` for an
/// ordinary name-only gate (grant = the tool name). `Eq` is dropped because `input`
/// holds a `serde_json::Value` (same reason as `LlmMessage`); `PartialEq` is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestItem {
    pub subject: String,
    pub reason: String,
    /// Content-scoped pin (`"<name>#<hash>"`) that must be granted to unlock this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_key: Option<String>,
    /// The gated tool input, surfaced so the control plane can show what is approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

/// What an agent run produces — wire-compatible (camelCase) with the TS
/// `AgentResult` subset: `reasoning`, `approvalRequests`, `requiresHumanReview`.
/// `Eq` is dropped (an `ApprovalRequestItem.input` may hold a non-`Eq`
/// `serde_json::Value`); `PartialEq` is retained for tests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResult {
    pub reasoning: String,
    pub approval_requests: Vec<ApprovalRequestItem>,
    pub requires_human_review: bool,
    /// The authoritative todo list from the most recent `writeTodos` call this run,
    /// if any (ADR 0022/0023, phase 1). Additive + optional: omitted from the wire
    /// when absent (`skip_serializing_if`), so existing `AgentResult` consumers stay
    /// compatible. The node handler persists it into the durable todos channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todos: Option<Vec<TodoItem>>,
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
    /// ADR 0014: cap (in chars) on the serialized `State` injected into the first message.
    /// `None` = inject the full state.
    context_budget: Option<usize>,
    /// ADR 0025: the agent middleware stack the loop drives. Default empty = a strict
    /// no-op (today's behaviour); seams fold onto it over phases 3b–3e.
    middleware: MiddlewareStack,
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
            context_budget: None,
            middleware: MiddlewareStack::new(),
        }
    }

    /// Install the agent middleware stack (ADR 0025). Default is empty (a no-op).
    pub fn with_middleware(mut self, middleware: MiddlewareStack) -> Self {
        self.middleware = middleware;
        self
    }

    /// Cap the serialized `State` injected into the agent's first message (ADR 0014 trim).
    pub fn with_context_budget(mut self, budget: usize) -> Self {
        self.context_budget = Some(budget);
        self
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
        // The latest `writeTodos` result this run (ADR 0022/0023). The node handler
        // sinks it into the durable todos channel.
        let mut last_todos: Option<Vec<TodoItem>> = None;
        let tool_defs = self.build_tool_defs();

        // `Value`'s Display is compact JSON — same output as `serde_json::to_string`.
        let state_value = Value::Object(
            channels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        // ADR 0014 trim: cap the injected state to `context_budget` chars when set.
        let state_str = {
            let full = state_value.to_string();
            match self.context_budget {
                Some(n) if full.chars().count() > n => {
                    full.chars().take(n).collect::<String>() + "…"
                }
                _ => full,
            }
        };
        let mut conversation = vec![LlmMessage::text(
            "user",
            format!("Input: {input}\nState: {state_str}"),
        )];

        // ADR 0025: `before_run` — fires once before the loop (empty stack = no-op). A
        // middleware may trim the seed state or stop the run.
        if let Flow::Stop { reason } = self
            .middleware
            .before_run(
                &mut conversation,
                &RunCtx {
                    iteration: 0,
                    approved_tool_names,
                    channels,
                },
            )
            .await?
        {
            trace.push(format!("stopped:{reason}"));
            return Ok(AgentResult {
                reasoning: trace.join("\n"),
                requires_human_review: !approval_requests.is_empty(),
                approval_requests,
                todos: last_todos,
            });
        }

        'iterations: for iteration in 0..self.max_iterations {
            let ctx = RunCtx {
                iteration,
                approved_tool_names,
                channels,
            };
            // ADR 0025: `before_model` (fail-closed — an Err short-circuits the run).
            let request = self
                .middleware
                .before_model(
                    LlmRequest {
                        provider: self.provider,
                        model: self.model.clone(),
                        messages: conversation.clone(),
                        system: self.system.clone(),
                        tools: tool_defs.clone(),
                        max_tokens: None,
                        temperature: None,
                    },
                    &ctx,
                )
                .await?;
            let response = self.gateway.complete(request.clone()).await?;
            // ADR 0025: `after_model`.
            let response = self
                .middleware
                .after_model(response, &request, &ctx)
                .await?;

            let content = response.content.trim().to_owned();
            trace.push(format!("thought:{content}"));
            // ADR 0025: `on_iteration` (loop-detection / budget / reflection trigger).
            if let Flow::Stop { reason } = self.middleware.on_iteration(iteration, &content, &ctx) {
                trace.push(format!("stopped:{reason}"));
                break;
            }

            // Native tool-calling takes precedence over the text protocol.
            let tool_calls = response.tool_calls.unwrap_or_default();
            if !tool_calls.is_empty() {
                // Echo the assistant's tool-call turn into history WITH the structured
                // `tool_calls`, so the provider sees a coherent function-calling transcript
                // (assistant tool_call → tool result) and does not redundantly re-call.
                conversation.push(LlmMessage {
                    role: "assistant".to_owned(),
                    content: content.clone(),
                    tool_calls: Some(tool_calls.clone()),
                    tool_call_id: None,
                    tool_name: None,
                });
                for call in &tool_calls {
                    let outcome = self
                        .execute_tool_call(
                            &call.name,
                            call.input.clone(),
                            Some(&call.id),
                            &ctx,
                            &mut trace,
                            &mut approval_requests,
                            &mut conversation,
                            &mut last_todos,
                        )
                        .await?;
                    if outcome == ToolOutcome::Approval {
                        break 'iterations;
                    }
                }
                continue;
            }

            // `ACTION: <tool> <json>` text-protocol call — execute, feed the
            // observation back, loop. Checked before the final-answer fallthrough.
            if let Some(rest) = content.strip_prefix("ACTION: ") {
                conversation.push(LlmMessage::text("assistant", content.clone()));
                let (tool_name, payload) = parse_action(rest);
                let outcome = self
                    .execute_tool_call(
                        &tool_name,
                        payload,
                        None,
                        &ctx,
                        &mut trace,
                        &mut approval_requests,
                        &mut conversation,
                        &mut last_todos,
                    )
                    .await?;
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

        let mut result = AgentResult {
            reasoning: trace.join("\n"),
            requires_human_review: !approval_requests.is_empty(),
            approval_requests,
            todos: last_todos,
        };
        // ADR 0025: `after_run` — finalize / reflection / metadata (empty stack = no-op).
        self.middleware
            .after_run(
                &mut result,
                &RunCtx {
                    iteration: self.max_iterations,
                    approved_tool_names,
                    channels,
                },
            )
            .await?;
        Ok(result)
    }

    /// Resolve a tool by name and either execute it or gate it. Shared by the
    /// native tool-call path and the `ACTION:` text protocol so both honour the
    /// approval rule identically: a `requires_approval` tool is never self-executed.
    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_call(
        &self,
        name: &str,
        input: Value,
        // `Some(id)` on the native function-calling path → the result is replayed as a
        // `role:"tool"` message linked to that call id. `None` on the `ACTION:` text
        // protocol → the result is a plain `user` observation (no structured id exists).
        tool_call_id: Option<&str>,
        // The loop-turn context (iteration / approved grants / channels) the middleware
        // hooks read. The approval gate reads `ctx.approved_tool_names`.
        ctx: &RunCtx<'_>,
        trace: &mut Vec<String>,
        approval_requests: &mut Vec<ApprovalRequestItem>,
        conversation: &mut Vec<LlmMessage>,
        // When the tool is `writeTodos`, its normalized list is captured here so the
        // node handler can persist it into the durable todos channel (ADR 0022/0023).
        last_todos: &mut Option<Vec<TodoItem>>,
    ) -> Result<ToolOutcome, LlmError> {
        let resolved = self.tools.as_ref().and_then(|tools| tools.resolve(name));
        let Some((definition, handler)) = resolved else {
            trace.push(format!("observation:tool_not_found:{name}"));
            return Ok(ToolOutcome::NotFound);
        };

        // ADR 0025 phase 3c: the approval gate (now intrinsic to the stack) + any installed
        // before_tool middleware (fs policy, …) decide here. The gate is enforced even with
        // an empty stack — see `MiddlewareStack::before_tool`.
        let decision = {
            let call = ToolCallCtx {
                name: &definition.name,
                input: &input,
                requires_approval: definition.requires_approval,
                content_scoped: definition.content_scoped,
            };
            self.middleware.before_tool(&call, ctx).await?
        };
        let input = match decision {
            // Approval-gated and not granted: record the request and stop here — the agent
            // never self-approves.
            ToolControl::Gate(item) => {
                approval_requests.push(item);
                trace.push(format!("observation:approval_required:{name}"));
                return Ok(ToolOutcome::Approval);
            }
            // A middleware refused the call (e.g. fs policy): the handler never runs; feed
            // the denial back as an observation so the model can adapt or finalize.
            ToolControl::Deny { reason } => {
                let observation = format!("tool_error:denied:{reason}");
                trace.push(format!("observation:{observation}"));
                Self::push_observation(conversation, tool_call_id, name, observation);
                return Ok(ToolOutcome::Executed);
            }
            // Allowed, possibly with an overridden input.
            ToolControl::Allow { input_override } => input_override.unwrap_or(input),
        };

        // A handler error is data, not a crash: it goes back to the model as an
        // observation so the loop can recover or finalize.
        let input_for_after = input.clone();
        let raw = match handler(input).await {
            Ok(value) => {
                // `writeTodos` returns the authoritative normalized list — capture it
                // (last write wins) for the node handler to persist durably.
                if name == WRITE_TODOS_TOOL {
                    if let Ok(todos) = serde_json::from_value::<Vec<TodoItem>>(value.clone()) {
                        *last_todos = Some(todos);
                    }
                }
                value
            }
            Err(message) => Value::String(format!("tool_error:{message}")),
        };
        // ADR 0025 phase 3c: `after_tool` may transform the observation (empty stack = no-op).
        let output = self
            .middleware
            .after_tool(name, &input_for_after, raw, ctx)
            .await?
            .to_string();
        trace.push(format!("observation:{output}"));
        Self::push_observation(conversation, tool_call_id, name, output);
        Ok(ToolOutcome::Executed)
    }

    /// Record a tool observation into the conversation: a structured `role:"tool"` message
    /// linked to the call id on the native path (the provider recognises the tool was
    /// answered and finalises instead of re-calling), or a plain `user` observation on the
    /// `ACTION:` text protocol (no structured id exists).
    fn push_observation(
        conversation: &mut Vec<LlmMessage>,
        tool_call_id: Option<&str>,
        name: &str,
        output: String,
    ) {
        match tool_call_id {
            Some(id) => conversation.push(LlmMessage {
                role: "tool".to_owned(),
                content: output,
                tool_calls: None,
                tool_call_id: Some(id.to_owned()),
                tool_name: Some(name.to_owned()),
            }),
            None => conversation.push(LlmMessage::text("user", format!("observation:{output}"))),
        }
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
                content_scoped: false,
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
                content_scoped: false,
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
                approval_key: None,
                input: None,
            }],
            requires_human_review: true,
            todos: None,
        };
        let wire = serde_json::to_string(&result).expect("serializes");
        assert!(wire.contains("\"approvalRequests\""));
        assert!(wire.contains("\"requiresHumanReview\":true"));
        // `todos` is omitted from the wire when absent — keeps old payloads valid.
        assert!(!wire.contains("\"todos\""));
        let back: AgentResult = serde_json::from_str(&wire).expect("round-trips");
        assert_eq!(back, result);
    }

    #[tokio::test]
    async fn write_todos_tool_call_populates_result_todos() {
        use crate::todos::{write_todos_tool, TodoStatus};

        let mut registry = InMemoryToolRegistry::new();
        let (definition, handler) = write_todos_tool();
        registry.register(definition, handler);

        // Native tool call carrying a todo payload, then a final answer.
        let tool_call = LlmResponse {
            content: String::new(),
            tool_calls: Some(vec![LlmToolCall {
                id: "tu1".to_owned(),
                name: "writeTodos".to_owned(),
                input: json!({
                    "todos": [
                        { "text": "plan", "status": "in_progress" },
                        { "text": "build", "status": "pending" }
                    ]
                }),
            }]),
            stop_reason: Some("tool_use".to_owned()),
            usage: LlmUsage::default(),
            model: "mock".to_owned(),
            provider: LlmProvider::Anthropic,
        };

        let agent = ReActAgent::new(
            "a",
            "planner",
            gateway_with(vec![tool_call, text("FINAL: done")]),
        )
        .with_tools(Arc::new(registry));

        let result = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();

        let todos = result.todos.expect("todos captured");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].id, "todo-1");
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        assert_eq!(todos[1].id, "todo-2");
        assert!(result.reasoning.contains("final:done"));
        assert!(!result.requires_human_review);
    }

    /// ADR 0024 phase 2c — the over-grant guard: a content-scoped tool's approval is
    /// pinned to the exact input. Approving one call must NOT unlock a different input.
    #[tokio::test]
    async fn content_scoped_gate_pins_approval_to_the_exact_input() {
        use crate::tools::approval_key;

        fn guarded_call(input: Value) -> LlmResponse {
            LlmResponse {
                content: String::new(),
                tool_calls: Some(vec![LlmToolCall {
                    id: "t1".to_owned(),
                    name: "guarded".to_owned(),
                    input,
                }]),
                stop_reason: Some("tool_use".to_owned()),
                usage: LlmUsage::default(),
                model: "mock".to_owned(),
                provider: LlmProvider::Anthropic,
            }
        }
        fn guarded_agent(calls: &Arc<AtomicUsize>, responses: Vec<LlmResponse>) -> ReActAgent {
            let counter = Arc::clone(calls);
            let mut registry = InMemoryToolRegistry::new();
            registry.register(
                ToolDefinition {
                    name: "guarded".to_owned(),
                    description: "content-scoped guarded tool".to_owned(),
                    requires_approval: true,
                    input_schema: Some(json!({ "type": "object" })),
                    content_scoped: true,
                },
                sync_tool(move |_input| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({ "ok": true }))
                }),
            );
            ReActAgent::new("a", "g", gateway_with(responses)).with_tools(Arc::new(registry))
        }

        let input_a = json!({ "path": "review/a.md", "content": "X" });
        let input_b = json!({ "path": "review/b.md", "content": "Y" });
        let key_a = approval_key("guarded", true, &input_a);
        let key_b = approval_key("guarded", true, &input_b);
        assert!(key_a.starts_with("guarded#") && key_a != key_b);

        // 1) Unapproved → gates, recording the content-scoped key + input for THIS call.
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = guarded_agent(
            &calls,
            vec![guarded_call(input_a.clone()), text("FINAL: x")],
        );
        let r = agent
            .run(&json!({}), &BTreeMap::new(), &HashSet::new())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(r.requires_human_review);
        assert_eq!(
            r.approval_requests[0].approval_key.as_deref(),
            Some(key_a.as_str())
        );
        assert_eq!(r.approval_requests[0].input.as_ref(), Some(&input_a));

        // 2) Granted A's key → the SAME call executes.
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = guarded_agent(
            &calls,
            vec![guarded_call(input_a.clone()), text("FINAL: x")],
        );
        let approved: HashSet<String> = [key_a.clone()].into_iter().collect();
        let r = agent
            .run(&json!({}), &BTreeMap::new(), &approved)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!r.requires_human_review);

        // 3) Granted A, but the agent calls a DIFFERENT input B → re-gates (no over-grant).
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = guarded_agent(
            &calls,
            vec![guarded_call(input_b.clone()), text("FINAL: x")],
        );
        let approved: HashSet<String> = [key_a].into_iter().collect();
        let r = agent
            .run(&json!({}), &BTreeMap::new(), &approved)
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a different input must NOT reuse the grant"
        );
        assert!(r.requires_human_review);
        assert_eq!(
            r.approval_requests[0].approval_key.as_deref(),
            Some(key_b.as_str())
        );
    }
}
