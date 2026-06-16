# Tutorial 07 — Python SDK

**Objective.** Use Adriane from Python. The Python SDK is a thin, Pythonic wrapper over the
**same Rust engine** the TypeScript SDK uses — one engine, one graph validator, one DSL
compiler, identical behaviour across languages. You'll validate a graph, compile DSL YAML, run
a pure component, and run a prebuilt micro-agent — all `dict`-in / `dict`-out.

> **Pre-release.** The Python package is not on PyPI yet. You build the native extension from
> source in this monorepo:
>
> ```bash
> pnpm py:build   # builds python/adriane/adriane.abi3.so from crates/py-bindings
> ```
>
> Then import `adriane` from the `python/` directory.

## What's in the module

```python
import adriane

adriane.engine_version()        # version string of the bound Rust engine
adriane.validate_graph(dict)    # -> list of validation-error dicts (empty == valid)
adriane.compile_graph_yaml(str) # -> compiled GraphDefinition dict
adriane.available_providers()   # -> provider ids enabled by env credentials
adriane.resolve_model(tier, …)  # -> a {provider, model, recommended} choice
adriane.list_components()       # -> component-kind strings
adriane.list_prebuilt()         # -> prebuilt-agent definition dicts
adriane.run_component(kind, params, channels)  # run one component, on Rust
adriane.run_prebuilt(name, input, …)           # run a prebuilt micro-agent, on Rust
adriane.prebuilt                # ergonomic accessor: adriane.prebuilt.summarizer(text)
```

The boundary is **JSON in / JSON out**; the module hides that, taking and returning native
Python `dict` / `list` values.

## Validate a graph

`validate_graph` takes a graph definition as a plain `dict` (same shape as the `GraphDefinition`
JSON). It returns a **list of error dicts** — an empty list means the graph is structurally
sound. (It raises `GraphValidationError` only if the input can't even be encoded as JSON.)

```python
import adriane

definition = {
    "id": "greeter",
    "version": "1.0.0",
    "name": "Greeter",
    "entryNodeId": "n1",
    "channels": {"greeting": {"type": "string", "reducer": "replace"}},
    "nodes": [{"id": "n1", "type": "action", "label": "Start"}],
    "edges": [],
}

errors = adriane.validate_graph(definition)
print(errors)  # [] when valid; otherwise [{"code": ..., "message": ..., "path": ...}, ...]
```

**Expected result:** `[]` for a valid graph. A bad edge reference, for instance, yields an
entry with a `code` like `"INVALID_EDGE_REFERENCE"`, a `message`, and a `path`.

## Compile DSL YAML

`compile_graph_yaml` compiles an Adriane graph DSL document (a string) into a validated
`GraphDefinition` dict. It raises `GraphCompileError` on a parse, DSL, or validation failure.

```python
import adriane

yaml_doc = """
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
"""

definition = adriane.compile_graph_yaml(yaml_doc)
print(definition["id"])    # "graph-1"
print(definition["name"])  # "Demo graph"
```

**Expected result:** a `dict` you can inspect or persist. The DSL shape is the subject of
[Tutorial 08](./08-the-adriane-dsl.md).

## Run a pure component

`run_component(kind, params, channels)` runs a single component handler fully on Rust and
returns its **channel-update map** (the output patch) as a dict.

```python
import adriane

out = adriane.run_component(
    "promptBuilder",
    {"template": "Hi {{name}}!", "into": "prompt"},
    {"name": "Ada"},
)
print(out)  # {"prompt": "Hi Ada!"}
```

**Expected result:** `{"prompt": "Hi Ada!"}`. Use `adriane.list_components()` to see all the
kinds the engine knows (the same 30 kinds as the TypeScript `components`).

## Run a prebuilt micro-agent

`run_prebuilt(name, input, provider=None, model=None)` runs a prebuilt agent on Rust and
returns a run outcome. The agent's model is resolved from its tier and the env-available
providers; with no credentials it falls back to a **deterministic mock**, so a run still
completes offline.

```python
import adriane

result = adriane.run_prebuilt("summarizer", "A long passage of text to condense…")
print(result["status"])         # e.g. "completed"
print(result["channels"])       # the agent's output channels
print(result["resolvedModel"])  # {"provider": ..., "model": ...}
```

There's an ergonomic accessor that's shorthand for `run_prebuilt`:

```python
import adriane

adriane.prebuilt.summarizer("…long text…")
adriane.prebuilt.classifier("I love this!")
adriane.prebuilt.translator("Bonjour", model="some-model")
```

`adriane.prebuilt.<name>(input, *, provider=None, model=None)` calls
`run_prebuilt("<name>", input, …)`. Use `adriane.list_prebuilt()` to discover the available
agent names and their definitions.

## Resolving a model tier

```python
import adriane

choice = adriane.resolve_model("balanced")
print(choice)  # {"provider": "...", "model": "...", "recommended": True}
```

`resolve_model(tier, available=None, *, provider=None, model=None)` maps a capability tier
(`"frontier" | "balanced" | "fast" | "creative"`) to a concrete `{provider, model,
recommended}`. With `available=None` the providers come from the process environment
(`MISTRAL_API_KEY` → `"mistral"`, `ANTHROPIC_API_KEY` → `"anthropic"`, `ADRIANE_USE_OLLAMA=1`
→ `"ollama"`). An explicit `provider`/`model` override wins and is flagged
`"recommended": False`.

## Errors

The module raises three typed errors, all `ValueError` subclasses:

- `GraphValidationError` — the definition can't be encoded/parsed as JSON.
- `GraphCompileError` — DSL YAML fails to parse, compile, or validate.
- `RunError` — an unknown component kind / agent name, invalid params/input, or a runtime
  failure reported by the engine.

> **Boundary note.** The binding is JSON-in / JSON-out and runs on a single-threaded tokio
> runtime inside Rust — no Python callbacks cross the boundary. That's why the Python surface
> is validation, compilation, single-component runs, and prebuilt-agent runs (rather than the
> callback-driven custom-node graphs the TypeScript SDK builds).

## Next

[Tutorial 08 — The Adriane DSL](./08-the-adriane-dsl.md): author graphs as YAML and compile
them from either language.
