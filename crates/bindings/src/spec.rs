//! The wire spec the SDK sends across the napi boundary, plus the outcome it
//! gets back. All camelCase, matching the TS SDK and `@adriane-ai/graph-core`.

use std::collections::BTreeMap;

use adriane_agents_core::ApprovalRequestItem;
use adriane_fs_backend::FsPermVerb;
use adriane_graph_core::{GraphDefinition, GraphState};
use adriane_llm_gateway::ModelTier;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for one agent node, keyed in [`EngineSpec::agents`] by node id.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    /// `"openai" | "anthropic" | "mistral"` — drives the provider slot used in the
    /// [`adriane_llm_gateway::LlmRequest`]. The actual adapter is chosen from env
    /// (see `bridge::build_gateway`); this only sets the request's `provider`.
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Optional capability tier (`"frontier" | "balanced" | "fast" | "creative"`).
    /// When set and no explicit `model` is given, the concrete model is resolved by
    /// the [`adriane_llm_gateway::ModelPolicy`] against the providers available in the
    /// process environment (see `bridge::resolve_agent_model`). An explicit `model`
    /// always wins over `tier`.
    #[serde(default)]
    pub tier: Option<ModelTier>,
    #[serde(default)]
    pub system: Option<String>,
    /// Names of tools this agent may call. A name in [`EngineSpec::js_tool_names`]
    /// is backed by a JS `execute` fn; otherwise it is a no-op stub tool.
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    /// When true, a tool that needs approval suspends the run (the agent-node
    /// dynamic-interrupt). When false, gated tools simply stop the agent loop.
    #[serde(default)]
    pub suspend_for_approval: bool,
    /// Tools (by name) that require human approval before the agent may run them.
    #[serde(default)]
    pub approval_tool_names: Vec<String>,
    /// The channel the agent writes its `AgentResult` into. Defaults to the
    /// agents-core `DEFAULT_AGENT_OUTPUT_CHANNEL` (`agentResult`).
    #[serde(default)]
    pub output_channel: Option<String>,
    /// Token-efficiency knobs (ADR 0014). `"terse"` appends a compact-output directive to
    /// the system prompt (cuts output tokens on prose; lossy — not for code). `None` = off.
    #[serde(default)]
    pub output_style: Option<String>,
    /// Cap (in chars) on the serialized `State` the agent injects into its first message —
    /// don't re-feed an unbounded channel map. `None` = no cap.
    #[serde(default)]
    pub context_budget: Option<u32>,
    /// Durable channel the agent's `writeTodos` list is persisted into (ADR 0022/0023,
    /// phase 1). When set and the agent has the `writeTodos` tool, the node handler
    /// writes the authoritative todo list here in the same checkpointed update as the
    /// result. `None` = no durable todos sink (the list still appears in the result).
    #[serde(default)]
    pub todos_channel: Option<String>,
    /// Opt this agent into the governed virtual filesystem tools (ADR 0024 phase 2b):
    /// `read_file`/`ls`/`glob`/`grep`/`write_file`/`edit_file`/`delete_file`/`move_file`,
    /// run-scoped and enforced by [`EngineSpec::fs_policy`] (fail-closed). Default off.
    #[serde(default)]
    pub enable_fs: bool,
}

/// One per-path permission rule (ADR 0024), compiled into the engine's
/// [`adriane_fs_backend::StaticPathPolicy`]. `verb` is `deny|read|write|gate`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsPolicyRule {
    pub glob: String,
    pub verb: FsPermVerb,
}

/// A graph node backed by a native Rust component, keyed in
/// [`EngineSpec::component_nodes`] by node id. The bridge builds the runtime handler
/// from [`adriane_components::ComponentRegistry::build_handler`] using `kind` +
/// `params`, so the component runs natively on Rust (it is **not** routed to the JS
/// `on_node` seam). `kind` is one of `ComponentRegistry::kinds()` (e.g.
/// `"promptBuilder"`, `"router"`, `"retriever"`); `params` is the component's
/// configuration object, validated up front at build time.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentNodeSpec {
    pub kind: String,
    #[serde(default)]
    pub params: Value,
}

/// One tool a human has granted on the approve path, carrying the governance
/// provenance the engine guard-rail validates: the principal who *requested* it and
/// the principal who *resolved* it must differ (no self-approval). The control plane
/// is the source of truth — it only ever sends tools an [`adriane_approval_engine`]
/// decision already approved — but the bridge re-checks the invariant before writing
/// the tool into the resume channel (defence in depth).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedTool {
    /// The tool name (the approval subject's `tool:<name>`, stripped of its prefix).
    pub name: String,
    /// The principal that *filed* the approval request (the agent/node).
    #[serde(default)]
    pub requested_by: String,
    /// The principal that *resolved* (approved) it — must be present and differ from
    /// `requested_by`, or the bridge rejects the resume.
    #[serde(default)]
    pub resolved_by: String,
}

/// The full spec for a run/resume/approve call.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSpec {
    pub graph: GraphDefinition,
    /// Child graphs that `subgraph`-type nodes resolve into, keyed by their own
    /// graph id. Their node handlers / agent / component configs are flattened into
    /// the same `jsNodeIds` / `agents` / `componentNodes` maps (by global node id),
    /// and their conditional edges into the same condition registry — so the bridge
    /// registers them alongside the parent's. Empty for a graph with no subgraphs.
    #[serde(default)]
    pub subgraphs: Vec<GraphDefinition>,
    /// Dynamic-message inbox to pre-queue before the run: per node id, a FIFO list of
    /// inputs (`send`). Each is consumed by that node's next execution via the reserved
    /// `__injected` channel. Empty for runs that don't use `send`.
    #[serde(default)]
    pub inbox: BTreeMap<String, Vec<Value>>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub initial_data: BTreeMap<String, Value>,
    /// Serialized [`GraphState`] of a suspended run — required by resume/approve,
    /// ignored by start.
    #[serde(default)]
    pub state: Option<GraphState>,
    /// Tools a human has granted on the approve OR resume path: each carries its
    /// `{ name, requestedBy, resolvedBy }` provenance. The bridge validates the
    /// no-self-approval invariant per tool, then writes only the validated *names* into
    /// the `__approvedTools` channel before resuming — on BOTH `Entry::Approve` and the
    /// production catalog `Entry::Resume`, so a malformed/forged resume cannot unlock a
    /// self-approved tool. `#[serde(default)]` keeps a spec that omits it (start, or an
    /// ordinary resume past a non-approval gate) deserializing to an empty list.
    #[serde(default)]
    pub approved_tools: Vec<ApprovedTool>,
    /// Per-node agent configuration, keyed by node id.
    #[serde(default)]
    pub agents: BTreeMap<String, AgentSpec>,
    /// Per-node native component configuration, keyed by node id. Such a node runs a
    /// Rust [`adriane_components`] handler (built at assemble time) instead of the JS
    /// seam, even if its id also appears in [`Self::js_node_ids`].
    #[serde(default)]
    pub component_nodes: BTreeMap<String, ComponentNodeSpec>,
    /// Node ids whose handler is a JS closure (action / tool / custom nodes).
    #[serde(default)]
    pub js_node_ids: Vec<String>,
    /// Tool names whose `execute` is a JS closure.
    #[serde(default)]
    pub js_tool_names: Vec<String>,
    /// Per-provider API keys the control plane injects for this run (ADR 0010), keyed by
    /// provider id (`"openai" | "anthropic" | "mistral" | "google" | "openrouter" | ...`).
    /// `bridge::build_gateway` uses a key here when present, falling back to the process env —
    /// so admin-managed, per-tenant keys reach the engine without going through `.env`. Empty
    /// for runs that rely on env (dev/self-host with deploy secrets).
    #[serde(default)]
    pub provider_keys: BTreeMap<String, String>,
    /// Per-path filesystem permission rules (ADR 0024 phase 2b), compiled into the
    /// run's `StaticPathPolicy` and applied to every fs-enabled agent. Empty = the
    /// fail-closed default (read-only everywhere; writes need an explicit rule).
    #[serde(default)]
    pub fs_policy: Vec<FsPolicyRule>,
}

/// What a run/resume/approve call returns to JS: the final state plus, when
/// suspended, the pending approvals and the state to feed back into approve.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    /// The final (or suspended) [`GraphState`].
    pub state: GraphState,
    /// `"running" | "suspended" | "completed" | "failed"` etc. — the state's
    /// status, surfaced at top level for convenience.
    pub status: String,
    /// Pending tool approvals gathered from the agent output channels when the run
    /// suspended for approval. Empty when not suspended on an approval gate.
    pub pending_approvals: Vec<ApprovalRequestItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_graph_json() -> Value {
        json!({
            "id": "g",
            "version": "0.0.0",
            "name": "g",
            "channels": {},
            "nodes": [{ "id": "a", "type": "action", "label": "a" }],
            "edges": [],
            "entryNodeId": "a"
        })
    }

    #[test]
    fn deserializes_a_minimal_start_spec() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "runId": "run-1",
            "jsNodeIds": ["a"]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        assert_eq!(spec.run_id.as_deref(), Some("run-1"));
        assert_eq!(spec.js_node_ids, vec!["a".to_owned()]);
        assert!(spec.agents.is_empty());
        assert!(spec.state.is_none());
        assert!(spec.initial_data.is_empty());
    }

    #[test]
    fn deserializes_approved_tools_with_provenance() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "approvedTools": [
                { "name": "refund", "requestedBy": "assistant", "resolvedBy": "alice" }
            ]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        assert_eq!(spec.approved_tools.len(), 1);
        let tool = &spec.approved_tools[0];
        assert_eq!(tool.name, "refund");
        assert_eq!(tool.requested_by, "assistant");
        assert_eq!(tool.resolved_by, "alice");
    }

    #[test]
    fn approved_tools_default_to_empty_and_provenance_defaults_to_blank() {
        // A start/resume spec omits `approvedTools` entirely (default empty); a tool
        // object may omit `requestedBy`/`resolvedBy` (both default to "" — the bridge
        // then treats the blank resolver as a self-approval violation).
        let spec: EngineSpec =
            serde_json::from_value(json!({ "graph": minimal_graph_json() })).expect("spec parses");
        assert!(spec.approved_tools.is_empty());

        let partial: ApprovedTool =
            serde_json::from_value(json!({ "name": "refund" })).expect("tool parses");
        assert_eq!(partial.requested_by, "");
        assert_eq!(partial.resolved_by, "");
    }

    #[test]
    fn deserializes_an_agent_spec_with_camel_case_keys() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": {
                    "provider": "anthropic",
                    "model": "claude-x",
                    "system": "be helpful",
                    "toolNames": ["refund"],
                    "maxIterations": 3,
                    "suspendForApproval": true,
                    "approvalToolNames": ["refund"]
                }
            },
            "jsToolNames": ["refund"]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let agent = spec.agents.get("assistant").expect("agent present");
        assert_eq!(agent.provider, "anthropic");
        assert_eq!(agent.model.as_deref(), Some("claude-x"));
        assert_eq!(agent.tool_names, vec!["refund".to_owned()]);
        assert_eq!(agent.max_iterations, Some(3));
        assert!(agent.suspend_for_approval);
        assert_eq!(agent.approval_tool_names, vec!["refund".to_owned()]);
        assert_eq!(spec.js_tool_names, vec!["refund".to_owned()]);
        assert!(agent.tier.is_none());
        assert!(spec.component_nodes.is_empty());
    }

    #[test]
    fn deserializes_the_todos_channel_from_camel_case() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": {
                    "provider": "anthropic",
                    "toolNames": ["writeTodos"],
                    "todosChannel": "__todos"
                }
            }
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let agent = spec.agents.get("assistant").expect("agent present");
        assert_eq!(agent.todos_channel.as_deref(), Some("__todos"));
        assert_eq!(agent.tool_names, vec!["writeTodos".to_owned()]);

        // Omitted → None (no durable sink).
        let bare: EngineSpec = serde_json::from_value(json!({
            "graph": minimal_graph_json(),
            "agents": { "assistant": { "provider": "anthropic" } }
        }))
        .expect("spec parses");
        assert!(bare
            .agents
            .get("assistant")
            .expect("agent present")
            .todos_channel
            .is_none());
    }

    #[test]
    fn deserializes_fs_enablement_and_policy_from_camel_case() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": { "provider": "anthropic", "enableFs": true }
            },
            "fsPolicy": [
                { "glob": "scratch/**", "verb": "write" },
                { "glob": "secret/**", "verb": "deny" }
            ]
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        assert!(
            spec.agents
                .get("assistant")
                .expect("agent present")
                .enable_fs
        );
        assert_eq!(spec.fs_policy.len(), 2);
        assert_eq!(spec.fs_policy[0].glob, "scratch/**");
        assert_eq!(spec.fs_policy[0].verb, FsPermVerb::Write);
        assert_eq!(spec.fs_policy[1].verb, FsPermVerb::Deny);

        // Omitted → fs disabled + empty policy (fail-closed default applies).
        let bare: EngineSpec =
            serde_json::from_value(json!({ "graph": minimal_graph_json() })).expect("parses");
        assert!(bare.fs_policy.is_empty());
    }

    #[test]
    fn deserializes_an_agent_tier_from_camel_case() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "agents": {
                "assistant": { "provider": "mistral", "tier": "fast" }
            }
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let agent = spec.agents.get("assistant").expect("agent present");
        assert_eq!(agent.tier, Some(ModelTier::Fast));
        assert!(agent.model.is_none());
    }

    #[test]
    fn subgraphs_default_to_empty_and_round_trip_child_definitions() {
        // Omitted → empty (an ordinary graph with no subgraph nodes).
        let spec: EngineSpec =
            serde_json::from_value(json!({ "graph": minimal_graph_json() })).expect("spec parses");
        assert!(spec.subgraphs.is_empty());

        // Present → each child GraphDefinition is carried verbatim, keyed elsewhere by
        // its own id. The bridge flattens child node ids into `jsNodeIds`/`agents`.
        let child = json!({
            "id": "child",
            "version": "0.0.0",
            "name": "child",
            "channels": {},
            "nodes": [{ "id": "c1", "type": "action", "label": "c1" }],
            "edges": [],
            "entryNodeId": "c1"
        });
        let spec: EngineSpec = serde_json::from_value(json!({
            "graph": minimal_graph_json(),
            "subgraphs": [child],
            "jsNodeIds": ["a", "c1"]
        }))
        .expect("spec parses");
        assert_eq!(spec.subgraphs.len(), 1);
        assert_eq!(spec.subgraphs[0].id.0, "child");
        assert_eq!(spec.js_node_ids, vec!["a".to_owned(), "c1".to_owned()]);
    }

    #[test]
    fn deserializes_a_component_node_spec() {
        let spec_json = json!({
            "graph": minimal_graph_json(),
            "componentNodes": {
                "a": {
                    "kind": "promptBuilder",
                    "params": { "template": "Hi {{name}}", "into": "prompt" }
                }
            }
        });
        let spec: EngineSpec = serde_json::from_value(spec_json).expect("spec parses");
        let component = spec.component_nodes.get("a").expect("component present");
        assert_eq!(component.kind, "promptBuilder");
        assert_eq!(
            component.params.get("into").and_then(Value::as_str),
            Some("prompt")
        );
    }

    #[test]
    fn run_outcome_serializes_camel_case() {
        let state = GraphState {
            run_id: adriane_graph_core::RunId::from("r"),
            graph_id: adriane_graph_core::GraphId::from("g"),
            current_node_id: adriane_graph_core::NodeId::from("a"),
            status: adriane_graph_core::GraphStatus::Suspended,
            channels: BTreeMap::new(),
            version: 1,
            checkpoint_id: Some("r:0".to_owned()),
            created_at: "0".to_owned(),
            updated_at: "0".to_owned(),
        };
        let outcome = RunOutcome {
            state,
            status: "suspended".to_owned(),
            pending_approvals: vec![ApprovalRequestItem {
                subject: "tool:refund".to_owned(),
                reason: "needs approval".to_owned(),
            }],
        };
        let wire = serde_json::to_string(&outcome).expect("serializes");
        assert!(wire.contains("\"pendingApprovals\""));
        assert!(wire.contains("\"tool:refund\""));
        assert!(wire.contains("\"currentNodeId\":\"a\""));
    }
}
