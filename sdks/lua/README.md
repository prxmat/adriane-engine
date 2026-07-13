# Adriane Lua SDK

Minimal LuaJIT FFI wrapper over `adriane-c-api`.

Build the C ABI library first:

```bash
cargo build --locked --manifest-path crates/Cargo.toml -p adriane-c-api
```

Then point LuaJIT at the library:

```bash
ADRIANE_C_API_LIB=crates/target/debug/libadriane_c_api.dylib luajit -e '
local adriane = require("sdks.lua.adriane")
print(adriane.engine_version())
print(adriane.list_components_json())
'
```

On Linux use `libadriane_c_api.so`; on Windows use `adriane_c_api.dll`.
