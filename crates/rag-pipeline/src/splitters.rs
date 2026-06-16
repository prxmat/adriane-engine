//! Splitters — the `Splitter` seam plus the two deterministic implementations.
//!
//! Ports `splitters/text-splitter.ts` (the `TextSplitter` interface),
//! `splitters/recursive-character-splitter.ts`, and `splitters/token-splitter.ts`.

use crate::types::{Chunk, Document, SplitConfig};

/// The splitter seam — turn a document into an ordered list of chunks.
///
/// Mirrors the TS `TextSplitter` interface
/// (`split(doc: Document, config: SplitConfig): Chunk[]`). It is synchronous and
/// pure, exactly like the TS interface.
pub trait Splitter {
    /// Split `doc` into chunks according to `config`.
    fn split(&self, doc: &Document, config: SplitConfig) -> Vec<Chunk>;
}

/// Build a chunk from a source document, mirroring the TS spread
/// `{ ...doc, sourceId, chunkIndex, id, content }`: the chunk inherits `doc`'s
/// `metadata` (and `embedding`), but overrides `id` and `content`.
fn make_chunk(doc: &Document, chunk_index: usize, content: String) -> Chunk {
    let document = Document {
        id: format!("{}:chunk:{}", doc.id, chunk_index),
        content,
        metadata: doc.metadata.clone(),
        embedding: doc.embedding.clone(),
    };
    Chunk {
        document,
        source_id: doc.id.clone(),
        chunk_index,
    }
}

/// Split text into sentence-ish units, mirroring the TS `splitSentences`:
///
/// 1. `text.split(/(?<=[.!?])\s+/)` — break after `.`/`!`/`?` followed by
///    whitespace (the whitespace is consumed; the terminator stays attached).
/// 2. `.flatMap(s => s.split(/\n{2,}/))` — further break on blank-line gaps.
/// 3. `.map(trim).filter(len > 0)`.
fn split_sentences(text: &str) -> Vec<String> {
    sentence_boundary_split(text)
        .into_iter()
        .flat_map(|part| split_on_double_newline(&part))
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Reproduce JS `String.split(/(?<=[.!?])\s+/)`: split at each run of
/// whitespace that is immediately preceded by `.`, `!`, or `?`. The lookbehind
/// keeps the terminator with the preceding piece and consumes the whitespace
/// run as the delimiter.
fn sentence_boundary_split(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        // A split point begins at a whitespace run preceded by a terminator.
        if ch.is_whitespace() && !current.is_empty() {
            if let Some(prev) = current.chars().last() {
                if matches!(prev, '.' | '!' | '?') {
                    // Consume the entire whitespace run as the delimiter.
                    parts.push(std::mem::take(&mut current));
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    continue;
                }
            }
        }
        current.push(ch);
        i += 1;
    }
    parts.push(current);
    parts
}

/// Reproduce JS `String.split(/\n{2,}/)`: split on runs of two-or-more newline
/// characters.
fn split_on_double_newline(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\n' {
            // Count the run of newlines.
            let mut run = 0;
            let mut j = i;
            while j < chars.len() && chars[j] == '\n' {
                run += 1;
                j += 1;
            }
            if run >= 2 {
                parts.push(std::mem::take(&mut current));
                i = j;
                continue;
            }
            // A lone newline is an ordinary character.
            current.push('\n');
            i += 1;
        } else {
            current.push(chars[i]);
            i += 1;
        }
    }
    parts.push(current);
    parts
}

/// Recursive character splitter — accumulates sentence units greedily up to
/// `chunkSize`, carrying a character overlap into the next chunk.
///
/// Faithful port of the TS `RecursiveCharacterSplitter`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecursiveCharacterSplitter;

impl RecursiveCharacterSplitter {
    /// Create a recursive character splitter.
    pub fn new() -> Self {
        Self
    }
}

impl Splitter for RecursiveCharacterSplitter {
    fn split(&self, doc: &Document, config: SplitConfig) -> Vec<Chunk> {
        let units = split_sentences(&doc.content);
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut current = String::new();
        let mut chunk_index = 0usize;

        for unit in &units {
            // `candidate = current.length === 0 ? unit : `${current} ${unit}``
            let candidate = if current.is_empty() {
                unit.clone()
            } else {
                format!("{current} {unit}")
            };

            // TS compares `.length`, which is UTF-16 code-unit length. For the
            // ASCII fixtures this equals `chars().count()`; we mirror the
            // code-point length, which is the more faithful Unicode measure.
            if char_len(&candidate) > config.chunk_size && !current.is_empty() {
                chunks.push(make_chunk(doc, chunk_index, current.clone()));
                chunk_index += 1;
                // `overlap = Math.max(0, config.chunkOverlap)` — already a
                // non-negative `usize`.
                let overlap = config.chunk_overlap;
                // `current = `${current.slice(current.length - overlap)} ${unit}`.trim()`
                let tail = slice_tail(&current, overlap);
                current = format!("{tail} {unit}").trim().to_string();
            } else {
                current = candidate;
            }
        }

        if !current.is_empty() {
            chunks.push(make_chunk(doc, chunk_index, current));
        }
        chunks
    }
}

/// Token splitter — whitespace-tokenizes the document and emits sliding windows
/// of `chunkSize` tokens stepping by `max(1, size - overlap)`.
///
/// Faithful port of the TS `TokenSplitter`.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokenSplitter;

impl TokenSplitter {
    /// Create a token splitter.
    pub fn new() -> Self {
        Self
    }
}

impl Splitter for TokenSplitter {
    fn split(&self, doc: &Document, config: SplitConfig) -> Vec<Chunk> {
        // `text.split(/\s+/).filter(t => t.length > 0)`
        let tokens: Vec<&str> = doc
            .content
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .collect();
        let size = config.chunk_size.max(1);
        let overlap = config.chunk_overlap; // already usize, >= 0
        let step = size.saturating_sub(overlap).max(1);

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut index = 0usize;
        let mut start = 0usize;
        while start < tokens.len() {
            let end = (start + size).min(tokens.len());
            let slice = &tokens[start..end];
            if !slice.is_empty() {
                chunks.push(make_chunk(doc, index, slice.join(" ")));
                index += 1;
            }
            // TS: `if (start + size >= tokens.length) break;`
            if start + size >= tokens.len() {
                break;
            }
            start += step;
        }
        chunks
    }
}

/// Number of Unicode scalar values in `s`.
fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// `s.slice(Math.max(0, s.length - n))` over code points: the last `n`
/// characters of `s` (or all of `s` if shorter).
fn slice_tail(s: &str, n: usize) -> String {
    let total = char_len(s);
    let skip = total.saturating_sub(n);
    s.chars().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, content: &str) -> Document {
        Document::new(id, content)
    }

    #[test]
    fn recursive_splitter_produces_multiple_chunks_with_source_id() {
        let splitter = RecursiveCharacterSplitter::new();
        let d = doc("d1", "Alpha beta. Gamma delta. Epsilon zeta.");
        let chunks = splitter.split(
            &d,
            SplitConfig {
                chunk_size: 15,
                chunk_overlap: 3,
            },
        );
        assert!(chunks.len() > 1, "expected more than one chunk");
        assert_eq!(chunks[0].source_id, "d1");
        assert_eq!(chunks[0].id(), "d1:chunk:0");
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn recursive_splitter_single_chunk_when_under_size() {
        let splitter = RecursiveCharacterSplitter::new();
        let d = doc("d2", "Short sentence.");
        let chunks = splitter.split(
            &d,
            SplitConfig {
                chunk_size: 100,
                chunk_overlap: 0,
            },
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content(), "Short sentence.");
    }

    #[test]
    fn token_splitter_windows_with_overlap() {
        let splitter = TokenSplitter::new();
        // 6 tokens, size 3, overlap 1 -> step 2 -> windows [0..3], [2..5], [4..6]
        let d = doc("t1", "one two three four five six");
        let chunks = splitter.split(
            &d,
            SplitConfig {
                chunk_size: 3,
                chunk_overlap: 1,
            },
        );
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].content(), "one two three");
        assert_eq!(chunks[1].content(), "three four five");
        assert_eq!(chunks[2].content(), "five six");
    }

    #[test]
    fn split_sentences_breaks_on_terminators_and_blank_lines() {
        let units = split_sentences("First sentence. Second one!\n\nThird block.");
        assert_eq!(
            units,
            vec!["First sentence.", "Second one!", "Third block."]
        );
    }
}
