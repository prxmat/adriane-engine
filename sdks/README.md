# Adriane polyglot SDKs

Adriane SDKs are thin surfaces over one Rust engine. The TypeScript SDK keeps its
N-API bridge because it supports callback-heavy graph execution. Python currently
uses PyO3 for wheels. New SDKs should start from the stable C ABI in
`crates/c-api` unless they need host callbacks.

## Binding strategy

| Family | Languages | Default binding |
| --- | --- | --- |
| Scripting | JavaScript, TypeScript | Existing N-API bridge (`crates/bindings`) |
| Scripting | Python | Existing PyO3 wheel (`crates/py-bindings`) |
| Scripting | Ruby, PHP, Lua, PowerShell | C ABI (`crates/c-api`) |
| Systems | C, C++, Zig, Swift, Objective-C | C ABI (`crates/c-api`) |
| Systems | Go | C ABI through cgo |
| Systems | Rust | Direct Rust crates |
| JVM / .NET | Java, Kotlin, Scala, C# | C ABI through JNA/JNI or P/Invoke |
| BEAM | Elixir | C ABI through a small NIF or port driver |

## Initial stable surface

The first cross-language contract is deliberately small:

- `engine_version`
- `validate_graph_json`
- `compile_graph_yaml_json`
- `available_providers_json`
- `resolve_model_json`
- `list_components_json`
- `list_prebuilt_json`
- `run_component_json`
- `run_prebuilt_json`

This gives every SDK the same validator, DSL compiler, model policy, catalogs,
native component runs, and prebuilt-agent runs immediately, without duplicating
the engine. Runtime execution with custom host callbacks remains on N-API until
the runtime has a callback-neutral C contract.

## Memory contract

Every ABI function that returns a string returns an owned UTF-8 C string. SDKs
must copy it into their native string type, then call `adriane_string_free` or
`adriane_result_free`.

Never free Adriane-owned strings with a host allocator.

## Layout

- `lua/`, `ruby/`, `php/`, and `powershell/` cover the remaining scripting surfaces.
- `go/`, `cpp/`, `zig/`, `swift/`, and `objc/` cover systems-language consumers.
- `csharp/` covers .NET; `jvm/` exposes Java plus Kotlin/Scala delegates.
- `elixir/` provides the BEAM entry point through a small C NIF.
- Future SDK packages should add native builders on top of these JSON methods,
  not reimplement engine behavior.
