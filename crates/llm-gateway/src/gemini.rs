//! Google Gemini provider adapter — a native adapter over the Generative Language
//! `generateContent` API, structured like [`crate::anthropic`].
//!
//! Gemini does not speak the OpenAI `/chat/completions` shape, so it gets its own
//! adapter rather than routing through [`crate::openai_compatible`]. The adapter talks
//! to the API through a single seam, [`GeminiPort`], so the request/response mapping is
//! fully covered by tests without a network call or an API key. [`HttpGeminiPort`] is
//! the real reqwest-backed implementation behind the same trait.
//!
//! Request assembly:
//! - `req.system` plus any system-role messages fold into one `systemInstruction`
//!   block (joined with a blank line), and system-role messages are removed from the
//!   `contents` list,
//! - the engine `assistant` role maps to Gemini's `model` role; everything else maps
//!   to `user`,
//! - tools map to a single `tools[0].functionDeclarations` array (omitted when empty),
//! - `temperature` / `maxOutputTokens` go under `generationConfig`, emitted only when
//!   present,
//! - a model that does not start with `gemini` resolves to the default model.
//!
//! Deviation (deferred, by design, same as the Anthropic adapter): the Rust
//! [`LlmMessage`] content is a plain `String` today, so structured content blocks
//! (`tool_use` / `tool_result` turns) and streaming are not ported yet.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::LlmError;
use crate::gateway::LlmProviderAdapter;
use crate::types::{
    ContentBlock, LlmProvider, LlmRequest, LlmResponse, LlmToolCall, LlmUsage, MediaSource,
    ResponseFormat,
};

/// Model used when the request does not name a Gemini model.
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.0-flash";

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

// ---------------------------------------------------------------------------
// Raw wire response (matches the generateContent JSON)
// ---------------------------------------------------------------------------

/// One function call inside a candidate's parts. Gemini gives no call id, so the
/// adapter synthesises a deterministic one when mapping to [`LlmToolCall`].
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GeminiFunctionCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// One content part: either text or a function call.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GeminiPart {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default, rename = "functionCall")]
    pub function_call: Option<GeminiFunctionCall>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GeminiContent {
    #[serde(default)]
    pub parts: Vec<GeminiPart>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GeminiCandidate {
    #[serde(default)]
    pub content: GeminiContent,
    #[serde(default, rename = "finishReason")]
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct GeminiUsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    pub prompt_token_count: Option<u32>,
    #[serde(default, rename = "candidatesTokenCount")]
    pub candidates_token_count: Option<u32>,
    #[serde(default, rename = "cachedContentTokenCount")]
    pub cached_content_token_count: Option<u32>,
}

/// Structural subset of the generateContent response the adapter reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GeminiRawResponse {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "usageMetadata")]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

// ---------------------------------------------------------------------------
// Port seam
// ---------------------------------------------------------------------------

/// The only seam onto the real Gemini API. Production uses [`HttpGeminiPort`]; tests
/// supply a fake so no network is ever touched. The body is the assembled
/// generateContent JSON; `model` is the resolved model id (it rides in the URL).
#[async_trait]
pub trait GeminiPort: Send + Sync {
    async fn generate(&self, model: String, body: Value) -> Result<GeminiRawResponse, LlmError>;
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct GeminiAdapter {
    port: Box<dyn GeminiPort>,
    default_model: String,
}

impl GeminiAdapter {
    /// Build an adapter over the given port with the standard default model.
    pub fn new(port: Box<dyn GeminiPort>) -> Self {
        Self::with_default_model(port, DEFAULT_GEMINI_MODEL)
    }

    /// Build an adapter over the given port, overriding the fallback model used when
    /// a request names a non-Gemini model.
    pub fn with_default_model(port: Box<dyn GeminiPort>, default_model: impl Into<String>) -> Self {
        GeminiAdapter {
            port,
            default_model: default_model.into(),
        }
    }

    /// Convenience constructor wiring the real HTTP port from `GEMINI_API_KEY`
    /// (falling back to `GOOGLE_API_KEY`). Errors when neither is set.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .map_err(|_| {
                LlmError::Provider("GEMINI_API_KEY (or GOOGLE_API_KEY) is not set".to_owned())
            })?;
        Ok(Self::new(Box::new(HttpGeminiPort::new(api_key))))
    }

    fn resolve_model(&self, model: &str) -> String {
        if model.starts_with("gemini") {
            model.to_owned()
        } else {
            self.default_model.clone()
        }
    }
}

/// `req.system` plus any system-role messages, joined with a blank line.
fn collect_system(req: &LlmRequest) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(system) = &req.system {
        if !system.is_empty() {
            parts.push(system);
        }
    }
    for message in &req.messages {
        if message.role == "system" {
            parts.push(&message.content);
        }
    }
    parts.join("\n\n")
}

/// Map an [`LlmRequest`] to the generateContent body. Pure, so tests assert on it
/// directly. System text folds into `systemInstruction`; `assistant` maps to the
/// `model` role; tools become a single `functionDeclarations` group (omitted when
/// empty); `temperature` / `maxOutputTokens` go under `generationConfig`.
/// ADR 0030: map content blocks to Gemini parts. Inline bytes → `inlineData`, a URL →
/// `fileData`; image/audio/file are uniform (the `mimeType` distinguishes them). An
/// unresolved Artifact source yields no part (resolved upstream by the gateway, 9c).
fn gemini_content_parts(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(json!({ "text": text })),
            ContentBlock::Image { source }
            | ContentBlock::Audio { source }
            | ContentBlock::File { source } => gemini_media_part(source),
        })
        .collect()
}

fn gemini_media_part(source: &MediaSource) -> Option<Value> {
    if let Some((media_type, data)) = source.as_base64() {
        Some(json!({ "inlineData": { "mimeType": media_type, "data": data } }))
    } else if let MediaSource::Url { url, media_type } = source {
        let mut file_data = Map::new();
        if let Some(mime) = media_type {
            file_data.insert("mimeType".to_owned(), json!(mime));
        }
        file_data.insert("fileUri".to_owned(), json!(url));
        Some(json!({ "fileData": Value::Object(file_data) }))
    } else {
        None // unresolved artifact
    }
}

pub fn build_request_body(req: &LlmRequest) -> Value {
    let mut body = Map::new();

    let system_text = collect_system(req);
    if !system_text.is_empty() {
        body.insert(
            "systemInstruction".to_owned(),
            json!({ "parts": [{ "text": system_text }] }),
        );
    }

    let contents: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| {
            // Tool result → a `functionResponse` part (Gemini links it by function NAME).
            if m.role == "tool" {
                let name = m.tool_name.clone().unwrap_or_default();
                return json!({
                    "role": "user",
                    "parts": [{ "functionResponse": { "name": name, "response": { "result": m.content } } }]
                });
            }
            let role = if m.role == "assistant" { "model" } else { "user" };
            // Assistant tool calls → `functionCall` parts (+ a leading text part if any).
            if let Some(calls) = &m.tool_calls {
                let mut parts: Vec<Value> = Vec::new();
                if !m.content.is_empty() {
                    parts.push(json!({ "text": m.content }));
                }
                for call in calls {
                    parts.push(json!({ "functionCall": { "name": call.name, "args": call.input } }));
                }
                return json!({ "role": role, "parts": parts });
            }
            // ADR 0030: multimodal — content blocks become Gemini parts.
            if let Some(blocks) = &m.content_blocks {
                return json!({ "role": role, "parts": gemini_content_parts(blocks) });
            }
            json!({ "role": role, "parts": [{ "text": m.content }] })
        })
        .collect();
    body.insert("contents".to_owned(), Value::Array(contents));

    // Never emit an empty tools array.
    if let Some(tools) = &req.tools {
        if !tools.is_empty() {
            let declarations: Vec<Value> = tools
                .iter()
                .map(|tool| {
                    let mut declaration = Map::new();
                    declaration.insert("name".to_owned(), json!(tool.name));
                    if let Some(description) = &tool.description {
                        declaration.insert("description".to_owned(), json!(description));
                    }
                    declaration.insert("parameters".to_owned(), tool.input_schema.clone());
                    Value::Object(declaration)
                })
                .collect();
            body.insert(
                "tools".to_owned(),
                json!([{ "functionDeclarations": declarations }]),
            );
        }
    }

    let mut generation = Map::new();
    if let Some(temperature) = req.temperature {
        generation.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(max_tokens) = req.max_tokens {
        generation.insert("maxOutputTokens".to_owned(), json!(max_tokens));
    }
    // ADR 0029: Gemini takes a native JSON schema under `generationConfig`
    // (`responseMimeType` + `responseSchema`). It ignores the OpenAI `name`/`strict`;
    // the in-engine validation floor enforces conformance regardless.
    if let Some(ResponseFormat::JsonSchema { schema, .. }) = &req.response_format {
        generation.insert("responseMimeType".to_owned(), json!("application/json"));
        generation.insert("responseSchema".to_owned(), schema.clone());
    }
    if !generation.is_empty() {
        body.insert("generationConfig".to_owned(), Value::Object(generation));
    }

    Value::Object(body)
}

fn to_response(request: &LlmRequest, model: String, raw: GeminiRawResponse) -> LlmResponse {
    let candidate = raw.candidates.into_iter().next().unwrap_or_default();
    let finish_reason = candidate.finish_reason;

    let mut content = String::new();
    let mut tool_calls: Vec<LlmToolCall> = Vec::new();
    for (index, part) in candidate.content.parts.into_iter().enumerate() {
        if let Some(text) = part.text {
            content.push_str(&text);
        }
        if let Some(call) = part.function_call {
            tool_calls.push(LlmToolCall {
                // Gemini supplies no call id; synthesise a deterministic one.
                id: format!("call_{index}_{}", call.name),
                name: call.name,
                input: if call.args.is_null() {
                    Value::Object(Map::new())
                } else {
                    call.args
                },
            });
        }
    }

    let usage = raw.usage_metadata.unwrap_or_default();

    // The agent loop keys on `tool_calls` presence; surface `tool_use` as the stop
    // reason when there are calls so consumers reading `stop_reason` agree.
    let stop_reason = if tool_calls.is_empty() {
        finish_reason
    } else {
        Some("tool_use".to_owned())
    };

    LlmResponse {
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        stop_reason,
        usage: LlmUsage {
            prompt_tokens: usage.prompt_token_count.unwrap_or(0),
            completion_tokens: usage.candidates_token_count.unwrap_or(0),
            cache_read_tokens: usage.cached_content_token_count,
            cache_write_tokens: None,
        },
        model,
        provider: request.provider,
        content_blocks: None,
    }
}

#[async_trait]
impl LlmProviderAdapter for GeminiAdapter {
    fn provider(&self) -> LlmProvider {
        LlmProvider::Google
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let model = self.resolve_model(&request.model);
        let body = build_request_body(&request);
        let raw = self.port.generate(model.clone(), body).await?;
        Ok(to_response(&request, model, raw))
    }
}

// ---------------------------------------------------------------------------
// HTTP port (the only code that touches the network)
// ---------------------------------------------------------------------------

/// Real [`GeminiPort`] over `POST {base}/models/{model}:generateContent`, sending the
/// key as `x-goog-api-key`. Never exercised in tests — the pure [`build_request_body`]
/// carries the wire-shape logic and is unit-tested instead.
pub struct HttpGeminiPort {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl HttpGeminiPort {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, GEMINI_BASE_URL)
    }

    /// Override the API host (e.g. a local stub during manual testing).
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        HttpGeminiPort {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl GeminiPort for HttpGeminiPort {
    async fn generate(&self, model: String, body: Value) -> Result<GeminiRawResponse, LlmError> {
        let url = format!(
            "{}/models/{model}:generateContent",
            self.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|err| LlmError::Provider(format!("gemini request failed: {err}")))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::Provider(format!(
                "gemini returned {status}: {text}"
            )));
        }

        response
            .json::<GeminiRawResponse>()
            .await
            .map_err(|err| LlmError::Provider(format!("gemini response decode failed: {err}")))
    }
}

// ---------------------------------------------------------------------------
// Tests (no network, fake recording port)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::gateway::{DefaultLlmGateway, LlmGateway};
    use crate::types::{LlmMessage, LlmToolDef, ResponseFormat};

    /// Captures the (model, body) the adapter sends and returns a canned response.
    /// The `(model, body)` pairs the adapter sends, captured for assertions.
    type RecordedCalls = Arc<Mutex<Vec<(String, Value)>>>;

    struct RecordingPort {
        calls: RecordedCalls,
        response: GeminiRawResponse,
    }

    #[async_trait]
    impl GeminiPort for RecordingPort {
        async fn generate(
            &self,
            model: String,
            body: Value,
        ) -> Result<GeminiRawResponse, LlmError> {
            self.calls.lock().unwrap().push((model, body));
            Ok(self.response.clone())
        }
    }

    fn recording_port(response: GeminiRawResponse) -> (Box<dyn GeminiPort>, RecordedCalls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let port = RecordingPort {
            calls: Arc::clone(&calls),
            response,
        };
        (Box::new(port), calls)
    }

    fn text_response() -> GeminiRawResponse {
        GeminiRawResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("hello".to_owned()),
                        function_call: None,
                    }],
                    role: Some("model".to_owned()),
                },
                finish_reason: Some("STOP".to_owned()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: Some(12),
                candidates_token_count: Some(8),
                cached_content_token_count: None,
            }),
        }
    }

    fn base_request() -> LlmRequest {
        LlmRequest {
            provider: LlmProvider::Google,
            model: "gemini-2.0-flash".to_owned(),
            messages: vec![LlmMessage::text("user", "Hi")],
            system: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            response_format: None,
        }
    }

    #[tokio::test]
    async fn response_format_maps_to_generation_config_response_schema() {
        // ADR 0029: Gemini takes a native schema under `generationConfig`.
        let request = LlmRequest {
            response_format: Some(ResponseFormat::JsonSchema {
                name: "Verdict".to_owned(),
                schema: json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                strict: true,
            }),
            ..base_request()
        };
        let body = build_request_body(&request);
        let gen = &body["generationConfig"];
        assert_eq!(gen["responseMimeType"], json!("application/json"));
        assert_eq!(
            gen["responseSchema"],
            json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } })
        );
    }

    #[tokio::test]
    async fn image_content_block_maps_to_an_inline_data_part() {
        // ADR 0030: content blocks → Gemini parts (inlineData for base64 bytes).
        let request = LlmRequest {
            messages: vec![LlmMessage::with_blocks(
                "user",
                vec![
                    ContentBlock::Text {
                        text: "what is this?".to_owned(),
                    },
                    ContentBlock::Image {
                        source: MediaSource::Base64 {
                            media_type: "image/png".to_owned(),
                            data: "AAAA".to_owned(),
                        },
                    },
                ],
            )],
            ..base_request()
        };
        let body = build_request_body(&request);
        let user = body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["parts"].as_array().map(|p| p.len() > 1).unwrap_or(false))
            .unwrap();
        let parts = user["parts"].as_array().unwrap();
        assert_eq!(parts[0], json!({ "text": "what is this?" }));
        assert_eq!(parts[1]["inlineData"]["mimeType"], json!("image/png"));
        assert_eq!(parts[1]["inlineData"]["data"], json!("AAAA"));
    }

    #[tokio::test]
    async fn folds_system_maps_roles_and_tools() {
        let (port, calls) = recording_port(text_response());
        let adapter = GeminiAdapter::new(port);

        let request = LlmRequest {
            system: Some("Base.".to_owned()),
            messages: vec![
                LlmMessage::text("system", "Extra rule."),
                LlmMessage::text("assistant", "Prior turn."),
                LlmMessage::text("user", "Go"),
            ],
            tools: Some(vec![LlmToolDef {
                name: "search".to_owned(),
                description: Some("Search things".to_owned()),
                input_schema: json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
            }]),
            ..base_request()
        };
        adapter.complete(request).await.unwrap();

        let calls = calls.lock().unwrap();
        let (_model, body) = &calls[0];

        // System (req.system + system-role message) folds into systemInstruction.
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            json!("Base.\n\nExtra rule.")
        );

        // System-role message is dropped from contents; assistant maps to `model`.
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], json!("model"));
        assert_eq!(contents[0]["parts"][0]["text"], json!("Prior turn."));
        assert_eq!(contents[1]["role"], json!("user"));
        assert_eq!(contents[1]["parts"][0]["text"], json!("Go"));

        // Tools map to a single functionDeclarations group.
        let declarations = body["tools"][0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(declarations[0]["name"], json!("search"));
        assert_eq!(declarations[0]["description"], json!("Search things"));
        assert_eq!(
            declarations[0]["parameters"],
            json!({ "type": "object", "properties": { "query": { "type": "string" } } })
        );
    }

    #[tokio::test]
    async fn never_emits_tools_or_generation_config_when_absent() {
        let (port, calls) = recording_port(text_response());
        let adapter = GeminiAdapter::new(port);

        adapter.complete(base_request()).await.unwrap();

        let calls = calls.lock().unwrap();
        let (_model, body) = &calls[0];
        assert!(body.get("tools").is_none());
        assert!(body.get("generationConfig").is_none());
    }

    #[tokio::test]
    async fn maps_text_and_usage() {
        let (port, calls) = recording_port(text_response());
        let adapter = GeminiAdapter::new(port);

        let request = LlmRequest {
            temperature: Some(0.5),
            response_format: None,
            max_tokens: Some(256),
            ..base_request()
        };
        let result = adapter.complete(request).await.unwrap();

        assert_eq!(result.content, "hello");
        assert_eq!(result.tool_calls, None);
        assert_eq!(result.stop_reason.as_deref(), Some("STOP"));
        assert_eq!(result.usage.prompt_tokens, 12);
        assert_eq!(result.usage.completion_tokens, 8);
        assert_eq!(result.provider, LlmProvider::Google);

        // generationConfig carries temperature + maxOutputTokens when present.
        let calls = calls.lock().unwrap();
        let (_model, body) = &calls[0];
        assert_eq!(body["generationConfig"]["temperature"], json!(0.5));
        assert_eq!(body["generationConfig"]["maxOutputTokens"], json!(256));
    }

    #[tokio::test]
    async fn surfaces_function_calls_as_structured_tool_calls() {
        let (port, _calls) = recording_port(GeminiRawResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![
                        GeminiPart {
                            text: Some("Let me search.".to_owned()),
                            function_call: None,
                        },
                        GeminiPart {
                            text: None,
                            function_call: Some(GeminiFunctionCall {
                                name: "search".to_owned(),
                                args: json!({ "query": "adriane" }),
                            }),
                        },
                    ],
                    role: Some("model".to_owned()),
                },
                finish_reason: Some("STOP".to_owned()),
            }],
            usage_metadata: None,
        });
        let adapter = GeminiAdapter::new(port);

        let result = adapter.complete(base_request()).await.unwrap();

        assert_eq!(result.content, "Let me search.");
        // A function call surfaces `tool_use` regardless of the raw STOP reason.
        assert_eq!(result.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(
            result.tool_calls,
            Some(vec![LlmToolCall {
                id: "call_1_search".to_owned(),
                name: "search".to_owned(),
                input: json!({ "query": "adriane" }),
            }])
        );
    }

    #[tokio::test]
    async fn falls_back_to_the_default_model_for_a_placeholder_then_keeps_a_gemini_id() {
        let (port, calls) = recording_port(text_response());
        let adapter = GeminiAdapter::with_default_model(port, "gemini-2.0-flash");

        // `claude-opus-4-8` is the agent placeholder, not a Gemini model id.
        adapter
            .complete(LlmRequest {
                model: "claude-opus-4-8".to_owned(),
                ..base_request()
            })
            .await
            .unwrap();
        adapter
            .complete(LlmRequest {
                model: "gemini-1.5-pro".to_owned(),
                ..base_request()
            })
            .await
            .unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].0, "gemini-2.0-flash");
        assert_eq!(calls[1].0, "gemini-1.5-pro");
    }

    #[tokio::test]
    async fn integrates_through_the_default_gateway_routing() {
        let (port, _calls) = recording_port(text_response());
        let mut gateway = DefaultLlmGateway::new();
        gateway.register_adapter(Box::new(GeminiAdapter::new(port)));

        let result = gateway.complete(base_request()).await.unwrap();

        assert_eq!(result.content, "hello");
        assert_eq!(result.provider, LlmProvider::Google);
    }

    #[test]
    fn deserializes_the_real_wire_response_shape() {
        let raw: GeminiRawResponse = serde_json::from_str(
            r#"{
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [
                            { "text": "Hi there." },
                            { "functionCall": { "name": "search", "args": { "q": "x" } } }
                        ]
                    },
                    "finishReason": "STOP"
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 4,
                    "cachedContentTokenCount": 2
                }
            }"#,
        )
        .unwrap();

        let candidate = &raw.candidates[0];
        assert_eq!(
            candidate.content.parts[0].text.as_deref(),
            Some("Hi there.")
        );
        assert_eq!(
            candidate.content.parts[1]
                .function_call
                .as_ref()
                .unwrap()
                .name,
            "search"
        );
        assert_eq!(candidate.finish_reason.as_deref(), Some("STOP"));
        let usage = raw.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, Some(10));
        assert_eq!(usage.cached_content_token_count, Some(2));
    }
}
