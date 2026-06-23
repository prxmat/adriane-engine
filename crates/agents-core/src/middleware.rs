//! The unified agent middleware API (ADR 0025, deep-agent platform phase 3) — the
//! keystone composition surface for an agent's governance + efficiency behaviours.
//!
//! One [`AgentMiddleware`] trait with seven optional async hooks (all pass-through by
//! default), composed into one ordered [`MiddlewareStack`] the ReAct loop drives. The
//! existing scattered seams (PII redaction, prompt compression, terse output, context
//! trim, the approval gate, fs policy, the todos sink, reflection) fold onto these hooks
//! over phases 3b–3e; **phase 3a ships the trait + an empty default stack wired into the
//! loop with ZERO behaviour change** (an empty stack is a strict no-op).
//!
//! Governed-by-construction (the ADR 0013 bet): the stack separates a **governed** layer
//! (redaction, approval gate, fs policy — builder-injected, sealed) from a **user-tunable
//! efficiency** layer, so an ungoverned stack is unrepresentable.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use adriane_llm_gateway::{
    LlmError, LlmMessage, LlmRequest, LlmResponse, PiiRedactor, PromptCompressor,
};
use serde_json::Value;

use crate::react::{AgentResult, ApprovalRequestItem};

/// Control-flow signal a hook returns: continue the run, or stop it with a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop { reason: String },
}

/// A `before_tool` decision (used from phase 3c, when the approval gate + fs policy fold
/// onto `before_tool`): allow the call (optionally overriding its input), deny it with a
/// reason (surfaced as an observation), or gate it for human approval.
#[derive(Clone, Debug)]
pub enum ToolControl {
    Allow { input_override: Option<Value> },
    Deny { reason: String },
    Gate(ApprovalRequestItem),
}

impl ToolControl {
    /// The pass-through default: allow, no override.
    pub fn allow() -> Self {
        ToolControl::Allow {
            input_override: None,
        }
    }
}

/// The tool call a `before_tool` hook inspects.
pub struct ToolCallCtx<'a> {
    pub name: &'a str,
    pub input: &'a Value,
    /// Whether the resolved tool is `requires_approval` (set by the loop).
    pub requires_approval: bool,
    /// Whether the resolved tool is `content_scoped` (ADR 0024 — guarded fs writes).
    pub content_scoped: bool,
}

/// A cheap, read-only snapshot of the loop state a hook may read. Built per hook-call,
/// never held across a loop mutation (avoids borrowing `conversation` while it mutates).
pub struct RunCtx<'a> {
    pub iteration: usize,
    pub approved_tool_names: &'a HashSet<String>,
    pub channels: &'a BTreeMap<String, Value>,
}

/// One composable agent middleware. Every hook has a pass-through default, so an impl
/// overrides only the lifecycle points it cares about. `async_trait` + `Arc<dyn …>`
/// (object-safe) matches the existing `PiiRedactor` / `PromptCompressor` seams.
#[async_trait::async_trait]
pub trait AgentMiddleware: Send + Sync {
    fn name(&self) -> &str {
        "middleware"
    }

    /// Once, after state injection, before the loop. May mutate the seed `conversation`
    /// (e.g. context-budget trim) or stop the run.
    async fn before_run(
        &self,
        _conversation: &mut Vec<LlmMessage>,
        _ctx: &RunCtx<'_>,
    ) -> Result<Flow, LlmError> {
        Ok(Flow::Continue)
    }

    /// In-loop, just before `gateway.complete()`. **Fallible** — an `Err` short-circuits
    /// the run (the fail-closed path, e.g. redaction-block).
    async fn before_model(
        &self,
        request: LlmRequest,
        _ctx: &RunCtx<'_>,
    ) -> Result<LlmRequest, LlmError> {
        Ok(request)
    }

    /// After `complete()` returns, before the response is parsed.
    async fn after_model(
        &self,
        response: LlmResponse,
        _request: &LlmRequest,
        _ctx: &RunCtx<'_>,
    ) -> Result<LlmResponse, LlmError> {
        Ok(response)
    }

    /// Before a tool executes (fired from phase 3c). Returns an allow/deny/gate decision.
    async fn before_tool(
        &self,
        _call: &ToolCallCtx<'_>,
        _ctx: &RunCtx<'_>,
    ) -> Result<ToolControl, LlmError> {
        Ok(ToolControl::allow())
    }

    /// After a tool's handler returns, before its observation is recorded (fired from 3c).
    async fn after_tool(
        &self,
        _name: &str,
        _input: &Value,
        output: Value,
        _ctx: &RunCtx<'_>,
    ) -> Result<Value, LlmError> {
        Ok(output)
    }

    /// At each loop-turn end (loop-detection / budget / reflection trigger).
    fn on_iteration(&self, _index: usize, _content: &str, _ctx: &RunCtx<'_>) -> Flow {
        Flow::Continue
    }

    /// After the loop, before `AgentResult` is returned (finalize, reflection, metadata).
    async fn after_run(
        &self,
        _result: &mut AgentResult,
        _ctx: &RunCtx<'_>,
    ) -> Result<(), LlmError> {
        Ok(())
    }
}

/// The ordered stack the loop drives. Two layers: **governed** (sealed, builder-injected
/// — redaction, approval gate, fs policy) and **efficiency** (user-tunable — compression,
/// terse, context-budget). Request-path hooks fold governed→efficiency; response-path
/// hooks fold in reverse (onion semantics). The default is **empty** = a strict no-op
/// (today's behaviour); only the builder may populate `governed`.
#[derive(Default, Clone)]
pub struct MiddlewareStack {
    governed: Vec<Arc<dyn AgentMiddleware>>,
    efficiency: Vec<Arc<dyn AgentMiddleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no middleware is installed — the hooks are then strict no-ops.
    pub fn is_empty(&self) -> bool {
        self.governed.is_empty() && self.efficiency.is_empty()
    }

    /// Append a GOVERNED middleware (builder-only; sealed/un-removable by users).
    pub fn push_governed(&mut self, middleware: Arc<dyn AgentMiddleware>) -> &mut Self {
        self.governed.push(middleware);
        self
    }

    /// Append a user-tunable EFFICIENCY middleware.
    pub fn push_efficiency(&mut self, middleware: Arc<dyn AgentMiddleware>) -> &mut Self {
        self.efficiency.push(middleware);
        self
    }

    /// Request-path order: governed first (outermost), then efficiency (inner).
    fn request_order(&self) -> impl Iterator<Item = &Arc<dyn AgentMiddleware>> {
        self.governed.iter().chain(self.efficiency.iter())
    }

    pub async fn before_run(
        &self,
        conversation: &mut Vec<LlmMessage>,
        ctx: &RunCtx<'_>,
    ) -> Result<Flow, LlmError> {
        for middleware in self.request_order() {
            if let Flow::Stop { reason } = middleware.before_run(conversation, ctx).await? {
                return Ok(Flow::Stop { reason });
            }
        }
        Ok(Flow::Continue)
    }

    pub async fn before_model(
        &self,
        mut request: LlmRequest,
        ctx: &RunCtx<'_>,
    ) -> Result<LlmRequest, LlmError> {
        for middleware in self.request_order() {
            request = middleware.before_model(request, ctx).await?;
        }
        Ok(request)
    }

    pub async fn after_model(
        &self,
        mut response: LlmResponse,
        request: &LlmRequest,
        ctx: &RunCtx<'_>,
    ) -> Result<LlmResponse, LlmError> {
        // Response path is the reverse of the request path (onion).
        let reversed: Vec<&Arc<dyn AgentMiddleware>> = self.request_order().collect();
        for middleware in reversed.into_iter().rev() {
            response = middleware.after_model(response, request, ctx).await?;
        }
        Ok(response)
    }

    pub async fn before_tool(
        &self,
        call: &ToolCallCtx<'_>,
        ctx: &RunCtx<'_>,
    ) -> Result<ToolControl, LlmError> {
        // The first non-Allow decision wins (a deny/gate short-circuits execution).
        for middleware in self.request_order() {
            match middleware.before_tool(call, ctx).await? {
                ToolControl::Allow {
                    input_override: None,
                } => {}
                decision => return Ok(decision),
            }
        }
        Ok(ToolControl::allow())
    }

    pub async fn after_tool(
        &self,
        name: &str,
        input: &Value,
        mut output: Value,
        ctx: &RunCtx<'_>,
    ) -> Result<Value, LlmError> {
        let reversed: Vec<&Arc<dyn AgentMiddleware>> = self.request_order().collect();
        for middleware in reversed.into_iter().rev() {
            output = middleware.after_tool(name, input, output, ctx).await?;
        }
        Ok(output)
    }

    pub fn on_iteration(&self, index: usize, content: &str, ctx: &RunCtx<'_>) -> Flow {
        for middleware in self.request_order() {
            if let Flow::Stop { reason } = middleware.on_iteration(index, content, ctx) {
                return Flow::Stop { reason };
            }
        }
        Flow::Continue
    }

    pub async fn after_run(
        &self,
        result: &mut AgentResult,
        ctx: &RunCtx<'_>,
    ) -> Result<(), LlmError> {
        let reversed: Vec<&Arc<dyn AgentMiddleware>> = self.request_order().collect();
        for middleware in reversed.into_iter().rev() {
            middleware.after_run(result, ctx).await?;
        }
        Ok(())
    }
}

// ── Built-in middleware (folded from the gateway seams, ADR 0025 phase 3b) ──────────────

/// GOVERNED — PII redaction (ADR 0008) as before/after-model hooks. `before_model` scrubs
/// the request (**fail-closed**: an `Err`, e.g. `PiiBlocked`, short-circuits the run);
/// `after_model` hydrates the response. Reuses the existing [`PiiRedactor`] (and its HTTP
/// impl) verbatim — only the composition moves from a gateway wrapper to the stack.
pub struct RedactMiddleware {
    redactor: Arc<dyn PiiRedactor>,
}

impl RedactMiddleware {
    pub fn new(redactor: Arc<dyn PiiRedactor>) -> Self {
        Self { redactor }
    }
}

#[async_trait::async_trait]
impl AgentMiddleware for RedactMiddleware {
    fn name(&self) -> &str {
        "redact"
    }
    async fn before_model(
        &self,
        request: LlmRequest,
        _ctx: &RunCtx<'_>,
    ) -> Result<LlmRequest, LlmError> {
        self.redactor.redact_request(request).await
    }
    async fn after_model(
        &self,
        response: LlmResponse,
        _request: &LlmRequest,
        _ctx: &RunCtx<'_>,
    ) -> Result<LlmResponse, LlmError> {
        Ok(self.redactor.hydrate_response(response).await)
    }
}

/// EFFICIENCY — prompt compression (ADR 0014) as a before-model hook: shrinks `user`-role
/// message content. **Fail-open** (the compressor returns the text unchanged on any
/// error; this hook never `Err`s). Reuses the existing [`PromptCompressor`] verbatim.
pub struct CompressMiddleware {
    compressor: Arc<dyn PromptCompressor>,
}

impl CompressMiddleware {
    pub fn new(compressor: Arc<dyn PromptCompressor>) -> Self {
        Self { compressor }
    }
}

#[async_trait::async_trait]
impl AgentMiddleware for CompressMiddleware {
    fn name(&self) -> &str {
        "compress"
    }
    async fn before_model(
        &self,
        mut request: LlmRequest,
        _ctx: &RunCtx<'_>,
    ) -> Result<LlmRequest, LlmError> {
        for message in request.messages.iter_mut() {
            if message.role == "user" {
                let text = std::mem::take(&mut message.content);
                message.content = self.compressor.compress(text).await;
            }
        }
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_stack_is_a_no_op() {
        let stack = MiddlewareStack::new();
        assert!(stack.is_empty());
        let approved = HashSet::new();
        let channels = BTreeMap::new();
        let ctx = RunCtx {
            iteration: 0,
            approved_tool_names: &approved,
            channels: &channels,
        };
        // before_run does not stop, does not mutate.
        let mut conversation = vec![LlmMessage::text("user", "hi")];
        assert_eq!(
            stack.before_run(&mut conversation, &ctx).await.unwrap(),
            Flow::Continue
        );
        assert_eq!(conversation.len(), 1);
        // before_tool allows by default.
        let call = ToolCallCtx {
            name: "t",
            input: &Value::Null,
            requires_approval: false,
            content_scoped: false,
        };
        assert!(matches!(
            stack.before_tool(&call, &ctx).await.unwrap(),
            ToolControl::Allow {
                input_override: None
            }
        ));
        assert_eq!(stack.on_iteration(0, "x", &ctx), Flow::Continue);
    }

    /// A recorder asserting the onion order: request-path governed→efficiency,
    /// response-path reversed. This locks the redact→compress order (ADR 0025 3b).
    struct Recorder {
        label: String,
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl AgentMiddleware for Recorder {
        async fn before_model(
            &self,
            request: LlmRequest,
            _ctx: &RunCtx<'_>,
        ) -> Result<LlmRequest, LlmError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("before:{}", self.label));
            Ok(request)
        }
        async fn after_model(
            &self,
            response: LlmResponse,
            _request: &LlmRequest,
            _ctx: &RunCtx<'_>,
        ) -> Result<LlmResponse, LlmError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("after:{}", self.label));
            Ok(response)
        }
    }

    #[tokio::test]
    async fn onion_order_governed_outermost_then_reversed_on_response() {
        use adriane_llm_gateway::{LlmProvider, LlmUsage};

        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut stack = MiddlewareStack::new();
        // governed = redact (outermost); efficiency = compress (inner).
        stack.push_governed(Arc::new(Recorder {
            label: "redact".to_owned(),
            log: log.clone(),
        }));
        stack.push_efficiency(Arc::new(Recorder {
            label: "compress".to_owned(),
            log: log.clone(),
        }));

        let approved = HashSet::new();
        let channels = BTreeMap::new();
        let ctx = RunCtx {
            iteration: 0,
            approved_tool_names: &approved,
            channels: &channels,
        };
        let request = LlmRequest {
            provider: LlmProvider::Anthropic,
            model: "m".to_owned(),
            messages: vec![],
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
        };
        let request = stack.before_model(request, &ctx).await.unwrap();
        let response = LlmResponse {
            content: String::new(),
            tool_calls: None,
            stop_reason: None,
            usage: LlmUsage::default(),
            model: "m".to_owned(),
            provider: LlmProvider::Anthropic,
        };
        let _ = stack.after_model(response, &request, &ctx).await.unwrap();

        // Request: governed (redact) before efficiency (compress). Response: reversed.
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "before:redact",
                "before:compress",
                "after:compress",
                "after:redact"
            ]
        );
    }
}
