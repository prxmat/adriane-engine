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

# On Apple Silicon, copying a freshly-linked Mach-O dylib to a new path invalidates its
# linker ad-hoc code signature, so dyld SIGKILLs the process with "Code Signature
# Invalid" the moment Node dlopen()s it. Re-sign the copy ad-hoc to make it loadable.
# (The `napi build` CLI does this for us; a plain `cp` does not.) No-op off macOS.
if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - "$DEST"
fi

echo "adriane-napi dev build OK -> $(pwd)/${DEST}"
