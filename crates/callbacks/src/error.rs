//! Error types for the callbacks crate.

use thiserror::Error;

/// Errors that can arise while working with callbacks.
///
/// Handler failures are swallowed by the manager by design (mirroring the TS
/// implementation), so this enum currently only covers (de)serialization of
/// events at boundaries.
#[derive(Debug, Error)]
pub enum CallbackError {
    /// An event could not be (de)serialized.
    #[error("failed to (de)serialize callback event: {0}")]
    Serde(#[from] serde_json::Error),
}
