//! The graph data model: node/edge/channel/state types. Field names serialize to
//! the same camelCase wire shape as the TS model, so definitions are interchangeable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{EdgeId, GraphId, NodeId, RunId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeType {
    Action,
    Agent,
    Tool,
    HumanGate,
    Subgraph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeType {
    Default,
    Conditional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphStatus {
    Idle,
    Running,
    Suspended,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelReducer {
    Replace,
    Append,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelDefinition {
    #[serde(rename = "type")]
    pub channel_type: String,
    pub reducer: ChannelReducer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(rename = "maxAttempts")]
    pub max_attempts: u32,
    #[serde(rename = "backoffMs")]
    pub backoff_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanOut {
    pub parallel_to: Vec<NodeId>,
    pub join_at: NodeId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinition {
    pub id: NodeId,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgraph_id: Option<GraphId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_mapping: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_out: Option<FanOut>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeDefinition {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDefinition {
    pub id: GraphId,
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursion_limit: Option<u32>,
    pub channels: BTreeMap<String, ChannelDefinition>,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
    pub entry_node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphState {
    pub run_id: RunId,
    pub graph_id: GraphId,
    pub current_node_id: NodeId,
    pub status: GraphStatus,
    pub channels: BTreeMap<String, serde_json::Value>,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_serializes_to_kebab_case() {
        assert_eq!(
            serde_json::to_string(&NodeType::HumanGate).unwrap(),
            "\"human-gate\""
        );
        let parsed: NodeType = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(parsed, NodeType::Agent);
    }

    #[test]
    fn node_uses_type_and_camel_case_keys() {
        let node = NodeDefinition {
            id: NodeId::from("review"),
            node_type: NodeType::HumanGate,
            label: "Review".to_owned(),
            subgraph_id: None,
            input_mapping: None,
            output_mapping: None,
            fan_out: None,
            retry_policy: Some(RetryPolicy {
                max_attempts: 2,
                backoff_ms: 100,
            }),
            metadata: None,
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"type\":\"human-gate\""));
        assert!(json.contains("\"retryPolicy\":{\"maxAttempts\":2,\"backoffMs\":100}"));
        // Round-trips back to an identical value.
        let back: NodeDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }
}
