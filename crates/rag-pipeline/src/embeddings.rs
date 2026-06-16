//! Embeddings — the `Embedder` seam and the deterministic `MockEmbedder`.
//!
//! Ports `embeddings/embeddings-adapter.ts` (the `EmbeddingsAdapter` interface)
//! and `embeddings/mock-embeddings-adapter.ts` (the offline mock).

use async_trait::async_trait;

use crate::error::RagError;
use crate::types::Embedding;

/// The embeddings seam — turns a batch of texts into a batch of vectors.
///
/// Mirrors the TS `EmbeddingsAdapter` interface
/// (`embed(texts: string[]): Promise<number[][]>`). Async + `Send + Sync` so an
/// adapter can be shared as `Arc<dyn Embedder>` across tasks.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed each input text, returning one vector per input in order.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, RagError>;
}

/// The number of buckets in the mock embedding vector. Matches the TS
/// `counts = [0, 0, 0, 0]`.
const MOCK_DIMS: usize = 4;

/// Deterministic, offline embedder used for tests and previews.
///
/// Faithful port of the TS `MockEmbeddingsAdapter`: each text maps to a
/// `MOCK_DIMS`-element vector where bucket `c % MOCK_DIMS` is incremented for
/// every character `c` (by its UTF-16/Unicode code point). The same text always
/// yields the same vector.
#[derive(Clone, Copy, Debug, Default)]
pub struct MockEmbedder;

impl MockEmbedder {
    /// Create a mock embedder.
    pub fn new() -> Self {
        Self
    }
}

/// Convert a single text into its deterministic mock vector.
///
/// Mirrors the TS `toVector`: iterate characters, bucket by
/// `charCodeAt(0) % counts.length`, and count occurrences. JavaScript's
/// `for (const char of text)` iterates Unicode code points (the same units
/// Rust's `char` yields), and `charCodeAt(0)` of that code point's first UTF-16
/// unit equals the code point for the Basic Multilingual Plane and the high
/// surrogate otherwise — for the ASCII test fixtures these coincide exactly.
fn to_vector(text: &str) -> Embedding {
    let mut counts = vec![0.0_f64; MOCK_DIMS];
    for ch in text.chars() {
        let idx = (ch as u32 as usize) % MOCK_DIMS;
        counts[idx] += 1.0;
    }
    counts
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, RagError> {
        Ok(texts.iter().map(|text| to_vector(text)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn embeds_each_input_in_order() {
        let adapter = MockEmbedder::new();
        let vectors = adapter
            .embed(&["abc".to_string(), "def".to_string()])
            .await
            .unwrap();
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), MOCK_DIMS);
        assert!(vectors[0].iter().sum::<f64>() > 0.0);
    }

    #[tokio::test]
    async fn is_deterministic_for_same_text() {
        let adapter = MockEmbedder::new();
        let a = adapter.embed(&["hello world".to_string()]).await.unwrap();
        let b = adapter.embed(&["hello world".to_string()]).await.unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn matches_ts_bucketing() {
        // 'a' = 97 -> 97 % 4 = 1, 'b' = 98 -> 2, 'c' = 99 -> 3.
        // So "abc" buckets to [0, 1, 1, 1].
        assert_eq!(to_vector("abc"), vec![0.0, 1.0, 1.0, 1.0]);
    }
}
