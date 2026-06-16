//! `GraphRuntime` — the deterministic executor. Honours the core contract: checkpoint
//! after every node, emit a lifecycle event per transition, and suspend cleanly on a
//! human gate so the run resumes from the latest checkpoint.
//!
//! Node execution is **async** (handlers return a future), so handlers can do real
//! I/O — LLM calls, tools — once the agent crates land. Covered: start / resume /
//! suspend, default + conditional edges, channel reducers, DynamicInterrupt +
//! `update_state`, fan-out → join, recursion limit, retries (`retryPolicy` —
//! backoff *timing* is deferred: the crate stays async-runtime-agnostic with no
//! timer dependency, so `backoffMs` round-trips but no sleep happens between
//! attempts), time-travel (`checkpoints` / `replay_from`), and live event
//! observation via `on_event` (the TS `stream()` modes stay deferred). Deferred:
//! subgraphs.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default cap on node executions per run when the graph declares no `recursionLimit`.
const DEFAULT_RECURSION_LIMIT: u64 = 1000;

use adriane_graph_core::{
    ChannelReducer, EdgeType, GraphDefinition, GraphState, GraphStatus, NodeDefinition, NodeId,
    NodeType, RunId,
};
use serde_json::Value;

use crate::interfaces::{
    Checkpointer, ConditionRegistry, EventBus, EventObserver, InMemoryCheckpointer,
    InMemoryConditionRegistry, InMemoryEventBus, InMemoryNodeRegistry, NodeRegistry,
};
use crate::types::{Checkpoint, CheckpointId, RunEvent};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("node '{0}' is not declared in graph")]
    NodeNotFound(String),
    #[error("no node handler registered for '{0}'")]
    NoHandler(String),
    #[error("no checkpoint found for run '{0}'")]
    NoCheckpoint(String),
    #[error("checkpoint '{0}' not found for run '{1}'")]
    CheckpointNotFound(String, String),
    #[error("recursion limit exceeded for run '{0}'")]
    RecursionLimit(String),
}

fn now_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    millis.to_string()
}

pub struct GraphRuntime {
    graph: GraphDefinition,
    node_by_id: HashMap<String, NodeDefinition>,
    nodes: InMemoryNodeRegistry,
    conditions: InMemoryConditionRegistry,
    checkpointer: InMemoryCheckpointer,
    events: InMemoryEventBus,
    seq: AtomicU64,
    steps: Mutex<HashMap<String, u64>>,
}

impl GraphRuntime {
    pub fn new(
        graph: GraphDefinition,
        nodes: InMemoryNodeRegistry,
        conditions: InMemoryConditionRegistry,
    ) -> Self {
        let node_by_id = graph
            .nodes
            .iter()
            .map(|node| (node.id.0.clone(), node.clone()))
            .collect();
        GraphRuntime {
            graph,
            node_by_id,
            nodes,
            conditions,
            checkpointer: InMemoryCheckpointer::new(),
            events: InMemoryEventBus::new(),
            seq: AtomicU64::new(0),
            steps: Mutex::new(HashMap::new()),
        }
    }

    /// Emitted events, for inspection / tests.
    pub fn events(&self) -> &InMemoryEventBus {
        &self.events
    }

    /// Register a live event observer, invoked synchronously for every emitted
    /// event — the embedder-facing streaming seam. The TS `stream()` modes
    /// (values / updates / messages) stay deferred; embedders build them on top
    /// of this callback.
    pub fn on_event(&self, callback: EventObserver) {
        self.events.on_event(callback);
    }

    /// The checkpoint store, for inspection / tests.
    pub fn checkpointer(&self) -> &InMemoryCheckpointer {
        &self.checkpointer
    }

    /// Start a fresh run from the entry node and execute until completion or suspension.
    pub async fn start(
        &self,
        run_id: RunId,
        initial_data: BTreeMap<String, Value>,
    ) -> Result<GraphState, RuntimeError> {
        let now = now_string();
        let state = GraphState {
            run_id,
            graph_id: self.graph.id.clone(),
            current_node_id: self.graph.entry_node_id.clone(),
            status: GraphStatus::Running,
            channels: self.build_initial_channels(initial_data),
            version: 0,
            checkpoint_id: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let state = self.persist_checkpoint(state);
        self.run_loop(state).await
    }

    /// Resume a previously suspended run from its latest checkpoint.
    pub async fn resume(&self, run_id: &RunId) -> Result<GraphState, RuntimeError> {
        let checkpoint = self
            .checkpointer
            .load(run_id)
            .ok_or_else(|| RuntimeError::NoCheckpoint(run_id.0.clone()))?;
        let mut state = checkpoint.graph_state;

        let advance_from_gate = state.status == GraphStatus::Suspended
            && self
                .node_by_id
                .get(state.current_node_id.as_str())
                .map(|node| node.node_type == NodeType::HumanGate)
                .unwrap_or(false);

        let next = if advance_from_gate {
            self.next_node(&state.current_node_id.clone(), &state)
        } else {
            Some(state.current_node_id.clone())
        };

        match next {
            Some(node_id) => {
                state.current_node_id = node_id;
                state.status = GraphStatus::Running;
            }
            None => state.status = GraphStatus::Completed,
        }
        state.updated_at = now_string();

        self.events.emit(RunEvent::RunResumed {
            run_id: state.run_id.clone(),
            node_id: state.current_node_id.clone(),
            timestamp: now_string(),
        });

        let state = self.persist_checkpoint(state);
        self.run_loop(state).await
    }

    /// Patch a suspended run's channels and mark it runnable again — the seam a
    /// control plane uses to inject a decision (e.g. an approval) before `resume`.
    pub fn update_state(
        &self,
        run_id: &RunId,
        patch: BTreeMap<String, Value>,
    ) -> Result<GraphState, RuntimeError> {
        let checkpoint = self
            .checkpointer
            .load(run_id)
            .ok_or_else(|| RuntimeError::NoCheckpoint(run_id.0.clone()))?;
        let mut state = checkpoint.graph_state;
        let mut channels = state.channels.clone();
        self.apply_update(&mut channels, patch);
        state.channels = channels;
        state.status = GraphStatus::Running;
        state.version += 1;
        state.updated_at = now_string();
        Ok(self.persist_checkpoint(state))
    }

    /// All checkpoints recorded for a run, oldest first (the TS `getCheckpoints`).
    pub fn checkpoints(&self, run_id: &RunId) -> Vec<Checkpoint> {
        self.checkpointer.list(run_id)
    }

    /// Time-travel: fork a **new** run from one of `run_id`'s checkpoints and
    /// re-execute it to completion or suspension. The original run is untouched;
    /// the fork gets its own run id and its own checkpoint history.
    ///
    /// Mirrors the TS `replayFrom`. One divergence: TS `createForkRunId` derives
    /// the fork id from `Date.now()` + `Math.random()` (`<run>:fork:<ts>:<rand>`);
    /// this port keeps the runtime deterministic and uses the shared `seq`
    /// counter instead: `<run>:fork:<seq>`.
    pub async fn replay_from(
        &self,
        run_id: &RunId,
        checkpoint_id: &CheckpointId,
    ) -> Result<GraphState, RuntimeError> {
        let checkpoint = self
            .checkpointer
            .load_by_id(checkpoint_id)
            .filter(|checkpoint| checkpoint.run_id == *run_id)
            .ok_or_else(|| {
                RuntimeError::CheckpointNotFound(checkpoint_id.0.clone(), run_id.0.clone())
            })?;

        let mut state = checkpoint.graph_state;
        state.run_id = self.create_fork_run_id(run_id);
        state.status = GraphStatus::Running;
        state.checkpoint_id = None;
        state.updated_at = now_string();

        let state = self.persist_checkpoint(state);
        self.run_loop(state).await
    }

    fn create_fork_run_id(&self, run_id: &RunId) -> RunId {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        RunId(format!("{}:fork:{}", run_id.0, n))
    }

    fn consume_step(&self, run_id: &RunId) -> Result<(), RuntimeError> {
        let limit = self
            .graph
            .recursion_limit
            .map(u64::from)
            .unwrap_or(DEFAULT_RECURSION_LIMIT);
        let mut steps = self.steps.lock().expect("steps mutex poisoned");
        let count = steps.entry(run_id.0.clone()).or_insert(0);
        *count += 1;
        if *count > limit {
            return Err(RuntimeError::RecursionLimit(run_id.0.clone()));
        }
        Ok(())
    }

    async fn run_loop(&self, mut state: GraphState) -> Result<GraphState, RuntimeError> {
        while state.status == GraphStatus::Running {
            let node_id = state.current_node_id.clone();
            state = self.execute_node(node_id, state).await?;
        }
        if state.status == GraphStatus::Completed {
            self.events.emit(RunEvent::RunCompleted {
                run_id: state.run_id.clone(),
                timestamp: now_string(),
            });
        }
        Ok(state)
    }

    async fn execute_node(
        &self,
        node_id: NodeId,
        mut state: GraphState,
    ) -> Result<GraphState, RuntimeError> {
        let node = self
            .node_by_id
            .get(node_id.as_str())
            .cloned()
            .ok_or_else(|| RuntimeError::NodeNotFound(node_id.0.clone()))?;

        self.consume_step(&state.run_id)?;

        self.events.emit(RunEvent::NodeStarted {
            run_id: state.run_id.clone(),
            node_id: node_id.clone(),
            timestamp: now_string(),
        });

        if node.node_type == NodeType::HumanGate {
            return Ok(self.suspend(state, &node_id, "human-gate"));
        }

        // Retry policy: attempt up to maxAttempts (default 1). Backoff *timing* is
        // deferred — the crate is async-runtime-agnostic and carries no timer
        // dependency — but backoffMs is still read so the policy round-trips.
        let max_attempts = node
            .retry_policy
            .as_ref()
            .map(|policy| policy.max_attempts.max(1))
            .unwrap_or(1);
        let _backoff_ms = node
            .retry_policy
            .as_ref()
            .map(|policy| policy.backoff_ms)
            .unwrap_or(0);

        let mut attempt: u32 = 1;
        let output = loop {
            // Build the handler future, releasing the registry borrow before awaiting.
            let future = {
                let handler = self
                    .nodes
                    .resolve(&node_id)
                    .ok_or_else(|| RuntimeError::NoHandler(node_id.0.clone()))?;
                handler(state.clone())
            };
            let output = future.await;

            // A failed attempt never applies its update, never advances routing.
            let Some(error) = output.failure.clone() else {
                break output;
            };
            self.events.emit(RunEvent::NodeFailed {
                run_id: state.run_id.clone(),
                node_id: node_id.clone(),
                error: error.clone(),
                attempt,
                timestamp: now_string(),
            });

            if attempt >= max_attempts {
                // Attempts exhausted: the run fails. A Failed status stops the
                // run loop and must NOT produce a RunCompleted event.
                state.status = GraphStatus::Failed;
                state.version += 1;
                state.updated_at = now_string();
                let persisted = self.persist_checkpoint(state);
                self.events.emit(RunEvent::RunFailed {
                    run_id: persisted.run_id.clone(),
                    error,
                    timestamp: now_string(),
                });
                return Ok(persisted);
            }
            // No sleep between attempts (see backoff note above).
            attempt += 1;
        };

        // A handler-raised interrupt suspends the run (the DynamicInterrupt analogue):
        // apply its patch, then suspend without completing the node. Resume re-runs it.
        if let Some(interrupt) = output.interrupt {
            let mut channels = state.channels.clone();
            self.apply_update(&mut channels, interrupt.patch);
            state.channels = channels;
            state.version += 1;
            state.updated_at = now_string();
            return Ok(self.suspend(state, &node_id, &interrupt.reason));
        }

        let mut channels = state.channels.clone();
        self.apply_update(&mut channels, output.update.clone());

        self.events.emit(RunEvent::NodeCompleted {
            run_id: state.run_id.clone(),
            node_id: node_id.clone(),
            output: output.update,
            timestamp: now_string(),
        });

        state.channels = channels;
        state.version += 1;
        state.updated_at = now_string();

        // Fan-out: run the parallel branches (sequentially, deterministic merge) and
        // then continue at the declared join node.
        let next = if let Some(fan) = node.fan_out.clone() {
            for parallel_id in &fan.parallel_to {
                self.events.emit(RunEvent::NodeStarted {
                    run_id: state.run_id.clone(),
                    node_id: parallel_id.clone(),
                    timestamp: now_string(),
                });
                let branch_future = self
                    .nodes
                    .resolve(parallel_id)
                    .map(|handler| handler(state.clone()));
                if let Some(future) = branch_future {
                    let branch = future.await;
                    let mut branch_channels = state.channels.clone();
                    self.apply_update(&mut branch_channels, branch.update.clone());
                    state.channels = branch_channels;
                    self.events.emit(RunEvent::NodeCompleted {
                        run_id: state.run_id.clone(),
                        node_id: parallel_id.clone(),
                        output: branch.update,
                        timestamp: now_string(),
                    });
                }
            }
            Some(fan.join_at)
        } else {
            match output.goto {
                Some(targets) => targets.into_iter().next(),
                None => self.next_node(&node_id, &state),
            }
        };
        match next {
            Some(target) => {
                state.current_node_id = target;
                state.status = GraphStatus::Running;
            }
            None => {
                state.current_node_id = node_id;
                state.status = GraphStatus::Completed;
            }
        }

        Ok(self.persist_checkpoint(state))
    }

    fn suspend(&self, mut state: GraphState, node_id: &NodeId, reason: &str) -> GraphState {
        state.status = GraphStatus::Suspended;
        state.updated_at = now_string();
        let persisted = self.persist_checkpoint(state);
        self.events.emit(RunEvent::RunSuspended {
            run_id: persisted.run_id.clone(),
            node_id: node_id.clone(),
            reason: reason.to_owned(),
            timestamp: now_string(),
        });
        persisted
    }

    fn next_node(&self, from: &NodeId, state: &GraphState) -> Option<NodeId> {
        for edge in &self.graph.edges {
            if edge.from != *from {
                continue;
            }
            match edge.edge_type {
                EdgeType::Default => return Some(edge.to.clone()),
                EdgeType::Conditional => {
                    if let Some(name) = &edge.condition {
                        if let Some(predicate) = self.conditions.resolve(name) {
                            if predicate(state) {
                                return Some(edge.to.clone());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn persist_checkpoint(&self, mut state: GraphState) -> GraphState {
        let id = self.next_checkpoint_id(&state.run_id);
        state.checkpoint_id = Some(id.0.clone());
        let checkpoint = Checkpoint {
            id,
            run_id: state.run_id.clone(),
            graph_state: state.clone(),
            created_at: now_string(),
        };
        self.checkpointer.save(checkpoint);
        state
    }

    fn next_checkpoint_id(&self, run_id: &RunId) -> CheckpointId {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        CheckpointId(format!("{}:{}", run_id.0, n))
    }

    fn build_initial_channels(&self, initial: BTreeMap<String, Value>) -> BTreeMap<String, Value> {
        let mut channels: BTreeMap<String, Value> = BTreeMap::new();
        for (name, def) in &self.graph.channels {
            let value = initial
                .get(name)
                .cloned()
                .or_else(|| def.default.clone())
                .unwrap_or(Value::Null);
            channels.insert(name.clone(), value);
        }
        for (name, value) in initial {
            channels.entry(name).or_insert(value);
        }
        channels
    }

    fn apply_update(
        &self,
        channels: &mut BTreeMap<String, Value>,
        update: BTreeMap<String, Value>,
    ) {
        for (key, value) in update {
            let reducer = self
                .graph
                .channels
                .get(&key)
                .map(|def| def.reducer)
                .unwrap_or(ChannelReducer::Replace);
            match reducer {
                ChannelReducer::Replace => {
                    channels.insert(key, value);
                }
                ChannelReducer::Append => {
                    let mut items = match channels.remove(&key) {
                        Some(Value::Array(existing)) => existing,
                        // An uninitialised channel (Null / absent) starts empty.
                        Some(Value::Null) | None => Vec::new(),
                        Some(other) => vec![other],
                    };
                    match value {
                        Value::Array(incoming) => items.extend(incoming),
                        other => items.push(other),
                    }
                    channels.insert(key, Value::Array(items));
                }
                ChannelReducer::Merge => match value {
                    Value::Object(incoming) => {
                        let mut target = match channels.remove(&key) {
                            Some(Value::Object(existing)) => existing,
                            _ => serde_json::Map::new(),
                        };
                        for (field, field_value) in incoming {
                            target.insert(field, field_value);
                        }
                        channels.insert(key, Value::Object(target));
                    }
                    other => {
                        channels.insert(key, other);
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use adriane_graph_core::{
        ChannelDefinition, ChannelReducer, EdgeDefinition, EdgeId, EdgeType, FanOut,
        GraphDefinition, GraphId, GraphStatus, NodeDefinition, NodeId, NodeType, RetryPolicy,
        RunId,
    };
    use serde_json::{json, Value};

    use super::*;
    use crate::interfaces::{
        sync_handler, InMemoryConditionRegistry, InMemoryNodeRegistry, NodeOutput,
    };

    fn node(id: &str, node_type: NodeType) -> NodeDefinition {
        NodeDefinition {
            id: NodeId::from(id),
            node_type,
            label: id.to_owned(),
            subgraph_id: None,
            input_mapping: None,
            output_mapping: None,
            fan_out: None,
            retry_policy: None,
            metadata: None,
        }
    }

    fn edge(
        id: &str,
        from: &str,
        to: &str,
        edge_type: EdgeType,
        condition: Option<&str>,
    ) -> EdgeDefinition {
        EdgeDefinition {
            id: EdgeId::from(id),
            from: NodeId::from(from),
            to: NodeId::from(to),
            edge_type,
            condition: condition.map(|c| c.to_owned()),
        }
    }

    fn channel(name: &str, reducer: ChannelReducer) -> (String, ChannelDefinition) {
        (
            name.to_owned(),
            ChannelDefinition {
                channel_type: "json".to_owned(),
                reducer,
                default: None,
            },
        )
    }

    fn upd(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn graph(
        nodes: Vec<NodeDefinition>,
        edges: Vec<EdgeDefinition>,
        entry: &str,
        channels: Vec<(String, ChannelDefinition)>,
    ) -> GraphDefinition {
        GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: channels.into_iter().collect(),
            nodes,
            edges,
            entry_node_id: NodeId::from(entry),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn runs_a_linear_two_node_graph_to_completion() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("first"),
            sync_handler(|_s| NodeOutput::update(upd(&[("count", json!(1))]))),
        );
        nodes.register(
            NodeId::from("second"),
            sync_handler(|s| {
                let current = s
                    .channels
                    .get("count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                NodeOutput::update(upd(&[("count", json!(current + 10))]))
            }),
        );

        let def = graph(
            vec![
                node("first", NodeType::Action),
                node("second", NodeType::Action),
            ],
            vec![edge("e1", "first", "second", EdgeType::Default, None)],
            "first",
            vec![channel("count", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-1"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("count"), Some(&json!(11)));
        assert!(state.checkpoint_id.is_some());
    }

    #[tokio::test]
    async fn suspends_at_a_human_gate_and_resumes_to_completion() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("draft"),
            sync_handler(|_s| NodeOutput::update(upd(&[("approved", json!(false))]))),
        );
        nodes.register(
            NodeId::from("publish"),
            sync_handler(|_s| NodeOutput::update(upd(&[("approved", json!(true))]))),
        );

        let def = graph(
            vec![
                node("draft", NodeType::Action),
                node("review", NodeType::HumanGate),
                node("publish", NodeType::Action),
            ],
            vec![
                edge("e1", "draft", "review", EdgeType::Default, None),
                edge("e2", "review", "publish", EdgeType::Default, None),
            ],
            "draft",
            vec![channel("approved", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let suspended = runtime
            .start(RunId::from("run-2"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        assert_eq!(suspended.current_node_id, NodeId::from("review"));

        let resumed = runtime.resume(&RunId::from("run-2")).await.unwrap();
        assert_eq!(resumed.status, GraphStatus::Completed);
        assert_eq!(resumed.channels.get("approved"), Some(&json!(true)));

        let events = runtime.events().events();
        assert!(events
            .iter()
            .any(|e| matches!(e, RunEvent::RunSuspended { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, RunEvent::RunResumed { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, RunEvent::RunCompleted { .. })));
    }

    #[tokio::test]
    async fn routes_through_a_named_conditional_edge() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("start"),
            sync_handler(|_s| NodeOutput::update(upd(&[("score", json!(5))]))),
        );
        nodes.register(
            NodeId::from("high"),
            sync_handler(|_s| NodeOutput::update(upd(&[("path", json!("high"))]))),
        );
        nodes.register(
            NodeId::from("low"),
            sync_handler(|_s| NodeOutput::update(upd(&[("path", json!("low"))]))),
        );

        let mut conditions = InMemoryConditionRegistry::new();
        conditions.register(
            "isHigh".to_owned(),
            Box::new(|s| {
                s.channels
                    .get("score")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    >= 3
            }),
        );
        conditions.register(
            "isLow".to_owned(),
            Box::new(|s| {
                s.channels
                    .get("score")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    < 3
            }),
        );

        let def = graph(
            vec![
                node("start", NodeType::Action),
                node("high", NodeType::Action),
                node("low", NodeType::Action),
            ],
            vec![
                edge("e1", "start", "high", EdgeType::Conditional, Some("isHigh")),
                edge("e2", "start", "low", EdgeType::Conditional, Some("isLow")),
            ],
            "start",
            vec![
                channel("score", ChannelReducer::Replace),
                channel("path", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, conditions);

        let state = runtime
            .start(RunId::from("run-3"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.channels.get("path"), Some(&json!("high")));
    }

    #[tokio::test]
    async fn append_reducer_accumulates_into_an_array() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("a"),
            sync_handler(|_s| NodeOutput::update(upd(&[("log", json!("x"))]))),
        );

        let def = graph(
            vec![node("a", NodeType::Action)],
            vec![],
            "a",
            vec![channel("log", ChannelReducer::Append)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-4"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.channels.get("log"), Some(&json!(["x"])));
    }

    #[tokio::test]
    async fn interrupts_then_resumes_after_a_state_update() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("act"),
            sync_handler(|s| {
                let approved = s
                    .channels
                    .get("approved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if approved {
                    NodeOutput::update(upd(&[("done", json!(true))]))
                } else {
                    NodeOutput::interrupt("approval-required", upd(&[("pending", json!(true))]))
                }
            }),
        );
        let def = graph(
            vec![node("act", NodeType::Action)],
            vec![],
            "act",
            vec![
                channel("approved", ChannelReducer::Replace),
                channel("done", ChannelReducer::Replace),
                channel("pending", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let suspended = runtime
            .start(RunId::from("run-i"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        assert_eq!(suspended.channels.get("pending"), Some(&json!(true)));

        runtime
            .update_state(&RunId::from("run-i"), upd(&[("approved", json!(true))]))
            .unwrap();
        let done = runtime.resume(&RunId::from("run-i")).await.unwrap();
        assert_eq!(done.status, GraphStatus::Completed);
        assert_eq!(done.channels.get("done"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn fans_out_to_parallel_branches_then_joins() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("split"),
            sync_handler(|_s| NodeOutput::update(upd(&[("a", json!(1))]))),
        );
        nodes.register(
            NodeId::from("b1"),
            sync_handler(|_s| NodeOutput::update(upd(&[("b", json!(2))]))),
        );
        nodes.register(
            NodeId::from("b2"),
            sync_handler(|_s| NodeOutput::update(upd(&[("c", json!(3))]))),
        );
        nodes.register(
            NodeId::from("join"),
            sync_handler(|_s| NodeOutput::update(upd(&[("joined", json!(true))]))),
        );

        let split = NodeDefinition {
            fan_out: Some(FanOut {
                parallel_to: vec![NodeId::from("b1"), NodeId::from("b2")],
                join_at: NodeId::from("join"),
            }),
            ..node("split", NodeType::Action)
        };
        let def = graph(
            vec![
                split,
                node("b1", NodeType::Action),
                node("b2", NodeType::Action),
                node("join", NodeType::Action),
            ],
            vec![],
            "split",
            vec![
                channel("a", ChannelReducer::Replace),
                channel("b", ChannelReducer::Replace),
                channel("c", ChannelReducer::Replace),
                channel("joined", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-f"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("b"), Some(&json!(2)));
        assert_eq!(state.channels.get("c"), Some(&json!(3)));
        assert_eq!(state.channels.get("joined"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn enforces_the_recursion_limit_on_a_cycle() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("a"),
            sync_handler(|_s| NodeOutput::update(upd(&[("n", json!(1))]))),
        );
        nodes.register(
            NodeId::from("b"),
            sync_handler(|_s| NodeOutput::update(upd(&[("n", json!(2))]))),
        );

        let mut def = graph(
            vec![node("a", NodeType::Action), node("b", NodeType::Action)],
            vec![
                edge("e1", "a", "b", EdgeType::Default, None),
                edge("e2", "b", "a", EdgeType::Default, None),
            ],
            "a",
            vec![channel("n", ChannelReducer::Replace)],
        );
        def.recursion_limit = Some(5);
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let result = runtime.start(RunId::from("run-r"), BTreeMap::new()).await;
        assert!(matches!(result, Err(RuntimeError::RecursionLimit(_))));
    }

    fn failed_attempts(runtime: &GraphRuntime) -> Vec<u32> {
        runtime
            .events()
            .events()
            .iter()
            .filter_map(|event| match event {
                RunEvent::NodeFailed { attempt, .. } => Some(*attempt),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn retries_a_failing_node_and_succeeds_on_the_second_attempt() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_handler = Arc::clone(&calls);
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("flaky"),
            sync_handler(move |_s| {
                if calls_in_handler.fetch_add(1, Ordering::SeqCst) == 0 {
                    NodeOutput::failure("transient boom")
                } else {
                    NodeOutput::update(upd(&[("done", json!(true))]))
                }
            }),
        );

        let flaky = NodeDefinition {
            retry_policy: Some(RetryPolicy {
                max_attempts: 2,
                backoff_ms: 10,
            }),
            ..node("flaky", NodeType::Action)
        };
        let def = graph(
            vec![flaky],
            vec![],
            "flaky",
            vec![channel("done", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-retry"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("done"), Some(&json!(true)));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(failed_attempts(&runtime), vec![1]);
        assert!(runtime
            .events()
            .events()
            .iter()
            .any(|e| matches!(e, RunEvent::RunCompleted { .. })));
    }

    #[tokio::test]
    async fn fails_the_run_after_retries_are_exhausted() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("doomed"),
            sync_handler(|_s| NodeOutput::failure("boom")),
        );
        let doomed = NodeDefinition {
            retry_policy: Some(RetryPolicy {
                max_attempts: 2,
                backoff_ms: 0,
            }),
            ..node("doomed", NodeType::Action)
        };
        let def = graph(vec![doomed], vec![], "doomed", vec![]);
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-exhausted"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Failed);
        assert_eq!(failed_attempts(&runtime), vec![1, 2]);
        let events = runtime.events().events();
        assert!(!events
            .iter()
            .any(|e| matches!(e, RunEvent::RunCompleted { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, RunEvent::RunFailed { .. })));
        // The terminal Failed state is checkpointed.
        let latest = runtime
            .checkpointer()
            .load(&RunId::from("run-exhausted"))
            .unwrap();
        assert_eq!(latest.graph_state.status, GraphStatus::Failed);
    }

    #[tokio::test]
    async fn fails_immediately_when_the_node_has_no_retry_policy() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_handler = Arc::clone(&calls);
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("solo"),
            sync_handler(move |_s| {
                calls_in_handler.fetch_add(1, Ordering::SeqCst);
                NodeOutput::failure("no second chance")
            }),
        );
        let def = graph(vec![node("solo", NodeType::Action)], vec![], "solo", vec![]);
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-noretry"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Failed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(failed_attempts(&runtime), vec![1]);
    }

    fn counter_graph_runtime() -> GraphRuntime {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("first"),
            sync_handler(|_s| NodeOutput::update(upd(&[("count", json!(1))]))),
        );
        nodes.register(
            NodeId::from("second"),
            sync_handler(|s| {
                let current = s
                    .channels
                    .get("count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                NodeOutput::update(upd(&[("count", json!(current + 10))]))
            }),
        );
        let def = graph(
            vec![
                node("first", NodeType::Action),
                node("second", NodeType::Action),
            ],
            vec![edge("e1", "first", "second", EdgeType::Default, None)],
            "first",
            vec![channel("count", ChannelReducer::Replace)],
        );
        GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new())
    }

    #[tokio::test]
    async fn replay_from_forks_a_new_run_from_an_early_checkpoint() {
        let runtime = counter_graph_runtime();
        let run_id = RunId::from("run-tt");
        let original = runtime
            .start(run_id.clone(), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(original.status, GraphStatus::Completed);

        let checkpoints = runtime.checkpoints(&run_id);
        // The checkpoint persisted after node 1: node 2 is next, run still running.
        let after_first = checkpoints
            .iter()
            .find(|cp| {
                cp.graph_state.current_node_id == NodeId::from("second")
                    && cp.graph_state.status == GraphStatus::Running
                    && cp.graph_state.channels.get("count") == Some(&json!(1))
            })
            .cloned()
            .expect("checkpoint after node 1");
        let original_checkpoint_count = checkpoints.len();

        let fork = runtime.replay_from(&run_id, &after_first.id).await.unwrap();

        assert_eq!(fork.status, GraphStatus::Completed);
        assert_ne!(fork.run_id, run_id);
        assert!(fork.run_id.0.starts_with("run-tt:fork:"));
        assert_eq!(fork.channels.get("count"), Some(&json!(11)));

        // Original run untouched: its checkpoint history did not grow.
        assert_eq!(
            runtime.checkpoints(&run_id).len(),
            original_checkpoint_count
        );
        // The fork has its own checkpoints, all under the fork run id.
        let fork_checkpoints = runtime.checkpoints(&fork.run_id);
        assert!(!fork_checkpoints.is_empty());
        assert!(fork_checkpoints.iter().all(|cp| cp.run_id == fork.run_id));
    }

    #[tokio::test]
    async fn replay_from_rejects_a_checkpoint_belonging_to_another_run() {
        let runtime = counter_graph_runtime();
        let run_a = RunId::from("run-a");
        let run_b = RunId::from("run-b");
        runtime.start(run_a.clone(), BTreeMap::new()).await.unwrap();
        runtime.start(run_b.clone(), BTreeMap::new()).await.unwrap();

        let checkpoint_of_a = runtime
            .checkpoints(&run_a)
            .first()
            .cloned()
            .expect("run-a checkpoint");

        let err = runtime
            .replay_from(&run_b, &checkpoint_of_a.id)
            .await
            .unwrap_err();
        assert!(matches!(err, RuntimeError::CheckpointNotFound(_, _)));
    }

    #[tokio::test]
    async fn observers_registered_before_start_see_ordered_lifecycle_events() {
        let runtime = counter_graph_runtime();
        let seen: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        runtime.on_event(Box::new(move |event| {
            sink.lock().unwrap().push(event.clone())
        }));

        runtime
            .start(RunId::from("run-obs"), BTreeMap::new())
            .await
            .unwrap();

        let events = seen.lock().unwrap();
        for node_name in ["first", "second"] {
            let started = events
                .iter()
                .position(|e| {
                    matches!(e, RunEvent::NodeStarted { node_id, .. } if node_id.as_str() == node_name)
                })
                .expect("node_started observed");
            let completed = events
                .iter()
                .position(|e| {
                    matches!(e, RunEvent::NodeCompleted { node_id, .. } if node_id.as_str() == node_name)
                })
                .expect("node_completed observed");
            assert!(started < completed);
        }
        assert!(matches!(events.last(), Some(RunEvent::RunCompleted { .. })));
        // The observer saw exactly what the bus recorded, in the same order.
        assert_eq!(*events, runtime.events().events());
    }

    /// Compile-time proof that the runtime and its run futures are `Send + Sync`.
    /// This is what lets `GraphRuntime` be driven from a napi async fn (whose
    /// future must be `Send`). A regression that reintroduces non-`Send` interior
    /// mutability (`Rc` / `RefCell` / `Cell`) across the seams fails to compile
    /// here rather than at the napi boundary.
    #[test]
    fn runtime_and_run_futures_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send<T: Send>(_value: &T) {}

        assert_send_sync::<GraphRuntime>();
        // The boxed seam types stored on the runtime must also be thread-safe.
        assert_send_sync::<crate::interfaces::NodeHandler>();
        assert_send_sync::<crate::interfaces::ConditionFn>();
        assert_send_sync::<crate::interfaces::EventObserver>();
        // Handler futures only need to be `Send` (they are awaited on one thread
        // at a time, never shared), which is what keeps the run future `Send`.
        fn assert_box_future_is_send<T: Send>() {}
        assert_box_future_is_send::<crate::interfaces::BoxFuture>();

        // The future returned by `start()` (and the other drive methods) must be
        // `Send` so it can cross thread boundaries inside an async runtime.
        let runtime = counter_graph_runtime();
        let start_future = runtime.start(RunId::from("send-proof"), BTreeMap::new());
        assert_send(&start_future);
        // Don't actually drive it — the type-level check above is the assertion.
        drop(start_future);
    }
}
