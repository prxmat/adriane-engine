//! Adriane agents-core (Rust).
//!
//! Rust port of `@adriane-ai/agents-core`: agent patterns over the async graph
//! runtime and LLM gateway. This slice ships the ReAct loop (native tool calls
//! plus the `FINAL:` / `ACTION:` text protocol), a tool registry with the
//! no-self-approval invariant, the graph-runtime node handler that suspends a run
//! for human approval and resumes once tools are granted, plus the plan-execute,
//! reflection, supervisor, and working-memory patterns.

#![forbid(unsafe_code)]

pub mod fs_tools;
pub mod middleware;
pub mod node;
pub mod plan_execute;
pub mod react;
pub mod reflection;
pub mod supervisor;
pub mod todos;
pub mod tools;
pub mod working_memory;

pub use fs_tools::{fs_tools, register_fs_tools};
pub use middleware::{AgentMiddleware, Flow, MiddlewareStack, RunCtx, ToolCallCtx, ToolControl};
pub use node::{
    agent_node_handler, AGENT_APPROVAL_INTERRUPT, APPROVED_TOOLS_CHANNEL,
    DEFAULT_AGENT_OUTPUT_CHANNEL,
};
pub use plan_execute::{PlanExecuteAgent, PlanExecuteResult, PlanStep, PLANNER_MODEL};
pub use react::{
    AgentResult, ApprovalRequestItem, ReActAgent, DEFAULT_MAX_ITERATIONS, DEFAULT_MODEL,
};
pub use reflection::{
    ReflectionAgent, ReflectionResult, DEFAULT_MAX_REFLECTIONS, REFLECTION_MODEL, REVISE_MARKERS,
};
pub use supervisor::{
    Routing, SupervisorAgent, SupervisorResult, Worker, NO_DESCRIPTION, SUPERVISOR_MODEL,
};
pub use todos::{
    normalize_todos, write_todos_tool, TodoItem, TodoStatus, TODOS_CHANNEL, WRITE_TODOS_TOOL,
};
pub use tools::{sync_tool, InMemoryToolRegistry, ToolDefinition, ToolFuture, ToolHandler};
pub use working_memory::{Message, WorkingMemory, COMPRESSOR_MODEL};
