//! Deterministic in-engine secrets redactor (ADR 0032 phase 10).
//!
//! Unlike PII (external Presidio/GLiNER over a URL, fail-open), secrets are a CLOSED, well-known
//! pattern set — so this is a fixed, versioned, constant regex matcher that runs **in-engine,
//! always-on, offline, replay-stable**, with no `eval`/dynamic patterns (the security hard rule).
//! It reuses the existing [`crate::redactor::PiiRedactor`] trait so it drops into the governed
//! `RedactMiddleware`-shaped machinery. Matches are replaced with a **typed one-way placeholder**
//! (the class, never the value or a hash); there is no vault and no hydration.

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;

use crate::error::LlmError;
use crate::redactor::PiiRedactor;
use crate::types::{ContentBlock, LlmRequest};

/// On a detected secret: mask-and-continue (default) or block the call (opt-in, fail-closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretPolicy {
    Mask,
    Block,
}

struct SecretPattern {
    regex: Regex,
    placeholder: &'static str,
}

/// The versioned, constant secret pattern set. Ordered longest/most-specific first so a
/// broader pattern does not pre-empt a typed class.
static SECRET_PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    let p = |re: &str, placeholder: &'static str| SecretPattern {
        regex: Regex::new(re).expect("valid secret regex"),
        placeholder,
    };
    vec![
        p(r"sk-proj-[A-Za-z0-9_-]{20,}", "[REDACTED:OPENAI_KEY]"),
        p(r"sk-[A-Za-z0-9]{20,}", "[REDACTED:OPENAI_KEY]"),
        p(r"AKIA[0-9A-Z]{16}", "[REDACTED:AWS_KEY]"),
        p(r"gh[oprsu]_[A-Za-z0-9]{36,}", "[REDACTED:GITHUB_TOKEN]"),
        p(r"xox[baprs]-[A-Za-z0-9-]{10,}", "[REDACTED:SLACK_TOKEN]"),
        p(r"AIza[A-Za-z0-9_-]{35}", "[REDACTED:GOOGLE_KEY]"),
        p(r"sk_live_[A-Za-z0-9]{20,}", "[REDACTED:STRIPE_KEY]"),
        p(
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "[REDACTED:JWT]",
        ),
        p(
            r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----",
            "[REDACTED:PRIVATE_KEY]",
        ),
        p(r"(?i)bearer\s+[A-Za-z0-9._-]{20,}", "[REDACTED_SECRET]"),
    ]
});

/// Scrub every known secret pattern from `text`. Returns `(scrubbed, found_any)`.
pub fn scrub_secrets(text: &str) -> (String, bool) {
    let mut out = text.to_owned();
    let mut found = false;
    for pattern in SECRET_PATTERNS.iter() {
        if pattern.regex.is_match(&out) {
            found = true;
            out = pattern
                .regex
                .replace_all(&out, pattern.placeholder)
                .into_owned();
        }
    }
    (out, found)
}

/// The in-engine secrets floor — a [`PiiRedactor`] that scrubs (or, under `Block`, fails closed).
pub struct RegexSecretsRedactor {
    policy: SecretPolicy,
}

impl RegexSecretsRedactor {
    pub fn new(policy: SecretPolicy) -> Self {
        Self { policy }
    }

    /// Read the policy from `ADRIANE_SECRETS_POLICY` (`block` → fail-closed; anything else → mask).
    pub fn from_env() -> Self {
        let policy = if std::env::var("ADRIANE_SECRETS_POLICY").as_deref() == Ok("block") {
            SecretPolicy::Block
        } else {
            SecretPolicy::Mask
        };
        Self { policy }
    }
}

#[async_trait]
impl PiiRedactor for RegexSecretsRedactor {
    async fn redact_request(&self, mut request: LlmRequest) -> Result<LlmRequest, LlmError> {
        let mut found = false;
        if let Some(system) = request.system.as_mut() {
            let (out, hit) = scrub_secrets(system);
            if hit {
                found = true;
                *system = out;
            }
        }
        for message in request.messages.iter_mut() {
            match message.content_blocks.as_mut() {
                Some(blocks) => {
                    for block in blocks.iter_mut() {
                        if let ContentBlock::Text { text } = block {
                            let (out, hit) = scrub_secrets(text);
                            if hit {
                                found = true;
                                *text = out;
                            }
                        }
                    }
                }
                None => {
                    let (out, hit) = scrub_secrets(&message.content);
                    if hit {
                        found = true;
                        message.content = out;
                    }
                }
            }
        }
        // Block policy fails closed AFTER scrubbing (the value never leaves the process either way).
        if found && self.policy == SecretPolicy::Block {
            return Err(LlmError::SecretsBlocked(
                "a secret/credential was detected in an outbound request".to_owned(),
            ));
        }
        Ok(request)
    }
    // after_model: strict identity (inherited default) — secrets are one-way, never re-hydrated.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LlmMessage, LlmProvider};

    fn req(content: &str) -> LlmRequest {
        LlmRequest {
            provider: LlmProvider::Anthropic,
            model: "m".to_owned(),
            messages: vec![LlmMessage::text("user", content)],
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            response_format: None,
        }
    }

    #[test]
    fn scrubs_known_secret_classes() {
        let (out, found) =
            scrub_secrets("key sk-abcdefghijklmnopqrstuv. and AKIA1234567890ABCDEF end");
        assert!(found);
        assert!(out.contains("[REDACTED:OPENAI_KEY]"));
        assert!(out.contains("[REDACTED:AWS_KEY]"));
        assert!(!out.contains("AKIA1234567890ABCDEF"));
    }

    #[test]
    fn leaves_clean_text_untouched() {
        let (out, found) = scrub_secrets("just a normal prompt about cats");
        assert!(!found);
        assert_eq!(out, "just a normal prompt about cats");
    }

    #[tokio::test]
    async fn mask_policy_scrubs_and_continues() {
        let r = RegexSecretsRedactor::new(SecretPolicy::Mask);
        let out = r
            .redact_request(req("here is ghp_012345678901234567890123456789012345 ok"))
            .await
            .unwrap();
        assert!(out.messages[0].content.contains("[REDACTED:GITHUB_TOKEN]"));
        assert!(!out.messages[0].content.contains("ghp_0123456789"));
    }

    #[tokio::test]
    async fn block_policy_fails_closed_on_a_secret() {
        let r = RegexSecretsRedactor::new(SecretPolicy::Block);
        let err = r
            .redact_request(req("token sk-abcdefghijklmnopqrstuvwxyz0"))
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::SecretsBlocked(_)));
    }

    #[tokio::test]
    async fn block_policy_passes_clean_requests() {
        let r = RegexSecretsRedactor::new(SecretPolicy::Block);
        assert!(r.redact_request(req("hello world")).await.is_ok());
    }
}
