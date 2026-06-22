//! LLM request/response types. Wire-compatible (camelCase) with the TS gateway.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Openai,
    Anthropic,
    Mistral,
    Ollama,
    Google,
    Minimax,
    Openrouter,
    Huggingface,
    Lmstudio,
    Mock,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessage {
    /// `"system" | "user" | "assistant" | "tool"`.
    pub role: String,
    pub content: String,
    /// Native tool calls the assistant emitted — set on `assistant` history entries so the
    /// provider sees a coherent function-calling transcript (no redundant re-calls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<LlmToolCall>>,
    /// Links a `role:"tool"` result back to the assistant tool call that produced it
    /// (the OpenAI / Anthropic id; Anthropic's `tool_result.tool_use_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Name of the tool a `role:"tool"` result answers — Gemini links its `functionResponse`
    /// by function name, not by id, so the result must carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl LlmMessage {
    /// A plain text message (the common case): role + content, no tool fields.
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        LlmMessage {
            role: role.into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequest {
    pub provider: LlmProvider,
    pub model: String,
    pub messages: Vec<LlmMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<LlmToolDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<LlmToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub usage: LlmUsage,
    pub model: String,
    pub provider: LlmProvider,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmStreamChunk {
    pub delta: String,
    pub done: bool,
}
