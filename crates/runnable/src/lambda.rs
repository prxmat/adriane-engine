//! [`RunnableLambda`] — the Rust port of the TS `RunnableLambda<I, O>`.
//!
//! The TS version wraps an `async (input) => output` function and exposes it
//! through the `Runnable` interface. The Rust version wraps a boxed async
//! closure: `Fn(I) -> Future<Output = Result<O, RunnableError>>`. Two
//! constructors are provided:
//!
//! - [`RunnableLambda::new`] for an asynchronous closure (the direct analogue of
//!   the TS async lambda), and
//! - [`RunnableLambda::sync`] / [`sync_lambda`] for a pure, synchronous closure,
//!   which is wrapped so its result resolves immediately — the common case where
//!   the TS body does no `await`.

use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;

use crate::error::RunnableError;
use crate::runnable::Runnable;

/// The boxed future a lambda body resolves to.
type LambdaFuture<O> = Pin<Box<dyn Future<Output = Result<O, RunnableError>> + Send>>;

/// The boxed async closure a [`RunnableLambda`] wraps.
type LambdaFn<I, O> = Box<dyn Fn(I) -> LambdaFuture<O> + Send + Sync>;

/// A [`Runnable`] backed by a user-supplied closure.
///
/// Faithful to the TS `RunnableLambda`: `invoke` simply calls the wrapped
/// function with the input and resolves to its output (or propagates its error).
pub struct RunnableLambda<I, O> {
    func: LambdaFn<I, O>,
}

impl<I, O> RunnableLambda<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// Wrap an asynchronous closure.
    ///
    /// The direct analogue of `new RunnableLambda(async (input) => …)`.
    pub fn new<F, Fut>(func: F) -> Self
    where
        F: Fn(I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, RunnableError>> + Send + 'static,
    {
        RunnableLambda {
            func: Box::new(move |input| Box::pin(func(input))),
        }
    }

    /// Wrap a pure, synchronous closure.
    ///
    /// Equivalent to a TS lambda whose body does no `await` — the result is
    /// produced eagerly and resolved immediately.
    pub fn sync<F>(func: F) -> Self
    where
        F: Fn(I) -> Result<O, RunnableError> + Send + Sync + 'static,
    {
        RunnableLambda {
            func: Box::new(move |input| {
                let result = func(input);
                Box::pin(async move { result })
            }),
        }
    }
}

#[async_trait]
impl<I, O> Runnable<I, O> for RunnableLambda<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    async fn invoke(&self, input: I) -> Result<O, RunnableError> {
        (self.func)(input).await
    }
}

/// Convenience constructor for a synchronous [`RunnableLambda`].
///
/// `sync_lambda(f)` is shorthand for [`RunnableLambda::sync`].
pub fn sync_lambda<I, O, F>(func: F) -> RunnableLambda<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
    F: Fn(I) -> Result<O, RunnableError> + Send + Sync + 'static,
{
    RunnableLambda::sync(func)
}
