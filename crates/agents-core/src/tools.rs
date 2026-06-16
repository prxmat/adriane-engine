//! Tool definitions, handlers, and the in-memory registry — the Rust port of
//! `@adriane/agents-core`'s `tools.ts`. Schemas are plain JSON Schema values
//! (no Zod here); validation belongs to the caller and the LLM contract.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Describes a tool an agent may call. Serializes camelCase (`requiresApproval`,
/// `inputSchema`), wire-compatible with the TS `ToolDefinition` subset.
///
/// Only tools that carry an `input_schema` are advertised to the LLM as native
/// tool definitions; schema-less tools remain callable through the `ACTION:`
/// text protocol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// When `true`, an agent never executes this tool on its own: it records an
    /// approval request instead, unless the name was explicitly granted.
    pub requires_approval: bool,
    /// JSON Schema advertised to the LLM so it can emit native tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

/// The boxed future a tool handler produces.
pub type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

/// An async tool handler: JSON input in, JSON output (or an error message) out.
pub type ToolHandler = Box<dyn Fn(Value) -> ToolFuture + Send + Sync>;

/// Wrap a synchronous closure as an async [`ToolHandler`] — convenient for pure
/// tools and tests that don't await anything.
pub fn sync_tool<F>(f: F) -> ToolHandler
where
    F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
{
    Box::new(move |input| Box::pin(std::future::ready(f(input))))
}

/// A plain in-memory tool registry keyed by tool name. Registration takes
/// `&mut self`; lookups are `&self`, so a populated registry stays `Sync` and
/// can be shared behind an `Arc` by concurrent node handlers.
#[derive(Default)]
pub struct InMemoryToolRegistry {
    entries: HashMap<String, (ToolDefinition, ToolHandler)>,
}

impl InMemoryToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a tool under its definition's name.
    pub fn register(&mut self, definition: ToolDefinition, handler: ToolHandler) {
        self.entries
            .insert(definition.name.clone(), (definition, handler));
    }

    /// Look a tool up by name.
    pub fn resolve(&self, name: &str) -> Option<(&ToolDefinition, &ToolHandler)> {
        self.entries.get(name).map(|entry| (&entry.0, &entry.1))
    }

    /// Every registered tool definition (unordered).
    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.entries.values().map(|entry| &entry.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn registers_resolves_and_runs_a_sync_tool() {
        let mut registry = InMemoryToolRegistry::new();
        registry.register(
            ToolDefinition {
                name: "echo".to_owned(),
                description: "Echoes its input.".to_owned(),
                requires_approval: false,
                input_schema: Some(json!({ "type": "object" })),
            },
            sync_tool(Ok),
        );

        assert!(registry.resolve("missing").is_none());
        assert_eq!(registry.list().len(), 1);

        let (definition, handler) = registry.resolve("echo").expect("echo is registered");
        assert!(!definition.requires_approval);
        let output = handler(json!({ "x": 1 })).await;
        assert_eq!(output, Ok(json!({ "x": 1 })));

        // camelCase wire shape, matching the TS model.
        let wire = serde_json::to_string(definition).expect("serializes");
        assert!(wire.contains("\"requiresApproval\":false"));
        assert!(wire.contains("\"inputSchema\""));
    }
}
