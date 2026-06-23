//! The gateway: route a request to the adapter registered for its provider. The
//! only seam onto real provider SDKs (a real adapter lands later behind this trait).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::LlmError;
use crate::media_resolver::{resolve_request_media, MediaResolver};
use crate::types::{LlmProvider, LlmRequest, LlmResponse};

#[async_trait]
pub trait LlmProviderAdapter: Send + Sync {
    fn provider(&self) -> LlmProvider;
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}

#[async_trait]
pub trait LlmGateway: Send + Sync {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}

#[derive(Default)]
pub struct DefaultLlmGateway {
    adapters: HashMap<LlmProvider, Box<dyn LlmProviderAdapter>>,
    /// ADR 0030 9c: resolves multimodal `Artifact` references to inline bytes at the boundary,
    /// before the adapter runs. `None` → no resolution (artifact refs reach adapters as-is and
    /// are skipped there; base64/url already work).
    media_resolver: Option<Arc<dyn MediaResolver>>,
}

impl DefaultLlmGateway {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_adapter(&mut self, adapter: Box<dyn LlmProviderAdapter>) {
        self.adapters.insert(adapter.provider(), adapter);
    }

    /// Install the media resolver (ADR 0030 9c) used to resolve `Artifact` media references.
    pub fn with_media_resolver(mut self, resolver: Arc<dyn MediaResolver>) -> Self {
        self.media_resolver = Some(resolver);
        self
    }
}

#[async_trait]
impl LlmGateway for DefaultLlmGateway {
    async fn complete(&self, mut request: LlmRequest) -> Result<LlmResponse, LlmError> {
        // ADR 0030 9c: resolve any Artifact media references to inline bytes before dispatch,
        // so the adapter only ever sees base64/url (and oversized inline media is rejected).
        if let Some(resolver) = &self.media_resolver {
            resolve_request_media(&mut request, resolver.as_ref()).await?;
        }
        let adapter = self
            .adapters
            .get(&request.provider)
            .ok_or(LlmError::ProviderNotFound(request.provider))?;
        adapter.complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;
    use crate::types::{LlmResponse, LlmUsage};

    fn response(content: &str) -> LlmResponse {
        LlmResponse {
            content: content.to_owned(),
            tool_calls: None,
            stop_reason: Some("end_turn".to_owned()),
            usage: LlmUsage::default(),
            model: "mock".to_owned(),
            provider: LlmProvider::Anthropic,
            content_blocks: None,
        }
    }

    fn request() -> LlmRequest {
        LlmRequest {
            provider: LlmProvider::Anthropic,
            model: "claude".to_owned(),
            messages: vec![],
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            response_format: None,
        }
    }

    #[tokio::test]
    async fn routes_to_the_registered_adapter() {
        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(MockAdapter::new(
            LlmProvider::Anthropic,
            vec![response("hello")],
        )));

        let result = gateway.complete(request()).await.unwrap();
        assert_eq!(result.content, "hello");
    }

    #[tokio::test]
    async fn errors_when_no_adapter_is_registered() {
        let gateway = DefaultLlmGateway::new();
        assert_eq!(
            gateway.complete(request()).await,
            Err(LlmError::ProviderNotFound(LlmProvider::Anthropic))
        );
    }

    #[tokio::test]
    async fn replays_scripted_responses_in_order() {
        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(MockAdapter::new(
            LlmProvider::Anthropic,
            vec![response("first"), response("second")],
        )));

        assert_eq!(gateway.complete(request()).await.unwrap().content, "first");
        assert_eq!(gateway.complete(request()).await.unwrap().content, "second");
        // Exhausted → repeats the last.
        assert_eq!(gateway.complete(request()).await.unwrap().content, "second");
    }
}
