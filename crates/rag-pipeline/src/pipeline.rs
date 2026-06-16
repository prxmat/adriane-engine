//! `RagPipeline` — an orchestrator that wires the seams together.
//!
//! The TypeScript package composes the steps ad hoc in callers (see
//! `rag-pipeline.test.ts`): load → split → embed → store → retrieve, with an
//! optional rerank. This orchestrator captures that wiring in one place over the
//! seam traits, so callers can swap any stage (a real embedder, a real vector
//! DB, an LLM reranker) without changing the flow. The default mock wiring is
//! deterministic and offline.

use std::sync::Arc;

use crate::embeddings::Embedder;
use crate::error::RagError;
use crate::loaders::DocumentLoader;
use crate::reranker::Reranker;
use crate::splitters::Splitter;
use crate::types::{Chunk, Document, RetrievalResult, SplitConfig};
use crate::vector_store::VectorStore;

/// Orchestrates the RAG flow over pluggable seams.
///
/// Holds a splitter, an embedder, and a vector store; `index` runs
/// load → split → embed → store, and `retrieve` embeds a query and searches.
pub struct RagPipeline {
    splitter: Box<dyn Splitter + Send + Sync>,
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn VectorStore>,
    split_config: SplitConfig,
    top_k: usize,
}

impl RagPipeline {
    /// Build a pipeline from its stages.
    pub fn new(
        splitter: Box<dyn Splitter + Send + Sync>,
        embedder: Arc<dyn Embedder>,
        store: Arc<dyn VectorStore>,
        split_config: SplitConfig,
        top_k: usize,
    ) -> Self {
        Self {
            splitter,
            embedder,
            store,
            split_config,
            top_k,
        }
    }

    /// Split `documents` into chunks, embed each chunk, and upsert them into the
    /// vector store. Returns the chunks that were stored (with embeddings).
    pub async fn index(&self, documents: &[Document]) -> Result<Vec<Chunk>, RagError> {
        // load (already loaded) -> split
        let mut chunks: Vec<Chunk> = Vec::new();
        for doc in documents {
            chunks.extend(self.splitter.split(doc, self.split_config));
        }

        // embed
        let texts: Vec<String> = chunks.iter().map(|c| c.content().to_string()).collect();
        let vectors = self.embedder.embed(&texts).await?;

        // attach embeddings, mirroring the TS
        // `chunks.map((chunk, index) => ({ ...chunk, embedding: vectors[index] }))`
        for (chunk, vector) in chunks.iter_mut().zip(vectors) {
            chunk.document.embedding = Some(vector);
        }

        // store
        self.store.upsert(chunks.clone()).await?;
        Ok(chunks)
    }

    /// Convenience: load documents from a loader, then [`index`](Self::index).
    pub async fn index_from(&self, loader: &dyn DocumentLoader) -> Result<Vec<Chunk>, RagError> {
        let documents = loader.load().await?;
        self.index(&documents).await
    }

    /// Embed `query` and return the top-`top_k` chunks by cosine similarity.
    pub async fn retrieve(&self, query: &str) -> Result<Vec<RetrievalResult>, RagError> {
        let embeddings = self.embedder.embed(&[query.to_string()]).await?;
        let Some(embedding) = embeddings.into_iter().next() else {
            return Ok(Vec::new());
        };
        self.store.search(&embedding, self.top_k).await
    }

    /// Retrieve, then rerank the results with the given reranker, returning the
    /// reranker's top-`top_k`.
    pub async fn retrieve_and_rerank(
        &self,
        query: &str,
        reranker: &dyn Reranker,
    ) -> Result<Vec<RetrievalResult>, RagError> {
        let retrieved = self.retrieve(query).await?;
        reranker.rerank(query, retrieved, self.top_k).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::MockEmbedder;
    use crate::splitters::RecursiveCharacterSplitter;
    use crate::vector_store::InMemoryVectorStore;

    fn mock_pipeline() -> RagPipeline {
        RagPipeline::new(
            Box::new(RecursiveCharacterSplitter::new()),
            Arc::new(MockEmbedder::new()),
            Arc::new(InMemoryVectorStore::new()),
            SplitConfig {
                chunk_size: 20,
                chunk_overlap: 3,
            },
            5,
        )
    }

    #[tokio::test]
    async fn end_to_end_load_split_embed_store_retrieve() {
        let pipeline = mock_pipeline();
        let docs = vec![Document::new(
            "d1",
            "Critical risk detected. General status update.",
        )];
        let chunks = pipeline.index(&docs).await.unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.embedding().is_some()));

        // Querying with text that buckets like the relevant chunk returns it.
        let results = pipeline.retrieve("Critical risk detected.").await.unwrap();
        assert!(!results.is_empty());
        // The top result is a chunk of the source document.
        assert_eq!(results[0].chunk.source_id, "d1");
    }

    #[tokio::test]
    async fn retrieve_top_result_is_planted_relevant_chunk() {
        // Two clearly distinct documents; the query equals one of them exactly,
        // so cosine similarity ranks that chunk first.
        let pipeline = RagPipeline::new(
            Box::new(RecursiveCharacterSplitter::new()),
            Arc::new(MockEmbedder::new()),
            Arc::new(InMemoryVectorStore::new()),
            SplitConfig {
                chunk_size: 200,
                chunk_overlap: 0,
            },
            2,
        );
        let docs = vec![
            Document::new("relevant", "aaaa"),
            Document::new("other", "zzzzzzzzzz"),
        ];
        pipeline.index(&docs).await.unwrap();
        let results = pipeline.retrieve("aaaa").await.unwrap();
        assert_eq!(results[0].chunk.source_id, "relevant");
        assert!((results[0].score - 1.0).abs() < 1e-12);
    }
}
