//! Built-in [`CallbackHandler`](crate::CallbackHandler) implementations.
//!
//! Ports the trivial handlers from `packages/callbacks/src/handlers`:
//! `NullCallbackHandler` (no-op), `ConsoleCallbackHandler` (prints each event),
//! and `MetricsCallbackHandler` (counts events and records chain durations).

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::interfaces::CallbackHandler;
use crate::types::CallbackEvent;

/// A handler that does nothing. Useful as a default.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullCallbackHandler;

#[async_trait]
impl CallbackHandler for NullCallbackHandler {
    async fn handle(&self, _event: &CallbackEvent) {}
}

/// A handler that prints each event to stdout.
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsoleCallbackHandler;

#[async_trait]
impl CallbackHandler for ConsoleCallbackHandler {
    async fn handle(&self, event: &CallbackEvent) {
        println!("[callback] {} {:?}", event.type_name(), event);
    }
}

/// A handler that counts events by type and records per-run chain durations.
///
/// Mirrors `MetricsCallbackHandler`: `onChainStart` records a start instant for
/// the run, `onChainEnd` records the elapsed duration, and every counted event
/// bumps a per-type counter.
#[derive(Debug, Default)]
pub struct MetricsCallbackHandler {
    inner: Mutex<MetricsState>,
}

#[derive(Debug, Default)]
struct MetricsState {
    counts: HashMap<String, u64>,
    durations_ms: HashMap<String, Vec<u128>>,
    starts: HashMap<String, Instant>,
}

impl MetricsCallbackHandler {
    /// Snapshot of how many times each event type has been counted.
    pub fn counts(&self) -> HashMap<String, u64> {
        self.inner.lock().unwrap().counts.clone()
    }

    /// Snapshot of recorded chain durations (in milliseconds) per run id.
    pub fn durations_ms(&self) -> HashMap<String, Vec<u128>> {
        self.inner.lock().unwrap().durations_ms.clone()
    }
}

#[async_trait]
impl CallbackHandler for MetricsCallbackHandler {
    async fn handle(&self, event: &CallbackEvent) {
        let mut state = self.inner.lock().unwrap();
        match event {
            CallbackEvent::OnChainStart { base, .. } => {
                state.starts.insert(base.run_id.clone(), Instant::now());
                *state.counts.entry("onChainStart".to_owned()).or_insert(0) += 1;
            }
            CallbackEvent::OnChainEnd { base, .. } => {
                *state.counts.entry("onChainEnd".to_owned()).or_insert(0) += 1;
                if let Some(start) = state.starts.get(&base.run_id).copied() {
                    let elapsed = start.elapsed().as_millis();
                    state
                        .durations_ms
                        .entry(base.run_id.clone())
                        .or_default()
                        .push(elapsed);
                }
            }
            CallbackEvent::OnNodeStart { .. } => {
                *state.counts.entry("onNodeStart".to_owned()).or_insert(0) += 1;
            }
            CallbackEvent::OnNodeEnd { .. } => {
                *state.counts.entry("onNodeEnd".to_owned()).or_insert(0) += 1;
            }
            CallbackEvent::OnNodeError { .. } => {
                *state.counts.entry("onNodeError".to_owned()).or_insert(0) += 1;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CallbackEventBase;
    use serde_json::json;

    fn base(run_id: &str) -> CallbackEventBase {
        CallbackEventBase {
            run_id: run_id.to_owned(),
            timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn metrics_counts_events_and_records_chain_duration() {
        let handler = MetricsCallbackHandler::default();

        handler
            .handle(&CallbackEvent::OnChainStart {
                base: base("run-1"),
                input: json!({}),
            })
            .await;
        handler
            .handle(&CallbackEvent::OnNodeStart {
                base: base("run-1"),
                input: json!({}),
            })
            .await;
        handler
            .handle(&CallbackEvent::OnNodeEnd {
                base: base("run-1"),
                output: json!({}),
            })
            .await;
        handler
            .handle(&CallbackEvent::OnChainEnd {
                base: base("run-1"),
                output: json!({}),
            })
            .await;

        let counts = handler.counts();
        assert_eq!(counts.get("onChainStart"), Some(&1));
        assert_eq!(counts.get("onChainEnd"), Some(&1));
        assert_eq!(counts.get("onNodeStart"), Some(&1));
        assert_eq!(counts.get("onNodeEnd"), Some(&1));
        assert_eq!(handler.durations_ms().get("run-1").map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn null_handler_is_a_noop() {
        let handler = NullCallbackHandler;
        handler
            .handle(&CallbackEvent::OnNodeStart {
                base: base("run-1"),
                input: json!({}),
            })
            .await;
    }
}
