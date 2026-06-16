#!/usr/bin/env bash
# Reproducible dev build for the adriane-py (pyo3) extension module.
# Builds the cdylib with cargo and copies it to python/adriane/adriane.abi3.so
# (abi3-py39), which `import adriane` loads. macOS linking is handled by
# crates/py-bindings/build.rs (-undefined dynamic_lookup).
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  # rustup installs cargo for login shells; non-login shells may need the env sourced.
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

cd "$(dirname "$0")/../crates"

cargo build --locked -p adriane-py

case "$(uname -s)" in
  Darwin) LIB_NAME="libadriane.dylib" ;;
  Linux) LIB_NAME="libadriane.so" ;;
  *) LIB_NAME="adriane.dll" ;;
esac

DEST="../python/adriane/adriane.abi3.so"
cp "target/debug/${LIB_NAME}" "$DEST"

echo "adriane-py dev build OK -> $(cd ../python/adriane && pwd)/adriane.abi3.so"
