# Adriane Ruby SDK

Ruby wrapper over `adriane-c-api` using the `ffi` gem.

```ruby
require "adriane"

puts Adriane.engine_version
puts Adriane.list_components_json
```

Set `ADRIANE_C_API_LIB` to the built dynamic library when it is not on the
system loader path.
