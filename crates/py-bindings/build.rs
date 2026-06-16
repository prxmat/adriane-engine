//! Build script for the pyo3 extension module.
//!
//! On macOS, a `cdylib` that resolves CPython symbols (`Py_*`) at import time must
//! be linked with `-undefined dynamic_lookup` so those symbols are deferred to the
//! host interpreter instead of being resolved against libpython at link time. The
//! `extension-module` feature keeps us from linking libpython, but the linker still
//! needs to be told the undefined symbols are intentional. We emit the flag here so
//! it is scoped to THIS crate only and never leaks into sibling crates.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-undefined");
        println!("cargo:rustc-link-arg=dynamic_lookup");
    }
}
