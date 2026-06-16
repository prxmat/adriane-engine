//! Error type shared across the RAG pipeline seams.

use thiserror::Error;

/// Errors a pipeline seam (embedder, vector store, retriever, reranker, loader)
/// can surface.
///
/// The in-memory/mock implementations in this crate never fail, but the seams
/// are fallible so that real backends (an embedding API, a vector database, an
/// LLM-backed reranker, a filesystem loader) can report I/O or parse failures
/// without changing the trait contract.
#[derive(Debug, Error)]
pub enum RagError {
    /// An embedding backend failed.
    #[error("embedding error: {0}")]
    Embedding(String),
    /// A vector store operation failed.
    #[error("vector store error: {0}")]
    VectorStore(String),
    /// A retriever step failed.
    #[error("retriever error: {0}")]
    Retriever(String),
    /// A reranker step failed.
    #[error("reranker error: {0}")]
    Reranker(String),
    /// A document loader failed.
    #[error("loader error: {0}")]
    Loader(String),
    /// A value could not be (de)serialized.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
