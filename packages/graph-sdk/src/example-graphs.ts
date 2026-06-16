import type { GraphDefinition } from "@adriane/graph-core";
import { InMemoryToolRegistry, type ToolId } from "@adriane/agents-core";
import { DefaultLLMGateway } from "@adriane/llm-gateway";

import { createGraph } from "./builder.js";
import { docQaReferenceDefinition } from "./reference-graph.js";

/**
 * Canonical example graphs authored with the SDK. Shipped so the Studio can render
 * them and the control plane can seed them — one source of truth for both. Only the
 * `.definition` (plain data) is meant to cross boundaries.
 */
export type ExampleGraph = {
  slug: string;
  name: string;
  description: string;
  definition: GraphDefinition;
};

const passthrough = { parse: (value: unknown) => value };

const publishFlow = (): GraphDefinition =>
  createGraph({ name: "publish-flow" })
    .channel("draft", { type: "string", default: "" })
    .channel("approved", { type: "boolean", default: false })
    .node("write", async () => ({ draft: "…" }))
    .humanGate("review")
    .node("publish", async () => ({ approved: true }))
    .edge("write", "review")
    .edge("review", "publish")
    .compile().definition;

const supportAgent = (): GraphDefinition => {
  const tools = new InMemoryToolRegistry();
  tools.register(
    {
      id: "refund" as ToolId,
      name: "refund",
      description: "Issues a customer refund. Sensitive.",
      inputSchema: passthrough,
      outputSchema: passthrough,
      permissions: ["payments:write"],
      requiresApproval: true,
      jsonSchema: { type: "object" }
    },
    async () => ({ ok: true })
  );

  return createGraph({ name: "support-agent" })
    .agentNode("assistant", {
      llm: new DefaultLLMGateway(),
      prompt: { system: "You are a support agent. Use tools when needed." },
      tools,
      suspendForApproval: true
    })
    .compile().definition;
};

/** Build the example graph definitions. Pure — no LLM call, no I/O. */
export const exampleGraphs = (): ExampleGraph[] => [
  {
    slug: "publish-flow",
    name: "Publish flow",
    description: "Un flux humain-dans-la-boucle : rédaction, porte d'approbation, publication.",
    definition: publishFlow()
  },
  {
    slug: "support-agent",
    name: "Support agent",
    description: "Un agent ReAct qui suspend le run pour approbation avant un outil sensible.",
    definition: supportAgent()
  },
  {
    slug: "doc-qa-reference",
    name: "Doc QA (reference)",
    description:
      "Pipeline RAG complet entrée → sortie : textCleaner → documentSplitter → retriever " +
      "→ reranker → promptBuilder → agent (ragAnswerer, balanced) → answerBuilder. " +
      "Chaque nœud porte le carrier catalog (node.metadata), donc le control plane " +
      "l'exécute sur le moteur Rust.",
    definition: docQaReferenceDefinition()
  }
];
