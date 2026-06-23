//! Adriane llm-gateway (Rust).
//!
//! The only crate allowed to talk to LLM providers — the Rust port of
//! `@adriane-ai/llm-gateway`. Requests route through a gateway to a provider adapter;
//! this slice ships the async traits + a mock. Real provider adapters and streaming
//! land behind the same traits later.

#![forbid(unsafe_code)]

pub mod anthropic;
pub mod compressor;
pub mod error;
pub mod gateway;
pub mod gemini;
pub mod mock;
pub mod model_policy;
pub mod openai_compatible;
pub mod redactor;
pub mod types;

pub use anthropic::{
    build_request_body, AnthropicAdapter, AnthropicCreateParams, AnthropicMessage, AnthropicPort,
    AnthropicRawResponse, AnthropicRole, AnthropicUsage, ContentBlock, HttpAnthropicPort,
    SystemBlock, ToolParam, DEFAULT_MAX_TOKENS, DEFAULT_MODEL,
};
pub use compressor::{
    CompressingGateway, HttpPromptCompressor, NoopPromptCompressor, PromptCompressor,
};
pub use error::LlmError;
pub use gateway::{DefaultLlmGateway, LlmGateway, LlmProviderAdapter};
pub use gemini::{
    build_request_body as build_gemini_request_body, GeminiAdapter, GeminiCandidate, GeminiContent,
    GeminiFunctionCall, GeminiPart, GeminiPort, GeminiRawResponse, GeminiUsageMetadata,
    HttpGeminiPort, DEFAULT_GEMINI_MODEL,
};
pub use mock::MockAdapter;
pub use model_policy::{ModelChoice, ModelPolicy, ModelTier};
pub use openai_compatible::{
    build_request_body as build_openai_request_body, HttpPort, OpenAiChatResponse,
    OpenAiCompatibleAdapter, OpenAiCompatiblePort, RawChoice, RawFunctionCall, RawMessage,
    RawToolCall, RawUsage, HUGGINGFACE_BASE_URL, HUGGINGFACE_DEFAULT_MODEL, LMSTUDIO_BASE_URL,
    LMSTUDIO_DEFAULT_MODEL, MINIMAX_BASE_URL, MINIMAX_DEFAULT_MODEL, MISTRAL_BASE_URL,
    MISTRAL_DEFAULT_MODEL, OLLAMA_BASE_URL, OLLAMA_DEFAULT_MODEL, OPENAI_BASE_URL,
    OPENAI_DEFAULT_MODEL, OPENROUTER_BASE_URL, OPENROUTER_DEFAULT_MODEL,
};
pub use redactor::{HttpPiiRedactor, NoopPiiRedactor, PiiRedactor, RedactingGateway};
pub use types::{
    LlmMessage, LlmProvider, LlmRequest, LlmResponse, LlmStreamChunk, LlmToolCall, LlmToolDef,
    LlmUsage, ResponseFormat,
};
