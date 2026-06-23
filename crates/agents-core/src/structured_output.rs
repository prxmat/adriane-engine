//! Structured-output validation (ADR 0029, phase 8).
//!
//! The in-engine validation floor for [`crate::middleware::StructuredOutputMiddleware`]:
//! pull the first JSON value out of a model response and check it against a JSON Schema.
//! Validation runs HERE, on the deterministic Rust engine — never only in the SDK — so a
//! worker-executed run can never emit unvalidated output and the verdict is part of the
//! checkpointed, auditable run state.

use serde_json::Value;

/// Extract the first complete JSON value (object or array) embedded in `text`.
///
/// Models wrap JSON in prose or ```json fences; this scans for the first `{`/`[`, then
/// returns the smallest balanced span starting there, ignoring braces inside strings.
/// Returns `None` when no balanced JSON value is present.
pub fn extract_first_json(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{' || b == b'[')?;
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    let span = &text[start..=start + offset];
                    return serde_json::from_str(span).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// Validate `instance` against the JSON Schema `schema`.
///
/// `Ok(())` on conformance; `Err(message)` with the first violation otherwise. An invalid
/// SCHEMA (not instance) is itself reported as an error rather than panicking.
pub fn validate_json(schema: &Value, instance: &Value) -> Result<(), String> {
    let validator =
        jsonschema::validator_for(schema).map_err(|e| format!("invalid output schema: {e}"))?;
    // Extract the message before the borrowing iterator/validator drop.
    let first_error = validator
        .iter_errors(instance)
        .next()
        .map(|e| e.to_string());
    match first_error {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_plain_object() {
        let v = extract_first_json(r#"{"a":1}"#).unwrap();
        assert_eq!(v, json!({ "a": 1 }));
    }

    #[test]
    fn extracts_object_from_prose_and_fences() {
        let v = extract_first_json("Here you go:\n```json\n{\"ok\": true}\n```\nthanks").unwrap();
        assert_eq!(v, json!({ "ok": true }));
    }

    #[test]
    fn ignores_braces_inside_strings() {
        let v = extract_first_json(r#"{"text":"a } b { c"}"#).unwrap();
        assert_eq!(v, json!({ "text": "a } b { c" }));
    }

    #[test]
    fn extracts_array() {
        let v = extract_first_json("result: [1, 2, 3] done").unwrap();
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn none_when_no_json() {
        assert!(extract_first_json("no json here").is_none());
        assert!(extract_first_json("{ unbalanced").is_none());
    }

    #[test]
    fn validates_nested_schema_pass_and_fail() {
        let schema = json!({
            "type": "object",
            "properties": {
                "score": { "type": "integer", "minimum": 0, "maximum": 10 },
                "label": { "type": "string", "enum": ["ok", "weak"] }
            },
            "required": ["score", "label"]
        });
        assert!(validate_json(&schema, &json!({ "score": 7, "label": "ok" })).is_ok());
        // enum violation
        assert!(validate_json(&schema, &json!({ "score": 7, "label": "nope" })).is_err());
        // range violation (nested keyword the flat validator could not catch)
        assert!(validate_json(&schema, &json!({ "score": 99, "label": "ok" })).is_err());
        // missing required field
        assert!(validate_json(&schema, &json!({ "score": 1 })).is_err());
    }

    #[test]
    fn invalid_schema_is_reported_not_panicked() {
        let bad_schema = json!({ "type": 123 });
        assert!(validate_json(&bad_schema, &json!({})).is_err());
    }
}
