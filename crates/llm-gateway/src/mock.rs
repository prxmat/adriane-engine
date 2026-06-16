//! A mock adapter that replays scripted responses — for offline runs and tests.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use crate::error::LlmError;
use crate::gateway::LlmProviderAdapter;
use crate::types::{LlmProvider, LlmRequest, LlmResponse};

pub struct MockAdapter {
    provider: LlmProvider,
    responses: Vec<LlmResponse>,
    index: AtomicUsize,
}

impl MockAdapter {
    pub fn new(provider: LlmProvider, responses: Vec<LlmResponse>) -> Self {
        MockAdapter {
            provider,
            responses,
            index: AtomicUsize::new(0),
        }
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
}
