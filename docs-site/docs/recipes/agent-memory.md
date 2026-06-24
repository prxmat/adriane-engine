---
sidebar_position: 11
title: Long-term agent memory (recall across runs)
description: Give an agent governed long-term memory — it recalls relevant past context before a run and persists after.
tags: ["state"]
difficulty: intermediate
---

# Long-term agent memory

Give an agent **memory that capitalizes across runs** (ADR 0026): before a run it recalls relevant
past context (vector search) and injects it into the seed; after a run it persists what it learned,
**attributed** (who/what/when). Add one `memory` overlay:

```ts
import { createGraph } from "@adriane-ai/graph-sdk";

const app = createGraph({ name: "assistant" })
  .agentNode("reply", {
    model: openai("gpt-4o"),
    prompt: { system: "Answer the user." },
    memory: { namespace: "tenant:acme:agent:assistant", topK: 5, recall: "vector" }
  })
  .compile();

// First run learns; a later run recalls it into context automatically.
await app.run({ question: "our deploy window is Tuesdays 9–11am" });
await app.run({ question: "when can I deploy?" }); // recalls the earlier fact
```

## The overlay

| field | meaning |
| --- | --- |
| `namespace` | Tenant-scoped memory partition. **Sealed** by the engine with the principal — never user-routable. |
| `topK` | How many memories to recall (default 5). |
| `recall` | `"vector"` (semantic) · `"graph"` (entity graph) · `"both"` (default). |

## What you get

- **Vector + graph** recall behind one seam; every write carries **provenance**.
- Recall is a **seed-inject** — it never changes run state, so determinism + resume are intact.
- Governed: installs sealed, can't be routed around.

The OSS engine recalls/persists **in-memory** (across runs within a process). The control plane
swaps a **Neo4j**-backed store (native vector index + entity graph) behind the same seam, with
governed LLM entity-extraction. See [ADR 0026](https://github.com/prxmat/adriane-engine/blob/main/docs/adr/0026-memory-architecture-engine-studio.md).

For the conceptual model — the four memory planes and the unified seam — see
[Memory architecture](/docs/core-concepts/memory-architecture).
