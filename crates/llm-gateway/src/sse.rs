//! A minimal Server-Sent Events decoder shared by the streaming provider ports
//! (ADR 0033, phase 13). It does the SSE **framing only** — accumulate raw bytes,
//! yield the `data:` payload of every event that has become complete — and is
//! provider-agnostic: each provider parses its own JSON payloads on top.
//!
//! The framing rules we honour (a pragmatic subset of the SSE spec, enough for the
//! Anthropic / OpenAI / Gemini streams):
//! - CRLF is normalised to LF.
//! - Events are separated by a blank line (`\n\n`).
//! - Within an event, every `data:` line contributes; multiple `data:` lines are
//!   joined with `\n` (per the spec). A single leading space after the colon is
//!   stripped. Non-`data:` lines (`event:`, `id:`, `:` comments) are ignored — each
//!   provider's JSON payload carries its own type discriminator.

/// Stateful decoder: feed it byte chunks as they arrive off the wire; it returns the
/// `data:` payloads of each event that completed within that chunk. Bytes that do not
/// yet form a complete event stay buffered for the next [`Self::push`].
#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Feed a chunk of decoded UTF-8 text. Returns the `data:` payload of every event
    /// terminated (by a blank line) within the accumulated buffer so far.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        // Normalise CRLF so the boundary search only deals with `\n`.
        self.buffer.push_str(&chunk.replace("\r\n", "\n"));
        let mut payloads = Vec::new();
        while let Some(boundary) = self.buffer.find("\n\n") {
            let event: String = self.buffer.drain(..boundary + 2).collect();
            if let Some(data) = event_data(&event) {
                payloads.push(data);
            }
        }
        payloads
    }

    /// Flush any trailing event the stream ended without a blank line after (some
    /// servers omit the final `\n\n`). Returns its `data:` payload if present.
    pub fn finish(&mut self) -> Option<String> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return None;
        }
        let event = std::mem::take(&mut self.buffer);
        event_data(&event)
    }
}

/// Extract and join the `data:` lines of one raw SSE event block. `None` when the
/// block carries no `data:` line (e.g. a bare `event:`/comment heartbeat).
fn event_data(event: &str) -> Option<String> {
    let mut data_lines: Vec<&str> = Vec::new();
    for line in event.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            // A single optional leading space after the colon is part of the framing.
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yields_data_payloads_per_event() {
        let mut decoder = SseDecoder::default();
        let out = decoder.push("event: a\ndata: {\"x\":1}\n\nevent: b\ndata: {\"y\":2}\n\n");
        assert_eq!(out, vec!["{\"x\":1}".to_owned(), "{\"y\":2}".to_owned()]);
    }

    #[test]
    fn buffers_a_partial_event_across_chunks() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push("data: {\"par").is_empty());
        let out = decoder.push("tial\":true}\n\n");
        assert_eq!(out, vec!["{\"partial\":true}".to_owned()]);
    }

    #[test]
    fn normalises_crlf_and_strips_one_leading_space() {
        let mut decoder = SseDecoder::default();
        let out = decoder.push("data: hello\r\n\r\n");
        assert_eq!(out, vec!["hello".to_owned()]);
    }

    #[test]
    fn ignores_comment_and_event_only_blocks() {
        let mut decoder = SseDecoder::default();
        let out = decoder.push(": ping\n\nevent: ping\n\ndata: real\n\n");
        assert_eq!(out, vec!["real".to_owned()]);
    }

    #[test]
    fn finish_flushes_a_trailing_unterminated_event() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push("data: last").is_empty());
        assert_eq!(decoder.finish(), Some("last".to_owned()));
    }
}
