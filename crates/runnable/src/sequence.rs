//! [`RunnableSequence`] — the Rust port of the TS `RunnableSequence`.
//!
//! ## Ergonomic divergence from TS (behaviour identical)
//!
//! In TS, `RunnableSequence` stores a heterogeneous `Runnable<unknown,
//! unknown>[]` and `invoke` threads the value left-to-right through each step:
//! `current = await runnable.invoke(current)`. Because every step is typed
//! `unknown -> unknown`, an arbitrary-length list type-checks.
//!
//! Rust generics cannot express "a list of runnables whose adjacent
//! input/output types line up" without erasing every step to a single dynamic
//! type. Rather than throw away the type safety the rest of the engine relies
//! on, the Rust port models a sequence as a **binary** chain: a
//! `RunnableSequence<I, M, O>` runs `A: Runnable<I, M>` and then feeds its
//! output to `B: Runnable<M, O>`. Longer pipelines are built by nesting — the
//! left side of a sequence may itself be a sequence — which yields the exact
//! same left-to-right "output of one feeds the next" semantics as the TS array
//! fold. The fluent `a.pipe(b)` call site becomes the free function
//! [`then`]`(a, b)` (or [`RunnableSequence::new`]); the runtime behaviour is the
//! same.

use std::marker::PhantomData;

use async_trait::async_trait;

use crate::error::RunnableError;
use crate::runnable::Runnable;

/// Two runnables composed left-to-right: run `first`, feed its output to
/// `second`.
///
/// `RunnableSequence<I, M, O>` is itself a `Runnable<I, O>`, so sequences nest
/// to form pipelines of any length while preserving end-to-end types.
pub struct RunnableSequence<I, M, O, A, B>
where
    A: Runnable<I, M>,
    B: Runnable<M, O>,
    I: Send + 'static,
    M: Send + 'static,
    O: Send + 'static,
{
    first: A,
    second: B,
    _marker: PhantomData<fn(I) -> (M, O)>,
}

impl<I, M, O, A, B> RunnableSequence<I, M, O, A, B>
where
    A: Runnable<I, M>,
    B: Runnable<M, O>,
    I: Send + 'static,
    M: Send + 'static,
    O: Send + 'static,
{
    /// Compose `first` then `second`.
    pub fn new(first: A, second: B) -> Self {
        RunnableSequence {
            first,
            second,
            _marker: PhantomData,
        }
    }
}

#[async_trait]
impl<I, M, O, A, B> Runnable<I, O> for RunnableSequence<I, M, O, A, B>
where
    A: Runnable<I, M>,
    B: Runnable<M, O>,
    I: Send + 'static,
    M: Send + 'static,
    O: Send + 'static,
{
    async fn invoke(&self, input: I) -> Result<O, RunnableError> {
        // Left-to-right: the output of `first` is the input of `second`, exactly
        // as the TS sequence threads `current` through each step.
        let intermediate = self.first.invoke(input).await?;
        self.second.invoke(intermediate).await
    }
}

/// Compose two runnables into a sequence — the Rust analogue of `a.pipe(b)`.
///
/// `then(a, b)` produces a [`RunnableSequence`] that runs `a` and feeds its
/// output to `b`.
pub fn then<I, M, O, A, B>(first: A, second: B) -> RunnableSequence<I, M, O, A, B>
where
    A: Runnable<I, M>,
    B: Runnable<M, O>,
    I: Send + 'static,
    M: Send + 'static,
    O: Send + 'static,
{
    RunnableSequence::new(first, second)
}
