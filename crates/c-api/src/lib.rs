//! Stable C ABI over the Adriane Rust engine.
//!
//! The contract is deliberately small and language-neutral: UTF-8 C strings in,
//! owned UTF-8 C strings out, and one explicit free function. Higher-level SDKs
//! should keep their ergonomic builders locally, then cross this boundary with
//! JSON/YAML documents so every language uses the same Rust validator/compiler.

#![deny(clippy::all)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

use adriane_sdk_core as core;

/// Call completed successfully.
pub const ADRIANE_OK: c_int = 0;
/// The caller passed a null pointer.
pub const ADRIANE_ERR_NULL: c_int = 1;
/// The caller passed bytes that are not valid UTF-8.
pub const ADRIANE_ERR_UTF8: c_int = 2;
/// The caller passed malformed JSON/YAML or the engine rejected the document.
pub const ADRIANE_ERR_INPUT: c_int = 3;
/// The engine produced a value that could not be serialized at the C boundary.
pub const ADRIANE_ERR_INTERNAL: c_int = 4;

/// Result returned by every fallible Adriane C-ABI function.
///
/// On success, `code == ADRIANE_OK`, `value` points to an owned null-terminated
/// UTF-8 string, and `error == NULL`.
///
/// On failure, `value == NULL` and `error` points to an owned null-terminated
/// UTF-8 string. The caller must release the returned allocation with
/// `adriane_result_free` or `adriane_string_free`.
#[repr(C)]
pub struct AdrianeResult {
    pub code: c_int,
    pub value: *mut c_char,
    pub error: *mut c_char,
}

impl AdrianeResult {
    fn ok(value: String) -> Self {
        AdrianeResult {
            code: ADRIANE_OK,
            value: into_c_string(value),
            error: ptr::null_mut(),
        }
    }

    fn err(code: c_int, error: impl Into<String>) -> Self {
        AdrianeResult {
            code,
            value: ptr::null_mut(),
            error: into_c_string(error.into()),
        }
    }
}

/// Version of the bound Rust engine.
///
/// The returned string is owned by the caller and must be released with
/// `adriane_string_free`.
#[no_mangle]
pub extern "C" fn adriane_engine_version() -> *mut c_char {
    into_c_string(core::engine_version())
}

/// Validate a graph definition JSON document.
///
/// Returns a JSON array of validation errors; an empty array means the graph is
/// structurally sound.
///
/// # Safety
///
/// `definition_json` must be a valid, null-terminated UTF-8 C string pointer.
#[no_mangle]
pub unsafe extern "C" fn adriane_validate_graph_json(
    definition_json: *const c_char,
) -> AdrianeResult {
    unsafe {
        with_c_str(definition_json, |raw| {
            core::validate_graph_json(raw).map_err(|error| (ADRIANE_ERR_INPUT, error))
        })
    }
}

/// Compile Adriane graph DSL YAML into a validated `GraphDefinition` JSON document.
///
/// # Safety
///
/// `yaml` must be a valid, null-terminated UTF-8 C string pointer.
#[no_mangle]
pub unsafe extern "C" fn adriane_compile_graph_yaml_json(yaml: *const c_char) -> AdrianeResult {
    unsafe {
        with_c_str(yaml, |raw| {
            core::compile_graph_yaml(raw).map_err(|error| (ADRIANE_ERR_INPUT, error))
        })
    }
}

/// Return the providers usable in the current process env as a JSON array.
#[no_mangle]
pub extern "C" fn adriane_available_providers_json() -> AdrianeResult {
    from_core(core::available_providers())
}

/// Resolve a capability tier to a concrete model choice JSON document.
///
/// `available_json` may be `NULL`; when present it must be a JSON array of
/// provider strings. `override_json` may be `NULL`; when present it must be
/// `{ "provider"?: string, "model"?: string }`.
///
/// # Safety
///
/// `tier` must be a valid, null-terminated UTF-8 C string pointer. Optional
/// pointers must be either `NULL` or valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn adriane_resolve_model_json(
    tier: *const c_char,
    available_json: *const c_char,
    override_json: *const c_char,
) -> AdrianeResult {
    let tier = match unsafe { read_required_c_str(tier) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let available = match unsafe { read_optional_c_str(available_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let override_ = match unsafe { read_optional_c_str(override_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };

    from_core(core::resolve_model(tier, available, override_))
}

/// Return every native component kind as a JSON array.
#[no_mangle]
pub extern "C" fn adriane_list_components_json() -> AdrianeResult {
    from_core(core::list_components())
}

/// Return every prebuilt micro-agent definition as JSON.
#[no_mangle]
pub extern "C" fn adriane_list_prebuilt_json() -> AdrianeResult {
    from_core(core::list_prebuilt())
}

/// Run a native component handler fully on Rust.
///
/// # Safety
///
/// All pointers must be valid, null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn adriane_run_component_json(
    kind: *const c_char,
    params_json: *const c_char,
    channels_json: *const c_char,
) -> AdrianeResult {
    let kind = match unsafe { read_required_c_str(kind) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let params = match unsafe { read_required_c_str(params_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let channels = match unsafe { read_required_c_str(channels_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };

    from_core(core::run_component(kind, params, channels))
}

/// Run a prebuilt micro-agent fully on Rust.
///
/// `options_json` may be `NULL`; when present it must be
/// `{ "provider"?: string, "model"?: string }`.
///
/// # Safety
///
/// Required pointers must be valid, null-terminated UTF-8 C strings. The optional
/// pointer must be either `NULL` or a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn adriane_run_prebuilt_json(
    name: *const c_char,
    input_json: *const c_char,
    options_json: *const c_char,
) -> AdrianeResult {
    let name = match unsafe { read_required_c_str(name) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let input = match unsafe { read_required_c_str(input_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };
    let options = match unsafe { read_optional_c_str(options_json) } {
        Ok(value) => value,
        Err(result) => return result,
    };

    from_core(core::run_prebuilt(name, input, options))
}

/// Free a string returned by the Adriane C ABI.
///
/// Passing `NULL` is allowed.
///
/// # Safety
///
/// `ptr` must be either `NULL` or a pointer previously returned by the Adriane C
/// ABI that has not already been freed.
#[no_mangle]
pub unsafe extern "C" fn adriane_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(ptr));
    }
}

/// Free both string fields carried by an `AdrianeResult`.
///
/// Passing a zeroed or already-empty result is allowed. Do not use the pointers
/// after calling this function.
///
/// # Safety
///
/// Any non-null pointer in `result` must have been returned by the Adriane C ABI
/// and must not already have been freed.
#[no_mangle]
pub unsafe extern "C" fn adriane_result_free(result: AdrianeResult) {
    unsafe {
        adriane_string_free(result.value);
        adriane_string_free(result.error);
    }
}

unsafe fn with_c_str(
    input: *const c_char,
    f: impl FnOnce(&str) -> Result<String, (c_int, String)>,
) -> AdrianeResult {
    let input = match unsafe { read_required_c_str(input) } {
        Ok(input) => input,
        Err(result) => return result,
    };

    match f(input) {
        Ok(value) => AdrianeResult::ok(value),
        Err((code, error)) => AdrianeResult::err(code, error),
    }
}

unsafe fn read_required_c_str<'a>(input: *const c_char) -> Result<&'a str, AdrianeResult> {
    if input.is_null() {
        return Err(AdrianeResult::err(
            ADRIANE_ERR_NULL,
            "input pointer must not be null",
        ));
    }

    unsafe { CStr::from_ptr(input) }.to_str().map_err(|error| {
        AdrianeResult::err(
            ADRIANE_ERR_UTF8,
            format!("input is not valid UTF-8: {error}"),
        )
    })
}

unsafe fn read_optional_c_str<'a>(input: *const c_char) -> Result<Option<&'a str>, AdrianeResult> {
    if input.is_null() {
        return Ok(None);
    }
    unsafe { read_required_c_str(input) }.map(Some)
}

fn from_core(result: Result<String, String>) -> AdrianeResult {
    match result {
        Ok(value) => AdrianeResult::ok(value),
        Err(error) => AdrianeResult::err(ADRIANE_ERR_INPUT, error),
    }
}

fn into_c_string(value: String) -> *mut c_char {
    let nul_safe = value.replace('\0', "\\0");
    CString::new(nul_safe)
        .expect("internal NULs were escaped before building CString")
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_version_string() {
        let ptr = adriane_engine_version();
        assert!(!ptr.is_null());
        let version = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        assert!(!version.is_empty());
        unsafe {
            adriane_string_free(ptr);
        }
    }

    #[test]
    fn validates_input_json_errors() {
        let input = CString::new("{").unwrap();
        let result = unsafe { adriane_validate_graph_json(input.as_ptr()) };

        assert_eq!(result.code, ADRIANE_ERR_INPUT);
        assert!(result.value.is_null());
        assert!(!result.error.is_null());

        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(error.contains("invalid graph JSON"));
        unsafe {
            adriane_result_free(result);
        }
    }

    #[test]
    fn rejects_null_input() {
        let result = unsafe { adriane_compile_graph_yaml_json(ptr::null()) };

        assert_eq!(result.code, ADRIANE_ERR_NULL);
        assert!(result.value.is_null());
        assert!(!result.error.is_null());

        unsafe {
            adriane_result_free(result);
        }
    }

    #[test]
    fn resolves_model_with_optional_inputs() {
        let tier = CString::new("fast").unwrap();
        let available = CString::new("[\"mistral\"]").unwrap();
        let result =
            unsafe { adriane_resolve_model_json(tier.as_ptr(), available.as_ptr(), ptr::null()) };

        assert_eq!(result.code, ADRIANE_OK);
        assert!(!result.value.is_null());
        let value = unsafe { CStr::from_ptr(result.value) }.to_str().unwrap();
        assert!(value.contains("\"provider\":\"mistral\""));
        assert!(value.contains("\"model\":\"mistral-small-latest\""));

        unsafe {
            adriane_result_free(result);
        }
    }

    #[test]
    fn exposes_catalogs() {
        let components = adriane_list_components_json();
        assert_eq!(components.code, ADRIANE_OK);
        let components_json = unsafe { CStr::from_ptr(components.value) }
            .to_str()
            .unwrap();
        assert!(components_json.contains("promptBuilder"));
        unsafe {
            adriane_result_free(components);
        }

        let prebuilt = adriane_list_prebuilt_json();
        assert_eq!(prebuilt.code, ADRIANE_OK);
        let prebuilt_json = unsafe { CStr::from_ptr(prebuilt.value) }.to_str().unwrap();
        assert!(prebuilt_json.contains("summarizer"));
        unsafe {
            adriane_result_free(prebuilt);
        }
    }

    #[test]
    fn runs_component() {
        let kind = CString::new("promptBuilder").unwrap();
        let params =
            CString::new("{\"template\":\"Hello {{name}}!\",\"into\":\"prompt\"}").unwrap();
        let channels = CString::new("{\"name\":\"Ada\"}").unwrap();
        let result = unsafe {
            adriane_run_component_json(kind.as_ptr(), params.as_ptr(), channels.as_ptr())
        };

        assert_eq!(result.code, ADRIANE_OK);
        let output = unsafe { CStr::from_ptr(result.value) }.to_str().unwrap();
        assert_eq!(output, "{\"prompt\":\"Hello Ada!\"}");

        unsafe {
            adriane_result_free(result);
        }
    }
}
