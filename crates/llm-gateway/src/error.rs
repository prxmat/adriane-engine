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
}
