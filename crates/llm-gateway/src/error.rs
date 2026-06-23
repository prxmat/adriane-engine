//! LLM gateway errors.

use crate::types::LlmProvider;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LlmError {
    #[error("no adapter registered for provider '{0:?}'")]
    ProviderNotFound(LlmProvider),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("blocked by PII policy: {0}")]
    PiiBlocked(String),
    /// ADR 0029: the model's output failed JSON-Schema validation after the bounded retry
    /// budget. Surfaced at the node sink as channel data (the gateway-error-as-data
    /// convention), never a panic.
    #[error("structured output did not match the schema: {0}")]
    StructuredOutputInvalid(String),
    /// ADR 0030: a multimodal media reference could not be resolved to bytes (e.g. a missing
    /// artifact), or inline media exceeded the size cap.
    #[error("media resolution failed: {0}")]
    MediaResolution(String),
    /// ADR 0032: a secret/credential was detected in an outbound request under the opt-in
    /// `block` policy (`ADRIANE_SECRETS_POLICY=block`). Surfaced at the node sink as channel
    /// data, never a panic. (The default policy masks-and-continues instead.)
    #[error("blocked by secrets policy: {0}")]
    SecretsBlocked(String),
}
