//! Runtime value types: checkpoints and the run-event vocabulary.

use adriane_graph_core::{GraphState, NodeId, RunId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Identifier of a persisted checkpoint.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointId(pub String);

/// A point-in-time snapshot of a run's state, persisted after every node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: CheckpointId,
    pub run_id: RunId,
    pub graph_state: GraphState,
    pub created_at: String,
}

/// Lifecycle events emitted for every node transition. The `type` tag matches the
/// TS event vocabulary (`node_started`, `run_suspended`, …).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    NodeStarted {
        run_id: RunId,
        node_id: NodeId,
        timestamp: String,
    },
    NodeCompleted {
        run_id: RunId,
        node_id: NodeId,
        output: BTreeMap<String, Value>,
        timestamp: String,
    },
    NodeFailed {
        run_id: RunId,
        node_id: NodeId,
        error: String,
        /// 1-based attempt number — one `NodeFailed` is emitted per failed attempt.
        attempt: u32,
        timestamp: String,
    },
    RunSuspended {
        run_id: RunId,
        node_id: NodeId,
        reason: String,
        timestamp: String,
    },
    RunResumed {
        run_id: RunId,
        node_id: NodeId,
        timestamp: String,
    },
    RunCompleted {
        run_id: RunId,
        timestamp: String,
    },
    /// Terminal failure after retry attempts are exhausted — mirrors the TS
    /// `run_failed` event. A failed run never also emits `run_completed`.
    RunFailed {
        run_id: RunId,
        error: String,
        timestamp: String,
    },
}
