//! Structural validation of a [`GraphDefinition`]. Mirrors the TS `validateGraph`:
//! returns every problem found rather than failing on the first.

use std::collections::HashSet;

use crate::error::{ValidationError, ValidationErrorCode};
use crate::types::{EdgeType, GraphDefinition};

/// Validate a graph definition. An empty vec means the graph is structurally sound.
pub fn validate_graph(def: &GraphDefinition) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    let mut node_ids: HashSet<&str> = HashSet::new();
    for node in &def.nodes {
        if !node_ids.insert(node.id.as_str()) {
            errors.push(ValidationError::new(
                ValidationErrorCode::DuplicateNodeId,
                format!("Duplicate node id '{}'.", node.id),
                vec![node.id.0.clone()],
            ));
        }
    }

    let mut edge_ids: HashSet<&str> = HashSet::new();
    for edge in &def.edges {
        if !edge_ids.insert(edge.id.as_str()) {
            errors.push(ValidationError::new(
                ValidationErrorCode::DuplicateEdgeId,
                format!("Duplicate edge id '{}'.", edge.id),
                vec![edge.id.0.clone()],
            ));
        }
        if !node_ids.contains(edge.from.as_str()) {
            errors.push(ValidationError::new(
                ValidationErrorCode::InvalidEdgeReference,
                format!(
                    "Edge '{}' references unknown node '{}'.",
                    edge.id, edge.from
                ),
                vec![edge.id.0.clone()],
            ));
        }
        if !node_ids.contains(edge.to.as_str()) {
            errors.push(ValidationError::new(
                ValidationErrorCode::InvalidEdgeReference,
                format!("Edge '{}' references unknown node '{}'.", edge.id, edge.to),
                vec![edge.id.0.clone()],
            ));
        }
        if edge.edge_type == EdgeType::Conditional {
            let valid = edge
                .condition
                .as_ref()
                .map(|condition| !condition.trim().is_empty())
                .unwrap_or(false);
            if !valid {
                errors.push(ValidationError::new(
                    ValidationErrorCode::InvalidConditionFormat,
                    format!(
                        "Conditional edge '{}' requires a non-empty named condition.",
                        edge.id
                    ),
                    vec![edge.id.0.clone()],
                ));
            }
        }
    }

    if !node_ids.contains(def.entry_node_id.as_str()) {
        errors.push(ValidationError::new(
            ValidationErrorCode::MissingEntryNode,
            format!(
                "Entry node '{}' is not declared in the graph.",
                def.entry_node_id
            ),
            vec![def.entry_node_id.0.clone()],
        ));
    }

    errors
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::ids::{EdgeId, GraphId, NodeId};
    use crate::types::{
        ChannelDefinition, ChannelReducer, EdgeDefinition, EdgeType, GraphDefinition,
        NodeDefinition, NodeType,
    };

    use super::*;

    fn node(id: &str, node_type: NodeType) -> NodeDefinition {
        NodeDefinition {
            id: NodeId::from(id),
            node_type,
            label: id.to_owned(),
            subgraph_id: None,
            input_mapping: None,
            output_mapping: None,
            fan_out: None,
            retry_policy: None,
            metadata: None,
        }
    }

    fn edge(
        id: &str,
        from: &str,
        to: &str,
        edge_type: EdgeType,
        condition: Option<&str>,
    ) -> EdgeDefinition {
        EdgeDefinition {
            id: EdgeId::from(id),
            from: NodeId::from(from),
            to: NodeId::from(to),
            edge_type,
            condition: condition.map(|c| c.to_owned()),
        }
    }

    fn graph(
        nodes: Vec<NodeDefinition>,
        edges: Vec<EdgeDefinition>,
        entry: &str,
    ) -> GraphDefinition {
        let mut channels = BTreeMap::new();
        channels.insert(
            "count".to_owned(),
            ChannelDefinition {
                channel_type: "number".to_owned(),
                reducer: ChannelReducer::Replace,
                default: None,
            },
        );
        GraphDefinition {
            id: GraphId::from("g"),
            version: "0.0.0".to_owned(),
            name: "g".to_owned(),
            recursion_limit: None,
            channels,
            nodes,
            edges,
            entry_node_id: NodeId::from(entry),
            metadata: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_graph() {
        let def = graph(
            vec![node("a", NodeType::Action), node("b", NodeType::Action)],
            vec![edge("e1", "a", "b", EdgeType::Default, None)],
            "a",
        );
        assert!(validate_graph(&def).is_empty());
    }

    #[test]
    fn flags_a_dangling_edge_reference() {
        let def = graph(
            vec![node("a", NodeType::Action)],
            vec![edge("e1", "a", "ghost", EdgeType::Default, None)],
            "a",
        );
        let errors = validate_graph(&def);
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::InvalidEdgeReference));
    }

    #[test]
    fn flags_a_missing_entry_node() {
        let def = graph(vec![node("a", NodeType::Action)], vec![], "nope");
        let errors = validate_graph(&def);
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::MissingEntryNode));
    }

    #[test]
    fn flags_a_conditional_edge_without_a_condition() {
        let def = graph(
            vec![node("a", NodeType::Action), node("b", NodeType::Action)],
            vec![edge("e1", "a", "b", EdgeType::Conditional, None)],
            "a",
        );
        let errors = validate_graph(&def);
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::InvalidConditionFormat));
    }

    #[test]
    fn flags_duplicate_node_ids() {
        let def = graph(
            vec![node("a", NodeType::Action), node("a", NodeType::Agent)],
            vec![],
            "a",
        );
        let errors = validate_graph(&def);
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::DuplicateNodeId));
    }
}
