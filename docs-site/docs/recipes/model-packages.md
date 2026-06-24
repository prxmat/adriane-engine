---
sidebar_position: 12
title: Choosing a model (per-provider packages)
description: Declare an agent's model with a per-provider package — like @langchain/openai, but executed in Rust.
---

# Choosing a model

Pick an agent's model with a **per-provider package** (ADR 0031) — the LangChain-style ergonomics,
but the call executes in the **one Rust engine** (no TS provider client, one consistent behaviour):

```ts
import { createGraph } from "@adriane-ai/graph-sdk";
import { openai, OpenAIModel } from "@adriane-ai/model-openai";

const app = createGraph({ name: "assistant" })
  .agentNode("reply", {
    model: openai("gpt-4o"),          // or new OpenAIModel("gpt-4o") / openai.frontier()
    prompt: { system: "Answer concisely." }
  })
  .compile();

await app.run({ question: "hi" });
```

`model` replaces the old required `llm` (now optional + deprecated). One package per provider:
`@adriane-ai/model-openai` · `-anthropic` · `-gemini` · `-mistral`, plus `openaiCompatible({ baseURL })`
for any OpenAI-compatible endpoint.

## Call a model standalone

A model is also callable on its own (the call still runs through the Rust gateway):

```ts
const m = new OpenAIModel("gpt-4o");
const res = await m.invoke("Summarize this in one line: …");
console.log(res.content);
```

## Tiers + custom endpoints

```ts
openai.frontier();                              // let the engine resolve the tier's model
openaiCompatible("llama-3.1", { baseURL: "http://localhost:1234/v1" });
```

Honest note: a model package is a **declaration** routed to a compiled-in Rust adapter (provider
slug + model + key env), not a new executor — the real extension point for an unknown endpoint is
`openaiCompatible`. API keys come from the environment (`OPENAI_API_KEY`, …). See
[ADR 0031](https://github.com/prxmat/adriane-engine/blob/main/docs/adr/0031-per-model-provider-packages.md).
