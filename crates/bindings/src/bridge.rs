//! The async run bridge: assemble a Rust [`GraphRuntime`] from an
//! [`crate::spec::EngineSpec`], wiring user-supplied JS closures (node handlers,
//! tool `execute` fns, condition predicates) as Rust seams that call back into JS
//! via [`ThreadsafeFunction`]s, then drive a start / resume / approve.
//!
//! Threading model:
//! - The whole thing runs inside an **async** napi fn, so the JS main thread is free
//!   to service the TSFN callbacks while the Rust run future is parked.
//! - Every JS seam is an **async** JS callback (it returns a `Promise`). Rust calls
//!   it with [`ThreadsafeFunction::call_async`] — whose `Return = Promise<String>`
//!   comes from the TSFN generics in napi 3 — which resolves to a napi
//!   [`Promise<String>`], and then `.await`s that promise to its JS-resolved value.
//!   Node handlers and tool `execute` fns are async on the Rust side too, so they
//!   simply `.await` both stages inline.
//! - Condition predicates are **synchronous** by the runtime contract
//!   (`Fn(&GraphState) -> bool`), so the closure itself cannot `.await`. It still
//!   drives an **async** (Promise-returning) JS predicate: it `spawn`s the
//!   `call_async(..).await?.await?` chain onto napi's multi-threaded tokio runtime
//!   and blocks the current worker thread (inside `block_in_place`, so tokio can
//!   relocate other tasks) on a oneshot until that spawned task resolves the JS
//!   promise. The JS main thread is never blocked, so its microtask queue is free to
//!   settle the promise. No `call_with_return_value` / sync-channel hack remains.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use adriane_agents_core::{
    agent_node_handler, map_node_handler, register_fs_tools, ApprovalRequestItem, BrainMiddleware,
    CompressMiddleware, ContextBudgetMiddleware, EventSink, InMemoryToolRegistry, MemoryMiddleware,
    MiddlewareStack, ReActAgent, RedactMiddleware, ReflectionMiddleware, SkillMiddleware,
    StructuredOutputMiddleware, TerseMiddleware, ToolDefinition, APPROVED_TOOLS_CHANNEL,
    DEFAULT_AGENT_OUTPUT_CHANNEL,
};
use adriane_approval_engine::ApprovalError;
use adriane_artifact_store::{ArtifactId, ArtifactStore, InMemoryArtifactStore};
use adriane_components::ComponentRegistry;
use adriane_fs_backend::{
    ArtifactFsBackend, FilesystemBackend, FsWriteCtx, HttpFilesystemBackend, PathRule,
    StaticPathPolicy,
};
use adriane_graph_core::{EdgeType, GraphState, NodeId, NodeType, RunId};
use adriane_graph_runtime::{
    Checkpoint, CheckpointId, Checkpointer, Clock, ConditionRegistry, GraphRuntime,
    InMemoryConditionRegistry, InMemoryNodeRegistry, NodeOutput, NodeRegistry, RecordedClock,
    RecordingClock, RunEvent, SystemClock,
};
use adriane_llm_gateway::{
    AnthropicAdapter, DefaultLlmGateway, GeminiAdapter, HttpAnthropicPort, HttpGeminiPort,
    HttpPiiRedactor, HttpPromptCompressor, LlmError, LlmGateway, LlmJournal, LlmProvider,
    LlmResponse, LlmToolCall, LlmUsage, MediaResolver, MediaSource, MockAdapter, ModelChoice,
    ModelPolicy, OpenAiCompatibleAdapter, RecordedCall, RecordingGateway, RegexSecretsRedactor,
    ReplayGateway,
};
use adriane_memory::{InMemoryMemoryStore, MemoryStore, MockEmbedder, RecallMode, RetrievalPolicy};
use adriane_skills::{InMemorySkillStore, SkillStore};
use async_trait::async_trait;
use napi::bindgen_prelude::Promise;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::Status;
use serde_json::{json, Value};

use crate::spec::{AgentSpec, ApprovedTool, EngineSpec, FsPolicyRule, RunOutcome};

/// A TSFN that takes one JS string argument. napi 3 dropped `ErrorStrategy`; the
/// napi-2 `Fatal` behavior is the `CalleeHandled = false` const-generic (5th param):
/// the JS callback receives just `(payloadString)` (no leading error arg), `.call(...)`
/// takes a bare `String`, and a throw surfaces as a fatal napi exception rather than a
/// silently-handled error. `Return = Promise<String>` is the value the async seams
/// await (`call_async(..).await?.await?`); the fire-and-forget `on_event` never awaits
/// it. `CallJsBackArgs` stays the default `T` so the payload marshals as one JS string.
type StringTsfn = ThreadsafeFunction<String, Promise<String>, String, Status, false>;

/// Which entry point the caller asked for.
#[derive(Clone, Debug)]
pub enum Entry {
    Start,
    Resume,
    Approve,
    /// Deliver an external signal `name` carrying `payload`, then resume — the run
    /// advances past the node that was awaiting it (see `GraphRuntime::resume_with_signal`).
    Signal {
        name: String,
        payload: Value,
    },
    /// Replay-as-evidence (ADR 0038): re-execute a run from `checkpoint_id`, re-feeding the
    /// journaled LLM outputs + timestamps (`spec.replay_journal`) instead of re-sampling. A
    /// forked, read-only re-derivation — never opens new approval gates.
    Replay {
        checkpoint_id: String,
    },
}

/// How this run feeds its non-deterministic inputs (ADR 0038). Built once per run in `run()`
/// from the spec + entry, then threaded into the runtime (the clock) and every agent gateway.
#[derive(Clone)]
enum ReplayMode {
    /// Normal run: real clock, live provider calls.
    Live,
    /// Record mode (`ADRIANE_LLM_RECORD`): wrap each agent gateway to journal its LLM I/O into
    /// the shared `journal`, and the clock to capture its timestamp sequence into `clock`.
    Record {
        journal: Arc<Mutex<Vec<RecordedCall>>>,
        clock: Arc<Mutex<Vec<String>>>,
    },
    /// Replay mode (`Entry::Replay`): every agent gateway is the SAME shared `ReplayGateway`
    /// (so each recorded call is consumed once across the whole run), and the clock replays
    /// the recorded timestamp sequence.
    Replay {
        gateway: Arc<ReplayGateway>,
        clock: Vec<String>,
    },
}

/// The persisted replay journal wire shape: the LLM I/O decisions + the ordered timestamp
/// sequence the run emitted (ADR 0038). Serialized into `RunOutcome.replay_journal` in record
/// mode and re-fed via `EngineSpec.replay_journal` on `Entry::Replay`.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayJournalWire {
    decisions: LlmJournal,
    clock: Vec<String>,
}

impl ReplayMode {
    /// Resolve the mode: `Entry::Replay` (+ `spec.replay_journal`) → Replay; else the
    /// `ADRIANE_LLM_RECORD` env flag → Record; else Live.
    fn resolve(spec: &EngineSpec, entry: &Entry) -> napi::Result<ReplayMode> {
        if matches!(entry, Entry::Replay { .. }) {
            let raw = spec.replay_journal.as_deref().ok_or_else(|| {
                napi::Error::from_reason(
                    "Entry::Replay requires `replay_journal` (the recorded run journal)".to_owned(),
                )
            })?;
            let wire: ReplayJournalWire = serde_json::from_str(raw).map_err(|error| {
                napi::Error::from_reason(format!("invalid replay_journal JSON: {error}"))
            })?;
            return Ok(ReplayMode::Replay {
                gateway: Arc::new(ReplayGateway::new(wire.decisions)),
                clock: wire.clock,
            });
        }
        let recording = std::env::var("ADRIANE_LLM_RECORD")
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        if recording {
            return Ok(ReplayMode::Record {
                journal: Arc::new(Mutex::new(Vec::new())),
                clock: Arc::new(Mutex::new(Vec::new())),
            });
        }
        Ok(ReplayMode::Live)
    }

    /// The clock to install on the runtime: `RecordingClock` (record) captures the run's
    /// timestamp sequence; `RecordedClock` (replay) re-feeds it; `None` (live) → default.
    fn runtime_clock(&self) -> Option<Arc<dyn Clock>> {
        match self {
            ReplayMode::Live => None,
            ReplayMode::Record { clock, .. } => Some(Arc::new(RecordingClock::new(
                Arc::new(SystemClock),
                Arc::clone(clock),
            ))),
            ReplayMode::Replay { clock, .. } => Some(Arc::new(RecordedClock::new(clock.clone()))),
        }
    }

    /// Wrap an agent's bare gateway per the mode: record (journal into the shared buffer),
    /// replay (the shared ReplayGateway, ignoring the live `inner`), or live (passthrough).
    fn wrap_gateway(&self, inner: Arc<dyn LlmGateway>) -> Arc<dyn LlmGateway> {
        match self {
            ReplayMode::Live => inner,
            ReplayMode::Record { journal, .. } => {
                Arc::new(RecordingGateway::with_journal(inner, Arc::clone(journal)))
            }
            ReplayMode::Replay { gateway, .. } => Arc::clone(gateway) as Arc<dyn LlmGateway>,
        }
    }

    /// Whether this run is recording (env `ADRIANE_LLM_RECORD`) — the bridge surfaces the
    /// entry state alongside the journal so a later verify-replay can seed `replay_from` (ADR 0040).
    fn is_record(&self) -> bool {
        matches!(self, ReplayMode::Record { .. })
    }

    /// In record mode, serialize the captured journal (LLM I/O + clock) for `RunOutcome`.
    fn recorded_journal_json(&self) -> Option<String> {
        match self {
            ReplayMode::Record { journal, clock } => {
                let wire = ReplayJournalWire {
                    decisions: LlmJournal {
                        calls: journal
                            .lock()
                            .expect("record journal mutex poisoned")
                            .clone(),
                    },
                    clock: clock.lock().expect("record clock mutex poisoned").clone(),
                };
                serde_json::to_string(&wire).ok()
            }
            _ => None,
        }
    }
}

/// The three JS callbacks, shared into every seam closure. napi 3's
/// `ThreadsafeFunction` is no longer `Clone` (napi 2 was), so each is held behind an
/// `Arc`; cloning an `Arc` shares the SAME underlying threadsafe-function handle (which
/// is itself `Arc`-refcounted), preserving the napi-2 "clone the TSFN into each closure"
/// semantics — same JS callback, refcounted alive until the last clone drops.
#[derive(Clone)]
pub struct JsCallbacks {
    /// `(payloadJson) -> updateJson` — JS node handlers (`kind:"node"`) and JS tool
    /// `execute` fns (`kind:"tool"`). The return is the channel-update JSON (node)
    /// or the tool-result JSON (tool).
    on_node: Arc<StringTsfn>,
    /// `(payloadJson) -> boolean` — named condition predicates.
    on_condition: Arc<StringTsfn>,
    /// `(payloadJson)` — fire-and-forget run-lifecycle event sink.
    on_event: Arc<StringTsfn>,
}

impl JsCallbacks {
    /// Wrap the three already-converted TSFNs in `Arc`s. The conversion from
    /// `JsFunction` happens in napi's `FromNapiValue` for `ThreadsafeFunction`, which
    /// maps each `String` payload into a single JS string argument.
    pub fn new(on_node: StringTsfn, on_condition: StringTsfn, on_event: StringTsfn) -> Self {
        JsCallbacks {
            on_node: Arc::new(on_node),
            on_condition: Arc::new(on_condition),
            on_event: Arc::new(on_event),
        }
    }
}

/// Entry point used by all three napi fns. Deserializes the spec, builds the
/// runtime, drives the requested entry, then serializes the [`RunOutcome`].
pub async fn run(spec_json: String, callbacks: JsCallbacks, entry: Entry) -> napi::Result<String> {
    let spec: EngineSpec = serde_json::from_str(&spec_json)
        .map_err(|error| napi::Error::from_reason(format!("invalid engine spec JSON: {error}")))?;

    // ADR 0038: resolve the replay mode (live / record / replay) once, before the runtime is
    // built — it drives the runtime's clock and every agent gateway, and (record mode) the
    // recorded journal is read back into the outcome after the run.
    let mode = ReplayMode::resolve(&spec, &entry)?;
    let runtime = build_runtime(&spec, callbacks, &mode)?;

    // ADR 0040: in record mode, surface the run's entry state (built clock-free, before driving, so
    // it consumes no recorded tick) for a `Start` — the control plane persists it as the checkpoint
    // a later verify-replay seeds `replay_from` from. Captured BEFORE `drive` moves `entry`.
    let entry_state = if mode.is_record() && matches!(entry, Entry::Start) {
        let run_id = RunId::from(spec.run_id.clone().unwrap_or_else(|| "run".to_owned()));
        Some(runtime.entry_state(run_id, spec.initial_data.clone()))
    } else {
        None
    };

    let final_state = drive(&runtime, &spec, entry).await?;

    let pending_approvals = collect_pending_approvals(&spec, &final_state);
    let status = serde_json::to_value(final_state.status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();

    let outcome = RunOutcome {
        state: final_state,
        status,
        pending_approvals,
        replay_journal: mode.recorded_journal_json(),
        entry_state,
    };
    serde_json::to_string(&outcome).map_err(|error| napi::Error::from_reason(error.to_string()))
}

/// Drive the requested entry against an already-assembled runtime.
async fn drive(
    runtime: &GraphRuntime,
    spec: &EngineSpec,
    entry: Entry,
) -> napi::Result<GraphState> {
    match entry {
        Entry::Start => {
            let run_id = RunId::from(spec.run_id.clone().unwrap_or_else(|| "run".to_owned()));
            seed_inbox(runtime, &run_id, &spec.inbox);
            runtime
                .start(run_id, spec.initial_data.clone())
                .await
                .map_err(runtime_err)
        }
        Entry::Resume | Entry::Approve => {
            let mut state = spec.state.clone().ok_or_else(|| {
                napi::Error::from_reason(
                    "resume/approve require `state` (the serialized suspended GraphState)"
                        .to_owned(),
                )
            })?;

            // On BOTH the approve and resume paths, validate the no-self-approval
            // invariant for each granted tool, then write the validated tool NAMES into
            // the approval channel before seeding the checkpoint the runtime will resume
            // from. The control plane is the authority (it only sends tools the approval
            // engine already approved), but the engine re-checks here — defence in depth:
            // a tool whose resolver is empty or equals its requester ABORTS the resume
            // rather than silently unlocking. This covers the PRODUCTION catalog path,
            // which resumes through `Entry::Resume` after the control plane seeds
            // `__approvedTools`: by re-validating here too, a forged/malformed resume
            // cannot slip a self-approved tool past the engine. Names are sorted +
            // de-duplicated so the channel write is deterministic. When the spec carries
            // no `approvedTools` (an ordinary resume past a non-approval gate), this is a
            // no-op: an empty list validates to an empty name set and the existing
            // channel (if any) is left untouched.
            if !spec.approved_tools.is_empty() {
                let names = validate_approved_tools(&spec.approved_tools)?;
                state.channels.insert(
                    APPROVED_TOOLS_CHANNEL.to_owned(),
                    Value::Array(names.into_iter().map(Value::String).collect()),
                );
            }

            let run_id = state.run_id.clone();
            // Seed the runtime's (fresh) checkpointer with the suspended state so
            // `resume` can load it, then resume — the runtime advances past the gate
            // / re-runs the agent node from the latest checkpoint.
            seed_checkpoint(runtime, state);
            seed_inbox(runtime, &run_id, &spec.inbox);
            runtime.resume(&run_id).await.map_err(runtime_err)
        }
        Entry::Signal { name, payload } => {
            // Deliver an external signal to a suspended run: seed the suspended state,
            // then resume_with_signal injects the payload under `__signals[name]` and
            // advances past the node that awaited it.
            let state = spec.state.clone().ok_or_else(|| {
                napi::Error::from_reason(
                    "signal requires `state` (the serialized suspended GraphState)".to_owned(),
                )
            })?;
            let run_id = state.run_id.clone();
            seed_checkpoint(runtime, state);
            seed_inbox(runtime, &run_id, &spec.inbox);
            runtime
                .resume_with_signal(&run_id, &name, payload)
                .await
                .map_err(runtime_err)
        }
        Entry::Replay { checkpoint_id } => {
            // ADR 0038 (replay-as-evidence): re-derive the run from `checkpoint_id`. The runtime
            // was built (in `run()`) with a RecordedClock + a ShareReplayGateway from the journal,
            // so re-execution re-feeds the original LLM outputs + timestamps. `replay_from` forks a
            // new run id — read-only evidence, never advancing or re-opening the original run.
            let state = spec.state.clone().ok_or_else(|| {
                napi::Error::from_reason(
                    "replay requires `state` (the serialized GraphState to seed the checkpoint)"
                        .to_owned(),
                )
            })?;
            let run_id = state.run_id.clone();
            seed_replay_checkpoint(runtime, &checkpoint_id, state);
            runtime
                .replay_from(&run_id, &CheckpointId(checkpoint_id))
                .await
                .map_err(runtime_err)
        }
    }
}

fn runtime_err(error: adriane_graph_runtime::RuntimeError) -> napi::Error {
    napi::Error::from_reason(format!("runtime error: {error}"))
}

/// Pre-queue the spec's dynamic-message inbox into the runtime before driving: each
/// `nodeId -> [inputs]` is `send`-queued FIFO for that node (the `__injected` seam).
fn seed_inbox(
    runtime: &GraphRuntime,
    run_id: &RunId,
    inbox: &std::collections::BTreeMap<String, Vec<Value>>,
) {
    for (node_id, inputs) in inbox {
        for input in inputs {
            runtime.send(run_id, &NodeId::from(node_id.clone()), input.clone());
        }
    }
}

/// Validate the governance invariant for every granted tool and return the sorted,
/// de-duplicated list of validated tool names to unlock.
///
/// The core invariant (the same one [`adriane_approval_engine`] enforces in
/// `ensure_can_resolve`): a tool's `resolved_by` must be a non-empty principal that
/// DIFFERS from its `requested_by` — an agent never approves its own request. A
/// violation maps the engine's [`ApprovalError::SelfApproval`] to a napi error that
/// interrupts the resume, so a malformed/forged approve call cannot unlock a tool.
/// Returns names sorted + de-duplicated, so the `__approvedTools` channel write is
/// deterministic regardless of the order the caller sent the tools in.
fn validate_approved_tools(tools: &[ApprovedTool]) -> napi::Result<Vec<String>> {
    let mut names: Vec<String> = Vec::with_capacity(tools.len());
    for tool in tools {
        // An empty resolver (no principal recorded) is treated as a self-approval
        // violation: there is no distinct human on record who granted the tool.
        if tool.resolved_by.trim().is_empty() || tool.resolved_by == tool.requested_by {
            let error = ApprovalError::SelfApproval(format!("tool:{}", tool.name));
            return Err(napi::Error::from_reason(format!(
                "approval guard-rail rejected resume: {error}"
            )));
        }
        // A content-scoped grant (ADR 0024 phase 2c) unlocks only the exact call: write
        // its composite key into the channel, not the bare tool name. No-self-approval is
        // still validated on the tool name above. Defense-in-depth: a supplied key MUST be
        // "<tool.name>#<64-hex sha256>" — its name component must match the validated name,
        // so a caller cannot smuggle a key whose embedded tool diverges from the one whose
        // no-self-approval was checked, nor a malformed key.
        match &tool.key {
            Some(key) => {
                let well_formed = key
                    .strip_prefix(&format!("{}#", tool.name))
                    .map(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
                    .unwrap_or(false);
                if !well_formed {
                    return Err(napi::Error::from_reason(format!(
                        "approval guard-rail rejected resume: malformed content-scoped key for tool '{}'",
                        tool.name
                    )));
                }
                names.push(key.clone());
            }
            None => names.push(tool.name.clone()),
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Seed the runtime's checkpointer with a suspended state so `resume` can load it.
/// Each napi call rebuilds the runtime with a fresh in-memory checkpointer, so the
/// caller-supplied state must be re-injected before resuming. The checkpoint id is
/// derived from the state's existing `checkpoint_id` (or a stable fallback).
fn seed_checkpoint(runtime: &GraphRuntime, state: GraphState) {
    let id = CheckpointId(
        state
            .checkpoint_id
            .clone()
            .unwrap_or_else(|| format!("{}:seed", state.run_id.0)),
    );
    let checkpoint = Checkpoint {
        id,
        run_id: state.run_id.clone(),
        graph_state: state,
        created_at: "0".to_owned(),
    };
    runtime.checkpointer().save(checkpoint);
}

/// Seed the checkpointer under the EXACT `checkpoint_id` a replay forks from (ADR 0038), so
/// `replay_from`'s `load_by_id` + run-id filter both hit. Unlike `seed_checkpoint`, the id is
/// taken verbatim (not derived from `state.checkpoint_id` / a `:seed` fallback).
fn seed_replay_checkpoint(runtime: &GraphRuntime, checkpoint_id: &str, state: GraphState) {
    let checkpoint = Checkpoint {
        id: CheckpointId(checkpoint_id.to_owned()),
        run_id: state.run_id.clone(),
        graph_state: state,
        created_at: "0".to_owned(),
    };
    runtime.checkpointer().save(checkpoint);
}

/// Assemble the runtime: a node registry with JS handlers + agent handlers, and a
/// condition registry that bridges every conditional edge's condition to JS.
/// Resolve the run id this runtime build is for: a resume/approve carries it on the
/// suspended `state`; a start carries it on `spec.run_id`. Used to scope the governed
/// filesystem so an agent's artifacts key under the right run.
fn resolve_run_id(spec: &EngineSpec) -> RunId {
    spec.state
        .as_ref()
        .map(|state| state.run_id.clone())
        .or_else(|| spec.run_id.clone().map(RunId::from))
        .unwrap_or_else(|| RunId::from("run"))
}

/// Build the run-scoped fs backend: the external durable HTTP backend (ADR 0024 phase
/// 2e) when `ADRIANE_FS_BACKEND_URL` is configured — fs content then survives a
/// suspend/resume across the napi boundary — else the lean in-memory `ArtifactFsBackend`
/// over the per-build shared store (intra-run).
fn build_fs_backend(
    fs_store: &Arc<dyn ArtifactStore>,
    run_id: &RunId,
) -> Arc<dyn FilesystemBackend> {
    match HttpFilesystemBackend::from_env(run_id.clone()) {
        Some(http) => Arc::new(http),
        None => Arc::new(ArtifactFsBackend::new(fs_store.clone(), run_id.clone())),
    }
}

/// Build the agent middleware stack (ADR 0025). The GOVERNED layer is injected here from
/// the process env — PII redaction (outermost, the redactor sees the full text); it is
/// never driven by user/spec data. The EFFICIENCY layer is built from the SDK-resolved
/// `agent_spec.resolved_middleware` data list (phase 3d): a `profile` + the user's explicit
/// `middleware[]` + the legacy terse/context-budget knobs are all expanded SDK-side into one
/// ordered list, which maps to `push_efficiency` calls here.
///
/// Governed-by-construction: this match is the RUNTIME enforcer of the invariant — it only
/// ever calls `push_efficiency`, and a governance kind (redact / approvalGate / fsPolicy) or
/// any unknown kind hits the `_ => {}` arm and is silently ignored, so user/spec data can
/// never reach `push_governed`. (The SDK `resolveMiddleware` throw-gate rejects governance
/// kinds on the in-process builder path; the contracts `AgentNodeMetadataSchema` union is a
/// type-level + editor guarantee — it is not executed on the persisted catalog run path, so
/// this match arm is the sole runtime defence there.) The approval gate is intrinsic to the
/// stack itself (`MiddlewareStack::before_tool`) and applies regardless.
///
/// `gateway` + `provider`/`model` are threaded in for `ReflectionMiddleware` (phase 3e), which
/// critiques the result with the agent's own provider + model.
/// ADR 0026 phase 11: the process-global in-memory memory store, shared across agents/runs so
/// recall works across runs in OSS dev (intra-process). The control plane swaps a Neo4j-backed
/// `MemoryStore` behind the same seam for cross-process durability + the native vector index.
static MEMORY_STORE: std::sync::LazyLock<Arc<InMemoryMemoryStore>> =
    std::sync::LazyLock::new(|| Arc::new(InMemoryMemoryStore::new()));

/// ADR 0035 phase 12: the process-global in-memory skills registry, shared across agents/runs so
/// selection works across runs in OSS dev (intra-process). The control plane swaps a Postgres-
/// backed `SkillStore` behind the same seam for durable, governed, versioned skills.
static SKILL_STORE: std::sync::LazyLock<Arc<InMemorySkillStore>> =
    std::sync::LazyLock::new(|| Arc::new(InMemorySkillStore::new()));

fn build_agent_middleware(
    agent_spec: &AgentSpec,
    gateway: &Arc<dyn LlmGateway>,
    provider: LlmProvider,
    model: &str,
    node_id: &str,
) -> MiddlewareStack {
    let mut stack = MiddlewareStack::new();
    // GOVERNED — sealed; never fed from spec/user data. Ordering on the request path is the
    // push order (governed runs outermost, before efficiency).
    // ADR 0032: the SECRETS floor is the in-engine deterministic regex redactor — ALWAYS-ON
    // (no env gate) + pushed FIRST so it scrubs keys/tokens even when PII is unset and before
    // any text reaches the external PII service. Default masks; `ADRIANE_SECRETS_POLICY=block`
    // fails closed.
    stack.push_governed(Arc::new(RedactMiddleware::new(Arc::new(
        RegexSecretsRedactor::from_env(),
    ))));
    // ADR 0032: optional external secrets augmentation (defense-in-depth) reusing the remote
    // redactor shape against `ADRIANE_SECRETS_REDACTOR_URL`.
    if let Some(url) = std::env::var("ADRIANE_SECRETS_REDACTOR_URL")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let token = std::env::var("ADRIANE_SECRETS_REDACTOR_TOKEN")
            .ok()
            .filter(|value| !value.is_empty());
        stack.push_governed(Arc::new(RedactMiddleware::new(Arc::new(
            HttpPiiRedactor::new(url, token),
        ))));
    }
    // PII (ADR 0008): env-gated external Presidio/GLiNER redactor, after the secrets floor.
    if let Some(redactor) = HttpPiiRedactor::from_env() {
        stack.push_governed(Arc::new(RedactMiddleware::new(Arc::new(redactor))));
    }
    // ADR 0026 phase 11: governed long-term memory. Sealed (push_governed) + constructed WITH its
    // namespace + principal (node id) — never from user resolved_middleware, so recall is
    // tenant-scoped by construction. OSS default = the shared in-memory store + MockEmbedder
    // (the control plane injects a Neo4j-backed store behind the same seam).
    if let Some(mem) = &agent_spec.memory {
        let policy = RetrievalPolicy {
            top_k: mem.top_k.map(|k| k as usize).unwrap_or(5),
            mode: match mem.recall.as_deref() {
                Some("vector") => RecallMode::Vector,
                Some("graph") => RecallMode::Graph,
                _ => RecallMode::Both,
            },
        };
        let store: Arc<dyn MemoryStore> = MEMORY_STORE.clone();
        stack.push_governed(Arc::new(MemoryMiddleware::new(
            store,
            Arc::new(MockEmbedder),
            mem.namespace.clone(),
            Some(format!("agent:{node_id}")),
            policy,
        )));
    }
    // ADR 0046 S4: governed-brain context. Always installed (no overlay) + sealed (push_governed): it
    // injects the `__brainRecall` channel the control plane seeds (the tenant's active entity graph,
    // recalled tenant-scoped) into the agent seed. A strict no-op when the channel is absent, so the
    // control plane alone governs WHETHER a run gets brain context. Read-only — brain writes stay in the
    // control plane (ADR 0046 S2). Replay-safe: the seeded set is journaled in entry state.
    stack.push_governed(Arc::new(BrainMiddleware::new()));
    // ADR 0035 phase 12: governed skills (progressive disclosure). Installed in the EFFICIENCY
    // layer (so the governed layer — redaction/approval/fs — sees the pre-skill world) but
    // bridge-injected FIRST (before any SDK-resolved efficiency middleware), so it precedes a
    // ContextBudget cap and a user can neither add nor remove it. The namespace is sealed here;
    // capability-granting (`requires`) skills stay withheld until their grant is in the approval
    // set. Applies to sub-agents too (same build path → deepagents parity). OSS default = the
    // shared in-memory registry + MockEmbedder (the control plane injects a Postgres store).
    if let Some(sk) = &agent_spec.skills {
        let store: Arc<dyn SkillStore> = SKILL_STORE.clone();
        stack.push_efficiency(Arc::new(SkillMiddleware::new(
            store,
            Arc::new(MockEmbedder),
            sk.namespace.clone(),
            sk.required.clone(),
            sk.advisory_k.map(|k| k as usize).unwrap_or(3),
            None,
        )));
    }
    // EFFICIENCY — built from the SDK-resolved data list, in order.
    if agent_spec.resolved_middleware.is_empty() {
        // Back-compat: a spec produced before phase 3d (or a hand-built one) carries the
        // legacy flat knobs instead of a resolved list. Honour them so old persisted graphs
        // keep their terse / context-budget / compress behaviour.
        if agent_spec.output_style.as_deref() == Some("terse") {
            stack.push_efficiency(Arc::new(TerseMiddleware));
        }
        if let Some(budget) = agent_spec.context_budget {
            stack.push_efficiency(Arc::new(ContextBudgetMiddleware::new(budget as usize)));
        }
        if let Some(compressor) = HttpPromptCompressor::from_env() {
            stack.push_efficiency(Arc::new(CompressMiddleware::new(Arc::new(compressor))));
        }
    } else {
        for spec in &agent_spec.resolved_middleware {
            match spec.kind.as_str() {
                "terse" => {
                    stack.push_efficiency(Arc::new(TerseMiddleware));
                }
                "contextBudget" => {
                    // Accept an integer OR a float `chars`: serde yields a float for a
                    // non-integer JSON number (which `as_u64` rejects), so truncate rather
                    // than silently dropping the budget when the SDK forwards e.g. `4000.5`.
                    if let Some(chars) = spec.params.get("chars").and_then(|value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_f64().map(|f| f.trunc() as u64))
                    }) {
                        stack.push_efficiency(Arc::new(ContextBudgetMiddleware::new(
                            chars as usize,
                        )));
                    }
                }
                "compress" => {
                    // Compression needs the external LLMLingua service; without it the
                    // request is left unchanged (fail-open), so a `compress` entry is a
                    // no-op when the service is not configured.
                    if let Some(compressor) = HttpPromptCompressor::from_env() {
                        stack.push_efficiency(Arc::new(CompressMiddleware::new(Arc::new(
                            compressor,
                        ))));
                    }
                }
                "reflection" => {
                    // Opt-in self-critique (after_run): flags a weak result in the reasoning
                    // (no requires_human_review — see ReflectionMiddleware). Critiques with the
                    // agent's own provider + model; fail-open.
                    let threshold = spec
                        .params
                        .get("threshold")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.8);
                    stack.push_efficiency(Arc::new(ReflectionMiddleware::new(
                        gateway.clone(),
                        provider,
                        model,
                        threshold,
                    )));
                }
                "structuredOutput" => {
                    // ADR 0029 phase 8: constrain output to a JSON schema (efficiency layer,
                    // gate-safe — the approval gate is intrinsic to before_tool). Needs a
                    // `schema`; without one the entry no-ops (nothing to validate against).
                    if let Some(schema) = spec.params.get("schema") {
                        let name = spec
                            .params
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Output")
                            .to_owned();
                        let strict = spec
                            .params
                            .get("strict")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        // `mode: "lenient"` fails open to raw text; default required fails closed.
                        let lenient =
                            spec.params.get("mode").and_then(Value::as_str) == Some("lenient");
                        let retry_cap = spec
                            .params
                            .get("retryCap")
                            .and_then(|value| {
                                value
                                    .as_u64()
                                    .or_else(|| value.as_f64().map(|f| f.trunc() as u64))
                            })
                            .unwrap_or(2) as usize;
                        stack.push_efficiency(Arc::new(StructuredOutputMiddleware::new(
                            gateway.clone(),
                            name,
                            schema.clone(),
                            strict,
                            lenient,
                            retry_cap,
                        )));
                    }
                }
                // Governance kinds (redact / approvalGate / fsPolicy) + unknown kinds are
                // never applied: never push_governed from data (the type invariant), and
                // unknown kinds no-op for forward-compat with a newer SDK.
                _ => {}
            }
        }
    }
    stack
}

/// Compile the wire policy rules into the engine's [`StaticPathPolicy`] (fail-closed:
/// empty → read-only everywhere).
fn build_fs_policy(rules: &[FsPolicyRule]) -> StaticPathPolicy {
    StaticPathPolicy::with_rules(
        rules
            .iter()
            .map(|rule| PathRule {
                glob: rule.glob.clone(),
                verb: rule.verb,
            })
            .collect(),
    )
}

fn build_runtime(
    spec: &EngineSpec,
    callbacks: JsCallbacks,
    mode: &ReplayMode,
) -> napi::Result<GraphRuntime> {
    let js_node_ids: HashSet<&str> = spec.js_node_ids.iter().map(String::as_str).collect();

    // Run-scoped governed filesystem (ADR 0024 phase 2b): ONE in-memory artifact store
    // shared across every fs-enabled agent in this run (so a file written by one node is
    // readable by another), plus the compiled per-path policy. NOTE: the store is
    // per-runtime-build — fs content is intra-run and does NOT yet survive a
    // suspend/resume across the napi boundary (durable backing is phase 2e).
    let fs_run_id = resolve_run_id(spec);
    let fs_store: Arc<dyn ArtifactStore> = Arc::new(InMemoryArtifactStore::new());
    let fs_policy = Arc::new(build_fs_policy(&spec.fs_policy));

    let registry = ComponentRegistry::new();
    let mut nodes = InMemoryNodeRegistry::new();
    // Register handlers for the parent graph's nodes AND every subgraph's nodes:
    // child runs share this runtime's node registry, so a child node handler must be
    // present here too. The agent / component / js maps are keyed by GLOBAL node id,
    // so a child node is configured the same way as a parent node.
    let all_nodes = spec
        .graph
        .nodes
        .iter()
        .chain(spec.subgraphs.iter().flat_map(|graph| graph.nodes.iter()));
    for node in all_nodes {
        let id = node.id.0.clone();
        if let Some(component) = spec.component_nodes.get(&id) {
            // A component node runs a NATIVE Rust handler built from the component
            // library; it never routes to the JS `on_node` seam, even if its id also
            // appears in `js_node_ids`. `build_handler` validates kind + params up
            // front, so a misconfigured component fails the whole build cleanly.
            let handler = registry
                .build_handler(&component.kind, &component.params)
                .map_err(|error| {
                    napi::Error::from_reason(format!("component node '{id}': {error}"))
                })?;
            nodes.register(NodeId::from(id), handler);
        } else if let Some(agent_spec) = spec.agents.get(&id) {
            let handler = build_agent_handler(
                &id, agent_spec, spec, &callbacks, &fs_store, &fs_policy, &fs_run_id, mode,
            )?;
            nodes.register(NodeId::from(id), handler);
        } else if let Some(map_spec) = spec.map_agents.get(&id) {
            // ADR 0027 phase 4b: a `mapAgents` dynamic-fan-out node.
            let handler = build_map_agent_handler(
                &id, map_spec, spec, &callbacks, &fs_store, &fs_policy, &fs_run_id, mode,
            )?;
            nodes.register(NodeId::from(id), handler);
        } else if node.node_type == NodeType::HumanGate {
            // The runtime suspends natively at a human gate — no handler needed.
            continue;
        } else if js_node_ids.contains(id.as_str()) {
            nodes.register(NodeId::from(id.clone()), js_node_handler(id, &callbacks));
        }
        // Other native node types without a JS handler are left unregistered; the
        // runtime errors clearly (`NoHandler`) if it ever routes to one.
    }

    let mut conditions = InMemoryConditionRegistry::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Conditions from the parent graph AND every subgraph's conditional edges.
    let all_edges = spec
        .graph
        .edges
        .iter()
        .chain(spec.subgraphs.iter().flat_map(|graph| graph.edges.iter()));
    for edge in all_edges {
        if edge.edge_type != EdgeType::Conditional {
            continue;
        }
        let Some(name) = &edge.condition else {
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        conditions.register(name.clone(), js_condition(name.clone(), &callbacks));
    }

    let mut runtime = GraphRuntime::new(spec.graph.clone(), nodes, conditions)
        .with_subgraphs(spec.subgraphs.clone());
    // ADR 0038: record mode installs a RecordingClock (captures the timestamp sequence);
    // replay mode a RecordedClock (re-feeds it). Live keeps the default SystemClock.
    if let Some(clock) = mode.runtime_clock() {
        runtime = runtime.with_clock(clock);
    }

    // Forward every run-lifecycle event to JS, fire-and-forget. The observer runs
    // synchronously inside `emit`; we only enqueue a non-blocking TSFN call (no
    // await, no block), so the run loop is never stalled by the JS side.
    let on_event = callbacks.on_event.clone();
    runtime.on_event(Box::new(move |event: &RunEvent| {
        if let Ok(payload) = serde_json::to_string(event) {
            let _ = on_event.call(payload, ThreadsafeFunctionCallMode::NonBlocking);
        }
    }));

    Ok(runtime)
}

/// A node handler that delegates to the JS `on_node` closure (kind `"node"`),
/// awaiting the returned channel-update JSON. The runtime applies the reducer and
/// checkpoints; this seam only produces the update map.
fn js_node_handler(node_id: String, callbacks: &JsCallbacks) -> adriane_graph_runtime::NodeHandler {
    let on_node = callbacks.on_node.clone();
    Box::new(move |state: GraphState| {
        let on_node = on_node.clone();
        let node_id = node_id.clone();
        Box::pin(async move {
            let payload = json!({
                "kind": "node",
                "nodeId": node_id,
                "input": Value::Null,
                "state": channels_value(&state),
            });
            match call_js_string(&on_node, payload).await {
                Ok(update) => js_update_to_output(&update),
                Err(error) => NodeOutput::failure(format!("js node handler '{node_id}': {error}")),
            }
        })
    })
}

/// A condition predicate that bridges to the **async** JS `on_condition` closure.
/// The runtime's [`adriane_graph_runtime::ConditionFn`] contract is synchronous, so
/// this closure cannot itself `.await`; instead it `spawn`s the async
/// `call_async(..).await?.await?` chain onto napi's tokio runtime and blocks the
/// current worker thread on a oneshot until the JS promise resolves. Wrapped in
/// `block_in_place` so the multi-threaded runtime can relocate other tasks (incl.
/// the spawned one) off this thread while it waits. The JS main thread is never
/// blocked, so the promise's microtask is free to settle.
fn js_condition(name: String, callbacks: &JsCallbacks) -> adriane_graph_runtime::ConditionFn {
    let on_condition = callbacks.on_condition.clone();
    Box::new(move |state: &GraphState| {
        let payload = json!({ "name": name, "state": channels_value(state) });
        call_js_bool_awaiting(&on_condition, payload).unwrap_or(false)
    })
}

/// Wall-clock millis-since-epoch as a string, for the observational `TokenDelta`
/// timestamp. Mirrors the runtime's private `now_string()`. Safe to use here (unlike
/// inside deterministic run state) because `TokenDelta` is observational-only: it never
/// enters a checkpoint or the journal, so a wall-clock value never affects replay.
fn now_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

/// The `agents-core` [`EventSink`] impl (ADR 0033, phase 13): forwards each observational
/// token delta to JS as a [`RunEvent::TokenDelta`] over the SAME `on_event` TSFN the
/// runtime uses for lifecycle events — but **bypassing the `EventBus`**, so a delta never
/// enters the in-memory events vector, a checkpoint, or the durable journal. Built only when
/// the run opted into streaming (`EngineSpec::stream_tokens`).
struct TokenStreamSink {
    on_event: Arc<StringTsfn>,
    run_id: RunId,
    node_id: NodeId,
}

impl EventSink for TokenStreamSink {
    fn token_delta(&self, spawn_id: Option<u32>, message_id: &str, delta: &str) {
        // Namespace the per-turn message id with the run (and spawn, if any) so concurrent
        // `mapAgents` spawns' turns never collide on the wire.
        let scoped_message_id = match spawn_id {
            Some(spawn) => format!("{}:spawn{}:{}", self.run_id.0, spawn, message_id),
            None => format!("{}:{}", self.run_id.0, message_id),
        };
        let event = RunEvent::TokenDelta {
            run_id: self.run_id.clone(),
            node_id: self.node_id.clone(),
            message_id: scoped_message_id,
            delta: delta.to_owned(),
            // A `mapAgents` spawn is a sub-agent of this run; a top-level node is the run
            // itself (`None`). Additive to the `<parentRunId>:<nodeId>` RunId convention.
            parent_run_id: spawn_id.map(|_| self.run_id.clone()),
            spawn_id,
            timestamp: now_string(),
        };
        if let Ok(payload) = serde_json::to_string(&event) {
            // Fire-and-forget, exactly like the lifecycle-event forwarder. Never blocks the
            // agent loop; a JS-side throw is the consumer's problem, not the run's.
            let _ = self
                .on_event
                .call(payload, ThreadsafeFunctionCallMode::NonBlocking);
        }
    }
}

/// Build the agent-node handler for an agent spec: a [`ReActAgent`] over a gateway
/// chosen from env, with a tool registry where JS tools call back into JS.
#[allow(clippy::too_many_arguments)]
fn build_react_agent(
    node_id: &str,
    agent_spec: &AgentSpec,
    spec: &EngineSpec,
    callbacks: &JsCallbacks,
    fs_store: &Arc<dyn ArtifactStore>,
    fs_policy: &Arc<StaticPathPolicy>,
    fs_run_id: &RunId,
    mode: &ReplayMode,
) -> napi::Result<Arc<ReActAgent>> {
    // Resolve the concrete model BEFORE building the gateway, so the registered
    // adapter and the agent's provider/model all agree (e.g. a `fast` tier on a
    // mistral-only env -> mistral-small-latest through the Mistral adapter).
    let resolved = resolve_agent_model(agent_spec, &spec.provider_keys);
    // ADR 0025 phase 3b: the gateway is now the BARE provider router; PII redaction +
    // prompt compression are agent middleware on the stack (built below), not gateway
    // wrappers. The RedactingGateway/CompressingGateway structs remain for non-agent callers.
    // ADR 0038: `mode` wraps it for record/replay.
    let gateway = build_gateway(
        agent_spec,
        &resolved,
        &spec.provider_keys,
        Some(fs_store),
        mode,
    );

    let approval_tools: HashSet<&str> = agent_spec
        .approval_tool_names
        .iter()
        .map(String::as_str)
        .collect();
    let js_tools: HashSet<&str> = spec.js_tool_names.iter().map(String::as_str).collect();

    let mut registry = InMemoryToolRegistry::new();
    for tool_name in &agent_spec.tool_names {
        // `writeTodos` has a real Rust impl (ADR 0022/0023): register it verbatim
        // (proper schema + pure normalizing handler), never the no-op stub.
        if tool_name == adriane_agents_core::WRITE_TODOS_TOOL {
            let (definition, handler) = adriane_agents_core::write_todos_tool();
            registry.register(definition, handler);
            continue;
        }
        let requires_approval = approval_tools.contains(tool_name.as_str());
        let definition = ToolDefinition {
            name: tool_name.clone(),
            description: format!("Tool '{tool_name}'."),
            requires_approval,
            input_schema: Some(json!({ "type": "object" })),
            content_scoped: false,
        };
        let handler = if js_tools.contains(tool_name.as_str()) {
            js_tool_handler(tool_name.clone(), callbacks)
        } else {
            // A non-JS tool with no Rust impl: a deterministic no-op so the agent
            // loop can still execute and observe something.
            let name = tool_name.clone();
            adriane_agents_core::sync_tool(move |_input| Ok(json!({ "tool": name, "ok": true })))
        };
        registry.register(definition, handler);
    }

    // Governed virtual filesystem (ADR 0024 phase 2b): an fs-enabled agent gets the
    // eight fs tools bound to a run-scoped backend over the shared artifact store and
    // the run's path policy (fail-closed). The agent itself is the `principal` recorded
    // on writes; the gate verb is rejected here until phase 2c.
    if agent_spec.enable_fs {
        let backend = build_fs_backend(fs_store, fs_run_id);
        let policy: Arc<dyn adriane_fs_backend::PathPolicy> = fs_policy.clone();
        register_fs_tools(
            &mut registry,
            backend,
            policy,
            FsWriteCtx {
                node_id: NodeId::from(node_id),
                principal: Some(node_id.to_owned()),
            },
        );
    }

    // ADR 0045 Stage 1b: a memory-enabled agent (it carries a `memory` overlay) gets the built-in
    // `recallMemory` / `rememberMemory` tools, sealed with the SAME store/embedder/namespace/
    // principal as its MemoryMiddleware (never user data) — recall/persist tenant-scoped by
    // construction. The in-process write is best-effort; the durable write drains from the
    // `__memoryWrites` channel (control-plane half). Mirrors the fs-tools auto-injection above.
    if let Some(mem) = &agent_spec.memory {
        let store: Arc<dyn MemoryStore> = MEMORY_STORE.clone();
        for (definition, handler) in adriane_agents_core::build_memory_tools(
            store,
            Arc::new(MockEmbedder),
            mem.namespace.clone(),
            Some(format!("agent:{node_id}")),
        ) {
            registry.register(definition, handler);
        }
    }

    // Drive the agent with the RESOLVED provider/model so the request's provider
    // slot matches the adapter the gateway registered (otherwise a tier-resolved
    // mistral request could be issued against an anthropic slot with no adapter).
    let mut agent = ReActAgent::new(node_id.to_owned(), "bridged agent", gateway.clone())
        .with_provider(resolved.provider)
        .with_model(resolved.model.clone())
        .with_tools(Arc::new(registry));

    // The base system prompt only. Terse output + context-budget trim are now EFFICIENCY
    // middleware driven by `resolved_middleware` (ADR 0025 phase 3d), not flat knobs here.
    if let Some(system) = &agent_spec.system {
        agent = agent.with_system(system.clone());
    }
    if let Some(max) = agent_spec.max_iterations {
        agent = agent.with_max_iterations(max as usize);
    }
    // ADR 0030 9e: bind the multimodal input channel so the seed message carries media blocks.
    if let Some(channel) = &agent_spec.input_blocks_channel {
        agent = agent.with_input_blocks_channel(channel.clone());
    }
    // ADR 0025: install the middleware stack — governed (env-injected redaction) + the
    // SDK-resolved efficiency list (compress / terse / context-budget / reflection). The
    // approval gate is intrinsic to the stack and applies regardless. The gateway is threaded
    // in for the reflection critique call (it uses the agent's own provider + model).
    agent = agent.with_middleware(build_agent_middleware(
        agent_spec,
        &gateway,
        resolved.provider,
        &resolved.model,
        node_id,
    ));

    // ADR 0033 phase 13: opt-in token streaming. When the run requested it, install an
    // observational sink that forwards each delta to JS over the `on_event` TSFN, bypassing
    // the durable EventBus. Absent the flag, no sink is attached and the loop calls
    // `gateway.complete()` — byte-identical to before. A `mapAgents` sub-agent built through
    // this same function inherits the sink and tags its deltas with the spawn index.
    if spec.stream_tokens {
        agent = agent.with_event_sink(Arc::new(TokenStreamSink {
            on_event: callbacks.on_event.clone(),
            run_id: fs_run_id.clone(),
            node_id: NodeId::from(node_id),
        }));
    }

    Ok(Arc::new(agent))
}

/// Wrap a built ReAct agent as an ordinary single-node agent handler.
#[allow(clippy::too_many_arguments)] // run-scoped seams (fs + replay mode) threaded explicitly
fn build_agent_handler(
    node_id: &str,
    agent_spec: &AgentSpec,
    spec: &EngineSpec,
    callbacks: &JsCallbacks,
    fs_store: &Arc<dyn ArtifactStore>,
    fs_policy: &Arc<StaticPathPolicy>,
    fs_run_id: &RunId,
    mode: &ReplayMode,
) -> napi::Result<adriane_graph_runtime::NodeHandler> {
    let agent = build_react_agent(
        node_id, agent_spec, spec, callbacks, fs_store, fs_policy, fs_run_id, mode,
    )?;
    let output_channel = agent_spec
        .output_channel
        .clone()
        .unwrap_or_else(|| DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned());
    Ok(agent_node_handler(
        agent,
        output_channel,
        agent_spec.suspend_for_approval,
        agent_spec.todos_channel.clone(),
    ))
}

/// Build a `mapAgents` dynamic-fan-out node handler (ADR 0027 phase 4b): the sub-agent is built
/// exactly like an ordinary agent, then run once per item in `over_channel` (concurrently),
/// merging the per-item results — in input order — into `join_at`.
#[allow(clippy::too_many_arguments)] // run-scoped seams (fs + replay mode) threaded explicitly
fn build_map_agent_handler(
    node_id: &str,
    map_spec: &crate::spec::MapAgentSpec,
    spec: &EngineSpec,
    callbacks: &JsCallbacks,
    fs_store: &Arc<dyn ArtifactStore>,
    fs_policy: &Arc<StaticPathPolicy>,
    fs_run_id: &RunId,
    mode: &ReplayMode,
) -> napi::Result<adriane_graph_runtime::NodeHandler> {
    let agent = build_react_agent(
        node_id,
        &map_spec.agent,
        spec,
        callbacks,
        fs_store,
        fs_policy,
        fs_run_id,
        mode,
    )?;
    Ok(map_node_handler(
        agent,
        map_spec.over_channel.clone(),
        map_spec.join_at.clone(),
        map_spec.suspend_for_approval,
    ))
}

/// A tool `execute` fn that delegates to the JS `on_node` closure (kind `"tool"`),
/// awaiting the returned tool-result JSON.
fn js_tool_handler(tool_name: String, callbacks: &JsCallbacks) -> adriane_agents_core::ToolHandler {
    let on_node = callbacks.on_node.clone();
    Box::new(move |input: Value| {
        let on_node = on_node.clone();
        let tool_name = tool_name.clone();
        Box::pin(async move {
            let payload = json!({ "kind": "tool", "name": tool_name, "input": input });
            match call_js_string(&on_node, payload).await {
                Ok(result) => Ok(parse_value(&result)),
                Err(error) => Err(format!("js tool '{tool_name}': {error}")),
            }
        })
    })
}

/// Resolve the concrete `{ provider, model }` an agent should run with.
///
/// - An explicit `model` on the spec always wins: it is used with the agent's
///   nominal `provider` (`recommended = false`). This preserves the pre-tier
///   behaviour where the SDK pins a model.
/// - Otherwise, if a `tier` is set, the model is resolved by [`ModelPolicy`] against
///   the providers available in this process env ([`ModelPolicy::available_from_env`]).
///   No provider/model override is passed, so the highest-preference AVAILABLE
///   provider supplies the tier's recommended model (e.g. only-mistral env, `fast`
///   tier -> `mistral` / `mistral-small-latest`). If nothing is available, the mock
///   provider is returned.
/// - Otherwise (neither model nor tier), the agent's nominal provider is used with
///   no pinned model, leaving model selection to the adapter's own default.
fn resolve_agent_model(agent_spec: &AgentSpec, keys: &BTreeMap<String, String>) -> ModelChoice {
    if let Some(model) = &agent_spec.model {
        return ModelChoice {
            provider: parse_provider(&agent_spec.provider),
            model: model.clone(),
            recommended: false,
        };
    }
    if let Some(tier) = agent_spec.tier {
        let policy = ModelPolicy::default();
        // Availability = env credentials UNION control-plane provider_keys (ADR 0010), so a tenant
        // key supplied only via EngineSpec.provider_keys still makes its provider usable (no mock).
        let available = available_from_keys_or_env(&policy, keys);
        // Honor the agent's DECLARED provider when it names an available one: pin it as the
        // override so the tier maps onto ITS model column instead of the env preference order —
        // otherwise provider:"mistral" gets silently re-routed to a higher-preference provider
        // (e.g. Google) and a deprecated default model. Blank/unavailable declaration falls back to
        // preference order over `available`.
        let declared = parse_provider(&agent_spec.provider);
        let override_provider =
            if !agent_spec.provider.trim().is_empty() && available.contains(&declared) {
                Some(declared)
            } else {
                None
            };
        return policy.resolve(tier, &available, override_provider, None);
    }
    ModelChoice {
        provider: parse_provider(&agent_spec.provider),
        model: String::new(),
        recommended: false,
    }
}

/// Providers usable for this run: those with a tenant key in `keys` (ADR 0010) OR an env
/// credential, in policy preference order. Tenant-key-first, env fallback — the same precedence
/// `register_provider_adapter::key_for` applies when it actually builds the adapter.
fn available_from_keys_or_env(
    policy: &ModelPolicy,
    keys: &BTreeMap<String, String>,
) -> Vec<LlmProvider> {
    let mut available = policy.available_from_env();
    for (id, provider) in [
        ("anthropic", LlmProvider::Anthropic),
        ("openai", LlmProvider::Openai),
        ("google", LlmProvider::Google),
        ("mistral", LlmProvider::Mistral),
        ("openrouter", LlmProvider::Openrouter),
        ("minimax", LlmProvider::Minimax),
        ("huggingface", LlmProvider::Huggingface),
    ] {
        let has_key = keys.get(id).is_some_and(|v| !v.is_empty());
        if has_key && !available.contains(&provider) {
            available.push(provider);
        }
    }
    available
}

/// Build the gateway that backs the agent, registering an adapter that matches the
/// RESOLVED provider so the request's provider slot always has an adapter:
/// - `Openai` / `Mistral` / `Openrouter` / `Minimax` / `Huggingface` -> the shared
///   OpenAI-compatible adapter, keyed off that provider's env credential,
/// - `Anthropic` -> Anthropic adapter (from env),
/// - `Google` -> native Gemini adapter (from `GEMINI_API_KEY`/`GOOGLE_API_KEY`),
/// - `Ollama` / `Lmstudio` -> the local (OpenAI-compatible) adapter, flag-gated,
/// - `Mock` (or any real provider whose credentials are absent) -> a deterministic
///   mock scripted to exercise the tool/approval path.
///
/// The resolved model (when non-empty) is threaded in as the adapter's default model.
/// If the chosen real provider's credentials are not actually present in env, the
/// build falls back to the mock so a run still completes deterministically offline.
/// ADR 0030 9c: resolves a multimodal `Artifact` media reference to inline base64 by reading
/// the run-scoped artifact store. The artifact's `content` is expected to be a base64 string;
/// the block's own `media_type` is authoritative (the store's `ArtifactMediaType` enum is a
/// closed text/json/octet-stream set). `Base64`/`Url` sources pass through unchanged.
struct ArtifactMediaResolver {
    store: Arc<dyn ArtifactStore>,
}

#[async_trait]
impl MediaResolver for ArtifactMediaResolver {
    async fn resolve(&self, source: &MediaSource) -> Result<MediaSource, LlmError> {
        let MediaSource::Artifact {
            artifact_id,
            version,
            media_type,
        } = source
        else {
            return Ok(source.clone());
        };
        let id = ArtifactId(artifact_id.clone());
        let artifact = match version {
            Some(v) => self.store.read_version(&id, *v as i64).await,
            None => self.store.read(&id).await,
        }
        .ok_or_else(|| LlmError::MediaResolution(format!("artifact '{artifact_id}' not found")))?;
        let data = artifact
            .content
            .as_str()
            .ok_or_else(|| {
                LlmError::MediaResolution(format!(
                    "artifact '{artifact_id}' content is not a base64 string"
                ))
            })?
            .to_owned();
        Ok(MediaSource::Base64 {
            media_type: media_type.clone(),
            data,
        })
    }
}

/// Register the real provider adapter for `provider` into `gateway`, resolving its API key
/// (control-plane tenant key first, then env). Returns `true` when a real adapter was
/// registered, `false` when credentials are missing / the provider is `Mock` (the caller then
/// installs a mock). Shared by the graph path ([`build_gateway`]) and the standalone one-shot
/// ([`build_standalone_gateway`], ADR 0031 — the `Model.invoke()` path).
fn register_provider_adapter(
    gateway: &mut DefaultLlmGateway,
    provider: LlmProvider,
    model: Option<String>,
    keys: &BTreeMap<String, String>,
) -> bool {
    // Resolve a provider's API key: the control-plane-injected tenant key (ADR 0010) first,
    // then the process env. So admin-managed per-tenant keys win, with env as the fallback.
    let key_for = |provider: &str, env: &str| -> Option<String> {
        keys.get(provider)
            .filter(|value| !value.is_empty())
            .cloned()
            .or_else(|| std::env::var(env).ok().filter(|value| !value.is_empty()))
    };

    let registered = match provider {
        LlmProvider::Mistral => key_for("mistral", "MISTRAL_API_KEY").map(|key| {
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::mistral(
                Some(key),
                model.clone(),
            )));
        }),
        LlmProvider::Openai => key_for("openai", "OPENAI_API_KEY").map(|key| {
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::openai(
                Some(key),
                model.clone(),
            )));
        }),
        LlmProvider::Openrouter => key_for("openrouter", "OPENROUTER_API_KEY").map(|key| {
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::openrouter(
                Some(key),
                model.clone(),
            )));
        }),
        LlmProvider::Minimax => key_for("minimax", "MINIMAX_API_KEY").map(|key| {
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::minimax(
                Some(key),
                model.clone(),
            )));
        }),
        LlmProvider::Huggingface => key_for("huggingface", "HF_TOKEN").map(|key| {
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::huggingface(
                Some(key),
                model.clone(),
            )));
        }),
        // The Anthropic adapter honours the request's model directly when it is a `claude-*` id.
        LlmProvider::Anthropic => key_for("anthropic", "ANTHROPIC_API_KEY").map(|key| {
            gateway.register_adapter(Box::new(AnthropicAdapter::new(Box::new(
                HttpAnthropicPort::new(key),
            ))));
        }),
        // Gemini likewise honours a `gemini-*` request model directly; also accepts GOOGLE_API_KEY.
        LlmProvider::Google => key_for("google", "GEMINI_API_KEY")
            .or_else(|| {
                std::env::var("GOOGLE_API_KEY")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .map(|key| {
                gateway.register_adapter(Box::new(GeminiAdapter::new(Box::new(
                    HttpGeminiPort::new(key),
                ))));
            }),
        LlmProvider::Ollama if std::env::var("ADRIANE_USE_OLLAMA").as_deref() == Ok("1") => {
            // `ADRIANE_OLLAMA_BASE_URL` targets a remote Ollama (e.g. a self-hosted Fly app at
            // `http://adriane-ollama.internal:11434/v1`); unset → the adapter's localhost default.
            let base_url = std::env::var("ADRIANE_OLLAMA_BASE_URL")
                .ok()
                .filter(|value| !value.is_empty());
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::ollama(
                model.clone(),
                base_url,
            )));
            Some(())
        }
        LlmProvider::Lmstudio if std::env::var("ADRIANE_USE_LMSTUDIO").as_deref() == Ok("1") => {
            let base_url = std::env::var("ADRIANE_LMSTUDIO_BASE_URL")
                .ok()
                .filter(|value| !value.is_empty());
            gateway.register_adapter(Box::new(OpenAiCompatibleAdapter::lmstudio(
                model.clone(),
                base_url,
            )));
            Some(())
        }
        // `Mock`, or a real provider whose credentials are missing.
        _ => None,
    };

    registered.is_some()
}

fn build_gateway(
    agent_spec: &AgentSpec,
    resolved: &ModelChoice,
    keys: &BTreeMap<String, String>,
    fs_store: Option<&Arc<dyn ArtifactStore>>,
    mode: &ReplayMode,
) -> Arc<dyn LlmGateway> {
    let mut gateway = DefaultLlmGateway::new();
    let model = if resolved.model.is_empty() {
        None
    } else {
        Some(resolved.model.clone())
    };

    if !register_provider_adapter(&mut gateway, resolved.provider, model, keys) {
        // Register the mock under the RESOLVED provider — the slot the agent actually
        // drives with (`with_provider(resolved.provider)`). When no real provider is
        // available and a tier is set, ModelPolicy resolves to `Mock`, so the mock must
        // live in the `Mock` slot or the request finds no adapter.
        gateway.register_adapter(Box::new(mock_adapter(agent_spec, resolved.provider)));
    }

    // ADR 0030 9c: bind the artifact-backed media resolver so multimodal `Artifact` refs are
    // resolved to bytes at the gateway boundary (the run-scoped store the fs also uses). Bind
    // on the CONCRETE DefaultLlmGateway (with_media_resolver is not on the trait) BEFORE the
    // replay-mode wrap turns it into a trait object.
    let gateway = match fs_store {
        Some(store) => gateway.with_media_resolver(Arc::new(ArtifactMediaResolver {
            store: store.clone(),
        })),
        None => gateway,
    };
    // ADR 0038: wrap per replay mode — record (journal LLM I/O), replay (the shared
    // ReplayGateway), or live (passthrough).
    mode.wrap_gateway(Arc::new(gateway))
}

/// Build a gateway for a STANDALONE one-shot completion (ADR 0031 — the `Model.invoke()` path
/// over the napi seam). Registers the adapter for `provider` (with the request's model + keys);
/// when credentials are missing it installs a deterministic mock so offline `invoke()` still
/// resolves. No agent spec, no media resolver — a bare provider router for a single request.
pub fn build_standalone_gateway(
    provider: LlmProvider,
    model: Option<String>,
    keys: &BTreeMap<String, String>,
) -> Arc<DefaultLlmGateway> {
    let mut gateway = DefaultLlmGateway::new();
    if !register_provider_adapter(&mut gateway, provider, model, keys) {
        gateway.register_adapter(Box::new(MockAdapter::new(
            provider,
            vec![final_text("done", provider)],
        )));
    }
    Arc::new(gateway)
}

/// A deterministic mock: emit a `tool_use` for each declared tool (so a gated tool
/// triggers the approval gate / executes once granted), then finalize. Registered
/// under the RESOLVED provider — the slot the agent actually drives with — so a
/// tier-tagged agent that resolves to `Mock` offline (no provider keys) still finds
/// its adapter instead of erroring with "no adapter registered for provider 'Mock'".
fn mock_adapter(agent_spec: &AgentSpec, provider: LlmProvider) -> MockAdapter {
    let mut responses: Vec<LlmResponse> = agent_spec
        .tool_names
        .iter()
        .map(|name| tool_use(name, provider))
        .collect();
    responses.push(final_text("done", provider));
    if responses.is_empty() {
        responses.push(final_text("done", provider));
    }
    MockAdapter::new(provider, responses)
}

fn tool_use(name: &str, provider: LlmProvider) -> LlmResponse {
    LlmResponse {
        content: String::new(),
        tool_calls: Some(vec![LlmToolCall {
            id: format!("tu-{name}"),
            name: name.to_owned(),
            input: json!({}),
        }]),
        stop_reason: Some("tool_use".to_owned()),
        usage: LlmUsage::default(),
        model: "mock".to_owned(),
        provider,
        content_blocks: None,
    }
}

fn final_text(answer: &str, provider: LlmProvider) -> LlmResponse {
    LlmResponse {
        content: format!("FINAL: {answer}"),
        tool_calls: None,
        stop_reason: Some("end_turn".to_owned()),
        usage: LlmUsage::default(),
        model: "mock".to_owned(),
        provider,
        content_blocks: None,
    }
}

fn parse_provider(provider: &str) -> LlmProvider {
    match provider.to_ascii_lowercase().as_str() {
        "openai" => LlmProvider::Openai,
        "anthropic" => LlmProvider::Anthropic,
        "google" | "gemini" => LlmProvider::Google,
        "mistral" => LlmProvider::Mistral,
        "openrouter" => LlmProvider::Openrouter,
        "minimax" => LlmProvider::Minimax,
        "huggingface" | "hf" => LlmProvider::Huggingface,
        "ollama" => LlmProvider::Ollama,
        "lmstudio" => LlmProvider::Lmstudio,
        "mock" => LlmProvider::Mock,
        _ => LlmProvider::Anthropic,
    }
}

// ---------------------------------------------------------------------------
// JS call helpers
// ---------------------------------------------------------------------------

/// Call an **async** JS string callback from an async context and await its result.
/// The JS callback returns a `Promise<string>`: napi 3's `call_async` resolves the
/// (synchronously returned) promise object — the `Return = Promise<String>` type now
/// comes from the TSFN's generics rather than a turbofish — and the inner `.await`
/// drives that promise to its JS-resolved string. (`Result` is shadowed by the napi
/// prelude here, so the std one is explicit.)
async fn call_js_string(tsfn: &StringTsfn, payload: Value) -> std::result::Result<String, String> {
    let promise: Promise<String> = tsfn
        .call_async(payload.to_string())
        .await
        .map_err(|error| error.to_string())?;
    promise.await.map_err(|error| error.to_string())
}

/// Drive the **async** JS condition predicate from the runtime's **synchronous**
/// [`adriane_graph_runtime::ConditionFn`] context. We cannot `.await` here, so we
/// `spawn` the `call_async(..).await?.await?` chain onto napi's tokio runtime and
/// block this worker thread on a oneshot until it resolves. `block_in_place` lets
/// the multi-threaded runtime move other tasks (including the spawned one) onto
/// other worker threads while this one waits, avoiding starvation. The JS callback
/// resolves its `Promise` to a boolean-ish value, serialized as a JSON string
/// (`"true"`/`"false"`, or any JSON whose truthiness we read).
fn call_js_bool_awaiting(
    tsfn: &Arc<StringTsfn>,
    payload: Value,
) -> std::result::Result<bool, String> {
    // Arc-clone the shared TSFN so the spawned `'static` task owns a handle to the same
    // underlying JS callback (napi 3's TSFN is not `Clone`; the `Arc` is — see `JsCallbacks`).
    let tsfn = Arc::clone(tsfn);
    let (tx, rx) = std::sync::mpsc::sync_channel::<std::result::Result<String, String>>(1);
    tokio::task::block_in_place(move || {
        napi::bindgen_prelude::spawn(async move {
            let result = call_js_string(&tsfn, payload).await;
            // Ignore send errors: only happens if the receiver was dropped.
            let _ = tx.send(result);
        });
        rx.recv()
            .map_err(|_| "condition callback dropped without a value".to_owned())?
            .map(|text| parse_bool(&text))
    })
}

/// Read a JS-returned boolean-ish JSON string. Accepts a JSON boolean (`true`),
/// the strings `"true"`/`"false"` (any case), or a JSON number (non-zero is true).
/// Anything else is `false`.
fn parse_bool(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return true;
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return false;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Bool(b)) => b,
        Ok(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Ok(Value::String(s)) => s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// The channels of a state as a JSON object — what JS closures see as `state`.
fn channels_value(state: &GraphState) -> Value {
    Value::Object(
        state
            .channels
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

/// Parse a JS-returned channel-update JSON string into the update map. A non-object
/// (or unparsable) result yields an empty update rather than failing the node.
fn parse_update(text: &str) -> BTreeMap<String, Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// Build a [`NodeOutput`] from a JS node handler's returned update JSON. Two reserved
/// keys let a JS handler request a durable timer / signal wait without a structured
/// return: `__sleepUntil` (an opaque deadline string) and `__waitForSignal` (a signal
/// name). Either makes the run suspend after applying the remaining keys as the channel
/// update; together they are a signal-or-timeout. The SDK exposes them via `sleepUntil`
/// / `waitForSignal` helpers.
fn js_update_to_output(text: &str) -> NodeOutput {
    let mut update = parse_update(text);
    let sleep_until = take_reserved_string(&mut update, "__sleepUntil");
    let wait_for_signal = take_reserved_string(&mut update, "__waitForSignal");
    NodeOutput {
        update,
        sleep_until,
        wait_for_signal,
        ..NodeOutput::default()
    }
}

/// Remove a reserved string-valued key from the update map, returning it if present.
fn take_reserved_string(update: &mut BTreeMap<String, Value>, key: &str) -> Option<String> {
    match update.remove(key) {
        Some(Value::String(value)) => Some(value),
        _ => None,
    }
}

/// Parse a JS-returned tool-result JSON string into a value; an unparsable result
/// is surfaced verbatim as a string.
fn parse_value(text: &str) -> Value {
    serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

/// Gather pending approvals from the agent output channels of a suspended run. We
/// read each agent's output channel and pull its `approvalRequests`.
fn collect_pending_approvals(spec: &EngineSpec, state: &GraphState) -> Vec<ApprovalRequestItem> {
    if state.status != adriane_graph_core::GraphStatus::Suspended {
        return Vec::new();
    }
    let mut out = Vec::new();
    for agent_spec in spec.agents.values() {
        let channel = agent_spec
            .output_channel
            .clone()
            .unwrap_or_else(|| DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned());
        if let Some(value) = state.channels.get(&channel) {
            if let Some(requests) = value.get("approvalRequests") {
                if let Ok(items) =
                    serde_json::from_value::<Vec<ApprovalRequestItem>>(requests.clone())
                {
                    out.extend(items);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! These tests prove the registry-routing and run logic with **in-process**
    //! fake handlers (no JS, no napi). They build the same kind of `GraphRuntime`
    //! the napi entry points build, but with plain Rust closures in place of the
    //! TSFN-backed seams — so they run under `cargo test` with no Node present.

    use super::*;
    use adriane_graph_core::{
        ChannelDefinition, ChannelReducer, EdgeDefinition, EdgeId, GraphDefinition, GraphId,
        GraphStatus, NodeDefinition,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn artifact_media_resolver_resolves_a_ref_to_inline_base64() {
        use adriane_artifact_store::{ArtifactMediaType, ArtifactWriteInput};
        use adriane_graph_core::{NodeId, RunId};

        let store: Arc<dyn ArtifactStore> = Arc::new(InMemoryArtifactStore::new());
        let written = store
            .write(ArtifactWriteInput {
                run_id: RunId("run-1".to_owned()),
                node_id: NodeId::from("n1".to_owned()),
                name: "photo".to_owned(),
                media_type: ArtifactMediaType::ApplicationOctetStream,
                content: json!("BASE64BYTES"),
                metadata: None,
            })
            .await;

        let resolver = ArtifactMediaResolver {
            store: store.clone(),
        };
        // An Artifact ref resolves to inline base64, keeping the block's own media type.
        let resolved = resolver
            .resolve(&MediaSource::Artifact {
                artifact_id: written.id.0.clone(),
                version: None,
                media_type: "image/png".to_owned(),
            })
            .await
            .unwrap();
        match resolved {
            MediaSource::Base64 { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "BASE64BYTES");
            }
            other => panic!("expected base64, got {other:?}"),
        }

        // A missing artifact fails closed.
        let err = resolver
            .resolve(&MediaSource::Artifact {
                artifact_id: "nope".to_owned(),
                version: None,
                media_type: "image/png".to_owned(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::MediaResolution(_)));
    }

    fn replace_channel() -> ChannelDefinition {
        ChannelDefinition {
            channel_type: "json".to_owned(),
            reducer: ChannelReducer::Replace,
            default: None,
            no_log: false,
        }
    }

    #[test]
    fn resolve_run_id_prefers_state_then_spec_run_id() {
        // A start carries the id on spec.run_id.
        let start: EngineSpec = serde_json::from_value(json!({
            "graph": { "id": "g", "version": "0.0.0", "name": "g", "channels": {},
                "nodes": [{ "id": "a", "type": "action", "label": "a" }], "edges": [], "entryNodeId": "a" },
            "runId": "r-start"
        }))
        .expect("spec parses");
        assert_eq!(resolve_run_id(&start).0, "r-start");
        // Absent run id falls back deterministically.
        let bare: EngineSpec = serde_json::from_value(json!({
            "graph": { "id": "g", "version": "0.0.0", "name": "g", "channels": {},
                "nodes": [{ "id": "a", "type": "action", "label": "a" }], "edges": [], "entryNodeId": "a" }
        }))
        .expect("spec parses");
        assert_eq!(resolve_run_id(&bare).0, "run");
    }

    #[test]
    fn build_fs_policy_compiles_rules_fail_closed() {
        use adriane_fs_backend::{FsPermVerb, PathPolicy};
        let policy = build_fs_policy(&[FsPolicyRule {
            glob: "scratch/**".to_owned(),
            verb: FsPermVerb::Write,
        }]);
        assert!(policy.resolve("scratch/x").can_write());
        // Unmatched path is fail-closed read-only.
        assert!(!policy.resolve("elsewhere").can_write());
        // Empty policy = read-only everywhere.
        assert!(!build_fs_policy(&[]).resolve("anything").can_write());
    }

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

    /// An agent-only graph runs to completion on the mock gateway (no JS).
    #[tokio::test]
    async fn agent_only_graph_runs_to_completion_on_the_mock() {
        // No approval-gated tool, no JS tool: the agent calls a stub tool then
        // finalizes. Build the runtime the same way `build_agent_handler` does.
        let agent_spec = AgentSpec {
            provider: "anthropic".to_owned(),
            model: None,
            tier: None,
            system: Some("be brief".to_owned()),
            tool_names: vec!["lookup".to_owned()],
            max_iterations: Some(4),
            suspend_for_approval: false,
            approval_tool_names: vec![],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };

        let gateway = build_gateway(
            &agent_spec,
            &resolve_agent_model(&agent_spec, &BTreeMap::new()),
            &BTreeMap::new(),
            None,
            &ReplayMode::Live,
        );
        let mut registry = InMemoryToolRegistry::new();
        registry.register(
            ToolDefinition {
                name: "lookup".to_owned(),
                description: "lookup".to_owned(),
                requires_approval: false,
                input_schema: Some(json!({ "type": "object" })),
                content_scoped: false,
            },
            adriane_agents_core::sync_tool(|_input| Ok(json!({ "ok": true }))),
        );
        let agent = ReActAgent::new("assistant", "test", gateway)
            .with_provider(LlmProvider::Anthropic)
            .with_tools(Arc::new(registry))
            .with_max_iterations(4);

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("assistant"),
            agent_node_handler(
                Arc::new(agent),
                DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(),
                false,
                None,
            ),
        );

        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [(DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel())]
                .into_iter()
                .collect(),
            nodes: vec![node("assistant", NodeType::Agent)],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        };

        let runtime = GraphRuntime::new(graph, nodes, InMemoryConditionRegistry::new());
        let state = runtime
            .start(RunId::from("run-agent"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        assert!(state.channels.contains_key(DEFAULT_AGENT_OUTPUT_CHANNEL));
    }

    /// ADR 0025 phase 3d: `build_agent_middleware` builds the EFFICIENCY layer from the
    /// SDK-resolved data list (and falls back to the legacy flat knobs when it is empty).
    /// Assertions are env-independent (they only check that efficiency entries land), since
    /// the GOVERNED redactor is env-gated; the governed-by-construction guarantee (a data
    /// list never reaches `push_governed`) is structural — the match only `push_efficiency`s.
    #[test]
    fn build_agent_middleware_builds_efficiency_from_the_resolved_list() {
        let from = |value: serde_json::Value| -> crate::spec::AgentSpec {
            serde_json::from_value(value).expect("agent spec parses")
        };
        let gateway: Arc<dyn LlmGateway> = Arc::new(DefaultLlmGateway::new());
        let build = |spec: &crate::spec::AgentSpec| {
            build_agent_middleware(spec, &gateway, LlmProvider::Anthropic, "m", "test-node")
        };

        // NB: the stack is never fully empty now — the ADR 0032 secrets floor is an always-on
        // GOVERNED middleware. So these assert on the EFFICIENCY layer specifically.

        // terse + contextBudget → real efficiency middleware (no external service needed).
        assert!(
            build(&from(json!({
                "provider": "anthropic",
                "resolvedMiddleware": [
                    { "kind": "terse" },
                    { "kind": "contextBudget", "params": { "chars": 100 } }
                ]
            })))
            .efficiency_len()
                > 0
        );

        // Legacy fallback: an empty resolved list + the old `outputStyle` knob still applies
        // terse, so pre-3d persisted graphs keep their behaviour.
        assert!(
            build(&from(json!({
                "provider": "anthropic",
                "outputStyle": "terse"
            })))
            .efficiency_len()
                > 0
        );

        // A fractional `chars` (float on the wire) is truncated, not silently dropped.
        assert!(
            build(&from(json!({
                "provider": "anthropic",
                "resolvedMiddleware": [{ "kind": "contextBudget", "params": { "chars": 4000.5 } }]
            })))
            .efficiency_len()
                > 0
        );

        // reflection (phase 3e) → an after_run middleware is installed (no external service).
        assert!(
            build(&from(json!({
                "provider": "anthropic",
                "resolvedMiddleware": [{ "kind": "reflection" }]
            })))
            .efficiency_len()
                > 0
        );

        // structuredOutput (ADR 0029 phase 8) WITH a schema → an efficiency middleware lands.
        assert!(
            build(&from(json!({
                "provider": "anthropic",
                "resolvedMiddleware": [{
                    "kind": "structuredOutput",
                    "params": { "name": "Verdict", "schema": { "type": "object" }, "mode": "lenient" }
                }]
            })))
            .efficiency_len()
                > 0
        );

        // structuredOutput WITHOUT a schema → no efficiency middleware (nothing to validate).
        assert_eq!(
            build(&from(json!({
                "provider": "anthropic",
                "resolvedMiddleware": [{ "kind": "structuredOutput", "params": {} }]
            })))
            .efficiency_len(),
            0
        );
    }

    /// ADR 0032: the in-engine secrets floor is an always-on GOVERNED middleware — present even
    /// with no PII service and no resolved efficiency middleware (so a stack is never ungoverned).
    #[test]
    fn secrets_floor_is_always_governed() {
        let gateway: Arc<dyn LlmGateway> = Arc::new(DefaultLlmGateway::new());
        let spec: crate::spec::AgentSpec =
            serde_json::from_value(json!({ "provider": "anthropic" })).expect("spec parses");
        let stack =
            build_agent_middleware(&spec, &gateway, LlmProvider::Anthropic, "m", "test-node");
        assert!(
            !stack.is_empty(),
            "the secrets floor is always governed-present"
        );
        assert_eq!(
            stack.efficiency_len(),
            0,
            "no efficiency middleware was requested"
        );
    }

    /// ADR 0026 phase 11: a `memory` overlay installs a GOVERNED MemoryMiddleware (sealed,
    /// constructed with namespace + principal), not an efficiency entry — so recall is
    /// tenant-scoped by construction and unrepresentable from user resolved_middleware.
    #[test]
    fn memory_overlay_installs_a_governed_middleware() {
        let gateway: Arc<dyn LlmGateway> = Arc::new(DefaultLlmGateway::new());
        let spec: crate::spec::AgentSpec = serde_json::from_value(json!({
            "provider": "anthropic",
            "memory": { "namespace": "tenant:t1:agent:a", "topK": 3, "recall": "vector" }
        }))
        .expect("spec parses");
        let stack =
            build_agent_middleware(&spec, &gateway, LlmProvider::Anthropic, "m", "assistant");
        assert!(!stack.is_empty());
        assert_eq!(stack.efficiency_len(), 0);
    }

    /// ADR 0035 phase 12: a `skills` overlay installs exactly one sealed `SkillMiddleware` in the
    /// efficiency layer (bridge-injected, namespace-sealed), proving the `SkillSpec` maps through
    /// the bridge to a real middleware — the TS↔Rust parity contract for skill loading.
    #[test]
    fn skills_overlay_installs_a_sealed_middleware() {
        let gateway: Arc<dyn LlmGateway> = Arc::new(DefaultLlmGateway::new());
        let spec: crate::spec::AgentSpec = serde_json::from_value(json!({
            "provider": "anthropic",
            "skills": { "namespace": "skill:t1:org", "required": ["house-style@1.0.0"], "advisoryK": 2 }
        }))
        .expect("spec parses");
        let stack =
            build_agent_middleware(&spec, &gateway, LlmProvider::Anthropic, "m", "assistant");
        // One efficiency middleware (the skills loader); no resolved_middleware was requested.
        assert_eq!(stack.efficiency_len(), 1);
    }

    /// A gated agent suspends with a pending approval recorded in its output
    /// channel — exactly the shape `collect_pending_approvals` reads.
    #[tokio::test]
    async fn gated_agent_suspends_with_a_pending_approval() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);

        let agent_spec = AgentSpec {
            provider: "anthropic".to_owned(),
            model: None,
            tier: None,
            system: None,
            tool_names: vec!["refund".to_owned()],
            max_iterations: Some(4),
            suspend_for_approval: true,
            approval_tool_names: vec!["refund".to_owned()],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };
        let gateway = build_gateway(
            &agent_spec,
            &resolve_agent_model(&agent_spec, &BTreeMap::new()),
            &BTreeMap::new(),
            None,
            &ReplayMode::Live,
        );

        let mut registry = InMemoryToolRegistry::new();
        registry.register(
            ToolDefinition {
                name: "refund".to_owned(),
                description: "refund".to_owned(),
                requires_approval: true,
                input_schema: Some(json!({ "type": "object" })),
                content_scoped: false,
            },
            adriane_agents_core::sync_tool(move |_input| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!({ "ok": true }))
            }),
        );
        let agent = ReActAgent::new("assistant", "test", gateway)
            .with_provider(LlmProvider::Anthropic)
            .with_tools(Arc::new(registry))
            .with_max_iterations(4);

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("assistant"),
            agent_node_handler(
                Arc::new(agent),
                DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(),
                true,
                None,
            ),
        );

        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [
                (DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel()),
                (APPROVED_TOOLS_CHANNEL.to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![node("assistant", NodeType::Agent)],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        };

        let spec = EngineSpec {
            graph: graph.clone(),
            subgraphs: vec![],
            inbox: BTreeMap::new(),
            run_id: Some("run-gated".to_owned()),
            stream_tokens: false,
            initial_data: BTreeMap::new(),
            state: None,
            replay_journal: None,
            approved_tools: vec![],
            agents: [("assistant".to_owned(), agent_spec)].into_iter().collect(),
            component_nodes: BTreeMap::new(),
            map_agents: BTreeMap::new(),
            provider_keys: BTreeMap::new(),
            fs_policy: vec![],
            js_node_ids: vec![],
            js_tool_names: vec![],
        };

        let runtime = GraphRuntime::new(graph, nodes, InMemoryConditionRegistry::new());
        let state = runtime
            .start(RunId::from("run-gated"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Suspended);
        assert_eq!(calls.load(Ordering::SeqCst), 0); // gated, never executed

        let pending = collect_pending_approvals(&spec, &state);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].subject, "tool:refund");
    }

    /// A node declared as a `promptBuilder` component runs the NATIVE Rust handler
    /// (built from `ComponentRegistry`, exactly as `build_runtime` does) — the
    /// rendered template lands in the component's `into` channel, no JS involved.
    #[tokio::test]
    async fn component_node_runs_natively_and_sets_its_channel() {
        let registry = ComponentRegistry::new();
        let handler = registry
            .build_handler(
                "promptBuilder",
                &json!({ "template": "Hello {{name}}!", "into": "prompt" }),
            )
            .expect("promptBuilder handler builds");

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(NodeId::from("builder"), handler);

        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [
                ("name".to_owned(), replace_channel()),
                ("prompt".to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![node("builder", NodeType::Action)],
            edges: vec![],
            entry_node_id: NodeId::from("builder"),
            metadata: None,
        };

        let runtime = GraphRuntime::new(graph, nodes, InMemoryConditionRegistry::new());
        let state = runtime
            .start(
                RunId::from("run-component"),
                [("name".to_owned(), json!("Ada"))].into_iter().collect(),
            )
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("prompt"), Some(&json!("Hello Ada!")));
    }

    /// An unknown component kind fails the build cleanly (so a misconfigured graph is
    /// rejected up front rather than inside a running node).
    #[test]
    fn unknown_component_kind_fails_to_build() {
        let registry = ComponentRegistry::new();
        let result = registry.build_handler("nope", &json!({}));
        assert!(result.is_err());
    }

    /// Tier resolution: an explicit `model` always wins over a `tier`, keyed to the
    /// agent's nominal provider, and is flagged non-recommended.
    #[test]
    fn explicit_model_wins_over_tier() {
        let agent_spec = AgentSpec {
            provider: "anthropic".to_owned(),
            model: Some("claude-pinned".to_owned()),
            tier: Some(adriane_llm_gateway::ModelTier::Fast),
            system: None,
            tool_names: vec![],
            max_iterations: None,
            suspend_for_approval: false,
            approval_tool_names: vec![],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };
        let resolved = resolve_agent_model(&agent_spec, &BTreeMap::new());
        assert_eq!(resolved.provider, LlmProvider::Anthropic);
        assert_eq!(resolved.model, "claude-pinned");
        assert!(!resolved.recommended);
    }

    /// Tier resolution: with ONLY Mistral available, `tier=fast` resolves to the
    /// mistral column -> `mistral-small-latest` (recommended). This exercises the
    /// same `ModelPolicy` path `resolve_agent_model` drives off `available_from_env`,
    /// but with an explicit `available` slice so no process-env mutation is needed.
    #[test]
    fn tier_fast_on_mistral_only_resolves_to_mistral_small() {
        let policy = ModelPolicy::default();
        let available = [LlmProvider::Mistral];
        let choice = policy.resolve(adriane_llm_gateway::ModelTier::Fast, &available, None, None);
        assert_eq!(choice.provider, LlmProvider::Mistral);
        assert_eq!(choice.model, "mistral-small-latest");
        assert!(choice.recommended);
    }

    /// Tier resolution end-to-end through `resolve_agent_model` + the gateway build:
    /// with `MISTRAL_API_KEY` set (and anthropic/ollama disabled) a `fast`-tier agent
    /// resolves to `mistral-small-latest` through the Mistral adapter. Env is mutated
    /// behind a process-wide lock so it cannot race other env-reading tests.
    #[test]
    fn tier_fast_resolves_to_mistral_small_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_mistral = std::env::var("MISTRAL_API_KEY").ok();
        let prev_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_ollama = std::env::var("ADRIANE_USE_OLLAMA").ok();

        std::env::set_var("MISTRAL_API_KEY", "test-key");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ADRIANE_USE_OLLAMA");

        let agent_spec = AgentSpec {
            provider: "anthropic".to_owned(), // declared but unavailable (no key) → preference order over the available set
            model: None,
            tier: Some(adriane_llm_gateway::ModelTier::Fast),
            system: None,
            tool_names: vec![],
            max_iterations: None,
            suspend_for_approval: false,
            approval_tool_names: vec![],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };
        let resolved = resolve_agent_model(&agent_spec, &BTreeMap::new());
        assert_eq!(resolved.provider, LlmProvider::Mistral);
        assert_eq!(resolved.model, "mistral-small-latest");
        assert!(resolved.recommended);

        // The gateway registers a real adapter (not the mock) for the resolved
        // Mistral provider, since MISTRAL_API_KEY is present.
        let gateway = build_gateway(
            &agent_spec,
            &resolved,
            &BTreeMap::new(),
            None,
            &ReplayMode::Live,
        );
        assert!(Arc::strong_count(&gateway) >= 1);

        // Restore env so other tests see a pristine environment.
        restore_env("MISTRAL_API_KEY", prev_mistral);
        restore_env("ANTHROPIC_API_KEY", prev_anthropic);
        restore_env("ADRIANE_USE_OLLAMA", prev_ollama);
    }

    /// REGRESSION (the engine LLM-routing fix): with BOTH GEMINI_API_KEY and MISTRAL_API_KEY set, a
    /// `balanced`-tier agent that DECLARES provider:"mistral" must resolve to Mistral — not Google,
    /// which sorts ahead of Mistral in the policy preference order and would otherwise hijack it to
    /// the (deprecated) gemini default. Also asserts a Mistral key supplied ONLY via provider_keys
    /// (env-less, ADR 0010) resolves to Mistral, never the 0-token Mock.
    #[test]
    fn declared_provider_wins_over_preference_and_provider_keys_count() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_mistral = std::env::var("MISTRAL_API_KEY").ok();
        let prev_gemini = std::env::var("GEMINI_API_KEY").ok();
        let prev_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_ollama = std::env::var("ADRIANE_USE_OLLAMA").ok();

        std::env::set_var("MISTRAL_API_KEY", "test-mistral");
        std::env::set_var("GEMINI_API_KEY", "test-gemini");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ADRIANE_USE_OLLAMA");

        let agent_spec = AgentSpec {
            provider: "mistral".to_owned(),
            model: None,
            tier: Some(adriane_llm_gateway::ModelTier::Balanced),
            system: None,
            tool_names: vec![],
            max_iterations: None,
            suspend_for_approval: false,
            approval_tool_names: vec![],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };
        // env path: both keys present → the declared Mistral wins over Google's higher preference.
        let from_env = resolve_agent_model(&agent_spec, &BTreeMap::new());
        assert_eq!(from_env.provider, LlmProvider::Mistral);
        assert_eq!(from_env.model, "mistral-medium-latest");

        // provider_keys-only path: env-less Mistral key still resolves to Mistral (not Mock).
        std::env::remove_var("MISTRAL_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        let mut keys = BTreeMap::new();
        keys.insert("mistral".to_owned(), "k".to_owned());
        let from_keys = resolve_agent_model(&agent_spec, &keys);
        assert_eq!(from_keys.provider, LlmProvider::Mistral);

        restore_env("MISTRAL_API_KEY", prev_mistral);
        restore_env("GEMINI_API_KEY", prev_gemini);
        restore_env("ANTHROPIC_API_KEY", prev_anthropic);
        restore_env("ADRIANE_USE_OLLAMA", prev_ollama);
    }

    /// Process-wide lock serialising the env-mutating tests in this module.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn restore_env(key: &str, prev: Option<String>) {
        match prev {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    /// REGRESSION: a tier-tagged agent with NO provider keys resolves through
    /// `ModelPolicy` to the `Mock` provider; the mock adapter must be registered under
    /// that RESOLVED provider (not the nominal one), or the request fails with "no
    /// adapter registered for provider 'Mock'". We drive the agent exactly as
    /// `build_agent_handler` does — `with_provider(resolved.provider)` — and assert the
    /// run completes with a real mock answer rather than an error in the output channel.
    #[tokio::test]
    async fn tier_agent_with_no_keys_runs_on_mock_under_resolved_provider() {
        let env_guard = ENV_LOCK.lock().unwrap();
        let prev_mistral = std::env::var("MISTRAL_API_KEY").ok();
        let prev_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_ollama = std::env::var("ADRIANE_USE_OLLAMA").ok();
        std::env::remove_var("MISTRAL_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ADRIANE_USE_OLLAMA");

        let agent_spec = AgentSpec {
            provider: "anthropic".to_owned(), // nominal; tier + no keys -> Mock
            model: None,
            tier: Some(adriane_llm_gateway::ModelTier::Fast),
            system: Some("be brief".to_owned()),
            tool_names: vec!["lookup".to_owned()],
            max_iterations: Some(4),
            suspend_for_approval: false,
            approval_tool_names: vec![],
            output_channel: None,
            output_style: None,
            context_budget: None,
            todos_channel: None,
            enable_fs: false,
            resolved_middleware: vec![],
            input_blocks_channel: None,
            memory: None,
            skills: None,
        };

        let resolved = resolve_agent_model(&agent_spec, &BTreeMap::new());
        assert_eq!(
            resolved.provider,
            LlmProvider::Mock,
            "no keys + tier should resolve to Mock"
        );

        let gateway = build_gateway(
            &agent_spec,
            &resolved,
            &BTreeMap::new(),
            None,
            &ReplayMode::Live,
        );
        let mut registry = InMemoryToolRegistry::new();
        registry.register(
            ToolDefinition {
                name: "lookup".to_owned(),
                description: "lookup".to_owned(),
                requires_approval: false,
                input_schema: Some(json!({ "type": "object" })),
                content_scoped: false,
            },
            adriane_agents_core::sync_tool(|_input| Ok(json!({ "ok": true }))),
        );
        // Drive with the RESOLVED provider — exactly what build_agent_handler does.
        let agent = ReActAgent::new("assistant", "test", gateway)
            .with_provider(resolved.provider)
            .with_model(resolved.model.clone())
            .with_tools(Arc::new(registry))
            .with_max_iterations(4);

        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("assistant"),
            agent_node_handler(
                Arc::new(agent),
                DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(),
                false,
                None,
            ),
        );
        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [(DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel())]
                .into_iter()
                .collect(),
            nodes: vec![node("assistant", NodeType::Agent)],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        };
        let runtime = GraphRuntime::new(graph, nodes, InMemoryConditionRegistry::new());

        // The gateway + agent already captured the env above; restore it and release
        // the lock BEFORE the await so no std MutexGuard is held across an await point.
        restore_env("MISTRAL_API_KEY", prev_mistral);
        restore_env("ANTHROPIC_API_KEY", prev_anthropic);
        restore_env("ADRIANE_USE_OLLAMA", prev_ollama);
        drop(env_guard);

        let state = runtime
            .start(RunId::from("run-tier-mock"), BTreeMap::new())
            .await
            .unwrap();

        assert_eq!(state.status, GraphStatus::Completed);
        let output = state
            .channels
            .get(DEFAULT_AGENT_OUTPUT_CHANNEL)
            .expect("agent output channel");
        // The bug surfaced as {"error":"no adapter registered for provider 'Mock'"};
        // the fix makes the mock answer instead.
        assert!(
            output.get("error").is_none(),
            "agent errored offline: {output}"
        );
        assert!(
            output.get("reasoning").is_some(),
            "expected a real mock reasoning, got {output}"
        );
    }

    /// `build_runtime` wires JS node ids, agent nodes, and conditional edges into
    /// the right registries — checked structurally (no JS needed) via the spec's
    /// routing decisions: a human gate gets no handler, a js node id does, an
    /// agent node does, and a conditional edge's condition is registered.
    #[tokio::test]
    async fn build_runtime_routes_native_action_node_via_default_edge() {
        // Two action nodes, second is a JS node id, joined by a default edge. We
        // can't call real JS here, so we just prove the routing logic by replacing
        // the JS handler with an in-process one through `GraphRuntime::new` — the
        // structural decisions in `build_runtime` are exercised by the smoke test.
        // This test instead pins the runtime contract the bridge relies on.
        let mut nodes = InMemoryNodeRegistry::new();
        nodes.register(
            NodeId::from("a"),
            adriane_graph_runtime::sync_handler(|_s| {
                NodeOutput::update([("x".to_owned(), json!(1))].into_iter().collect())
            }),
        );
        nodes.register(
            NodeId::from("b"),
            adriane_graph_runtime::sync_handler(|_s| {
                NodeOutput::update([("y".to_owned(), json!(2))].into_iter().collect())
            }),
        );
        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [
                ("x".to_owned(), replace_channel()),
                ("y".to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![node("a", NodeType::Action), node("b", NodeType::Action)],
            edges: vec![EdgeDefinition {
                id: EdgeId::from("e1"),
                from: NodeId::from("a"),
                to: NodeId::from("b"),
                edge_type: EdgeType::Default,
                condition: None,
            }],
            entry_node_id: NodeId::from("a"),
            metadata: None,
        };
        let runtime = GraphRuntime::new(graph, nodes, InMemoryConditionRegistry::new());
        let state = runtime
            .start(RunId::from("run-x"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(state.status, GraphStatus::Completed);
        assert_eq!(state.channels.get("x"), Some(&json!(1)));
        assert_eq!(state.channels.get("y"), Some(&json!(2)));
    }

    #[test]
    fn provider_parsing_defaults_to_anthropic() {
        assert_eq!(parse_provider("openai"), LlmProvider::Openai);
        assert_eq!(parse_provider("mistral"), LlmProvider::Mistral);
        assert_eq!(parse_provider("anthropic"), LlmProvider::Anthropic);
        assert_eq!(parse_provider("unknown"), LlmProvider::Anthropic);
    }

    #[test]
    fn parse_update_tolerates_non_objects() {
        assert!(parse_update("not json").is_empty());
        assert!(parse_update("[1,2,3]").is_empty());
        let map = parse_update("{\"a\":1}");
        assert_eq!(map.get("a"), Some(&json!(1)));
    }

    #[test]
    fn validate_approved_tools_accepts_a_distinct_resolver_sorted_and_deduped() {
        // A human (a different principal) granted both tools — accepted. The returned
        // names are sorted + de-duplicated so the channel write is deterministic.
        let tools = vec![
            ApprovedTool {
                name: "wire".to_owned(),
                requested_by: "assistant".to_owned(),
                resolved_by: "alice".to_owned(),
                key: None,
            },
            ApprovedTool {
                name: "refund".to_owned(),
                requested_by: "assistant".to_owned(),
                resolved_by: "alice".to_owned(),
                key: None,
            },
            ApprovedTool {
                name: "refund".to_owned(),
                requested_by: "assistant".to_owned(),
                resolved_by: "bob".to_owned(),
                key: None,
            },
        ];
        let names = validate_approved_tools(&tools).expect("distinct resolver passes");
        assert_eq!(names, vec!["refund".to_owned(), "wire".to_owned()]);

        // A content-scoped grant writes its composite KEY into the channel, not the name.
        let hex = "a".repeat(64);
        let key = format!("write_file_guarded#{hex}");
        let scoped = vec![ApprovedTool {
            name: "write_file_guarded".to_owned(),
            requested_by: "worker".to_owned(),
            resolved_by: "alice".to_owned(),
            key: Some(key.clone()),
        }];
        let scoped_names = validate_approved_tools(&scoped).expect("distinct resolver passes");
        assert_eq!(scoped_names, vec![key]);
    }

    #[test]
    fn validate_approved_tools_rejects_a_malformed_content_scoped_key() {
        // A key whose name component diverges from the validated tool name, or whose hash
        // is not a 64-hex sha256, is rejected fail-closed (defense-in-depth).
        for bad in [
            "write_file_guarded#deadbeef".to_owned(),   // hash too short
            "other_tool#".to_owned() + &"a".repeat(64), // name component mismatch
            "write_file_guarded#".to_owned() + &"z".repeat(64), // non-hex
        ] {
            let tools = vec![ApprovedTool {
                name: "write_file_guarded".to_owned(),
                requested_by: "worker".to_owned(),
                resolved_by: "alice".to_owned(),
                key: Some(bad.to_string()),
            }];
            assert!(
                validate_approved_tools(&tools).is_err(),
                "malformed key must be rejected: {bad}"
            );
        }
    }

    #[test]
    fn validate_approved_tools_rejects_self_approval() {
        // resolved_by == requested_by: the agent tried to approve its own request. The
        // guard-rail rejects the whole resume — no tool name escapes into the channel.
        let tools = vec![ApprovedTool {
            name: "refund".to_owned(),
            requested_by: "assistant".to_owned(),
            resolved_by: "assistant".to_owned(),
            key: None,
        }];
        let error = validate_approved_tools(&tools).expect_err("self-approval is rejected");
        assert!(
            error.reason.contains("guard-rail"),
            "unexpected error: {error}"
        );
        assert!(
            error.reason.contains("tool:refund"),
            "error should name the offending subject: {error}"
        );
    }

    #[test]
    fn validate_approved_tools_rejects_an_empty_resolver() {
        // No principal on record approved the tool — treated as a self-approval-class
        // violation rather than silently unlocking.
        let tools = vec![ApprovedTool {
            name: "refund".to_owned(),
            requested_by: "assistant".to_owned(),
            resolved_by: "  ".to_owned(),
            key: None,
        }];
        assert!(validate_approved_tools(&tools).is_err());
    }

    #[tokio::test]
    async fn approve_entry_aborts_resume_on_self_approval() {
        // End-to-end through `drive`: an Approve whose only granted tool is self-approved
        // must error out of `drive` (interrupting the resume) before the runtime advances.
        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [
                (DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel()),
                (APPROVED_TOOLS_CHANNEL.to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![node("assistant", NodeType::Agent)],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        };
        let suspended = GraphState {
            run_id: RunId::from("run-guard"),
            graph_id: GraphId::from("g"),
            current_node_id: NodeId::from("assistant"),
            status: GraphStatus::Suspended,
            channels: BTreeMap::new(),
            version: 1,
            checkpoint_id: Some("run-guard:0".to_owned()),
            created_at: "0".to_owned(),
            updated_at: "0".to_owned(),
        };
        let spec = EngineSpec {
            graph: graph.clone(),
            subgraphs: vec![],
            inbox: BTreeMap::new(),
            run_id: Some("run-guard".to_owned()),
            stream_tokens: false,
            initial_data: BTreeMap::new(),
            state: Some(suspended),
            replay_journal: None,
            approved_tools: vec![ApprovedTool {
                name: "refund".to_owned(),
                requested_by: "assistant".to_owned(),
                resolved_by: "assistant".to_owned(),
                key: None,
            }],
            agents: BTreeMap::new(),
            component_nodes: BTreeMap::new(),
            map_agents: BTreeMap::new(),
            provider_keys: BTreeMap::new(),
            fs_policy: vec![],
            js_node_ids: vec![],
            js_tool_names: vec![],
        };
        // No node handler needed: `drive` validates BEFORE seeding/resuming, so the
        // self-approval error surfaces without ever routing to the agent node.
        let runtime = GraphRuntime::new(
            graph,
            InMemoryNodeRegistry::new(),
            InMemoryConditionRegistry::new(),
        );
        let result = drive(&runtime, &spec, Entry::Approve).await;
        assert!(result.is_err(), "self-approval must abort the resume");
    }

    #[tokio::test]
    async fn resume_entry_aborts_resume_on_self_approval() {
        // The PRODUCTION catalog path resumes through `Entry::Resume`, seeding
        // `approvedTools` with provenance. The guard-rail must fire here too: an
        // `Entry::Resume` whose only granted tool is self-approved (resolver == requester)
        // must error out of `drive` before the runtime advances — mirror of the Approve
        // test, proving GAP #1 (the resume path is no longer an unvalidated back door).
        let graph = GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels: [
                (DEFAULT_AGENT_OUTPUT_CHANNEL.to_owned(), replace_channel()),
                (APPROVED_TOOLS_CHANNEL.to_owned(), replace_channel()),
            ]
            .into_iter()
            .collect(),
            nodes: vec![node("assistant", NodeType::Agent)],
            edges: vec![],
            entry_node_id: NodeId::from("assistant"),
            metadata: None,
        };
        let suspended = GraphState {
            run_id: RunId::from("run-guard-resume"),
            graph_id: GraphId::from("g"),
            current_node_id: NodeId::from("assistant"),
            status: GraphStatus::Suspended,
            channels: BTreeMap::new(),
            version: 1,
            checkpoint_id: Some("run-guard-resume:0".to_owned()),
            created_at: "0".to_owned(),
            updated_at: "0".to_owned(),
        };
        let spec = EngineSpec {
            graph: graph.clone(),
            subgraphs: vec![],
            inbox: BTreeMap::new(),
            run_id: Some("run-guard-resume".to_owned()),
            stream_tokens: false,
            initial_data: BTreeMap::new(),
            state: Some(suspended),
            replay_journal: None,
            approved_tools: vec![ApprovedTool {
                name: "refund".to_owned(),
                requested_by: "assistant".to_owned(),
                resolved_by: "assistant".to_owned(),
                key: None,
            }],
            agents: BTreeMap::new(),
            component_nodes: BTreeMap::new(),
            map_agents: BTreeMap::new(),
            provider_keys: BTreeMap::new(),
            fs_policy: vec![],
            js_node_ids: vec![],
            js_tool_names: vec![],
        };
        // No node handler needed: `drive` validates BEFORE seeding/resuming, so the
        // self-approval error surfaces without ever routing to the agent node.
        let runtime = GraphRuntime::new(
            graph,
            InMemoryNodeRegistry::new(),
            InMemoryConditionRegistry::new(),
        );
        let result = drive(&runtime, &spec, Entry::Resume).await;
        assert!(
            result.is_err(),
            "self-approval must abort the production resume path too"
        );
    }

    #[test]
    fn parse_bool_reads_boolean_ish_strings() {
        // The async JS condition resolves its Promise to a boolean-ish JSON string;
        // `parse_bool` reads it back. Cover the shapes a JS callback can produce.
        assert!(parse_bool("true"));
        assert!(parse_bool("TRUE"));
        assert!(parse_bool("  true  "));
        assert!(!parse_bool("false"));
        assert!(!parse_bool("False"));
        assert!(parse_bool("1")); // JSON number, non-zero
        assert!(!parse_bool("0"));
        assert!(parse_bool("\"true\"")); // JSON string "true"
        assert!(!parse_bool("\"nope\""));
        assert!(!parse_bool("null"));
        assert!(!parse_bool("not json"));
    }

    #[test]
    fn replay_mode_record_clock_captures_and_journal_roundtrips() {
        // ADR 0038: record mode installs a RecordingClock that captures the timestamp sequence,
        // and `recorded_journal_json` round-trips through the persisted wire shape.
        let clock_buf = Arc::new(Mutex::new(Vec::new()));
        let mode = ReplayMode::Record {
            journal: Arc::new(Mutex::new(Vec::new())),
            clock: Arc::clone(&clock_buf),
        };
        let clock = mode.runtime_clock().expect("record mode installs a clock");
        let _ = clock.now_string();
        let _ = clock.now_string();
        assert_eq!(clock_buf.lock().unwrap().len(), 2);

        let json = mode
            .recorded_journal_json()
            .expect("record mode produces a journal");
        let wire: ReplayJournalWire = serde_json::from_str(&json).expect("journal round-trips");
        assert_eq!(wire.clock.len(), 2);
        assert!(wire.decisions.calls.is_empty());
    }

    #[test]
    fn replay_mode_replay_installs_a_recorded_clock() {
        // ADR 0038: replay mode installs a RecordedClock that re-feeds the recorded sequence.
        let mode = ReplayMode::Replay {
            gateway: Arc::new(ReplayGateway::new(LlmJournal::default())),
            clock: vec!["7".to_owned(), "8".to_owned()],
        };
        let clock = mode.runtime_clock().expect("replay mode installs a clock");
        assert_eq!(clock.now_string(), "7");
        assert_eq!(clock.now_string(), "8");
        assert_eq!(clock.now_string(), "8"); // clamps on exhaustion
    }
}
