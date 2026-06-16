//! [`RunnablePassthrough`] — the Rust port of the TS `RunnablePassthrough<T>`.
//!
//! Its `invoke` returns the input unchanged. Useful as the identity element in a
//! composition, exactly as in the TS package.

use async_trait::async_trait;

use crate::error::RunnableError;
use crate::runnable::Runnable;

/// A [`Runnable`] that returns its input untouched.
///
/// Faithful to the TS `RunnablePassthrough`: `invoke(input)` resolves to
/// `input`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RunnablePassthrough;

impl RunnablePassthrough {
    /// Construct a passthrough.
    pub fn new() -> Self {
        RunnablePassthrough
    }
}

#[async_trait]
impl<T> Runnable<T, T> for RunnablePassthrough
where
    T: Send + 'static,
{
    async fn invoke(&self, input: T) -> Result<T, RunnableError> {
        Ok(input)
    }
}
