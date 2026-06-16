//! The `CallbackEvent` vocabulary — the union of lifecycle events emitted while a
//! graph, agent, tool, or LLM call runs.
//!
//! Mirrors `packages/callbacks/src/types.ts`. The wire format is an internally
//! tagged enum: the `type` field carries the discriminant (`onLLMStart`,
//! `onNodeEnd`, …) and the base fields (`runId`, `nodeId`, `tags`, `metadata`,
//! `timestamp`) are flattened alongside the variant-specific payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Fields common to every callback event, flattened into each variant on the
/// wire.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackEventBase {
    /// Identifier of the run the event belongs to.
    pub run_id: String,
    /// Identifier of the node that produced the event, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub node_id: Option<String>,
    /// Tags accumulated through the manager hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tags: Option<Vec<String>>,
    /// Free-form metadata accumulated through the manager hierarchy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<BTreeMap<String, Value>>,
    /// ISO-8601 timestamp of the event.
    pub timestamp: String,
}

/// A lifecycle event. Serialized as `{ "type": "...", ...base, ...payload }` to
/// stay wire-compatible with the TypeScript discriminated union.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CallbackEvent {
    /// An LLM call started.
    #[serde(rename = "onLLMStart")]
    OnLlmStart {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The prompt/input handed to the model.
        input: Value,
    },
    /// A streaming token was produced by an LLM.
    #[serde(rename = "onLLMToken")]
    OnLlmToken {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The streamed token.
        token: String,
    },
    /// An LLM call finished.
    #[serde(rename = "onLLMEnd")]
    OnLlmEnd {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The model output.
        output: Value,
    },
    /// An LLM call errored.
    #[serde(rename = "onLLMError")]
    OnLlmError {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The error message.
        error: String,
    },
    /// A tool invocation started.
    #[serde(rename = "onToolStart")]
    OnToolStart {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The tool name.
        tool: String,
        /// The tool input.
        input: Value,
    },
    /// A tool invocation finished.
    #[serde(rename = "onToolEnd")]
    OnToolEnd {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The tool name.
        tool: String,
        /// The tool output.
        output: Value,
    },
    /// A tool invocation errored.
    #[serde(rename = "onToolError")]
    OnToolError {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The tool name.
        tool: String,
        /// The error message.
        error: String,
    },
    /// A node started executing.
    #[serde(rename = "onNodeStart")]
    OnNodeStart {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The node input.
        input: Value,
    },
    /// A node finished executing.
    #[serde(rename = "onNodeEnd")]
    OnNodeEnd {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The node output.
        output: Value,
    },
    /// A node errored.
    #[serde(rename = "onNodeError")]
    OnNodeError {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The error message.
        error: String,
    },
    /// A chain started executing.
    #[serde(rename = "onChainStart")]
    OnChainStart {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The chain input.
        input: Value,
    },
    /// A chain finished executing.
    #[serde(rename = "onChainEnd")]
    OnChainEnd {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The chain output.
        output: Value,
    },
    /// A chain errored.
    #[serde(rename = "onChainError")]
    OnChainError {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The error message.
        error: String,
    },
    /// An agent took an action.
    #[serde(rename = "onAgentAction")]
    OnAgentAction {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The action taken.
        action: String,
        /// An optional payload describing the action.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        payload: Option<Value>,
    },
    /// An agent finished.
    #[serde(rename = "onAgentFinish")]
    OnAgentFinish {
        /// Shared event fields.
        #[serde(flatten)]
        base: CallbackEventBase,
        /// The agent result.
        result: Value,
    },
}

impl CallbackEvent {
    /// Borrow the shared base fields of this event.
    pub fn base(&self) -> &CallbackEventBase {
        match self {
            CallbackEvent::OnLlmStart { base, .. }
            | CallbackEvent::OnLlmToken { base, .. }
            | CallbackEvent::OnLlmEnd { base, .. }
            | CallbackEvent::OnLlmError { base, .. }
            | CallbackEvent::OnToolStart { base, .. }
            | CallbackEvent::OnToolEnd { base, .. }
            | CallbackEvent::OnToolError { base, .. }
            | CallbackEvent::OnNodeStart { base, .. }
            | CallbackEvent::OnNodeEnd { base, .. }
            | CallbackEvent::OnNodeError { base, .. }
            | CallbackEvent::OnChainStart { base, .. }
            | CallbackEvent::OnChainEnd { base, .. }
            | CallbackEvent::OnChainError { base, .. }
            | CallbackEvent::OnAgentAction { base, .. }
            | CallbackEvent::OnAgentFinish { base, .. } => base,
        }
    }

    /// Mutably borrow the shared base fields of this event.
    pub fn base_mut(&mut self) -> &mut CallbackEventBase {
        match self {
            CallbackEvent::OnLlmStart { base, .. }
            | CallbackEvent::OnLlmToken { base, .. }
            | CallbackEvent::OnLlmEnd { base, .. }
            | CallbackEvent::OnLlmError { base, .. }
            | CallbackEvent::OnToolStart { base, .. }
            | CallbackEvent::OnToolEnd { base, .. }
            | CallbackEvent::OnToolError { base, .. }
            | CallbackEvent::OnNodeStart { base, .. }
            | CallbackEvent::OnNodeEnd { base, .. }
            | CallbackEvent::OnNodeError { base, .. }
            | CallbackEvent::OnChainStart { base, .. }
            | CallbackEvent::OnChainEnd { base, .. }
            | CallbackEvent::OnChainError { base, .. }
            | CallbackEvent::OnAgentAction { base, .. }
            | CallbackEvent::OnAgentFinish { base, .. } => base,
        }
    }

    /// The wire discriminant for this event (`onNodeStart`, `onLLMEnd`, …),
    /// matching the TypeScript `type` field.
    pub fn type_name(&self) -> &'static str {
        match self {
            CallbackEvent::OnLlmStart { .. } => "onLLMStart",
            CallbackEvent::OnLlmToken { .. } => "onLLMToken",
            CallbackEvent::OnLlmEnd { .. } => "onLLMEnd",
            CallbackEvent::OnLlmError { .. } => "onLLMError",
            CallbackEvent::OnToolStart { .. } => "onToolStart",
            CallbackEvent::OnToolEnd { .. } => "onToolEnd",
            CallbackEvent::OnToolError { .. } => "onToolError",
            CallbackEvent::OnNodeStart { .. } => "onNodeStart",
            CallbackEvent::OnNodeEnd { .. } => "onNodeEnd",
            CallbackEvent::OnNodeError { .. } => "onNodeError",
            CallbackEvent::OnChainStart { .. } => "onChainStart",
            CallbackEvent::OnChainEnd { .. } => "onChainEnd",
            CallbackEvent::OnChainError { .. } => "onChainError",
            CallbackEvent::OnAgentAction { .. } => "onAgentAction",
            CallbackEvent::OnAgentFinish { .. } => "onAgentFinish",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_with_type_discriminant_and_camel_case_base() {
        let event = CallbackEvent::OnNodeStart {
            base: CallbackEventBase {
                run_id: "run-1".to_owned(),
                node_id: Some("n1".to_owned()),
                tags: Some(vec!["a".to_owned()]),
                metadata: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
            },
            input: json!({ "x": 1 }),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "onNodeStart");
        assert_eq!(value["runId"], "run-1");
        assert_eq!(value["nodeId"], "n1");
        assert_eq!(value["tags"], json!(["a"]));
        assert_eq!(value["input"], json!({ "x": 1 }));
        // metadata is None -> omitted entirely
        assert!(value.get("metadata").is_none());
    }

    #[test]
    fn round_trips_through_json() {
        let event = CallbackEvent::OnToolEnd {
            base: CallbackEventBase {
                run_id: "run-2".to_owned(),
                node_id: None,
                tags: None,
                metadata: None,
                timestamp: "2026-01-01T00:00:00.000Z".to_owned(),
            },
            tool: "search".to_owned(),
            output: json!(["result"]),
        };

        let json_str = serde_json::to_string(&event).unwrap();
        let back: CallbackEvent = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn deserializes_typescript_shaped_payload() {
        let raw = json!({
            "type": "onLLMToken",
            "runId": "run-3",
            "timestamp": "2026-01-01T00:00:00.000Z",
            "token": "hello"
        });
        let event: CallbackEvent = serde_json::from_value(raw).unwrap();
        match event {
            CallbackEvent::OnLlmToken { base, token } => {
                assert_eq!(base.run_id, "run-3");
                assert_eq!(token, "hello");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn type_name_matches_discriminant() {
        let event = CallbackEvent::OnAgentFinish {
            base: CallbackEventBase::default(),
            result: json!(null),
        };
        assert_eq!(event.type_name(), "onAgentFinish");
    }
}
