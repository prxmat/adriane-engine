//! Loaders — the `DocumentLoader` seam plus the per-format implementations.
//!
//! Ports `loaders/document-loader.ts` and the markdown/json/csv/html/pdf
//! loaders. Each TS loader takes an `input` that is either a file path (read via
//! `node:fs`) or, on read failure, treated as inline content. The Rust port
//! keeps that exact fallback: `resolve_input` tries to read the path and falls
//! back to the literal string, so the deterministic transforms remain testable
//! offline by passing inline content.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RagError;
use crate::types::Document;

/// The loader seam — produce a batch of documents.
///
/// Mirrors the TS `DocumentLoader` interface (`load(): Promise<Document[]>`).
#[async_trait]
pub trait DocumentLoader: Send + Sync {
    /// Load and parse the loader's input into documents.
    async fn load(&self) -> Result<Vec<Document>, RagError>;
}

/// Try to read `input` as a file path; on any failure, treat `input` itself as
/// the content. Mirrors the TS `resolveInput` (`try readFile … catch return
/// input`).
fn resolve_input(input: &str) -> String {
    std::fs::read_to_string(input).unwrap_or_else(|_| input.to_string())
}

/// PDF-loader variant of `resolveInput`: only attempt a file read when the input
/// looks like a path (no embedded newline or space). Mirrors the TS
/// `pdf-loader.ts` `resolveInput`.
fn resolve_input_pdf(input: &str) -> String {
    if input.contains('\n') || input.contains(' ') {
        return input.to_string();
    }
    std::fs::read_to_string(input).unwrap_or_else(|_| input.to_string())
}

/// Build a metadata map from `(key, value)` pairs.
fn metadata(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Loads a single markdown document. Faithful port of `MarkdownLoader`.
pub struct MarkdownLoader {
    input: String,
}

impl MarkdownLoader {
    /// Create a markdown loader for `input` (a path or inline content).
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

#[async_trait]
impl DocumentLoader for MarkdownLoader {
    async fn load(&self) -> Result<Vec<Document>, RagError> {
        let content = resolve_input(&self.input);
        Ok(vec![Document {
            id: "md:0".to_string(),
            content,
            metadata: metadata([("loader", Value::from("markdown"))]),
            embedding: None,
        }])
    }
}

/// Loads a single HTML document, stripping tags. Faithful port of `HtmlLoader`.
pub struct HtmlLoader {
    input: String,
}

impl HtmlLoader {
    /// Create an HTML loader for `input` (a path or inline content).
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

/// Strip HTML tags and collapse whitespace, mirroring the TS `stripHtml`
/// (`replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim()`).
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => {
                in_tag = false;
            }
            _ if in_tag => {}
            _ => out.push(ch),
        }
    }
    // Collapse runs of whitespace into a single space and trim.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[async_trait]
impl DocumentLoader for HtmlLoader {
    async fn load(&self) -> Result<Vec<Document>, RagError> {
        let content = strip_html(&resolve_input(&self.input));
        Ok(vec![Document {
            id: "html:0".to_string(),
            content,
            metadata: metadata([("loader", Value::from("html"))]),
            embedding: None,
        }])
    }
}

/// Loads a single PDF-ish document. Faithful port of `PdfLoader`.
pub struct PdfLoader {
    input: String,
}

impl PdfLoader {
    /// Create a PDF loader for `input` (a path or inline content).
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

#[async_trait]
impl DocumentLoader for PdfLoader {
    async fn load(&self) -> Result<Vec<Document>, RagError> {
        let content = resolve_input_pdf(&self.input);
        Ok(vec![Document {
            id: "pdf:0".to_string(),
            content,
            metadata: metadata([("loader", Value::from("pdf"))]),
            embedding: None,
        }])
    }
}

/// Loads a CSV file as one document per non-blank line. Faithful port of
/// `CsvLoader`.
pub struct CsvLoader {
    input: String,
}

impl CsvLoader {
    /// Create a CSV loader for `input` (a path or inline content).
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

#[async_trait]
impl DocumentLoader for CsvLoader {
    async fn load(&self) -> Result<Vec<Document>, RagError> {
        let csv = resolve_input(&self.input);
        // `csv.split(/\r?\n/).filter(line => line.trim().length > 0)`
        let docs: Vec<Document> = csv
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .filter(|line| !line.trim().is_empty())
            .enumerate()
            .map(|(index, line)| Document {
                id: format!("csv:{index}"),
                content: line.to_string(),
                metadata: metadata([("loader", Value::from("csv")), ("row", Value::from(index))]),
                embedding: None,
            })
            .collect();
        Ok(docs)
    }
}

/// Loads a JSON document as one document per top-level array element (or the
/// whole value if it is not an array). Faithful port of `JsonLoader`.
pub struct JsonLoader {
    input: String,
}

impl JsonLoader {
    /// Create a JSON loader for `input` (a path or inline content).
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

#[async_trait]
impl DocumentLoader for JsonLoader {
    async fn load(&self) -> Result<Vec<Document>, RagError> {
        let raw = resolve_input(&self.input);
        let parsed: Value = serde_json::from_str(&raw)?;
        // `const rows = Array.isArray(parsed) ? parsed : [parsed];`
        let rows: Vec<Value> = match parsed {
            Value::Array(items) => items,
            other => vec![other],
        };
        let docs: Vec<Document> = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                // `JSON.stringify(row)` — compact, no spaces.
                let content = serde_json::to_string(&row).unwrap_or_default();
                Document {
                    id: format!("json:{index}"),
                    content,
                    metadata: metadata([
                        ("loader", Value::from("json")),
                        ("index", Value::from(index)),
                    ]),
                    embedding: None,
                }
            })
            .collect();
        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn markdown_loader_uses_inline_content() {
        let loader = MarkdownLoader::new("# Title\n\nBody text.");
        let docs = loader.load().await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].id, "md:0");
        assert_eq!(docs[0].content, "# Title\n\nBody text.");
        assert_eq!(docs[0].metadata["loader"], Value::from("markdown"));
    }

    #[tokio::test]
    async fn html_loader_strips_tags() {
        let loader = HtmlLoader::new("<p>Hello <b>world</b></p>");
        let docs = loader.load().await.unwrap();
        assert_eq!(docs[0].content, "Hello world");
    }

    #[tokio::test]
    async fn csv_loader_one_doc_per_line() {
        let loader = CsvLoader::new("a,b\n\nc,d\n");
        let docs = loader.load().await.unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].id, "csv:0");
        assert_eq!(docs[0].content, "a,b");
        assert_eq!(docs[1].id, "csv:1");
        assert_eq!(docs[1].content, "c,d");
        assert_eq!(docs[1].metadata["row"], Value::from(1usize));
    }

    #[tokio::test]
    async fn json_loader_array_rows() {
        let loader = JsonLoader::new(r#"[{"a":1},{"b":2}]"#);
        let docs = loader.load().await.unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].content, r#"{"a":1}"#);
        assert_eq!(docs[1].metadata["index"], Value::from(1usize));
    }

    #[tokio::test]
    async fn json_loader_single_object() {
        let loader = JsonLoader::new(r#"{"a":1}"#);
        let docs = loader.load().await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].id, "json:0");
    }
}
