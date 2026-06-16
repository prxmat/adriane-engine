//! The `CallbackHandler` and `CallbackManager` seams.
//!
//! Mirrors `packages/callbacks/src/interfaces.ts`. In the TypeScript model a
//! handler exposes one optional method per event type and the manager dispatches
//! `handler[event.type]`. In Rust the same dispatch semantics are expressed with
//! a single async `handle` method: the handler matches on the `CallbackEvent`
//! variant it cares about. This keeps the trait object-safe and idiomatic while
//! preserving the "only matching handlers react" behaviour — a handler simply
//! ignores variants it does not implement.

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::types::CallbackEvent;

/// A consumer of lifecycle events. Implementors react to the variants they care
/// about and ignore the rest; the default implementation is a no-op, mirroring
/// the all-optional methods of the TypeScript `CallbackHandler`.
#[async_trait]
pub trait CallbackHandler: Send + Sync {
    /// Handle a single emitted event. Errors are intentionally not returned:
    /// handler failures must never break emission (see the manager). Implementors
    /// should swallow their own recoverable errors.
    async fn handle(&self, event: &CallbackEvent);
}

/// Registers handlers and fans emitted events out to all of them.
///
/// Mirrors the TypeScript `CallbackManager`. `emit` is async because handlers
/// may perform async work; it never propagates handler errors.
#[async_trait]
pub trait CallbackManager: Send + Sync {
    /// Register an additional handler.
    fn add_handler(&mut self, handler: Arc<dyn CallbackHandler>);

    /// Emit an event to every registered handler, after merging inherited tags
    /// and metadata into the event's base fields.
    async fn emit(&self, event: CallbackEvent);

    /// Create a child manager that inherits the current handlers and extends the
    /// inherited tags/metadata.
    fn create_child(
        &self,
        tags: Vec<String>,
        metadata: BTreeMap<String, serde_json::Value>,
    ) -> Box<dyn CallbackManager>;
}
