#!/usr/bin/env bash
# Reproducible dev build for the adriane-napi native addon.
# Builds the cdylib with cargo and copies it to crates/bindings/adriane_napi.node,
# which is what crates/bindings/index.js requires.
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  # rustup installs cargo for login shells; non-login shells (CI steps, npm scripts
  # from some environments) may need the env file sourced explicitly.
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

cd "$(dirname "$0")/../crates"

cargo build --locked -p adriane-napi

case "$(uname -s)" in
  Darwin) LIB_NAME="libadriane_napi.dylib" ;;
  Linux) LIB_NAME="libadriane_napi.so" ;;
  *) LIB_NAME="adriane_napi.dll" ;;
esac

DEST="bindings/adriane_napi.node"
cp "target/debug/${LIB_NAME}" "$DEST"

echo "adriane-napi dev build OK -> $(pwd)/${DEST}"
