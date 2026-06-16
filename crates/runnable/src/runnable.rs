//! The core [`Runnable`] contract — the Rust port of the TS `Runnable<I, O>`
//! interface.
//!
//! ## What carries over, and what diverges
//!
//! The TS interface exposes `invoke`, `stream`, `batch`, plus the fluent
//! combinators `pipe`, `withRetry`, and `withFallbacks`. The single load-bearing
//! method is `invoke(input) -> Promise<output>`; `stream` and `batch` are thin
//! wrappers over it (each TS implementation defines `stream` as
//! `yield await this.invoke(input)` and `batch` as `Promise.all(map(invoke))`).
//!
//! The Rust trait keeps **`invoke`** as the single required method and provides
//! `batch` as a default method built on it, faithfully reproducing the TS
//! `Promise.all` semantics. `stream` is intentionally omitted: a single-yield
//! async iterator over `invoke` carries no information the caller cannot get
//! from `invoke` itself, and modelling `AsyncIterable` in a generic object-safe
//! trait would force an allocation-heavy stream type on every implementor for no
//! behavioural gain. The combinators (`pipe`/`then`, `parallel`,
//! `withRetry`-style flows) are provided as explicit constructor functions and
//! types in the sibling modules rather than as trait methods, because Rust's
//! generics cannot express a `pipe<TNext>` that returns a new boxed `Runnable`
//! without committing to specific input/output types — see the module docs on
//! [`RunnableSequence`](crate::RunnableSequence) for the ergonomic divergence.
//! Behaviour is identical; only the call site shape differs.

use async_trait::async_trait;

use crate::error::RunnableError;

/// A composable unit of work that transforms an input into an output.
///
/// The Rust analogue of the TS `Runnable<TInput, TOutput>`. Implementors define
/// the asynchronous [`invoke`](Runnable::invoke); [`batch`](Runnable::batch) is
/// derived from it. The trait is `Send + Sync` so a runnable can be shared as
/// `Arc<dyn Runnable<I, O>>` across tasks, mirroring how TS runnables are passed
/// around freely.
#[async_trait]
pub trait Runnable<I, O>: Send + Sync
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Run the unit of work for a single input.
    ///
    /// Faithful to the TS `invoke(input): Promise<TOutput>` — a resolved promise
    /// becomes [`Ok`], a rejected promise becomes [`Err`].
    async fn invoke(&self, input: I) -> Result<O, RunnableError>;

    /// Run the unit of work for many inputs, resolving all of them.
    ///
    /// Mirrors the TS `batch(inputs) = Promise.all(inputs.map(invoke))`: every
    /// input is dispatched and the first error short-circuits the whole batch.
    async fn batch(&self, inputs: Vec<I>) -> Result<Vec<O>, RunnableError> {
        let mut outputs = Vec::with_capacity(inputs.len());
        for input in inputs {
            outputs.push(self.invoke(input).await?);
        }
        Ok(outputs)
    }
}
