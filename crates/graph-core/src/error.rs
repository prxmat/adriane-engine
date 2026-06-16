//! Validation error types, mirroring `GRAPH_VALIDATION_ERROR_CODES` in the TS model.

use serde::{Deserialize, Serialize};

/// Stable machine-readable code for a validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationErrorCode {
    DuplicateNodeId,
    DuplicateEdgeId,
    MissingEntryNode,
    InvalidEdgeReference,
    CycleDetected,
    InvalidConditionFormat,
}

/// A single graph validation failure. `validate_graph` returns a list of these
/// rather than failing on the first, so callers can surface every problem at once.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{code:?}: {message}")]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub message: String,
    /// Path into the definition (e.g. node/edge id) the error concerns.
    pub path: Vec<String>,
}

impl ValidationError {
    pub fn new(code: ValidationErrorCode, message: impl Into<String>, path: Vec<String>) -> Self {
        ValidationError {
            code,
            message: message.into(),
            path,
        }
    }
}
