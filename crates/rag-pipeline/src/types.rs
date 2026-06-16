//! Core data shapes for the RAG pipeline — the Rust port of `types.ts`.
//!
//! All shapes serialize with `camelCase` field names so they are wire-compatible
//! with the TypeScript model where data crosses an API boundary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An embedding is a dense numeric vector. The TS model uses `number[]`
/// (JavaScript doubles), so the Rust port uses `Vec<f64>`.
pub type Embedding = Vec<f64>;

/// A loaded document: an opaque id, its textual `content`, free-form
/// `metadata`, and an optional `embedding`.
///
/// Mirrors the TS `Document` type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Stable identifier for the document.
    pub id: String,
    /// Raw textual content.
    pub content: String,
    /// Arbitrary key/value metadata. Kept ordered (`BTreeMap`) for
    /// deterministic serialization.
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    /// Optional precomputed embedding for this document.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub embedding: Option<Embedding>,
}

impl Document {
    /// Build a document with empty metadata and no embedding.
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            metadata: BTreeMap::new(),
            embedding: None,
        }
    }
}

/// A chunk is a `Document` produced by a splitter, carrying back-references to
/// the source document and its position within it.
///
/// Mirrors the TS `Chunk = Document & { sourceId; chunkIndex }`. The TS type is
/// an intersection, so the chunk has the full `Document` field set inlined; we
/// flatten an embedded [`Document`] with `#[serde(flatten)]` to reproduce that
/// exact wire shape (`id`, `content`, `metadata`, `embedding`, `sourceId`,
/// `chunkIndex`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    /// The document fields (`id`, `content`, `metadata`, `embedding`).
    #[serde(flatten)]
    pub document: Document,
    /// The id of the document this chunk was derived from.
    pub source_id: String,
    /// The zero-based index of this chunk within its source document.
    pub chunk_index: usize,
}

impl Chunk {
    /// Borrow the chunk's id (the document id).
    pub fn id(&self) -> &str {
        &self.document.id
    }

    /// Borrow the chunk's textual content.
    pub fn content(&self) -> &str {
        &self.document.content
    }

    /// Borrow the chunk's embedding, if present.
    pub fn embedding(&self) -> Option<&Embedding> {
        self.document.embedding.as_ref()
    }
}

/// Configuration for a [`crate::Splitter`]: the maximum chunk size and the
/// number of characters/tokens to overlap between adjacent chunks.
///
/// Mirrors the TS `SplitConfig`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitConfig {
    /// Target maximum size of a chunk (characters for the character splitter,
    /// tokens for the token splitter).
    pub chunk_size: usize,
    /// Amount of overlap carried from the previous chunk into the next.
    pub chunk_overlap: usize,
}

/// A scored chunk returned from a vector search or retriever.
///
/// Mirrors the TS `RetrievalResult`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalResult {
    /// The matched chunk.
    pub chunk: Chunk,
    /// The similarity / relevance score (higher is more relevant).
    pub score: f64,
}
