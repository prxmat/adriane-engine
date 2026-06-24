//! `GraphRuntime` — the deterministic executor. Honours the core contract: checkpoint
//! after every node, emit a lifecycle event per transition, and suspend cleanly on a
//! human gate so the run resumes from the latest checkpoint.
//!
//! Node execution is **async** (handlers return a future), so handlers can do real
//! I/O — LLM calls, tools — once the agent crates land. Covered: start / resume /
//! suspend, default + conditional edges, channel reducers, DynamicInterrupt +
//! `update_state`, fan-out → join (branches run CONCURRENTLY off a shared
//! pre-fan-out snapshot, updates merged in declared order — deterministic; see
//! ADR 0015), recursion limit, retries (`retryPolicy` —
//! backoff *timing* is deferred: the crate stays async-runtime-agnostic with no
//! timer dependency, so `backoffMs` round-trips but no sleep happens between
//! attempts), time-travel (`checkpoints` / `replay_from`), subgraphs (a
//! `subgraph`-type node runs a registered child graph, sharing this runtime's
//! registries / checkpointer / event bus; child suspension propagates to the
//! parent and a parent resume re-attaches to the child — see `execute_subgraph`),
//! and live event observation via `on_event` (the TS `stream()` modes stay
//! deferred; embedders build them on top of this callback).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default cap on node executions per run when the graph declares no `recursionLimit`.
const DEFAULT_RECURSION_LIMIT: u64 = 1000;

/// Channel key holding the per-node child run ids of subgraph nodes
/// (`{ <nodeId>: <childRunId> }`). Mirrors the TS `SUBGRAPH_RUNS_KEY`. Lets a
/// resumed parent re-attach to the SAME suspended child run instead of starting
/// a fresh one.
const SUBGRAPH_RUNS_KEY: &str = "__subgraphRuns";

/// Channel key holding the reason a run is suspended, with any durable-timer /
/// signal-wait metadata: `{ reason, wakeAt?, awaitingSignal? }`. `reason` is one of
/// `"human-gate" | "interrupt" | "timer" | "signal"`. The control-plane scheduler
/// reads `wakeAt` to know when to resume a timer; `awaitingSignal` names the signal a
/// `resume_with_signal` must deliver. Cleared on resume.
const SUSPEND_META_KEY: &str = "__suspend";

/// Channel holding delivered external-signal payloads, keyed by signal name
/// (`{ <signalName>: <payload> }`). A node that waited on a signal reads its payload
/// here after [`GraphRuntime::resume_with_signal`].
const SIGNALS_KEY: &str = "__signals";

/// Reserved channel exposing a [`GraphRuntime::send`]-injected input to a node handler
/// for the current execution only (never persisted). The dynamic-message / map-reduce
/// primitive: pre-queue inputs for a node, each execution consumes the next one.
const INJECTED_KEY: &str = "__injected";

/// Channel key holding the suspended child snapshots of subgraph nodes
/// (`{ <childRunId>: <GraphState> }`). The parent state carries them so a resume on a
/// FRESH runtime (each napi call rebuilds the checkpointer) can re-seed the child and
/// re-attach to it, instead of restarting the child from scratch. Cleared once the
/// child completes. In-process runs (CLI, tests) don't need it — the shared
/// checkpointer already holds the child — but writing it is harmless there.
const SUBGRAPH_STATES_KEY: &str = "__subgraphStates";

use adriane_graph_core::{
    ChannelDefinition, ChannelReducer, EdgeType, GraphDefinition, GraphState, GraphStatus,
    NodeDefinition, NodeId, NodeType, RunId,
};
use serde_json::Value;

/// ADR 0032: sentinel a `no_log` channel's value is masked to in emitted run events / logs.
const NO_LOG_SENTINEL: &str = "[REDACTED_NO_LOG]";

/// Mask `no_log` channels for the EVENT view only (ADR 0032 phase 10). Replaces (not omits) a
/// no-log channel's value with [`NO_LOG_SENTINEL`] so streaming UIs still see the channel
/// changed. Operates on a clone destined for the event — `apply_update`/checkpoint keep the
/// unmasked values, so determinism + resume are unaffected (durability ≠ observability).
fn mask_no_log(
    output: BTreeMap<String, Value>,
    channels: &BTreeMap<String, ChannelDefinition>,
) -> BTreeMap<String, Value> {
    output
        .into_iter()
        .map(|(key, value)| {
            if channels.get(&key).map(|c| c.no_log).unwrap_or(false) {
                (key, Value::String(NO_LOG_SENTINEL.to_owned()))
            } else {
                (key, value)
            }
        })
        .collect()
}

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
    #[error("subgraph node '{0}' has no subgraphId / the subgraph is not registered")]
    SubgraphNotResolvable(String),
    #[error("subgraph '{0}' is not registered with the runtime")]
    SubgraphNotFound(String),
    #[error("subgraph '{0}' failed")]
    SubgraphFailed(String),
}

fn now_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    millis.to_string()
}

/// Build the id→node index for a graph (used for both the top-level graph and each
/// registered subgraph).
fn index_nodes(graph: &GraphDefinition) -> HashMap<String, NodeDefinition> {
    graph
        .nodes
        .iter()
        .map(|node| (node.id.0.clone(), node.clone()))
        .collect()
}

/// Project parent channels into a child's initial data. With no mapping the child
/// receives a copy of every parent channel; with a mapping each `childKey ←
/// parentKey` is carried over (absent parent keys are skipped so the child falls
/// back to its own channel defaults). Mirrors the TS `applyInputMapping`.
fn apply_input_mapping(
    parent: &BTreeMap<String, Value>,
    mapping: Option<&BTreeMap<String, String>>,
) -> BTreeMap<String, Value> {
    match mapping {
        None => parent.clone(),
        Some(map) => {
            let mut out = BTreeMap::new();
            for (child_key, parent_key) in map {
                if let Some(value) = parent.get(parent_key) {
                    out.insert(child_key.clone(), value.clone());
                }
            }
            out
        }
    }
}

/// Merge a completed child's channels back into the parent. With no mapping every
/// child channel is written onto the parent (child wins on collisions); with a
/// mapping each `parentKey ← childKey` is written. Mirrors the TS
/// `applyOutputMapping`.
fn apply_output_mapping(
    parent: &mut BTreeMap<String, Value>,
    child: &BTreeMap<String, Value>,
    mapping: Option<&BTreeMap<String, String>>,
) {
    match mapping {
        None => {
            for (key, value) in child {
                parent.insert(key.clone(), value.clone());
            }
        }
        Some(map) => {
            for (parent_key, child_key) in map {
                let value = child.get(child_key).cloned().unwrap_or(Value::Null);
                parent.insert(parent_key.clone(), value);
            }
        }
    }
}

/// Record a subgraph node's child run id under `__subgraphRuns[nodeId]`.
fn set_subgraph_run_id(
    channels: &mut BTreeMap<String, Value>,
    node_id: &NodeId,
    child_run_id: &RunId,
) {
    let mut map = match channels.get(SUBGRAPH_RUNS_KEY) {
        Some(Value::Object(existing)) => existing.clone(),
        _ => serde_json::Map::new(),
    };
    map.insert(node_id.0.clone(), Value::String(child_run_id.0.clone()));
    channels.insert(SUBGRAPH_RUNS_KEY.to_owned(), Value::Object(map));
}

/// The child run id for a subgraph node: the one already recorded in
/// `__subgraphRuns` (so a resume re-attaches), else a deterministic
/// `<parentRunId>:<nodeId>`. Mirrors the TS `getOrCreateSubgraphRunId`.
fn subgraph_run_id(state: &GraphState, node_id: &NodeId) -> RunId {
    if let Some(Value::Object(map)) = state.channels.get(SUBGRAPH_RUNS_KEY) {
        if let Some(Value::String(existing)) = map.get(node_id.as_str()) {
            return RunId(existing.clone());
        }
    }
    RunId(format!("{}:{}", state.run_id.0, node_id.0))
}

/// Store a child run's suspended snapshot under `__subgraphStates[childRunId]`.
fn set_subgraph_state(
    channels: &mut BTreeMap<String, Value>,
    child_run_id: &RunId,
    child_state: &GraphState,
) {
    let Ok(value) = serde_json::to_value(child_state) else {
        return;
    };
    let mut map = match channels.get(SUBGRAPH_STATES_KEY) {
        Some(Value::Object(existing)) => existing.clone(),
        _ => serde_json::Map::new(),
    };
    map.insert(child_run_id.0.clone(), value);
    channels.insert(SUBGRAPH_STATES_KEY.to_owned(), Value::Object(map));
}

/// Read a child run's round-trip snapshot from `__subgraphStates`, if present.
fn get_subgraph_state(state: &GraphState, child_run_id: &RunId) -> Option<GraphState> {
    if let Some(Value::Object(map)) = state.channels.get(SUBGRAPH_STATES_KEY) {
        if let Some(value) = map.get(child_run_id.0.as_str()) {
            return serde_json::from_value(value.clone()).ok();
        }
    }
    None
}

/// Drop a completed child's snapshot from `__subgraphStates`.
fn clear_subgraph_state(channels: &mut BTreeMap<String, Value>, child_run_id: &RunId) {
    if let Some(Value::Object(map)) = channels.get_mut(SUBGRAPH_STATES_KEY) {
        map.remove(child_run_id.0.as_str());
    }
}

/// Record why a run suspended (durable timer / signal wait), with the scheduler hints
/// the control plane needs: `wakeAt` (when to resume a timer) and `awaitingSignal`
/// (the signal name a `resume_with_signal` must deliver).
fn set_suspend_meta(
    channels: &mut BTreeMap<String, Value>,
    reason: &str,
    wake_at: Option<&str>,
    awaiting_signal: Option<&str>,
) {
    let mut meta = serde_json::Map::new();
    meta.insert("reason".to_owned(), Value::String(reason.to_owned()));
    if let Some(wake_at) = wake_at {
        meta.insert("wakeAt".to_owned(), Value::String(wake_at.to_owned()));
    }
    if let Some(signal) = awaiting_signal {
        meta.insert(
            "awaitingSignal".to_owned(),
            Value::String(signal.to_owned()),
        );
    }
    channels.insert(SUSPEND_META_KEY.to_owned(), Value::Object(meta));
}

/// The `reason` of the current suspend metadata, if any (`"timer"` / `"signal"`).
fn read_suspend_reason(channels: &BTreeMap<String, Value>) -> Option<String> {
    if let Some(Value::Object(meta)) = channels.get(SUSPEND_META_KEY) {
        if let Some(Value::String(reason)) = meta.get("reason") {
            return Some(reason.clone());
        }
    }
    None
}

/// Drop the suspend metadata once the suspension is resolved.
fn clear_suspend_meta(channels: &mut BTreeMap<String, Value>) {
    channels.remove(SUSPEND_META_KEY);
}

/// Inject a delivered signal's payload under `__signals[name]`.
fn set_signal_payload(channels: &mut BTreeMap<String, Value>, name: &str, payload: Value) {
    let mut map = match channels.get(SIGNALS_KEY) {
        Some(Value::Object(existing)) => existing.clone(),
        _ => serde_json::Map::new(),
    };
    map.insert(name.to_owned(), payload);
    channels.insert(SIGNALS_KEY.to_owned(), Value::Object(map));
}

/// Clone `state`, exposing a `send`-injected input under the reserved `__injected`
/// channel when present. Used only for the handler call; the returned state is never
/// persisted.
fn with_injected(state: &GraphState, injected: Option<&Value>) -> GraphState {
    let mut clone = state.clone();
    if let Some(value) = injected {
        clone
            .channels
            .insert(INJECTED_KEY.to_owned(), value.clone());
    }
    clone
}

/// A registered subgraph: its definition plus a pre-built id→node index, so a
/// subgraph node can run the child without rebuilding the lookup each time.
struct SubgraphEntry {
    graph: GraphDefinition,
    node_by_id: HashMap<String, NodeDefinition>,
}

/// The active graph context a traversal runs against — the top-level graph for an
/// ordinary run, or a child's graph while a subgraph node executes. The node
/// handlers, conditions, checkpointer and event bus are SHARED across levels (they
/// live on `self`); only the graph structure being walked differs.
#[derive(Clone, Copy)]
struct GraphCtx<'a> {
    graph: &'a GraphDefinition,
    node_by_id: &'a HashMap<String, NodeDefinition>,
}

pub struct GraphRuntime {
    graph: GraphDefinition,
    node_by_id: HashMap<String, NodeDefinition>,
    nodes: InMemoryNodeRegistry,
    conditions: InMemoryConditionRegistry,
    checkpointer: InMemoryCheckpointer,
    events: InMemoryEventBus,
    /// Subgraphs reachable from `subgraph`-type nodes, keyed by their graph id.
    /// Child runs share this runtime's registries, checkpointer and event bus.
    subgraphs: HashMap<String, SubgraphEntry>,
    /// Dynamic-message inbox (`send`): FIFO inputs queued per `<runId>|<nodeId>`, each
    /// consumed by the target node's next execution (the reserved `__injected` channel).
    inbox: Mutex<HashMap<String, Vec<Value>>>,
    seq: AtomicU64,
    steps: Mutex<HashMap<String, u64>>,
}

impl GraphRuntime {
    pub fn new(
        graph: GraphDefinition,
        nodes: InMemoryNodeRegistry,
        conditions: InMemoryConditionRegistry,
    ) -> Self {
        let node_by_id = index_nodes(&graph);
        GraphRuntime {
            graph,
            node_by_id,
            nodes,
            conditions,
            checkpointer: InMemoryCheckpointer::new(),
            events: InMemoryEventBus::new(),
            subgraphs: HashMap::new(),
            inbox: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            steps: Mutex::new(HashMap::new()),
        }
    }

    /// Queue an input for `node_id` in run `run_id` (the dynamic-message `send`). Each
    /// queued input is consumed, FIFO, by the node's next execution and exposed under the
    /// reserved `__injected` channel (visible to the handler only, never persisted). The
    /// primitive map-reduce / dynamic-dispatch seam: pre-queue N inputs for a worker node
    /// (typically inside a cycle) and each pass processes the next one.
    pub fn send(&self, run_id: &RunId, node_id: &NodeId, input: Value) {
        let key = format!("{}|{}", run_id.0, node_id.0);
        self.inbox
            .lock()
            .expect("inbox mutex poisoned")
            .entry(key)
            .or_default()
            .push(input);
    }

    /// Pop the next queued input for a node (FIFO), if any.
    fn consume_injected_input(&self, run_id: &RunId, node_id: &NodeId) -> Option<Value> {
        let key = format!("{}|{}", run_id.0, node_id.0);
        let mut inbox = self.inbox.lock().expect("inbox mutex poisoned");
        let queue = inbox.get_mut(&key)?;
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    }

    /// Register the subgraphs that `subgraph`-type nodes resolve into, keyed by
    /// graph id. Child runs share this runtime's node/condition registries (so the
    /// child's node handlers must be registered alongside the parent's, by global
    /// node id), its checkpointer and its event bus — matching the TS engine, where
    /// the child `GraphRuntime` is built from the parent's seams.
    pub fn with_subgraphs(mut self, subgraphs: Vec<GraphDefinition>) -> Self {
        self.subgraphs = subgraphs
            .into_iter()
            .map(|graph| {
                let node_by_id = index_nodes(&graph);
                (graph.id.0.clone(), SubgraphEntry { graph, node_by_id })
            })
            .collect();
        self
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

    /// The active context for an ordinary (top-level) run.
    fn top_ctx(&self) -> GraphCtx<'_> {
        GraphCtx {
            graph: &self.graph,
            node_by_id: &self.node_by_id,
        }
    }

    /// Start a fresh run from the entry node and execute until completion or suspension.
    pub async fn start(
        &self,
        run_id: RunId,
        initial_data: BTreeMap<String, Value>,
    ) -> Result<GraphState, RuntimeError> {
        self.start_with_ctx(run_id, initial_data, self.top_ctx())
            .await
    }

    /// Start a run against a specific graph context (the top-level graph, or a
    /// child's graph when launched from a subgraph node).
    async fn start_with_ctx(
        &self,
        run_id: RunId,
        initial_data: BTreeMap<String, Value>,
        ctx: GraphCtx<'_>,
    ) -> Result<GraphState, RuntimeError> {
        let now = now_string();
        let state = GraphState {
            run_id,
            graph_id: ctx.graph.id.clone(),
            current_node_id: ctx.graph.entry_node_id.clone(),
            status: GraphStatus::Running,
            channels: self.build_initial_channels(initial_data, ctx.graph),
            version: 0,
            checkpoint_id: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let state = self.persist_checkpoint(state);
        self.run_loop(state, ctx).await
    }

    /// Resume a previously suspended run from its latest checkpoint.
    pub async fn resume(&self, run_id: &RunId) -> Result<GraphState, RuntimeError> {
        self.resume_with_ctx(run_id, self.top_ctx()).await
    }

    /// Resume a run against a specific graph context. The public [`Self::resume`]
    /// passes the top-level context; a subgraph node re-attaching to a suspended
    /// child passes the child's context (so the human gate that suspended the child
    /// is advanced against the child's edges, not the parent's).
    async fn resume_with_ctx(
        &self,
        run_id: &RunId,
        ctx: GraphCtx<'_>,
    ) -> Result<GraphState, RuntimeError> {
        let checkpoint = self
            .checkpointer
            .load(run_id)
            .ok_or_else(|| RuntimeError::NoCheckpoint(run_id.0.clone()))?;
        let mut state = checkpoint.graph_state;

        // Resume ADVANCES past the suspended node for a human gate (it has no handler to
        // re-run) and for a durable timer / external signal (one-shot — the wait is
        // over). A dynamic interrupt (e.g. an agent awaiting approval) instead RE-RUNS
        // the node, so it is not in this set.
        let is_gate = ctx
            .node_by_id
            .get(state.current_node_id.as_str())
            .map(|node| node.node_type == NodeType::HumanGate)
            .unwrap_or(false);
        let suspend_reason = read_suspend_reason(&state.channels);
        let advance = state.status == GraphStatus::Suspended
            && (is_gate || matches!(suspend_reason.as_deref(), Some("timer") | Some("signal")));

        // The suspension is being resolved — drop its metadata before continuing.
        clear_suspend_meta(&mut state.channels);

        let next = if advance {
            self.next_node(&state.current_node_id.clone(), &state, ctx.graph)
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
        self.run_loop(state, ctx).await
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
        self.apply_update(&mut channels, patch, &self.graph);
        state.channels = channels;
        state.status = GraphStatus::Running;
        state.version += 1;
        state.updated_at = now_string();
        Ok(self.persist_checkpoint(state))
    }

    /// Deliver an external signal to a suspended run, then resume it: inject `payload`
    /// into the `__signals` channel under `name` and continue. The run ADVANCES past the
    /// node that was awaiting the signal (the signal-wait suspension is one-shot). The
    /// twin of [`Self::update_state`] + [`Self::resume`] for the signal reason — the seam
    /// a control plane uses to wake a run on an external event.
    ///
    /// The status is left `Suspended` across the re-persist so `resume` recognises the
    /// signal-wait and advances (rather than re-running the node, the way an approval
    /// `update_state` → `resume` does). A signal targets the top-level run; delivering a
    /// signal awaited *inside* a subgraph is not yet modelled.
    pub async fn resume_with_signal(
        &self,
        run_id: &RunId,
        name: &str,
        payload: Value,
    ) -> Result<GraphState, RuntimeError> {
        let checkpoint = self
            .checkpointer
            .load(run_id)
            .ok_or_else(|| RuntimeError::NoCheckpoint(run_id.0.clone()))?;
        let mut state = checkpoint.graph_state;
        set_signal_payload(&mut state.channels, name, payload);
        state.version += 1;
        state.updated_at = now_string();
        // Keep the Suspended status so `resume` advances past the waiting node.
        self.persist_checkpoint(state);
        self.resume(run_id).await
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
        // A fork replays the TOP-level graph (time-travel is a top-run concern).
        self.run_loop(state, self.top_ctx()).await
    }

    fn create_fork_run_id(&self, run_id: &RunId) -> RunId {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        RunId(format!("{}:fork:{}", run_id.0, n))
    }

    fn consume_step(&self, run_id: &RunId, graph: &GraphDefinition) -> Result<(), RuntimeError> {
        let limit = graph
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

    async fn run_loop(
        &self,
        mut state: GraphState,
        ctx: GraphCtx<'_>,
    ) -> Result<GraphState, RuntimeError> {
        while state.status == GraphStatus::Running {
            let node_id = state.current_node_id.clone();
            state = self.execute_node(node_id, state, ctx).await?;
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
        ctx: GraphCtx<'_>,
    ) -> Result<GraphState, RuntimeError> {
        let node = ctx
            .node_by_id
            .get(node_id.as_str())
            .cloned()
            .ok_or_else(|| RuntimeError::NodeNotFound(node_id.0.clone()))?;

        self.consume_step(&state.run_id, ctx.graph)?;

        self.events.emit(RunEvent::NodeStarted {
            run_id: state.run_id.clone(),
            node_id: node_id.clone(),
            timestamp: now_string(),
        });

        if node.node_type == NodeType::HumanGate {
            return Ok(self.suspend(state, &node_id, "human-gate"));
        }

        // A subgraph node runs a registered child graph to completion or suspension,
        // mapping channels in and out. Child runs share this runtime's registries,
        // checkpointer and event bus (see `execute_subgraph`).
        if node.node_type == NodeType::Subgraph {
            return self.execute_subgraph(&node, node_id, state, ctx).await;
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

        // Dynamic-message inbox (`send`): consume one queued input for this node, if any,
        // and expose it to the handler under the reserved `__injected` channel. Consumed
        // ONCE per execution and reused across retry attempts (a deliberate simplification
        // over the TS port, which re-consumes per attempt). The injected value is visible
        // only to the handler — it is never persisted into the run's channels.
        let injected = self.consume_injected_input(&state.run_id, &node_id);

        let mut attempt: u32 = 1;
        let output = loop {
            // Build the handler future, releasing the registry borrow before awaiting.
            let future = {
                let handler = self
                    .nodes
                    .resolve(&node_id)
                    .ok_or_else(|| RuntimeError::NoHandler(node_id.0.clone()))?;
                handler(with_injected(&state, injected.as_ref()))
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
            self.apply_update(&mut channels, interrupt.patch, ctx.graph);
            state.channels = channels;
            state.version += 1;
            state.updated_at = now_string();
            return Ok(self.suspend(state, &node_id, &interrupt.reason));
        }

        // Durable timer / external signal: the node ran and produced its update; the RUN
        // then waits. Apply the update, emit completion, record the suspend reason (+
        // wakeAt / awaitingSignal for the scheduler), then suspend. Unlike a dynamic
        // interrupt, resume ADVANCES past this node (one-shot — see `resume_with_ctx`).
        if output.sleep_until.is_some() || output.wait_for_signal.is_some() {
            let mut channels = state.channels.clone();
            self.apply_update(&mut channels, output.update.clone(), ctx.graph);
            let reason = if output.wait_for_signal.is_some() {
                "signal"
            } else {
                "timer"
            };
            set_suspend_meta(
                &mut channels,
                reason,
                output.sleep_until.as_deref(),
                output.wait_for_signal.as_deref(),
            );
            state.channels = channels;
            self.events.emit(RunEvent::NodeCompleted {
                run_id: state.run_id.clone(),
                node_id: node_id.clone(),
                output: mask_no_log(output.update, &ctx.graph.channels),
                timestamp: now_string(),
            });
            state.version += 1;
            state.updated_at = now_string();
            return Ok(self.suspend(state, &node_id, reason));
        }

        let mut channels = state.channels.clone();
        self.apply_update(&mut channels, output.update.clone(), ctx.graph);

        self.events.emit(RunEvent::NodeCompleted {
            run_id: state.run_id.clone(),
            node_id: node_id.clone(),
            output: mask_no_log(output.update, &ctx.graph.channels),
            timestamp: now_string(),
        });

        state.channels = channels;
        state.version += 1;
        state.updated_at = now_string();

        // Fan-out: run the parallel branches CONCURRENTLY, then continue at the
        // declared join node. Two invariants make this deterministic despite the
        // concurrency (see ADR 0015):
        //   1. Every branch handler is called with the SAME pre-fan-out snapshot —
        //      no branch observes another's update mid-flight (map-reduce, not a
        //      chain). This matches the TS `executeFanOut` semantics; the previous
        //      sequential port wrongly let branch N+1 see branch N's writes.
        //   2. Branch updates are merged into the channels in DECLARED order (the
        //      order of `parallel_to`), so `append`/`merge` reducers fold the same
        //      way on every run regardless of which branch's future settles first.
        let next = if let Some(fan) = node.fan_out.clone() {
            let snapshot = state.clone();
            // Announce all branches up front (they run concurrently from here).
            for parallel_id in &fan.parallel_to {
                self.events.emit(RunEvent::NodeStarted {
                    run_id: state.run_id.clone(),
                    node_id: parallel_id.clone(),
                    timestamp: now_string(),
                });
            }
            // Build each branch future from the shared snapshot. Calling the handler
            // returns an owned `BoxFuture`, so the registry borrow is released before
            // the await — the futures can then run concurrently under `join_all`.
            let mut branch_ids: Vec<NodeId> = Vec::with_capacity(fan.parallel_to.len());
            let mut branch_futures = Vec::with_capacity(fan.parallel_to.len());
            for parallel_id in &fan.parallel_to {
                if let Some(handler) = self.nodes.resolve(parallel_id) {
                    branch_ids.push(parallel_id.clone());
                    branch_futures.push(handler(snapshot.clone()));
                }
            }
            let outputs = futures_util::future::join_all(branch_futures).await;
            // Merge in declared branch order (deterministic), emitting completion per
            // branch as its update lands.
            for (parallel_id, branch) in branch_ids.into_iter().zip(outputs) {
                self.apply_update(&mut state.channels, branch.update.clone(), ctx.graph);
                self.events.emit(RunEvent::NodeCompleted {
                    run_id: state.run_id.clone(),
                    node_id: parallel_id,
                    output: mask_no_log(branch.update, &ctx.graph.channels),
                    timestamp: now_string(),
                });
            }
            Some(fan.join_at)
        } else {
            match output.goto {
                Some(targets) => targets.into_iter().next(),
                None => self.next_node(&node_id, &state, ctx.graph),
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

    /// Run a `subgraph`-type node: resolve the registered child graph, map the
    /// parent's channels in, run the child to completion or suspension (sharing this
    /// runtime's registries / checkpointer / event bus), then map the child's
    /// channels back out and route on in the PARENT graph. Mirrors the TS subgraph
    /// branch of `executeNode`.
    ///
    /// Suspension propagation: if the child suspends (e.g. an internal human gate),
    /// the parent suspends "during" at this node, recording the child run id in
    /// `__subgraphRuns`. A later parent resume re-enters this node, finds the child
    /// checkpoint, and resumes the child rather than restarting it.
    async fn execute_subgraph(
        &self,
        node: &NodeDefinition,
        node_id: NodeId,
        mut state: GraphState,
        parent_ctx: GraphCtx<'_>,
    ) -> Result<GraphState, RuntimeError> {
        let subgraph_id = node
            .subgraph_id
            .as_ref()
            .ok_or_else(|| RuntimeError::SubgraphNotResolvable(node_id.0.clone()))?;
        let entry = self
            .subgraphs
            .get(subgraph_id.0.as_str())
            .ok_or_else(|| RuntimeError::SubgraphNotFound(subgraph_id.0.clone()))?;
        let child_ctx = GraphCtx {
            graph: &entry.graph,
            node_by_id: &entry.node_by_id,
        };

        let child_run_id = subgraph_run_id(&state, &node_id);
        let child_initial = apply_input_mapping(&state.channels, node.input_mapping.as_ref());

        // Across a napi boundary every call rebuilds the checkpointer, so a child that
        // suspended on a prior call is absent from `self.checkpointer`. The parent state
        // carries the child's suspended snapshot in `__subgraphStates` (written on the
        // suspend path below); reseed it here so the in-process AND cross-call paths both
        // resume the child rather than restarting it.
        if self.checkpointer.load(&child_run_id).is_none() {
            if let Some(child_state) = get_subgraph_state(&state, &child_run_id) {
                self.seed_subgraph_checkpoint(&child_run_id, child_state);
            }
        }

        // Re-attach to a suspended child if one exists; otherwise start it fresh.
        // The recursive child run is boxed to break the async-recursion cycle
        // (execute_node → execute_subgraph → start/resume → run_loop → execute_node).
        let child_state = if self.checkpointer.load(&child_run_id).is_some() {
            Box::pin(self.resume_with_ctx(&child_run_id, child_ctx)).await?
        } else {
            Box::pin(self.start_with_ctx(child_run_id.clone(), child_initial, child_ctx)).await?
        };

        if child_state.status == GraphStatus::Failed {
            return Err(RuntimeError::SubgraphFailed(subgraph_id.0.clone()));
        }

        // Record the child run id so a later parent resume re-attaches to it.
        set_subgraph_run_id(&mut state.channels, &node_id, &child_run_id);

        if child_state.status == GraphStatus::Suspended {
            // Carry the child's suspended snapshot in the parent state so a resume on a
            // fresh runtime (napi) can re-seed and re-attach to it.
            set_subgraph_state(&mut state.channels, &child_run_id, &child_state);
            state.version += 1;
            state.updated_at = now_string();
            return Ok(self.suspend(state, &node_id, "human-gate"));
        }

        // Child completed: drop its round-trip snapshot, then map channels back out.
        clear_subgraph_state(&mut state.channels, &child_run_id);
        apply_output_mapping(
            &mut state.channels,
            &child_state.channels,
            node.output_mapping.as_ref(),
        );

        self.events.emit(RunEvent::NodeCompleted {
            run_id: state.run_id.clone(),
            node_id: node_id.clone(),
            output: mask_no_log(child_state.channels.clone(), &entry.graph.channels),
            timestamp: now_string(),
        });

        state.version += 1;
        state.updated_at = now_string();

        let next = self.next_node(&node_id, &state, parent_ctx.graph);
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

    /// Seed a child run's suspended snapshot into this runtime's checkpointer (used when
    /// resuming a subgraph across a fresh runtime — the parent state carried the child
    /// snapshot in `__subgraphStates`).
    fn seed_subgraph_checkpoint(&self, run_id: &RunId, graph_state: GraphState) {
        let id = CheckpointId(
            graph_state
                .checkpoint_id
                .clone()
                .unwrap_or_else(|| format!("{}:sub-seed", run_id.0)),
        );
        self.checkpointer.save(Checkpoint {
            id,
            run_id: run_id.clone(),
            graph_state,
            created_at: now_string(),
        });
    }

    fn next_node(
        &self,
        from: &NodeId,
        state: &GraphState,
        graph: &GraphDefinition,
    ) -> Option<NodeId> {
        for edge in &graph.edges {
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

    fn build_initial_channels(
        &self,
        initial: BTreeMap<String, Value>,
        graph: &GraphDefinition,
    ) -> BTreeMap<String, Value> {
        let mut channels: BTreeMap<String, Value> = BTreeMap::new();
        for (name, def) in &graph.channels {
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
        graph: &GraphDefinition,
    ) {
        for (key, value) in update {
            let reducer = graph
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
                no_log: false,
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

    fn no_log_channel(name: &str) -> (String, ChannelDefinition) {
        (
            name.to_owned(),
            ChannelDefinition {
                channel_type: "json".to_owned(),
                reducer: ChannelReducer::Replace,
                default: None,
                no_log: true,
            },
        )
    }

    #[tokio::test]
    async fn no_log_channel_is_masked_in_events_but_checkpointed_in_full() {
        // ADR 0032: a no_log channel's value is masked in the emitted event but stored in full
        // in the run state (checkpointed) — durability ≠ observability.
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("emit"),
            sync_handler(|_s| {
                NodeOutput::update(upd(&[
                    ("secret", json!("hunter2")),
                    ("public", json!("ok")),
                ]))
            }),
        );
        let def = graph(
            vec![node("emit", NodeType::Action)],
            vec![],
            "emit",
            vec![
                no_log_channel("secret"),
                channel("public", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());
        let final_state = runtime
            .start(RunId::from("run-nolog"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(final_state.status, GraphStatus::Completed);
        // State (checkpointed) keeps the REAL value.
        assert_eq!(final_state.channels.get("secret"), Some(&json!("hunter2")));
        assert_eq!(final_state.channels.get("public"), Some(&json!("ok")));
        // The emitted NodeCompleted event MASKS the no-log channel, not the public one.
        let events = runtime.events().events();
        let output = events
            .iter()
            .find_map(|e| match e {
                RunEvent::NodeCompleted {
                    output, node_id, ..
                } if node_id.as_str() == "emit" => Some(output.clone()),
                _ => None,
            })
            .expect("a NodeCompleted event for emit");
        assert_eq!(output.get("secret"), Some(&json!("[REDACTED_NO_LOG]")));
        assert_eq!(output.get("public"), Some(&json!("ok")));
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
    async fn fan_out_merges_branch_updates_in_declared_order() {
        // Two branches append to the same channel. The deterministic-merge invariant
        // (ADR 0015) requires the result to fold in `parallel_to` order — b1 then b2 —
        // no matter which branch future settles first.
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("split"),
            sync_handler(|_s| NodeOutput::update(upd(&[("seed", json!(true))]))),
        );
        nodes.register(
            NodeId::from("b1"),
            sync_handler(|_s| NodeOutput::update(upd(&[("log", json!("first"))]))),
        );
        nodes.register(
            NodeId::from("b2"),
            sync_handler(|_s| NodeOutput::update(upd(&[("log", json!("second"))]))),
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
                channel("seed", ChannelReducer::Replace),
                channel("log", ChannelReducer::Append),
                channel("joined", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-fan-order"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("log"), Some(&json!(["first", "second"])));
        assert_eq!(state.channels.get("joined"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn fan_out_branches_run_concurrently() {
        // Proof of real concurrency: both branches rendezvous on a 2-party barrier.
        // If the runtime executed branches sequentially, the first `wait()` would
        // block forever (the second party never arrives) and the run would deadlock.
        // It completes only because both branch futures are in flight at once.
        use std::sync::Arc;
        use std::time::Duration;

        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let b1_barrier = Arc::clone(&barrier);
        let b2_barrier = Arc::clone(&barrier);

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("split"),
            sync_handler(|_s| NodeOutput::update(upd(&[("seed", json!(true))]))),
        );
        nodes.register(
            NodeId::from("b1"),
            Box::new(move |_s| {
                let barrier = Arc::clone(&b1_barrier);
                Box::pin(async move {
                    barrier.wait().await;
                    NodeOutput::update(upd(&[("b", json!(1))]))
                })
            }),
        );
        nodes.register(
            NodeId::from("b2"),
            Box::new(move |_s| {
                let barrier = Arc::clone(&b2_barrier);
                Box::pin(async move {
                    barrier.wait().await;
                    NodeOutput::update(upd(&[("c", json!(2))]))
                })
            }),
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
                channel("seed", ChannelReducer::Replace),
                channel("b", ChannelReducer::Replace),
                channel("c", ChannelReducer::Replace),
                channel("joined", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.start(RunId::from("run-fan-conc"), BTreeMap::new()),
        )
        .await
        .expect("fan-out branches did not deadlock (ran concurrently)")
        .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("b"), Some(&json!(1)));
        assert_eq!(state.channels.get("c"), Some(&json!(2)));
        assert_eq!(state.channels.get("joined"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn fan_out_branches_see_the_same_pre_fan_out_snapshot() {
        // Map-reduce semantics: each branch observes the channels as they were BEFORE
        // the fan-out, never a sibling's write. b1 writes `shared`; b2 reads it and
        // must NOT see b1's value (it sees the pre-fan-out absence).
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("split"),
            sync_handler(|_s| NodeOutput::update(upd(&[("seed", json!(true))]))),
        );
        nodes.register(
            NodeId::from("b1"),
            sync_handler(|_s| NodeOutput::update(upd(&[("shared", json!("from-b1"))]))),
        );
        nodes.register(
            NodeId::from("b2"),
            sync_handler(|s| {
                // Whatever b1 wrote must be invisible here.
                let saw = s.channels.get("shared").cloned().unwrap_or(Value::Null);
                NodeOutput::update(upd(&[("b2_saw", saw)]))
            }),
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
                channel("seed", ChannelReducer::Replace),
                channel("shared", ChannelReducer::Replace),
                channel("b2_saw", ChannelReducer::Replace),
                channel("joined", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let state = runtime
            .start(RunId::from("run-fan-snap"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.channels.get("b2_saw"), Some(&Value::Null));
        assert_eq!(state.channels.get("shared"), Some(&json!("from-b1")));
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

    #[tokio::test]
    async fn runs_a_subgraph_node_mapping_channels_in_and_out() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("pre"),
            sync_handler(|_s| NodeOutput::update(upd(&[("x", json!(21))]))),
        );
        // Child node: doubles the mapped-in value.
        nodes.register(
            NodeId::from("c1"),
            sync_handler(|s| {
                let v = s
                    .channels
                    .get("child_in")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                NodeOutput::update(upd(&[("child_out", json!(v * 2))]))
            }),
        );

        let child = GraphDefinition {
            id: GraphId::from("child"),
            ..graph(
                vec![node("c1", NodeType::Action)],
                vec![],
                "c1",
                vec![
                    channel("child_in", ChannelReducer::Replace),
                    channel("child_out", ChannelReducer::Replace),
                ],
            )
        };

        let sub_node = NodeDefinition {
            subgraph_id: Some(GraphId::from("child")),
            input_mapping: Some(
                [("child_in".to_owned(), "x".to_owned())]
                    .into_iter()
                    .collect(),
            ),
            output_mapping: Some(
                [("y".to_owned(), "child_out".to_owned())]
                    .into_iter()
                    .collect(),
            ),
            ..node("sub", NodeType::Subgraph)
        };
        let def = graph(
            vec![node("pre", NodeType::Action), sub_node],
            vec![edge("e1", "pre", "sub", EdgeType::Default, None)],
            "pre",
            vec![
                channel("x", ChannelReducer::Replace),
                channel("y", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new())
            .with_subgraphs(vec![child]);

        let state = runtime
            .start(RunId::from("run-sub"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.status, GraphStatus::Completed);
        // 21 mapped in → doubled to 42 → mapped back out to `y`.
        assert_eq!(state.channels.get("y"), Some(&json!(42)));
    }

    #[tokio::test]
    async fn suspends_when_a_subgraph_hits_an_internal_gate_then_resumes() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("c_draft"),
            sync_handler(|_s| NodeOutput::update(upd(&[("drafted", json!(true))]))),
        );
        nodes.register(
            NodeId::from("c_publish"),
            sync_handler(|_s| NodeOutput::update(upd(&[("child_out", json!("published"))]))),
        );
        nodes.register(
            NodeId::from("after"),
            sync_handler(|_s| NodeOutput::update(upd(&[("done", json!(true))]))),
        );

        let child = GraphDefinition {
            id: GraphId::from("child"),
            ..graph(
                vec![
                    node("c_draft", NodeType::Action),
                    node("c_gate", NodeType::HumanGate),
                    node("c_publish", NodeType::Action),
                ],
                vec![
                    edge("ce1", "c_draft", "c_gate", EdgeType::Default, None),
                    edge("ce2", "c_gate", "c_publish", EdgeType::Default, None),
                ],
                "c_draft",
                vec![
                    channel("drafted", ChannelReducer::Replace),
                    channel("child_out", ChannelReducer::Replace),
                ],
            )
        };

        let sub_node = NodeDefinition {
            subgraph_id: Some(GraphId::from("child")),
            output_mapping: Some(
                [("result".to_owned(), "child_out".to_owned())]
                    .into_iter()
                    .collect(),
            ),
            ..node("sub", NodeType::Subgraph)
        };
        let def = graph(
            vec![sub_node, node("after", NodeType::Action)],
            vec![edge("e1", "sub", "after", EdgeType::Default, None)],
            "sub",
            vec![
                channel("result", ChannelReducer::Replace),
                channel("done", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new())
            .with_subgraphs(vec![child]);

        // The internal gate suspends the parent AT the subgraph node.
        let suspended = runtime
            .start(RunId::from("run-subgate"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        assert_eq!(suspended.current_node_id, NodeId::from("sub"));

        // Parent resume re-attaches to the child, advances past its gate, completes,
        // maps the child output out, and routes on to `after`.
        let resumed = runtime.resume(&RunId::from("run-subgate")).await.unwrap();
        assert_eq!(resumed.status, GraphStatus::Completed);
        assert_eq!(resumed.channels.get("result"), Some(&json!("published")));
        assert_eq!(resumed.channels.get("done"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn durable_timer_suspends_then_advances_on_resume() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("wait"),
            sync_handler(|_s| NodeOutput::sleep("2026-01-01T00:00:00Z")),
        );
        nodes.register(
            NodeId::from("after"),
            sync_handler(|_s| NodeOutput::update(upd(&[("done", json!(true))]))),
        );
        let def = graph(
            vec![
                node("wait", NodeType::Action),
                node("after", NodeType::Action),
            ],
            vec![edge("e1", "wait", "after", EdgeType::Default, None)],
            "wait",
            vec![channel("done", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let suspended = runtime
            .start(RunId::from("run-timer"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        assert_eq!(suspended.current_node_id, NodeId::from("wait"));
        // The scheduler reads reason + wakeAt off the suspended run.
        let meta = suspended.channels.get("__suspend").expect("suspend meta");
        assert_eq!(meta.get("reason"), Some(&json!("timer")));
        assert_eq!(meta.get("wakeAt"), Some(&json!("2026-01-01T00:00:00Z")));

        // Resuming at wakeAt advances PAST the timer node (one-shot).
        let resumed = runtime.resume(&RunId::from("run-timer")).await.unwrap();
        assert_eq!(resumed.status, GraphStatus::Completed);
        assert_eq!(resumed.channels.get("done"), Some(&json!(true)));
        assert!(!resumed.channels.contains_key("__suspend"));
    }

    #[tokio::test]
    async fn external_signal_suspends_then_advances_with_payload() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("wait"),
            sync_handler(|_s| NodeOutput::wait_for_signal("approval")),
        );
        nodes.register(
            NodeId::from("after"),
            sync_handler(|s| {
                let payload = s
                    .channels
                    .get("__signals")
                    .and_then(|signals| signals.get("approval"))
                    .cloned()
                    .unwrap_or(Value::Null);
                NodeOutput::update(upd(&[("received", payload)]))
            }),
        );
        let def = graph(
            vec![
                node("wait", NodeType::Action),
                node("after", NodeType::Action),
            ],
            vec![edge("e1", "wait", "after", EdgeType::Default, None)],
            "wait",
            vec![channel("received", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let suspended = runtime
            .start(RunId::from("run-sig"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        let meta = suspended.channels.get("__suspend").expect("suspend meta");
        assert_eq!(meta.get("reason"), Some(&json!("signal")));
        assert_eq!(meta.get("awaitingSignal"), Some(&json!("approval")));

        // Delivering the signal injects its payload and advances past the wait node.
        let resumed = runtime
            .resume_with_signal(&RunId::from("run-sig"), "approval", json!({ "ok": true }))
            .await
            .unwrap();
        assert_eq!(resumed.status, GraphStatus::Completed);
        assert_eq!(
            resumed.channels.get("received"),
            Some(&json!({ "ok": true }))
        );
    }

    #[tokio::test]
    async fn signal_or_timeout_can_be_woken_by_the_timer() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("wait"),
            sync_handler(|_s| {
                NodeOutput::wait_for_signal_or_timeout("approval", "2026-01-01T00:00:00Z")
            }),
        );
        nodes.register(
            NodeId::from("after"),
            sync_handler(|s| {
                let via_signal = s
                    .channels
                    .get("__signals")
                    .and_then(|signals| signals.get("approval"))
                    .is_some();
                NodeOutput::update(upd(&[("viaSignal", json!(via_signal))]))
            }),
        );
        let def = graph(
            vec![
                node("wait", NodeType::Action),
                node("after", NodeType::Action),
            ],
            vec![edge("e1", "wait", "after", EdgeType::Default, None)],
            "wait",
            vec![channel("viaSignal", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        let suspended = runtime
            .start(RunId::from("run-sot"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(suspended.status, GraphStatus::Suspended);
        let meta = suspended.channels.get("__suspend").expect("suspend meta");
        // A signal-or-timeout suspends as a signal wait that ALSO carries a wakeAt.
        assert_eq!(meta.get("reason"), Some(&json!("signal")));
        assert_eq!(meta.get("awaitingSignal"), Some(&json!("approval")));
        assert_eq!(meta.get("wakeAt"), Some(&json!("2026-01-01T00:00:00Z")));

        // Timeout path: a plain resume (no signal delivered) still advances; downstream
        // sees that no signal arrived.
        let resumed = runtime.resume(&RunId::from("run-sot")).await.unwrap();
        assert_eq!(resumed.status, GraphStatus::Completed);
        assert_eq!(resumed.channels.get("viaSignal"), Some(&json!(false)));
    }

    #[tokio::test]
    async fn send_injects_a_queued_input_into_the_target_node() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("worker"),
            sync_handler(|s| {
                let injected = s.channels.get("__injected").cloned().unwrap_or(Value::Null);
                NodeOutput::update(upd(&[("processed", injected)]))
            }),
        );
        let def = graph(
            vec![node("worker", NodeType::Action)],
            vec![],
            "worker",
            vec![channel("processed", ChannelReducer::Replace)],
        );
        let runtime = GraphRuntime::new(def, nodes, InMemoryConditionRegistry::new());

        // Pre-queue an input for the worker, then run.
        runtime.send(
            &RunId::from("run-send"),
            &NodeId::from("worker"),
            json!({ "task": 1 }),
        );
        let state = runtime
            .start(RunId::from("run-send"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.channels.get("processed"), Some(&json!({ "task": 1 })));
        // The injected value is exposed to the handler only — never persisted.
        assert!(!state.channels.contains_key("__injected"));
    }

    #[tokio::test]
    async fn send_drains_the_inbox_fifo_across_a_cycle() {
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("worker"),
            sync_handler(|s| {
                let injected = s.channels.get("__injected").cloned().unwrap_or(Value::Null);
                let n = s.channels.get("n").and_then(|v| v.as_i64()).unwrap_or(0);
                NodeOutput::update(upd(&[("log", injected), ("n", json!(n + 1))]))
            }),
        );
        let mut conditions = InMemoryConditionRegistry::new();
        conditions.register(
            "more".to_owned(),
            Box::new(|s| s.channels.get("n").and_then(|v| v.as_i64()).unwrap_or(0) < 2),
        );
        let def = graph(
            vec![node("worker", NodeType::Action)],
            vec![edge(
                "e1",
                "worker",
                "worker",
                EdgeType::Conditional,
                Some("more"),
            )],
            "worker",
            vec![
                channel("log", ChannelReducer::Append),
                channel("n", ChannelReducer::Replace),
            ],
        );
        let runtime = GraphRuntime::new(def, nodes, conditions);

        // Two queued inputs, consumed FIFO across the self-loop.
        runtime.send(
            &RunId::from("run-fifo"),
            &NodeId::from("worker"),
            json!("first"),
        );
        runtime.send(
            &RunId::from("run-fifo"),
            &NodeId::from("worker"),
            json!("second"),
        );
        let state = runtime
            .start(RunId::from("run-fifo"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("log"), Some(&json!(["first", "second"])));
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
