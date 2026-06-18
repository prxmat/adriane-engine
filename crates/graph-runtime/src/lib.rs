//! Adriane graph-runtime (Rust).
//!
//! Execution engine over a validated `GraphDefinition` — the Rust port of
//! `@adriane-ai/graph-runtime`. The deterministic, checkpoint-after-every-node,
//! resumable run loop with human-gate suspension lives here.

#![forbid(unsafe_code)]

pub mod interfaces;
pub mod runtime;
pub mod types;

pub use interfaces::{
    sync_handler, BoxFuture, Checkpointer, ConditionFn, ConditionRegistry, EventBus, EventObserver,
    InMemoryCheckpointer, InMemoryConditionRegistry, InMemoryEventBus, InMemoryNodeRegistry,
    Interrupt, NodeHandler, NodeOutput, NodeRegistry,
};
pub use runtime::{GraphRuntime, RuntimeError};
pub use types::{Checkpoint, CheckpointId, RunEvent};
