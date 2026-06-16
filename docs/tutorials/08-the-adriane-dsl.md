# Tutorial 08 — The Adriane DSL

**Objective.** Author a graph as **YAML** instead of code, then validate and compile it into a
`GraphDefinition`. The DSL is useful when graphs are authored by hand, stored in a repo, or
edited in a tool — the compiled definition is the same wire format the SDK builder produces, so
either path feeds the same engine.

Prerequisites: [Tutorial 01](./01-your-first-graph.md). The Python entry points are covered in
[Tutorial 07](./07-python-sdk.md).

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
  `append`, or `merge` (see [Tutorial 01](./01-your-first-graph.md)).
- **`nodes`** — each has an `id`, a `type` (`action`, `agent`, `tool`, `human-gate`,
  `subgraph`), and a `label`.
- **`edges`** — `{ from, to, type }`; conditional edges reference a named condition.
- **`entryNodeId`** — the node the run starts at.

## Compile from Python

The supported, shipped way to compile DSL YAML is through the Python SDK's
`compile_graph_yaml` (it runs the Rust compiler). It returns the compiled `GraphDefinition`
dict and raises `GraphCompileError` on failure.

```python
import adriane

definition = adriane.compile_graph_yaml(open("graph.yaml").read())
print(definition["id"])               # "graph-1"
print(len(definition["nodes"]))       # 1
```

**Expected result:** a compiled `GraphDefinition` dict you can persist, inspect, or pass to
`validate_graph` for a second check:

```python
errors = adriane.validate_graph(definition)
assert errors == []   # structurally sound
```

## Validate before compiling

`validate_graph(dict)` checks a definition without running it, returning a list of error dicts
(empty when valid). This is the fast feedback loop for hand-authored graphs:

```python
import adriane

errors = adriane.validate_graph({
    "id": "g", "version": "1.0.0", "name": "x",
    "entryNodeId": "missing",          # references a node that doesn't exist
    "channels": {},
    "nodes": [{"id": "n1", "type": "action", "label": "Start"}],
    "edges": [],
})
print(errors)  # a non-empty list pinpointing the bad reference (code, message, path)
```

**Expected result:** a non-empty list of `{ code, message, path }` entries describing each
problem. The pipeline is `parse → ast → validate → transform → compiled definition`; structural
errors stop it at the validate stage.

## The CLI (diagnostic / authoring)

The repo ships an `adriane` CLI for working with DSL files. These commands are **internal
authoring/diagnostic tooling** (not a marketed public API surface), but they're handy when
iterating on YAML:

```bash
adriane validate <file>              # validate a DSL file
adriane compile <file> --out <dir>   # compile a DSL file into a directory
adriane init <kind> --id <id> --out <file>   # scaffold a graph/agent/prompt file
adriane diff <left> <right>          # diff two definitions
adriane run <file> --input <json> [--watch]  # run a compiled file
adriane publish <file> --registry <url>      # publish to a registry
```

`init` accepts `graph`, `agent`, or `prompt` as `<kind>`. `compile` and `init` require their
respective `--out`; `publish` requires `--registry`.

## DSL vs the builder

| | DSL (YAML) | SDK builder (`createGraph`) |
| --- | --- | --- |
| Authoring | hand-written / tool-edited files | TypeScript code, fully typed |
| Handlers | declared structurally (node types) | real closures attached to nodes |
| Output | a `GraphDefinition` (data) | a runnable `CompiledGraph` (data + handlers) |
| Best for | persisting/sharing graph **structure** | running graphs with custom node logic |

The builder produces the same `GraphDefinition` under the hood — so a builder-authored graph
and a DSL-authored graph are validated and executed by the same engine. The DSL is the path
when the **shape** of the graph is the artifact you want to store and review; the builder is
the path when you need real handler code.

> **Note on prompt/agent/chain DSL.** Adriane has a second DSL compiler for prompt/agent/chain
> YAML (the `lang-adriane` pipeline) alongside this graph compiler (`graph-adriane`). The
> public, shipped entry point exposed to SDK consumers today is **graph YAML** via
> `compile_graph_yaml`. The prompt/agent/chain DSL compiles through the same
> `parse → ast → validate → transform → compile` pipeline internally.

## Next

You've covered the full public surface: graphs, agents, tools, human gates, checkpointing,
streaming, the Python SDK, and the DSL. Loop back to the [doc index](../README.md) for the
shipped end-to-end examples (`examples/qa-rag.ts`, `examples/doc-qa-reference.ts`,
`examples/startup-e2e.ts`), which compose everything you've learned.
