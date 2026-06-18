//! Reflection — the Rust port of `@adriane-ai/agents-core`'s `createReflectionNode`
//! (`reflection-node.ts`).
//!
//! The TS ships a graph node that runs **one** critique per invocation and either
//! loops back to a previous node (to re-draft) or accepts the current output. The
//! looping is driven by the runtime; the per-round contract is:
//!
//! 1. critique the current output with an LLM call;
//! 2. lowercase the critique. If the round budget is not yet exhausted **and** the
//!    critique contains `"problem"` **or** `"retry"`, revise (loop back, round + 1);
//! 3. otherwise accept — keep the output and bump `confidence` by `0.1` (capped 1.0).
//!
//! `__reflectionCount` carries the round across invocations; the default budget is
//! `maxReflections = 2` (i.e. up to two revisions after the first draft).
//!
//! This module collapses that runtime-driven loop into one in-process driver:
//! [`ReflectionAgent::run`] takes an initial draft plus a `revise` seam (the Rust
//! analogue of "loop back to the previous node to produce a new draft") and repeats
//! critique → revise until the critique accepts or the budget is spent. It returns
//! the final draft, the rounds taken, and the confidence the TS would have set.

use std::sync::Arc;

use adriane_llm_gateway::{LlmError, LlmGateway, LlmMessage, LlmProvider, LlmRequest};
use serde::{Deserialize, Serialize};

/// Model used for the critique call. Matches the TS `model: "reflection-node"`.
pub const REFLECTION_MODEL: &str = "reflection-node";

/// Default round budget. Matches the TS `maxReflections ?? 2`.
pub const DEFAULT_MAX_REFLECTIONS: usize = 2;

/// Substrings (checked against the lowercased critique) that signal the draft is not
/// yet acceptable and must be revised. Mirrors the TS
/// `critique.includes("problem") || critique.includes("retry")`.
pub const REVISE_MARKERS: [&str; 2] = ["problem", "retry"];

/// Whether a (lowercased) critique asks for another revision. Exactly the TS rule.
fn critique_requests_revision(critique_lower: &str) -> bool {
    REVISE_MARKERS
        .iter()
        .any(|marker| critique_lower.contains(marker))
}

/// What a reflection run produces: the accepted (or budget-exhausted) draft, how
/// many revision rounds ran, and the confidence the TS node would have set on
/// acceptance. `rounds` mirrors the TS `__reflectionCount` carried through the loop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionResult {
    pub draft: String,
    pub rounds: usize,
    pub confidence: f64,
}

/// A reflection agent: critique a draft, revise while the critique flags a problem,
/// stop on acceptance or the round budget. Cheap to share behind an `Arc`.
pub struct ReflectionAgent {
    gateway: Arc<dyn LlmGateway>,
    provider: LlmProvider,
    model: String,
    max_reflections: usize,
}

impl ReflectionAgent {
    /// An agent with the Rust defaults: Anthropic provider, the TS
    /// [`REFLECTION_MODEL`], and [`DEFAULT_MAX_REFLECTIONS`] rounds.
    pub fn new(gateway: Arc<dyn LlmGateway>) -> Self {
        ReflectionAgent {
            gateway,
            provider: LlmProvider::Anthropic,
            model: REFLECTION_MODEL.to_owned(),
            max_reflections: DEFAULT_MAX_REFLECTIONS,
        }
    }

    pub fn with_provider(mut self, provider: LlmProvider) -> Self {
        self.provider = provider;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_max_reflections(mut self, max_reflections: usize) -> Self {
        self.max_reflections = max_reflections;
        self
    }

    async fn critique(&self, draft: &str) -> Result<String, LlmError> {
        let response = self
            .gateway
            .complete(LlmRequest {
                provider: self.provider,
                model: self.model.clone(),
                messages: vec![LlmMessage {
                    role: "user".to_owned(),
                    // Mirrors the TS `Critique output: ${JSON.stringify(input)}`.
                    content: format!("Critique output: {draft}"),
                }],
                system: None,
                tools: None,
                max_tokens: None,
                temperature: None,
            })
            .await?;
        Ok(response.content)
    }

    /// Critique `draft`, and while the critique flags a `"problem"`/`"retry"` and the
    /// budget allows, call `revise` to produce a fresh draft. `revise` is the Rust
    /// analogue of the TS node looping back to its previous node; it receives the
    /// current draft and the critique that rejected it. Stops on acceptance or after
    /// `max_reflections` revisions, then reports the final draft + round count.
    ///
    /// `base_confidence` is the incoming `confidence` channel value (TS default 0.5);
    /// on acceptance the result confidence is `min(1.0, base_confidence + 0.1)`,
    /// matching the TS bump.
    pub async fn run<F, Fut>(
        &self,
        initial_draft: impl Into<String>,
        base_confidence: f64,
        mut revise: F,
    ) -> Result<ReflectionResult, LlmError>
    where
        F: FnMut(&str, &str) -> Fut,
        Fut: std::future::Future<Output = Result<String, LlmError>>,
    {
        let mut draft = initial_draft.into();
        let mut rounds = 0usize;

        loop {
            let critique = self.critique(&draft).await?;
            let critique_lower = critique.to_lowercase();
            // `count < maxReflections && (problem || retry)` → revise.
            if rounds < self.max_reflections && critique_requests_revision(&critique_lower) {
                draft = revise(&draft, &critique).await?;
                rounds += 1;
                continue;
            }
            // Accept: bump confidence by 0.1, capped at 1.0 (TS `Math.min(1, …)`).
            return Ok(ReflectionResult {
                draft,
                rounds,
                confidence: (base_confidence + 0.1).min(1.0),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use adriane_llm_gateway::{DefaultLlmGateway, LlmResponse, LlmUsage, MockAdapter};

    use super::*;

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

    fn gateway_with(responses: Vec<LlmResponse>) -> Arc<DefaultLlmGateway> {
        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(MockAdapter::new(
            LlmProvider::Anthropic,
            responses,
        )));
        Arc::new(gateway)
    }

    #[tokio::test]
    async fn accepts_early_when_the_critique_approves() {
        // First critique has no "problem"/"retry" → accept immediately, no revision.
        let agent = ReflectionAgent::new(gateway_with(vec![text("Looks great, ship it.")]));
        let revisions = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&revisions);
        let result = agent
            .run("draft v0", 0.5, |_draft, _critique| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok("revised".to_owned())
                }
            })
            .await
            .unwrap();

        assert_eq!(revisions.load(Ordering::SeqCst), 0);
        assert_eq!(result.rounds, 0);
        assert_eq!(result.draft, "draft v0");
        // 0.5 + 0.1 bump on acceptance.
        assert!((result.confidence - 0.6).abs() < 1e-9);
    }

    #[tokio::test]
    async fn revises_once_then_accepts_on_clean_critique() {
        // Round 0: "problem" → revise. Round 1: clean → accept.
        let agent = ReflectionAgent::new(gateway_with(vec![
            text("There is a problem with the intro."),
            text("All good now."),
        ]));
        let result = agent
            .run("draft v0", 0.5, |draft, _critique| {
                let next = format!("{draft}+fix");
                async move { Ok(next) }
            })
            .await
            .unwrap();

        assert_eq!(result.rounds, 1);
        assert_eq!(result.draft, "draft v0+fix");
        assert!((result.confidence - 0.6).abs() < 1e-9);
    }

    #[tokio::test]
    async fn stops_at_max_rounds_when_the_critique_never_approves() {
        // Every critique flags "retry"; budget is the default 2 → exactly 2 revisions.
        let agent = ReflectionAgent::new(gateway_with(vec![text("please retry")]));
        let revisions = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&revisions);
        let result = agent
            .run("draft v0", 0.5, |_draft, _critique| {
                let counter = Arc::clone(&counter);
                async move {
                    let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(format!("draft v{n}"))
                }
            })
            .await
            .unwrap();

        assert_eq!(revisions.load(Ordering::SeqCst), DEFAULT_MAX_REFLECTIONS);
        assert_eq!(result.rounds, DEFAULT_MAX_REFLECTIONS);
        assert_eq!(result.draft, "draft v2");
        // Budget-exhausted draft is still "accepted" with the same confidence bump.
        assert!((result.confidence - 0.6).abs() < 1e-9);
    }

    #[tokio::test]
    async fn confidence_bump_is_capped_at_one() {
        let agent = ReflectionAgent::new(gateway_with(vec![text("approved")]));
        let result = agent
            .run("draft", 0.95, |_d, _c| async { Ok(String::new()) })
            .await
            .unwrap();
        assert!((result.confidence - 1.0).abs() < 1e-9);
    }
}
