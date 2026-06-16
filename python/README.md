# Adriane — Python SDK

A thin, Pythonic SDK over the Adriane **Rust engine**, exposed through a
[pyo3](https://pyo3.rs) native extension module.

## Multi-language SDK strategy: one engine, many SDKs

Adriane's graph model, validator, and DSL compiler live **once** in Rust (under
`crates/`). Every language SDK is a thin shim over that single engine rather than
a re-implementation:

- **TypeScript SDK** — calls the Rust core via [napi-rs](https://napi.rs)
  (`crates/bindings`, package `adriane-napi`).
- **Python SDK** (this package) — calls the *same* Rust core via pyo3
  (`crates/py-bindings`, package `adriane-py`).

Both bindings expose the identical JSON-in / JSON-out surface (graph validation,
DSL compilation, the model policy, the component/prebuilt catalogs, and the
fully-Rust run paths), so a graph that validates one way in TypeScript validates
exactly the same way in Python. There is no second source of truth to drift.

## API

### Graph model & DSL

```python
import adriane

adriane.engine_version()            # -> "0.0.1" (the bound Rust engine version)

adriane.validate_graph({            # -> list[dict] of validation errors ([] if sound)
    "id": "g", "version": "0.0.0", "name": "g", "channels": {},
    "nodes": [{"id": "a", "type": "action", "label": "a"}],
    "edges": [{"id": "e1", "from": "a", "to": "ghost", "type": "default"}],
    "entryNodeId": "a",
})
# [{'code': 'INVALID_EDGE_REFERENCE', 'message': "Edge 'e1' references unknown node 'ghost'.", 'path': ['e1']}]

adriane.compile_graph_yaml("""    # -> dict (a compiled GraphDefinition)
id: g
version: 0.0.0
name: g
entryNodeId: a
nodes:
  - id: a
    type: action
    label: A
edges: []
channels: {}
""")
```

`validate_graph` returns the full list of structural errors (it does not raise on
an invalid-but-parseable graph). `compile_graph_yaml` raises `ValueError`
(`adriane.GraphCompileError`) when the DSL fails to parse, compile, or validate.

### Model policy

```python
adriane.available_providers()       # -> list[str], from process env credentials
# e.g. ["mistral"] when MISTRAL_API_KEY is set; [] when none are.

adriane.resolve_model("fast", available=["mistral"])
# -> {'provider': 'mistral', 'model': 'mistral-small-latest', 'recommended': True}

# Tiers: "frontier" | "balanced" | "fast" | "creative".
# Omit `available` to derive it from the env. A provider/model override wins
# over the policy choice and flags `recommended = False`:
adriane.resolve_model("frontier", available=["anthropic"], provider="mistral", model="mistral-tiny")
# -> {'provider': 'mistral', 'model': 'mistral-tiny', 'recommended': False}
```

### Catalogs

```python
adriane.list_components()   # -> list[str] of the 28 component kinds, e.g. "promptBuilder"
adriane.list_prebuilt()     # -> list[dict] of the 16 prebuilt micro-agents
# each: {'name', 'description', 'tier', 'systemPrompt', 'toolNames',
#        'suspendForApproval', 'outputChannel'}  (camelCase, from the Rust engine)
```

### Run paths (fully on Rust)

Both runs execute end-to-end in Rust — no Python callbacks. When no provider
credentials are present in the env, `run_prebuilt` falls back to a deterministic
mock gateway, so a run still completes offline.

```python
adriane.run_component(              # -> dict, the component's channel-update map
    "promptBuilder",
    {"template": "Hello {{name}}!", "into": "prompt"},
    {"name": "Ada"},
)
# {'prompt': 'Hello Ada!'}

adriane.run_prebuilt("summarizer", "please summarise this long text")
# -> {'status': 'completed',
#     'channels': {'input': ..., 'summary': {...}},
#     'resolvedModel': {'provider': 'mock', 'model': 'mock-model'}}

# Ergonomic accessor: each attribute is bound to that agent name.
adriane.prebuilt.summarizer("please summarise this long text")   # same as run_prebuilt("summarizer", ...)
adriane.prebuilt.classifier("is this spam?", provider="mistral") # override forwarded through
```

`run_component` and `run_prebuilt` raise `ValueError` (`adriane.RunError`) on an
unknown kind/agent, invalid input, or an engine/runtime failure.

## Install

Once published, the wheel installs from PyPI like any package — a single
`cp39-abi3` wheel covers CPython 3.9+ (the extension targets the stable ABI), so
nothing compiles on the user's machine:

```bash
pip install adriane
```

> **Pre-release:** not on PyPI yet — build from source (below).

### From source (dev)

The package is built with [maturin](https://www.maturin.rs) over the Rust
workspace crate `crates/py-bindings`, driven by `python/pyproject.toml`:

```bash
. "$HOME/.cargo/env"
python3 -m venv .venv && source .venv/bin/activate
pip install maturin

cd python
maturin develop            # build the extension + install into the active venv
# …or build a distributable wheel:
maturin build --release    # -> target/wheels/adriane-0.0.1-cp39-abi3-*.whl

python -c "import adriane; print(adriane.engine_version())"
```

`maturin` compiles the pyo3 cdylib and places it as the `adriane.adriane`
submodule — the leaf import name `adriane` resolves the `PyInit_adriane` symbol
emitted by `#[pymodule] fn adriane` in `crates/py-bindings/src/lib.rs`.

## Tests

```bash
cd python
maturin develop                 # build + install the extension into the venv
python -m pytest tests -q       # if pytest is installed
python tests/test_adriane.py    # plain-assert fallback when pytest is absent
```
