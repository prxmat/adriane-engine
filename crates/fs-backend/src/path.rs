//! Path normalization — the sole barrier (with the store-controlled `run_id`
//! prefix as defense-in-depth) against traversal at the artifact-name layer.
//! Fail-closed: anything suspicious is rejected, never silently rewritten across
//! a boundary.

use crate::types::FsError;

/// Normalize an agent-supplied path into a canonical forward-slash artifact name,
/// or reject it. Rejects null bytes, backslashes, absolute paths, and `..` parent
/// traversal; collapses `.` and empty segments. The result never contains `..`,
/// never starts with `/`, and is non-empty.
pub fn normalize_path(raw: &str) -> Result<String, FsError> {
    let invalid = |reason: &str| FsError::InvalidPath {
        reason: reason.to_owned(),
    };
    if raw.contains('\0') {
        return Err(invalid("null byte"));
    }
    if raw.contains('\\') {
        return Err(invalid("backslash separator"));
    }
    if raw.starts_with('/') {
        return Err(invalid("absolute path"));
    }
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err(invalid("parent traversal (..)")),
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return Err(invalid("empty path"));
    }
    Ok(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_clean_relative_path() {
        assert_eq!(normalize_path("a/b/c.txt").unwrap(), "a/b/c.txt");
    }

    #[test]
    fn collapses_dot_and_empty_segments() {
        assert_eq!(normalize_path("a/./b//c").unwrap(), "a/b/c");
        assert_eq!(normalize_path("./notes.md").unwrap(), "notes.md");
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(matches!(
            normalize_path("a/../../etc/passwd"),
            Err(FsError::InvalidPath { .. })
        ));
        assert!(matches!(
            normalize_path("../secret"),
            Err(FsError::InvalidPath { .. })
        ));
    }

    #[test]
    fn rejects_absolute_backslash_null_and_empty() {
        assert!(matches!(
            normalize_path("/abs"),
            Err(FsError::InvalidPath { .. })
        ));
        assert!(matches!(
            normalize_path("a\\b"),
            Err(FsError::InvalidPath { .. })
        ));
        assert!(matches!(
            normalize_path("a\0b"),
            Err(FsError::InvalidPath { .. })
        ));
        assert!(matches!(
            normalize_path("///"),
            Err(FsError::InvalidPath { .. })
        ));
    }
}
