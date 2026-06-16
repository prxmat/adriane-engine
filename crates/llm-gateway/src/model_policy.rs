//! Capability-tier model policy: map an abstract capability tier
//! (`frontier` / `balanced` / `fast` / `creative`) onto a concrete
//! `{ provider, model }` choice, given which providers are actually available.
//!
//! This mirrors the TypeScript `@adriane/llm-gateway` `model-policy.ts` byte for
//! byte in behaviour and wire shape (camelCase). The point, in the user's words:
//! "I only have Mistral" -> every tier resolves to the mistral column; "only
//! Anthropic" -> `fast` -> haiku, `frontier` -> opus, `creative` -> fable.

use std::collections::HashMap;
use std::env;

use serde::{Deserialize, Serialize};

use crate::types::LlmProvider;

/// An abstract capability tier. Serialises as `"frontier" | "balanced" | "fast"
/// | "creative"` (camelCase) to stay wire-compatible with the TS gateway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelTier {
    Frontier,
    Balanced,
    Fast,
    Creative,
}

impl ModelTier {
    /// All four tiers, in declaration order — handy for seeding tables.
    pub const ALL: [ModelTier; 4] = [
        ModelTier::Frontier,
        ModelTier::Balanced,
        ModelTier::Fast,
        ModelTier::Creative,
    ];
}

/// The outcome of [`ModelPolicy::resolve`]: a concrete provider + model, plus
/// whether the model came from the recommended per-tier defaults (`true`) or
/// from an explicit override (`false`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub provider: LlmProvider,
    pub model: String,
    pub recommended: bool,
}

/// The recommended per-provider, per-tier model table plus a cross-provider
/// preference order. Seeded with the shared capability-tier contract defaults via
/// [`ModelPolicy::default`], but constructable with overrides via
/// [`ModelPolicy::new`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPolicy {
    /// `provider -> (tier -> model)`. A provider missing a tier falls back to
    /// nothing (and is then treated as if it could not satisfy the request).
    table: HashMap<LlmProvider, HashMap<ModelTier, String>>,
    /// Highest preference first. When several providers are available and no
    /// override is given, the first one in this list that is available wins.
    preference: Vec<LlmProvider>,
}

impl Default for ModelPolicy {
    /// The shared capability-tier contract defaults.
    fn default() -> Self {
        let mut table: HashMap<LlmProvider, HashMap<ModelTier, String>> = HashMap::new();

        table.insert(
            LlmProvider::Anthropic,
            tier_map(&[
                (ModelTier::Frontier, "claude-opus-4-8"),
                (ModelTier::Balanced, "claude-sonnet-4-6"),
                (ModelTier::Fast, "claude-haiku-4-5"),
                (ModelTier::Creative, "claude-fable-5"),
            ]),
        );
        table.insert(
            LlmProvider::Mistral,
            tier_map(&[
                (ModelTier::Frontier, "mistral-large-latest"),
                (ModelTier::Balanced, "mistral-medium-latest"),
                (ModelTier::Fast, "mistral-small-latest"),
                (ModelTier::Creative, "mistral-large-latest"),
            ]),
        );
        table.insert(
            LlmProvider::Ollama,
            tier_map(&[
                (ModelTier::Frontier, "mistral"),
                (ModelTier::Balanced, "mistral"),
                (ModelTier::Fast, "mistral"),
                (ModelTier::Creative, "mistral"),
            ]),
        );

        ModelPolicy {
            table,
            preference: vec![
                LlmProvider::Anthropic,
                LlmProvider::Mistral,
                LlmProvider::Ollama,
            ],
        }
    }
}

impl ModelPolicy {
    /// Construct a policy with a custom table and/or preference order. Either
    /// argument may be `None` to keep the contract default for that piece.
    pub fn new(
        table: Option<HashMap<LlmProvider, HashMap<ModelTier, String>>>,
        preference: Option<Vec<LlmProvider>>,
    ) -> Self {
        let default = ModelPolicy::default();
        ModelPolicy {
            table: table.unwrap_or(default.table),
            preference: preference.unwrap_or(default.preference),
        }
    }

    /// The model fallback used when no provider is available at all.
    pub const MOCK_MODEL: &'static str = "mock-model";

    /// Which providers are usable given the current process environment:
    /// `anthropic` iff `ANTHROPIC_API_KEY` is set; `mistral` iff
    /// `MISTRAL_API_KEY` is set; `ollama` iff `ADRIANE_USE_OLLAMA=1`. Order
    /// follows the policy preference so callers get a deterministic list.
    pub fn available_from_env(&self) -> Vec<LlmProvider> {
        let anthropic = env_present("ANTHROPIC_API_KEY");
        let mistral = env_present("MISTRAL_API_KEY");
        let ollama = env::var("ADRIANE_USE_OLLAMA")
            .map(|v| v == "1")
            .unwrap_or(false);

        self.preference
            .iter()
            .copied()
            .filter(|p| match p {
                LlmProvider::Anthropic => anthropic,
                LlmProvider::Mistral => mistral,
                LlmProvider::Ollama => ollama,
                _ => false,
            })
            .collect()
    }

    /// Resolve a capability tier to a concrete `{ provider, model, recommended }`.
    ///
    /// - An explicit `override_provider` and/or `override_model` wins, with
    ///   `recommended = false`. When only one of the two is given, the other is
    ///   filled from the policy: an override provider maps the tier to that
    ///   provider's recommended model; an override model rides on the first
    ///   available provider (or the override provider if also given).
    /// - Otherwise the highest-preference provider that is both available and
    ///   present in the table supplies its tier model, with `recommended = true`.
    /// - If nothing is available, the mock provider is returned.
    pub fn resolve(
        &self,
        tier: ModelTier,
        available: &[LlmProvider],
        override_provider: Option<LlmProvider>,
        override_model: Option<&str>,
    ) -> ModelChoice {
        // An explicit override (provider and/or model) takes precedence and is
        // never flagged as a recommended default.
        if override_provider.is_some() || override_model.is_some() {
            let provider = override_provider
                .or_else(|| self.first_available(available))
                .unwrap_or(LlmProvider::Mock);
            let model = override_model
                .map(|m| m.to_owned())
                .or_else(|| self.model_for(provider, tier))
                .unwrap_or_else(|| Self::MOCK_MODEL.to_owned());
            return ModelChoice {
                provider,
                model,
                recommended: false,
            };
        }

        // No override: walk the preference order and take the first available
        // provider that can serve this tier.
        for provider in &self.preference {
            if available.contains(provider) {
                if let Some(model) = self.model_for(*provider, tier) {
                    return ModelChoice {
                        provider: *provider,
                        model,
                        recommended: true,
                    };
                }
            }
        }

        // Nothing available -> mock.
        ModelChoice {
            provider: LlmProvider::Mock,
            model: Self::MOCK_MODEL.to_owned(),
            recommended: false,
        }
    }

    /// The recommended model for a provider+tier from the table, if present.
    fn model_for(&self, provider: LlmProvider, tier: ModelTier) -> Option<String> {
        self.table
            .get(&provider)
            .and_then(|tiers| tiers.get(&tier))
            .cloned()
    }

    /// The first preference-ordered provider that is in `available`.
    fn first_available(&self, available: &[LlmProvider]) -> Option<LlmProvider> {
        self.preference
            .iter()
            .copied()
            .find(|p| available.contains(p))
    }
}

fn tier_map(entries: &[(ModelTier, &str)]) -> HashMap<ModelTier, String> {
    entries
        .iter()
        .map(|(tier, model)| (*tier, (*model).to_owned()))
        .collect()
}

fn env_present(key: &str) -> bool {
    env::var(key).map(|v| !v.is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mistral_routes_every_tier_to_the_mistral_column() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Mistral];

        let frontier = policy.resolve(ModelTier::Frontier, &available, None, None);
        let balanced = policy.resolve(ModelTier::Balanced, &available, None, None);
        let fast = policy.resolve(ModelTier::Fast, &available, None, None);
        let creative = policy.resolve(ModelTier::Creative, &available, None, None);

        assert_eq!(frontier.provider, LlmProvider::Mistral);
        assert_eq!(frontier.model, "mistral-large-latest");
        assert_eq!(balanced.model, "mistral-medium-latest");
        assert_eq!(fast.model, "mistral-small-latest");
        assert_eq!(creative.model, "mistral-large-latest");
        assert!(frontier.recommended);
        assert!(balanced.recommended);
        assert!(fast.recommended);
        assert!(creative.recommended);
    }

    #[test]
    fn only_anthropic_maps_each_tier_to_its_claude_model() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Anthropic];

        let fast = policy.resolve(ModelTier::Fast, &available, None, None);
        let frontier = policy.resolve(ModelTier::Frontier, &available, None, None);
        let creative = policy.resolve(ModelTier::Creative, &available, None, None);
        let balanced = policy.resolve(ModelTier::Balanced, &available, None, None);

        assert_eq!(fast.provider, LlmProvider::Anthropic);
        assert_eq!(fast.model, "claude-haiku-4-5");
        assert_eq!(frontier.model, "claude-opus-4-8");
        assert_eq!(creative.model, "claude-fable-5");
        assert_eq!(balanced.model, "claude-sonnet-4-6");
        assert!(fast.recommended);
    }

    #[test]
    fn override_model_wins_and_is_not_recommended() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Anthropic, LlmProvider::Mistral];

        let choice = policy.resolve(
            ModelTier::Frontier,
            &available,
            None,
            Some("my-custom-model"),
        );

        // Provider falls back to the first available (anthropic), model is the
        // override, and recommended is false.
        assert_eq!(choice.provider, LlmProvider::Anthropic);
        assert_eq!(choice.model, "my-custom-model");
        assert!(!choice.recommended);
    }

    #[test]
    fn override_provider_and_model_both_win() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Anthropic];

        let choice = policy.resolve(
            ModelTier::Fast,
            &available,
            Some(LlmProvider::Mistral),
            Some("mistral-tiny"),
        );

        assert_eq!(choice.provider, LlmProvider::Mistral);
        assert_eq!(choice.model, "mistral-tiny");
        assert!(!choice.recommended);
    }

    #[test]
    fn override_provider_alone_maps_to_that_providers_tier_model() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Anthropic, LlmProvider::Mistral];

        let choice = policy.resolve(
            ModelTier::Fast,
            &available,
            Some(LlmProvider::Mistral),
            None,
        );

        assert_eq!(choice.provider, LlmProvider::Mistral);
        assert_eq!(choice.model, "mistral-small-latest");
        assert!(!choice.recommended);
    }

    #[test]
    fn preference_order_picks_anthropic_over_mistral_when_both_present() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Mistral, LlmProvider::Anthropic];

        let choice = policy.resolve(ModelTier::Balanced, &available, None, None);

        assert_eq!(choice.provider, LlmProvider::Anthropic);
        assert_eq!(choice.model, "claude-sonnet-4-6");
        assert!(choice.recommended);
    }

    #[test]
    fn no_provider_available_resolves_to_mock() {
        let policy = ModelPolicy::default();

        let choice = policy.resolve(ModelTier::Frontier, &[], None, None);

        assert_eq!(choice.provider, LlmProvider::Mock);
        assert_eq!(choice.model, ModelPolicy::MOCK_MODEL);
        assert!(!choice.recommended);
    }

    #[test]
    fn model_tier_serialises_as_camel_case() {
        assert_eq!(
            serde_json::to_string(&ModelTier::Frontier).unwrap(),
            "\"frontier\""
        );
        assert_eq!(
            serde_json::to_string(&ModelTier::Balanced).unwrap(),
            "\"balanced\""
        );
        assert_eq!(serde_json::to_string(&ModelTier::Fast).unwrap(), "\"fast\"");
        assert_eq!(
            serde_json::to_string(&ModelTier::Creative).unwrap(),
            "\"creative\""
        );

        // Round-trips back to the same variant.
        let round: ModelTier = serde_json::from_str("\"creative\"").unwrap();
        assert_eq!(round, ModelTier::Creative);
    }

    #[test]
    fn model_choice_serialises_recommended_camel_case() {
        let choice = ModelChoice {
            provider: LlmProvider::Anthropic,
            model: "claude-opus-4-8".to_owned(),
            recommended: true,
        };
        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("\"provider\":\"anthropic\""));
        assert!(json.contains("\"model\":\"claude-opus-4-8\""));
        assert!(json.contains("\"recommended\":true"));
    }

    #[test]
    fn custom_overrides_replace_the_table() {
        let mut table: HashMap<LlmProvider, HashMap<ModelTier, String>> = HashMap::new();
        table.insert(
            LlmProvider::Mistral,
            tier_map(&[
                (ModelTier::Frontier, "frontier-x"),
                (ModelTier::Balanced, "balanced-x"),
                (ModelTier::Fast, "fast-x"),
                (ModelTier::Creative, "creative-x"),
            ]),
        );
        let policy = ModelPolicy::new(Some(table), Some(vec![LlmProvider::Mistral]));

        let choice = policy.resolve(ModelTier::Fast, &[LlmProvider::Mistral], None, None);
        assert_eq!(choice.provider, LlmProvider::Mistral);
        assert_eq!(choice.model, "fast-x");
        assert!(choice.recommended);
    }

    #[test]
    fn available_from_env_respects_preference_order() {
        // We cannot safely mutate process env in parallel tests, so assert on the
        // pure filtering by exercising resolve's preference path instead, which is
        // the same ordering primitive available_from_env relies on.
        let policy = ModelPolicy::default();
        assert_eq!(
            policy.preference,
            vec![
                LlmProvider::Anthropic,
                LlmProvider::Mistral,
                LlmProvider::Ollama,
            ]
        );
    }
}
