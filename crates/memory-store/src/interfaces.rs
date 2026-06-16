//! The `BaseStore` contract — the Rust port of the TS `BaseStore` interface.
//!
//! The TS methods are `async` (they return `Promise<T>` and may reject), so the
//! trait is async via [`async_trait`]. A rejected promise maps to an `Err`, so
//! every method returns [`Result`] over [`MemoryError`]. The trait is bounded
//! `Send + Sync` so a store can be shared as `Arc<dyn BaseStore>` across tasks.

use async_trait::async_trait;
use thiserror::Error;

use crate::types::{MemoryItem, MemoryKey, MemoryNamespace};

/// Errors a [`BaseStore`] implementation can surface.
///
/// The in-memory store never fails, but the contract is fallible so that
/// backend-specific stores (e.g. the deferred Postgres store) can report I/O or
/// (de)serialization failures without changing the trait.
#[derive(Debug, Error)]
pub enum MemoryError {
    /// The backing store failed to perform an operation.
    #[error("memory store backend error: {0}")]
    Backend(String),
    /// A stored value could not be (de)serialized.
    #[error("memory store serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A namespaced key/value store for agent memory.
///
/// Mirrors the TS `BaseStore` method set: `get`, `put`, `delete`, `search`, and
/// `list`. Methods take `&self`; implementations use interior mutability so a
/// single store can be shared behind a reference.
#[async_trait]
pub trait BaseStore: Send + Sync {
    /// Fetch the item at `(namespace, key)`, or `None` if it is absent.
    async fn get(
        &self,
        namespace: &MemoryNamespace,
        key: &MemoryKey,
    ) -> Result<Option<MemoryItem>, MemoryError>;

    /// Write `value` at `(namespace, key)`, returning the stored item.
    ///
    /// On overwrite the original `createdAt` is preserved and `updatedAt` is
    /// refreshed, matching the TS implementation.
    async fn put(
        &self,
        namespace: &MemoryNamespace,
        key: &MemoryKey,
        value: serde_json::Value,
    ) -> Result<MemoryItem, MemoryError>;

    /// Remove the item at `(namespace, key)`. Deleting an absent key is a no-op.
    async fn delete(&self, namespace: &MemoryNamespace, key: &MemoryKey)
        -> Result<(), MemoryError>;

    /// Return up to `top_k` items in `namespace` whose value or key contains
    /// `query` (case-insensitive substring match).
    async fn search(
        &self,
        namespace: &MemoryNamespace,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<MemoryItem>, MemoryError>;

    /// List items in `namespace`, optionally filtered to keys starting with
    /// `prefix`.
    async fn list(
        &self,
        namespace: &MemoryNamespace,
        prefix: Option<&str>,
    ) -> Result<Vec<MemoryItem>, MemoryError>;
}
