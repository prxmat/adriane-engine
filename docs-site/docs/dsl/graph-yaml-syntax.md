---
sidebar_position: 1
title: The Adriane DSL
description: Author a graph as YAML, then validate and compile it into a GraphDefinition.
---

# The Adriane DSL

Author a graph as **YAML** instead of code, then validate and compile it into a
`GraphDefinition`. The DSL is useful when graphs are authored by hand, stored in a repo, or
edited in a tool — the compiled definition is the **same wire format** the SDK builder produces,
so either path feeds the same engine.

## The graph DSL shape

A graph document mirrors the `GraphDefinition` fields: `id`, `version`, `name`, `entryNodeId`,
`channels`, `nodes`, `edges`.

```yaml
id: graph-1
version: 1.0.0
name: Demo graph
entryNodeId: n1
channels:
  messages:
    type: messages
    reducer: append
nodes:
  - id: n1
    type: action
    label: Start
edges: []
```

- **`channels`** — a map of channel name → `{ type, reducer }`. Reducers are `replace`,
  `append`, or `merge` (see [channels and reducers](/docs/core-concepts/channels-and-reducers)).
- **`nodes`** — each has an `id`, a `type` (`action`, `agent`, `tool`, `human-gate`,
  `subgraph`), and a `label`.
- **`edges`** — `{ from, to, type }`; conditional edges reference a named condition.
- **`entryNodeId`** — the node the run starts at.

## Compile from Python

The shipped way to compile DSL YAML is the Python SDK's `compile_graph_yaml` (it runs the Rust
compiler). It returns the compiled `GraphDefinition` dict and raises `GraphCompileError` on
failure.

```python
import adriane_ai

definition = adriane_ai.compile_graph_yaml(open("graph.yaml").read())
print(definition["id"])         # "graph-1"
print(len(definition["nodes"])) # 1
```

## Validate before compiling

`validate_graph(dict)` checks a definition without running it, returning a list of error dicts
(empty when valid) — the fast feedback loop for hand-authored graphs:

```python
import adriane_ai

errors = adriane_ai.validate_graph({
    "id": "g", "version": "1.0.0", "name": "x",
    "entryNodeId": "missing",          # references a node that doesn't exist
    "channels": {},
    "nodes": [{"id": "n1", "type": "action", "label": "Start"}],
    "edges": [],
})
print(errors)  # a non-empty list pinpointing the bad reference (code, message, path)
```

The pipeline is `parse → ast → validate → transform → compiled definition`; structural errors
stop it at the validate stage. More in the [compiler pipeline](./compiler-pipeline).

## DSL vs the builder

| | DSL (YAML) | SDK builder (`createGraph`) |
| --- | --- | --- |
| Authoring | hand-written / tool-edited files | TypeScript code, fully typed |
| Handlers | declared structurally (node types) | real closures attached to nodes |
| Output | a `GraphDefinition` (data) | a runnable `CompiledGraph` (data + handlers) |
| Best for | persisting/sharing graph **structure** | running graphs with custom node logic |

The builder produces the same `GraphDefinition` under the hood — a builder-authored graph and a
DSL-authored graph are validated and executed by the same engine. Reach for the DSL when the
**shape** of the graph is the artifact you want to store and review; reach for the builder when
you need real handler code.

:::note prompt/agent/chain DSL
Adriane has a second DSL compiler for prompt/agent/chain YAML (the `lang-adriane` pipeline)
alongside this graph compiler (`graph-adriane`). The public, shipped entry point today is
**graph YAML** via `compile_graph_yaml`. Both compile through the same
`parse → ast → validate → transform → compile` pipeline.
:::

## Next

- [The compiler pipeline](./compiler-pipeline)
- [CLI authoring](/docs/cli/commands)
