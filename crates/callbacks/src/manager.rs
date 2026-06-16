//! `InMemoryCallbackManager` — the default in-process manager.
//!
//! Mirrors `packages/callbacks/src/manager.ts`: it holds a set of handlers plus
//! inherited tags/metadata, merges those into each event's base on `emit`, and
//! fans the merged event out to every handler. Handler failures are isolated:
//! the trait's `handle` returns `()`, so a handler cannot abort the loop, and a
//! handler that panics would only unwind its own future — emission to the
//! remaining handlers is preserved by catching that unwind.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;

use crate::interfaces::{CallbackHandler, CallbackManager};
use crate::types::CallbackEvent;

/// In-process [`CallbackManager`]. Cloning is cheap: handlers are shared via
/// `Arc`, and child managers inherit a snapshot of the current handler list.
#[derive(Clone, Default)]
pub struct InMemoryCallbackManager {
    handlers: Vec<Arc<dyn CallbackHandler>>,
    inherited_tags: Vec<String>,
    inherited_metadata: BTreeMap<String, Value>,
}

impl InMemoryCallbackManager {
    /// Create a manager with the given handlers and no inherited context.
    pub fn new(handlers: Vec<Arc<dyn CallbackHandler>>) -> Self {
        Self {
            handlers,
            inherited_tags: Vec::new(),
            inherited_metadata: BTreeMap::new(),
        }
    }

    /// Create a manager with handlers and explicit inherited tags/metadata.
    pub fn with_context(
        handlers: Vec<Arc<dyn CallbackHandler>>,
        inherited_tags: Vec<String>,
        inherited_metadata: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            handlers,
            inherited_tags,
            inherited_metadata,
        }
    }

    /// Merge inherited tags/metadata into the event's base, mirroring the TS
    /// `emit` precedence (inherited first, then event-specific).
    fn merge(&self, mut event: CallbackEvent) -> CallbackEvent {
        let base = event.base_mut();

        let mut tags = self.inherited_tags.clone();
        if let Some(event_tags) = base.tags.take() {
            tags.extend(event_tags);
        }
        base.tags = Some(tags);

        let mut metadata = self.inherited_metadata.clone();
        if let Some(event_metadata) = base.metadata.take() {
            metadata.extend(event_metadata);
        }
        base.metadata = Some(metadata);

        event
    }
}

#[async_trait]
impl CallbackManager for InMemoryCallbackManager {
    fn add_handler(&mut self, handler: Arc<dyn CallbackHandler>) {
        self.handlers.push(handler);
    }

    async fn emit(&self, event: CallbackEvent) {
        let merged = self.merge(event);
        for handler in &self.handlers {
            // Isolate each handler: a panic in one must not break emission to the
            // others, mirroring the TS try/catch around each dispatch.
            let _ = AssertUnwindSafe(handler.handle(&merged))
                .catch_unwind()
                .await;
        }
    }

    fn create_child(
        &self,
        tags: Vec<String>,
        metadata: BTreeMap<String, Value>,
    ) -> Box<dyn CallbackManager> {
        let mut child_tags = self.inherited_tags.clone();
        child_tags.extend(tags);

        let mut child_metadata = self.inherited_metadata.clone();
        child_metadata.extend(metadata);

        Box::new(InMemoryCallbackManager {
            handlers: self.handlers.clone(),
            inherited_tags: child_tags,
            inherited_metadata: child_metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CallbackEventBase;
    use serde_json::json;
    use std::sync::Mutex;

    /// A handler that records, in order, the type names of events it receives.
    #[derive(Default)]
    struct RecordingHandler {
        seen: Mutex<Vec<String>>,
        seen_tags: Mutex<Vec<Vec<String>>>,
        seen_metadata: Mutex<Vec<BTreeMap<String, Value>>>,
    }

    #[async_trait]
    impl CallbackHandler for RecordingHandler {
        async fn handle(&self, event: &CallbackEvent) {
            self.seen.lock().unwrap().push(event.type_name().to_owned());
            let base = event.base();
            self.seen_tags
                .lock()
                .unwrap()
                .push(base.tags.clone().unwrap_or_default());
            self.seen_metadata
                .lock()
                .unwrap()
                .push(base.metadata.clone().unwrap_or_default());
        }
    }

    /// A handler that always panics — used to prove emission is isolated.
    struct PanicHandler;

    #[async_trait]
    impl CallbackHandler for PanicHandler {
        async fn handle(&self, _event: &CallbackEvent) {
            panic!("boom");
        }
    }

    fn node_start(run_id: &str) -> CallbackEvent {
        CallbackEvent::OnNodeStart {
            base: CallbackEventBase {
                run_id: run_id.to_owned(),
                timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
                ..Default::default()
            },
            input: json!({}),
        }
    }

    #[tokio::test]
    async fn emit_calls_matching_handler_method() {
        let handler = Arc::new(RecordingHandler::default());
        let manager = InMemoryCallbackManager::new(vec![handler.clone()]);

        manager.emit(node_start("run-1")).await;

        assert_eq!(*handler.seen.lock().unwrap(), vec!["onNodeStart"]);
    }

    #[tokio::test]
    async fn multiple_handlers_all_fire_in_order() {
        let first = Arc::new(RecordingHandler::default());
        let second = Arc::new(RecordingHandler::default());
        let manager = InMemoryCallbackManager::new(vec![first.clone(), second.clone()]);

        manager.emit(node_start("run-1")).await;
        manager
            .emit(CallbackEvent::OnNodeEnd {
                base: CallbackEventBase {
                    run_id: "run-1".to_owned(),
                    timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
                    ..Default::default()
                },
                output: json!({}),
            })
            .await;

        assert_eq!(
            *first.seen.lock().unwrap(),
            vec!["onNodeStart", "onNodeEnd"]
        );
        assert_eq!(
            *second.seen.lock().unwrap(),
            vec!["onNodeStart", "onNodeEnd"]
        );
    }

    #[tokio::test]
    async fn child_inherits_handlers_and_merges_tags_and_metadata() {
        let handler = Arc::new(RecordingHandler::default());
        let root = InMemoryCallbackManager::with_context(
            vec![handler.clone()],
            vec!["root".to_owned()],
            BTreeMap::from([("scope".to_owned(), json!("global"))]),
        );
        let child = root.create_child(
            vec!["child".to_owned()],
            BTreeMap::from([("unit".to_owned(), json!("test"))]),
        );

        child
            .emit(CallbackEvent::OnChainStart {
                base: CallbackEventBase {
                    run_id: "run-2".to_owned(),
                    timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
                    ..Default::default()
                },
                input: json!({}),
            })
            .await;

        assert_eq!(
            handler.seen_tags.lock().unwrap()[0],
            vec!["root".to_owned(), "child".to_owned()]
        );
        assert_eq!(
            handler.seen_metadata.lock().unwrap()[0],
            BTreeMap::from([
                ("scope".to_owned(), json!("global")),
                ("unit".to_owned(), json!("test")),
            ])
        );
    }

    #[tokio::test]
    async fn handler_panics_do_not_break_emit() {
        let recorder = Arc::new(RecordingHandler::default());
        let manager = InMemoryCallbackManager::new(vec![Arc::new(PanicHandler), recorder.clone()]);

        manager
            .emit(CallbackEvent::OnNodeEnd {
                base: CallbackEventBase {
                    run_id: "run-3".to_owned(),
                    timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
                    ..Default::default()
                },
                output: json!({}),
            })
            .await;

        // The recorder still fired despite the earlier handler panicking.
        assert_eq!(*recorder.seen.lock().unwrap(), vec!["onNodeEnd"]);
    }

    #[tokio::test]
    async fn add_handler_registers_after_construction() {
        let mut manager = InMemoryCallbackManager::default();
        let handler = Arc::new(RecordingHandler::default());
        manager.add_handler(handler.clone());

        manager.emit(node_start("run-4")).await;

        assert_eq!(*handler.seen.lock().unwrap(), vec!["onNodeStart"]);
    }
}
