# @adriane-ai/cli

The Adriane command-line — author, validate, compile and run [Adriane
DSL](https://github.com/prxmat/adriane) graphs from your terminal. It ships as a
self-contained bundle (the engine is inlined), so the `adriane` command works the
moment it's installed.

## Install

```bash
npm i -g @adriane-ai/cli      # or: pnpm add -g @adriane-ai/cli
adriane --help
```

> **Naming.** The npm package is `@adriane-ai/cli`; the installed command is
> `adriane`. (The TypeScript SDK is `@adriane-ai/graph-sdk`; the Python SDK is
> `pip install adriane-ai` / `import adriane_ai`.)

## Commands

| Command | What it does |
| --- | --- |
| `adriane validate <file>` | Validate an Adriane DSL document; non-zero exit on errors. |
| `adriane compile <file> --out <dir>` | Compile DSL YAML into a `GraphDefinition` JSON. |
| `adriane run <file> [--watch]` | Run a graph locally; `--watch` re-runs on change. |
| `adriane publish <file>` | Package a graph for publishing. |
| `adriane diff <left> <right>` | Diff two graph definitions. |
| `adriane init <kind>` | Scaffold a new graph/agent/prompt document. |

```bash
adriane init graph --out ./my-graph.graph.yaml
adriane validate ./my-graph.graph.yaml
adriane compile ./my-graph.graph.yaml --out ./dist
adriane run ./my-graph.graph.yaml --watch
```

## License

Apache-2.0.
