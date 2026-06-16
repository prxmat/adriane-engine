//! The error type surfaced by every [`Runnable`](crate::Runnable) invocation.
//!
//! In the TypeScript package a `Runnable.invoke` returns a `Promise<T>` that may
//! reject with an arbitrary thrown value. The Rust port makes that fallibility
//! explicit: `invoke` returns `Result<O, RunnableError>`. A thrown step error in
//! TS maps to an [`Err`] here, and combinators (`sequence`, `parallel`)
//! propagate the first error exactly as the TS `await`/`Promise.all` chains do.

use thiserror::Error;

/// An error produced while running a [`Runnable`](crate::Runnable).
#[derive(Debug, Error)]
pub enum RunnableError {
    /// A step (lambda body or composed runnable) failed.
    ///
    /// This is the Rust equivalent of a thrown error inside a TS lambda. The
    /// message is preserved verbatim so callers can assert on it the way the TS
    /// tests assert on `Error.message`.
    #[error("{0}")]
    Step(String),

    /// A value could not be (de)serialized while crossing a runnable boundary.
    ///
    /// Parallel branches and `serde_json::Value`-typed runnables can surface
    /// (de)serialization failures; they are reported here rather than panicking.
    #[error("runnable serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl RunnableError {
    /// Construct a [`RunnableError::Step`] from anything string-like.
    ///
    /// Mirrors the common TS pattern of `throw new Error("…")` inside a lambda.
    pub fn step(message: impl Into<String>) -> Self {
        RunnableError::Step(message.into())
    }
}
