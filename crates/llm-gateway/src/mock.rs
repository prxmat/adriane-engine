//! A mock adapter that replays scripted responses — for offline runs and tests.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use crate::error::LlmError;
use crate::gateway::{LlmProviderAdapter, TokenSink};
use crate::types::{LlmProvider, LlmRequest, LlmResponse};

pub struct MockAdapter {
    provider: LlmProvider,
    responses: Vec<LlmResponse>,
    /// Optional per-call delta scripts (ADR 0033 phase 13): `stream_scripts[i]` is the
    /// sequence of token deltas `stream()` emits for the i-th call. Empty (the default)
    /// → `stream()` emits the whole `responses[i].content` as one delta (chunk-once),
    /// so existing `MockAdapter::new(..)` call sites stream without change.
    stream_scripts: Vec<Vec<String>>,
    index: AtomicUsize,
}

impl MockAdapter {
    pub fn new(provider: LlmProvider, responses: Vec<LlmResponse>) -> Self {
        MockAdapter {
            provider,
            responses,
            stream_scripts: Vec::new(),
            index: AtomicUsize::new(0),
        }
    }

    /// Attach per-call token-delta scripts so `stream()` emits multiple chunks (ADR 0033).
    /// `scripts[i]` is replayed for the i-th call; absent / empty entries fall back to
    /// chunk-once. The returned [`LlmResponse`] is always `responses[i]` — a test that
    /// wants the byte-identical guarantee should make `scripts[i].concat()` equal
    /// `responses[i].content`.
    pub fn with_stream_scripts(mut self, scripts: Vec<Vec<String>>) -> Self {
        self.stream_scripts = scripts;
        self
    }
}

#[async_trait]
impl LlmProviderAdapter for MockAdapter {
    fn provider(&self) -> LlmProvider {
        self.provider
    }

    async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, LlmError> {
        if self.responses.is_empty() {
            return Err(LlmError::Provider(
                "mock adapter has no responses".to_owned(),
            ));
        }
        let next = self.index.fetch_add(1, Ordering::SeqCst);
        let index = next.min(self.responses.len() - 1);
        Ok(self.responses[index].clone())
    }

    async fn stream(
        &self,
        _request: LlmRequest,
        on_delta: &TokenSink<'_>,
    ) -> Result<LlmResponse, LlmError> {
        if self.responses.is_empty() {
            return Err(LlmError::Provider(
                "mock adapter has no responses".to_owned(),
            ));
        }
        let next = self.index.fetch_add(1, Ordering::SeqCst);
        let index = next.min(self.responses.len() - 1);
        let response = self.responses[index].clone();
        match self.stream_scripts.get(index) {
            // Multi-chunk: replay the scripted deltas.
            Some(deltas) if !deltas.is_empty() => {
                for delta in deltas {
                    on_delta(delta);
                }
            }
            // Chunk-once: emit the whole content as one delta.
            _ => {
                if !response.content.is_empty() {
                    on_delta(&response.content);
                }
            }
        }
        Ok(response)
    }
}
