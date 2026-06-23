//! Apply line-based [`EditOp`]s to text content. Ops are 1-indexed inclusive and
//! applied **in order**, each seeing the result of the previous one. Out-of-range
//! line references fail-closed with [`FsError::InvalidEdit`].

use crate::types::{EditOp, FsError};

/// Apply `patches` to `content` (UTF-8 text), returning the new text. Lines are
/// split on `\n`; the result is re-joined with `\n`.
pub fn apply_edits(content: &str, patches: &[EditOp]) -> Result<String, FsError> {
    let mut lines: Vec<String> = if content.is_empty() {
        Vec::new()
    } else {
        content.split('\n').map(str::to_owned).collect()
    };

    let invalid = |reason: String| FsError::InvalidEdit { reason };
    let to_lines = |text: &str| -> Vec<String> { text.split('\n').map(str::to_owned).collect() };

    for patch in patches {
        match patch {
            EditOp::Replace {
                start_line,
                end_line,
                text,
            } => {
                if *start_line == 0 || start_line > end_line || *end_line > lines.len() {
                    return Err(invalid(format!(
                        "replace {start_line}..={end_line} out of range (len {})",
                        lines.len()
                    )));
                }
                lines.splice((start_line - 1)..=(end_line - 1), to_lines(text));
            }
            EditOp::Insert { after_line, text } => {
                if *after_line > lines.len() {
                    return Err(invalid(format!(
                        "insert after {after_line} out of range (len {})",
                        lines.len()
                    )));
                }
                let at = *after_line; // 0 = prepend; n = after the nth line
                lines.splice(at..at, to_lines(text));
            }
            EditOp::Delete {
                start_line,
                end_line,
            } => {
                if *start_line == 0 || start_line > end_line || *end_line > lines.len() {
                    return Err(invalid(format!(
                        "delete {start_line}..={end_line} out of range (len {})",
                        lines.len()
                    )));
                }
                lines.drain((start_line - 1)..=(end_line - 1));
            }
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_a_line_range() {
        let out = apply_edits(
            "a\nb\nc",
            &[EditOp::Replace {
                start_line: 2,
                end_line: 2,
                text: "B".to_owned(),
            }],
        )
        .unwrap();
        assert_eq!(out, "a\nB\nc");
    }

    #[test]
    fn inserts_after_a_line_and_prepends_at_zero() {
        assert_eq!(
            apply_edits(
                "a\nc",
                &[EditOp::Insert {
                    after_line: 1,
                    text: "b".to_owned()
                }]
            )
            .unwrap(),
            "a\nb\nc"
        );
        assert_eq!(
            apply_edits(
                "a",
                &[EditOp::Insert {
                    after_line: 0,
                    text: "first".to_owned()
                }]
            )
            .unwrap(),
            "first\na"
        );
    }

    #[test]
    fn deletes_a_line_range() {
        let out = apply_edits(
            "a\nb\nc\nd",
            &[EditOp::Delete {
                start_line: 2,
                end_line: 3,
            }],
        )
        .unwrap();
        assert_eq!(out, "a\nd");
    }

    #[test]
    fn applies_ops_in_order_on_current_numbering() {
        // Delete line 1 (-> "b\nc"), then replace the new line 1.
        let out = apply_edits(
            "a\nb\nc",
            &[
                EditOp::Delete {
                    start_line: 1,
                    end_line: 1,
                },
                EditOp::Replace {
                    start_line: 1,
                    end_line: 1,
                    text: "B".to_owned(),
                },
            ],
        )
        .unwrap();
        assert_eq!(out, "B\nc");
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(matches!(
            apply_edits(
                "a",
                &[EditOp::Replace {
                    start_line: 5,
                    end_line: 6,
                    text: "x".to_owned()
                }]
            ),
            Err(FsError::InvalidEdit { .. })
        ));
        assert!(matches!(
            apply_edits(
                "",
                &[EditOp::Delete {
                    start_line: 1,
                    end_line: 1
                }]
            ),
            Err(FsError::InvalidEdit { .. })
        ));
    }
}
