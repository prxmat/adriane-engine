//! Vector store — the `VectorStore` seam plus the in-memory cosine store.
//!
//! Ports `vector-store/vector-store.ts` (the `VectorStore` interface) and
//! `vector-store/in-memory-vector-store.ts`.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::RagError;
use crate::types::{Chunk, Embedding, RetrievalResult};

/// The vector-store seam — upsert chunks (which carry embeddings) and search by
/// a query embedding.
///
/// Mirrors the TS `VectorStore` interface (`upsert`, `search`). Async +
/// `Send + Sync` so a store can be shared as `Arc<dyn VectorStore>`.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Insert or replace the given chunks (keyed by chunk id).
    async fn upsert(&self, chunks: Vec<Chunk>) -> Result<(), RagError>;

    /// Return the `top_k` chunks most similar to `embedding`, scored.
    async fn search(
        &self,
        embedding: &Embedding,
        top_k: usize,
    ) -> Result<Vec<RetrievalResult>, RagError>;
}

/// Cosine similarity between two vectors, padding the shorter with zeros.
///
/// Faithful port of the TS `cosineSimilarity`: iterate over `max(a.len, b.len)`,
/// treat missing components as `0`, and return `0` when either vector has zero
/// norm.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let size = a.len().max(b.len());
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for i in 0..size {
        let av = a.get(i).copied().unwrap_or(0.0);
        let bv = b.get(i).copied().unwrap_or(0.0);
        dot += av * bv;
        norm_a += av * av;
        norm_b += bv * bv;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// In-memory vector store backed by an insertion-ordered chunk map.
///
/// Faithful port of the TS `InMemoryVectorStore`: chunks are keyed by id
/// (overwrite on re-upsert), and `search` scores every chunk by cosine
/// similarity, sorts by score descending, and returns the top `top_k`.
/// Insertion order is preserved (mirroring JS `Map` iteration) so ties resolve
/// deterministically.
#[derive(Default)]
pub struct InMemoryVectorStore {
    /// `(chunkId, chunk)` pairs in insertion order, like a JS `Map`.
    chunks: Mutex<Vec<(String, Chunk)>>,
}

impl InMemoryVectorStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(&self, chunks: Vec<Chunk>) -> Result<(), RagError> {
        let mut guard = self.chunks.lock().expect("vector store mutex poisoned");
        for chunk in chunks {
            let id = chunk.id().to_string();
            if let Some(slot) = guard.iter_mut().find(|(k, _)| *k == id) {
                slot.1 = chunk;
            } else {
                guard.push((id, chunk));
            }
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &Embedding,
        top_k: usize,
    ) -> Result<Vec<RetrievalResult>, RagError> {
        let guard = self.chunks.lock().expect("vector store mutex poisoned");
        let mut results: Vec<RetrievalResult> = guard
            .iter()
            .map(|(_, chunk)| {
                let stored = chunk.embedding().map(Vec::as_slice).unwrap_or(&[]);
                RetrievalResult {
                    chunk: chunk.clone(),
                    score: cosine_similarity(embedding, stored),
                }
            })
            .collect();
        // `.sort((l, r) => r.score - l.score)` — descending by score. A stable
        // sort preserves insertion order on ties, matching JS `Array.sort`'s
        // stability guarantee.
        results.sort_by(|l, r| {
            r.score
                .partial_cmp(&l.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_k);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Document;

    fn chunk(id: &str, source: &str, index: usize, embedding: Vec<f64>) -> Chunk {
        Chunk {
            document: Document {
                id: id.to_string(),
                content: id.to_string(),
                metadata: Default::default(),
                embedding: Some(embedding),
            },
            source_id: source.to_string(),
            chunk_index: index,
        }
    }

    #[test]
    fn cosine_identical_vectors_is_one() {
        assert!((cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cosine_zero_norm_is_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[tokio::test]
    async fn search_ranks_planted_relevant_chunk_first() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![
                chunk("c1", "d1", 0, vec![1.0, 0.0, 0.0]),
                chunk("c2", "d1", 1, vec![0.0, 1.0, 0.0]),
            ])
            .await
            .unwrap();
        let results = store.search(&vec![1.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id(), "c1");
    }

    #[tokio::test]
    async fn upsert_overwrites_by_id() {
        let store = InMemoryVectorStore::new();
        store
            .upsert(vec![chunk("c1", "d1", 0, vec![1.0, 0.0])])
            .await
            .unwrap();
        store
            .upsert(vec![chunk("c1", "d1", 0, vec![0.0, 1.0])])
            .await
            .unwrap();
        let results = store.search(&vec![0.0, 1.0], 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!((results[0].score - 1.0).abs() < 1e-12);
    }
}
