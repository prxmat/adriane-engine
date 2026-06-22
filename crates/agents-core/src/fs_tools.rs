//! Agent-callable filesystem tools (ADR 0024, phase 2a) over a
//! [`FilesystemBackend`] + [`PathPolicy`]. Eight tools — `read_file`, `ls`, `glob`,
//! `grep`, `write_file`, `edit_file`, `delete_file`, `move_file` — registered into
//! the same [`crate::tools::InMemoryToolRegistry`] the ReAct agent already drives.
//!
//! Permission is enforced here, fail-closed, on the NORMALIZED path:
//! - reads (`read_file`/`ls`/`glob`/`grep`): a `deny` path is invisible (reported as
//!   not-found, never permission-denied), everything else is readable.
//! - writes (`write_file`/`edit_file`/`delete_file`/`move_file`): require the `write`
//!   verb. A `gate` path is rejected in phase 2a (the guarded/approval path lands in
//!   phase 2c); `read` → permission-denied; `deny` → not-found.
//!
//! None of these tools is `requires_approval` in 2a — the gate verb (and the guarded
//! tool variants) arrive in phase 2c.

use std::sync::Arc;

use adriane_fs_backend::{
    normalize_path, ArtifactMediaType, EditOp, FilesystemBackend, FsError, FsPermVerb, FsWriteCtx,
    PathPolicy,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tools::{ToolDefinition, ToolHandler};

/// Resolve the normalized path + its policy verb, or an `FsError` (invalid path).
fn resolve(policy: &Arc<dyn PathPolicy>, raw: &str) -> Result<(String, FsPermVerb), FsError> {
    let name = normalize_path(raw)?;
    let verb = policy.resolve(&name);
    Ok((name, verb))
}

/// Map a read verb to an access decision: `deny` → not-found (no existence leak).
fn require_read(name: String, verb: FsPermVerb) -> Result<String, FsError> {
    if verb.can_read() {
        Ok(name)
    } else {
        Err(FsError::NotFound { path: name })
    }
}

/// Map a write verb to an access decision (2a: only `write` permits; `gate` is the
/// 2c guarded path; `read` denies; `deny` is invisible).
fn require_write(name: String, verb: FsPermVerb, action: &str) -> Result<String, FsError> {
    match verb {
        FsPermVerb::Write => Ok(name),
        FsPermVerb::Deny => Err(FsError::NotFound { path: name }),
        FsPermVerb::Read => Err(FsError::PermissionDenied {
            action: action.to_owned(),
            path: name,
        }),
        // A gate path needs the guarded tool + approval (phase 2c), not available here.
        FsPermVerb::Gate => Err(FsError::PermissionDenied {
            action: format!("{action} (path requires approval; not available in this build)"),
            path: name,
        }),
    }
}

#[derive(Deserialize)]
struct ReadFileInput {
    path: String,
    #[serde(default)]
    version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileInput {
    path: String,
    content: String,
    #[serde(default)]
    media_type: Option<ArtifactMediaType>,
}

#[derive(Deserialize)]
struct EditFileInput {
    path: String,
    patches: Vec<EditOp>,
}

#[derive(Deserialize)]
struct PathInput {
    path: String,
}

#[derive(Deserialize)]
struct MoveInput {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct GlobInput {
    pattern: String,
}

#[derive(Deserialize)]
struct GrepInput {
    pattern: String,
    #[serde(default)]
    paths: Vec<String>,
}

fn parse<T: for<'de> Deserialize<'de>>(input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|e| format!("invalid input: {e}"))
}

fn ok<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// Build all eight fs tools and register them into `registry`.
pub fn register_fs_tools(
    registry: &mut crate::tools::InMemoryToolRegistry,
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) {
    for (definition, handler) in fs_tools(backend, policy, ctx) {
        registry.register(definition, handler);
    }
}

/// The eight `(ToolDefinition, ToolHandler)` pairs (also handy for direct testing).
pub fn fs_tools(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) -> Vec<(ToolDefinition, ToolHandler)> {
    vec![
        read_file_tool(backend.clone(), policy.clone()),
        ls_tool(backend.clone(), policy.clone()),
        glob_tool(backend.clone(), policy.clone()),
        grep_tool(backend.clone(), policy.clone()),
        write_file_tool(backend.clone(), policy.clone(), ctx.clone()),
        edit_file_tool(backend.clone(), policy.clone(), ctx.clone()),
        delete_file_tool(backend.clone(), policy.clone(), ctx.clone()),
        move_file_tool(backend, policy, ctx),
    ]
}

fn def(name: &str, description: &str, schema: Value, requires_approval: bool) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: description.to_owned(),
        requires_approval,
        input_schema: Some(schema),
    }
}

fn read_file_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "read_file",
        "Read a file's content (optionally a specific version). Returns not-found if the path is missing or unreadable.",
        json!({ "type": "object", "properties": { "path": { "type": "string" }, "version": { "type": "integer" } }, "required": ["path"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        Box::pin(async move {
            let parsed: ReadFileInput = parse(input)?;
            let (name, verb) = resolve(&policy, &parsed.path).map_err(|e| e.to_string())?;
            let name = require_read(name, verb).map_err(|e| e.to_string())?;
            let content = backend
                .read(&name, parsed.version)
                .await
                .map_err(|e| e.to_string())?;
            ok(content)
        })
    });
    (definition, handler)
}

fn ls_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "ls",
        "List the immediate entries (files + synthetic directories) under a directory path. Use an empty path for the root.",
        json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        Box::pin(async move {
            let parsed: PathInput = parse(input)?;
            let entries = backend.ls(&parsed.path).await.map_err(|e| e.to_string())?;
            // Deny-invisibility applies to synthetic directories too: a directory whose
            // path resolves to `deny` (a wholly-denied subtree, e.g. `secret/**`) must not
            // leak its existence. A dir over readable files resolves to `read` and stays.
            let visible: Vec<_> = entries
                .into_iter()
                .filter(|e| policy.resolve(&e.path).can_read())
                .collect();
            ok(visible)
        })
    });
    (definition, handler)
}

fn glob_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "glob",
        "Return file paths matching a glob (`*` within a path segment, `**` across segments).",
        json!({ "type": "object", "properties": { "pattern": { "type": "string" } }, "required": ["pattern"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        Box::pin(async move {
            let parsed: GlobInput = parse(input)?;
            let paths = backend
                .glob(&parsed.pattern)
                .await
                .map_err(|e| e.to_string())?;
            let visible: Vec<String> = paths
                .into_iter()
                .filter(|p| policy.resolve(p).can_read())
                .collect();
            ok(visible)
        })
    });
    (definition, handler)
}

fn grep_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "grep",
        "Substring-search file contents. Optionally restrict to specific paths; otherwise searches every readable file.",
        json!({ "type": "object", "properties": { "pattern": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } } }, "required": ["pattern"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        Box::pin(async move {
            let parsed: GrepInput = parse(input)?;
            let matches = backend
                .grep(&parsed.pattern, parsed.paths)
                .await
                .map_err(|e| e.to_string())?;
            let visible: Vec<_> = matches
                .into_iter()
                .filter(|m| policy.resolve(&m.path).can_read())
                .collect();
            ok(visible)
        })
    });
    (definition, handler)
}

fn write_file_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "write_file",
        "Write (create or overwrite) a file's full content, producing a new version. Requires a writable path.",
        json!({ "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" }, "mediaType": { "type": "string", "enum": ["text/plain", "text/markdown", "application/json", "application/octet-stream"] } }, "required": ["path", "content"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            let parsed: WriteFileInput = parse(input)?;
            let (name, verb) = resolve(&policy, &parsed.path).map_err(|e| e.to_string())?;
            let name = require_write(name, verb, "write").map_err(|e| e.to_string())?;
            let media_type = parsed.media_type.unwrap_or(ArtifactMediaType::TextPlain);
            let reference = backend
                .write(&name, parsed.content, media_type, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            ok(reference)
        })
    });
    (definition, handler)
}

fn edit_file_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "edit_file",
        "Apply line-based patches (replace/insert/delete, 1-indexed inclusive) to a file, producing a new version.",
        json!({ "type": "object", "properties": { "path": { "type": "string" }, "patches": { "type": "array", "items": { "type": "object" } } }, "required": ["path", "patches"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            let parsed: EditFileInput = parse(input)?;
            let (name, verb) = resolve(&policy, &parsed.path).map_err(|e| e.to_string())?;
            let name = require_write(name, verb, "edit").map_err(|e| e.to_string())?;
            let reference = backend
                .edit(&name, parsed.patches, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            ok(reference)
        })
    });
    (definition, handler)
}

fn delete_file_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "delete_file",
        "Delete a file (tombstone — a new version flagged deleted; history is retained for audit). Requires a writable path.",
        json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            let parsed: PathInput = parse(input)?;
            let (name, verb) = resolve(&policy, &parsed.path).map_err(|e| e.to_string())?;
            let name = require_write(name, verb, "delete").map_err(|e| e.to_string())?;
            backend
                .delete(&name, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            ok(json!({ "deleted": name }))
        })
    });
    (definition, handler)
}

fn move_file_tool(
    backend: Arc<dyn FilesystemBackend>,
    policy: Arc<dyn PathPolicy>,
    ctx: FsWriteCtx,
) -> (ToolDefinition, ToolHandler) {
    let definition = def(
        "move_file",
        "Move/rename a file: copy its content to the destination and tombstone the source. Both paths must be writable.",
        json!({ "type": "object", "properties": { "from": { "type": "string" }, "to": { "type": "string" } }, "required": ["from", "to"], "additionalProperties": false }),
        false,
    );
    let handler: ToolHandler = Box::new(move |input: Value| {
        let backend = backend.clone();
        let policy = policy.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            let parsed: MoveInput = parse(input)?;
            let (from, from_verb) = resolve(&policy, &parsed.from).map_err(|e| e.to_string())?;
            let from = require_write(from, from_verb, "move").map_err(|e| e.to_string())?;
            let (to, to_verb) = resolve(&policy, &parsed.to).map_err(|e| e.to_string())?;
            let to = require_write(to, to_verb, "move").map_err(|e| e.to_string())?;
            let reference = backend
                .rename(&from, &to, &ctx)
                .await
                .map_err(|e| e.to_string())?;
            ok(reference)
        })
    });
    (definition, handler)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use adriane_artifact_store::InMemoryArtifactStore;
    use adriane_fs_backend::{ArtifactFsBackend, PathRule, StaticPathPolicy};
    use adriane_graph_core::{NodeId, RunId};
    use serde_json::json;

    use super::*;

    fn build(policy: StaticPathPolicy) -> HashMap<String, ToolHandler> {
        let backend: Arc<dyn FilesystemBackend> = Arc::new(ArtifactFsBackend::new(
            Arc::new(InMemoryArtifactStore::new()),
            RunId::from("run-1"),
        ));
        let policy: Arc<dyn PathPolicy> = Arc::new(policy);
        let ctx = FsWriteCtx {
            node_id: NodeId::from("agent"),
            principal: Some("agent".to_owned()),
        };
        fs_tools(backend, policy, ctx)
            .into_iter()
            .map(|(def, handler)| (def.name, handler))
            .collect()
    }

    fn writable() -> StaticPathPolicy {
        StaticPathPolicy::with_rules(vec![PathRule {
            glob: "scratch/**".to_owned(),
            verb: FsPermVerb::Write,
        }])
    }

    #[tokio::test]
    async fn write_then_read_through_the_tools() {
        let tools = build(writable());
        let r = tools["write_file"](json!({ "path": "scratch/a.txt", "content": "hi" }))
            .await
            .unwrap();
        assert_eq!(r.get("version"), Some(&json!(1)));
        let content = tools["read_file"](json!({ "path": "scratch/a.txt" }))
            .await
            .unwrap();
        assert_eq!(content.get("content"), Some(&json!("hi")));
    }

    #[tokio::test]
    async fn write_to_a_read_only_path_is_permission_denied() {
        // Default policy = read-only everywhere; no scratch rule here.
        let tools = build(StaticPathPolicy::read_only());
        let err = tools["write_file"](json!({ "path": "anywhere.txt", "content": "x" }))
            .await
            .unwrap_err();
        assert!(err.contains("permission denied"), "got: {err}");
    }

    #[tokio::test]
    async fn deny_path_is_invisible_not_permission_denied() {
        let policy = StaticPathPolicy::with_rules(vec![
            PathRule {
                glob: "scratch/**".to_owned(),
                verb: FsPermVerb::Write,
            },
            PathRule {
                glob: "secret/**".to_owned(),
                verb: FsPermVerb::Deny,
            },
        ]);
        let tools = build(policy);
        // Reading a deny path → not found (no existence disclosure).
        let err = tools["read_file"](json!({ "path": "secret/key" }))
            .await
            .unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
        // Writing a deny path → also not found, not permission-denied.
        let err = tools["write_file"](json!({ "path": "secret/key", "content": "x" }))
            .await
            .unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[tokio::test]
    async fn ls_hides_a_fully_denied_subtree_directory() {
        // A wholly-denied subtree must not leak its existence via a synthetic directory.
        // Seed BOTH files via the backend directly (storage is policy-agnostic) so a
        // `secret/` subtree exists; the `ls` tool must still hide its synthetic dir.
        let backend: Arc<dyn FilesystemBackend> = Arc::new(ArtifactFsBackend::new(
            Arc::new(InMemoryArtifactStore::new()),
            RunId::from("run-1"),
        ));
        let ctx = FsWriteCtx {
            node_id: NodeId::from("agent"),
            principal: None,
        };
        backend
            .write(
                "docs/a.txt",
                "a".to_owned(),
                ArtifactMediaType::TextPlain,
                &ctx,
            )
            .await
            .unwrap();
        backend
            .write(
                "secret/k.txt",
                "s".to_owned(),
                ArtifactMediaType::TextPlain,
                &ctx,
            )
            .await
            .unwrap();

        let policy: Arc<dyn PathPolicy> = Arc::new(StaticPathPolicy::with_rules(vec![PathRule {
            glob: "secret/**".to_owned(),
            verb: FsPermVerb::Deny,
        }]));
        let tools: HashMap<String, ToolHandler> = fs_tools(backend, policy, ctx)
            .into_iter()
            .map(|(def, handler)| (def.name, handler))
            .collect();

        let listing = tools["ls"](json!({ "path": "" })).await.unwrap();
        let paths: Vec<String> = listing
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e.get("path").and_then(|p| p.as_str()).map(str::to_owned))
            .collect();
        assert!(
            paths.iter().any(|p| p == "docs"),
            "readable dir hidden: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p == "secret"),
            "denied subtree leaked via synthetic dir: {paths:?}"
        );
    }

    #[tokio::test]
    async fn gate_path_write_is_rejected_until_phase_2c() {
        let policy = StaticPathPolicy::with_rules(vec![PathRule {
            glob: "review/**".to_owned(),
            verb: FsPermVerb::Gate,
        }]);
        let tools = build(policy);
        let err = tools["write_file"](json!({ "path": "review/doc.md", "content": "x" }))
            .await
            .unwrap_err();
        assert!(err.contains("requires approval"), "got: {err}");
    }

    #[tokio::test]
    async fn edit_delete_move_through_the_tools() {
        let tools = build(writable());
        tools["write_file"](json!({ "path": "scratch/f.txt", "content": "a\nb\nc" }))
            .await
            .unwrap();
        tools["edit_file"](json!({
            "path": "scratch/f.txt",
            "patches": [{ "op": "delete", "startLine": 2, "endLine": 2 }]
        }))
        .await
        .unwrap();
        assert_eq!(
            tools["read_file"](json!({ "path": "scratch/f.txt" }))
                .await
                .unwrap()
                .get("content"),
            Some(&json!("a\nc"))
        );
        tools["move_file"](json!({ "from": "scratch/f.txt", "to": "scratch/g.txt" }))
            .await
            .unwrap();
        assert!(tools["read_file"](json!({ "path": "scratch/f.txt" }))
            .await
            .is_err());
        tools["delete_file"](json!({ "path": "scratch/g.txt" }))
            .await
            .unwrap();
        assert!(tools["read_file"](json!({ "path": "scratch/g.txt" }))
            .await
            .is_err());
    }
}
