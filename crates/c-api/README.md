# adriane-c-api

Stable C ABI over the Adriane Rust engine for thin polyglot SDKs.

This crate is the common ABI for SDKs outside TypeScript/Python. It exports both
the callback-neutral JSON/YAML helpers and the callback-capable runtime entry
points:

- `adriane_engine_version() -> char*`
- `adriane_validate_graph_json(const char*) -> AdrianeResult`
- `adriane_compile_graph_yaml_json(const char*) -> AdrianeResult`
- `adriane_available_providers_json() -> AdrianeResult`
- `adriane_resolve_model_json(const char*, const char*, const char*) -> AdrianeResult`
- `adriane_list_components_json() -> AdrianeResult`
- `adriane_list_prebuilt_json() -> AdrianeResult`
- `adriane_run_component_json(const char*, const char*, const char*) -> AdrianeResult`
- `adriane_run_prebuilt_json(const char*, const char*, const char*) -> AdrianeResult`
- `adriane_engine_run_json(const char*, AdrianeCallbacks) -> AdrianeResult`
- `adriane_engine_resume_json(const char*, AdrianeCallbacks) -> AdrianeResult`
- `adriane_engine_approve_and_resume_json(const char*, AdrianeCallbacks) -> AdrianeResult`
- `adriane_engine_signal_json(const char*, const char*, const char*, AdrianeCallbacks) -> AdrianeResult`
- `adriane_engine_replay_json(const char*, const char*, AdrianeCallbacks) -> AdrianeResult`
- `adriane_string_free(char*)`
- `adriane_result_free(AdrianeResult)`

All returned strings are owned by the caller and must be freed with one of the
free functions. See `include/adriane.h` for the C contract.

Runtime string callbacks use `int callback(payload, user_data, &value, &error)`.
Rust copies callback `value` or `error` immediately before returning to the
engine; the host owns those callback pointers and only needs to keep them valid
for the duration of the callback call. Callbacks may be invoked from runtime
worker threads, so host SDK adapters must make their callback storage
thread-safe.

```bash
cargo build --locked --manifest-path crates/Cargo.toml -p adriane-c-api
```

The produced library is platform-specific:

- macOS: `crates/target/debug/libadriane_c_api.dylib`
- Linux: `crates/target/debug/libadriane_c_api.so`
- Windows: `crates/target/debug/adriane_c_api.dll`

Higher-level SDKs should wrap this ABI with native types and keep the engine
boundary as JSON in / JSON out.
