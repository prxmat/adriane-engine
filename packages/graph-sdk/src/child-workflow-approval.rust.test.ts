import { describe, expect, it } from "vitest";
// Import the in-memory engine directly (not the package index) so the test never
// pulls the Pg engine and its `db`/`pg` dependency chain — same discipline as
// governance-enforcement.test.ts.
import { InMemoryApprovalEngine } from "../../approval-engine/src/in-memory-approval-engine.js";

import { runCatalogGraph, rustEngineAvailable, type GraphDefinition } from "./index.js";

/**
 * ADR 0042 D2/D3 (product ADR 0068 D5.4) — the falsifiable approval-non-inheritance test.
 *
 * Revision 5's own framing assumed the property to prove was "a child's own tool call
 * gets a genuinely DISTINCT grant key" (i.e. approval does not leak from parent to
 * child, or vice versa). Writing this test against the REAL engine (governance-
 * enforcement.test.ts's own pattern: deterministic mock gateway, InMemoryApprovalEngine,
 * `rustEngineAvailable()`-gated) falsifies a stronger, prior claim instead: a child's
 * gated tool call files **no ApprovalEngine request at all**.
 *
 * `fileApprovalRequests` (run-catalog-graph.ts) walks only the TOP-LEVEL
 * `definition.nodes` — it has no subgraph recursion, unlike `assembleParts`'s
 * `classifyNode` (which DOES walk `subgraphs ?? []` to register native handlers). A
 * subgraph node itself never carries an `agent` carrier (it carries `subgraphId`), so
 * the top-level walk finds nothing to file a request for. The child's agent DOES
 * correctly suspend the run (`execute_subgraph` returns `self.suspend(...)` when the
 * child suspends) and its `approvalRequests` payload IS captured — but only inside the
 * parent's own `__subgraphStates` channel, a serialization-only bookkeeping field, never
 * surfaced to the control plane's `ApprovalEngine`. A run can sit suspended on a child's
 * gated tool call with ZERO pending approvals for a human to act on.
 *
 * This is a genuine governance gap, not a design choice — tracked in a dedicated issue,
 * scoped for its own ADR before any fix (CLAUDE.md mandatory human review: security-
 * relevant runtime-approval-flow changes). This test only PROVES the gap; it changes no
 * production code.
 */
const gatedChild: GraphDefinition = {
  id: "sub-gated-tool",
  version: "1",
  name: "sub-gated-tool",
  channels: {
    agentResult: { type: "agentResult", reducer: "replace" },
    __approvedTools: { type: "string[]", reducer: "replace", default: [] },
    __approvalIds: { type: "string[]", reducer: "replace", default: [] }
  },
  nodes: [
    {
      id: "c_assistant",
      type: "agent",
      label: "c_assistant",
      metadata: {
        agent: {
          provider: "anthropic",
          toolNames: ["refund"],
          suspendForApproval: true,
          approvalToolNames: ["refund"],
          outputChannel: "agentResult"
        }
      }
    }
  ],
  edges: [],
  entryNodeId: "c_assistant"
} as unknown as GraphDefinition;

const parentWithGatedChild: GraphDefinition = {
  id: "parent-with-gated-child",
  version: "1",
  name: "parent-with-gated-child",
  channels: {},
  nodes: [
    {
      id: "sub",
      type: "subgraph",
      label: "sub",
      subgraphId: "sub-gated-tool",
      inputMapping: {},
      outputMapping: {}
    }
  ],
  edges: [],
  entryNodeId: "sub"
} as unknown as GraphDefinition;

const rustOnly = rustEngineAvailable() ? describe : describe.skip;

rustOnly(
  "@adriane-ai/graph-sdk — child-workflow approval filing (ADR 0042 D2/D3, product ADR 0068 D5.4)",
  () => {
    it(
      "KNOWN GAP: a child's gated tool call suspends the run but files NO ApprovalEngine " +
        "request — the control plane's governance surface never sees it",
      async () => {
        const engine = new InMemoryApprovalEngine();
        const outcome = await runCatalogGraph(parentWithGatedChild, {
          runId: "run_child_approval_gap" as never,
          subgraphs: [gatedChild],
          approvalEngine: engine
        });

        // The run genuinely suspends — the child's suspension propagates to the parent
        // exactly as catalog-subgraph.rust.test.ts's own gated-child test proves.
        expect(outcome.status).toBe("suspended");

        // The gap: the seam files ZERO requests for the child's gate. If this ever
        // starts returning 1, `fileApprovalRequests` has been extended to recurse into
        // subgraphs — replace this assertion with a real non-inheritance proof (a
        // DISTINCT grant key per node) and close the tracked issue.
        const pending = await engine.getPending("run_child_approval_gap" as never);
        expect(pending).toHaveLength(0);

        // The top-level `__approvalIds` channel — what the control plane reads to know
        // which requests to resolve — stays empty too, even though a human decision is
        // genuinely required to make progress.
        const ids = (outcome.state.channels as Record<string, unknown>).__approvalIds;
        expect(ids === undefined || (ids as unknown[]).length === 0).toBe(true);
      }
    );
  }
);
