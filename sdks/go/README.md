# Adriane Go SDK

Go cgo wrapper over `adriane-c-api`.

```go
package main

import (
	"fmt"

	"adriane/sdks/go/adriane"
)

func main() {
	fmt.Println(adriane.EngineVersion())
}
```

Build `adriane-c-api` first and make the dynamic library discoverable with
`DYLD_LIBRARY_PATH`, `LD_LIBRARY_PATH`, or your platform equivalent.
