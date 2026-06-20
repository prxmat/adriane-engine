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
}
