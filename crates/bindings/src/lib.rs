//! Node bindings (napi-rs) over the Rust engine.
//!
//! Two layers:
//!
//! 1. **Sync JSON helpers** (`validate_graph_json`, `compile_graph_yaml_json`,
//!    `engine_version`) — JSON in / JSON out, no callbacks. The original surface,
//!    kept working.
//! 2. **The async run bridge** (`engine_run` / `engine_resume` /
//!    `engine_approve_and_resume`) — drives a Rust [`GraphRuntime`] from a JS caller
//!    while calling user-supplied JS closures back from Rust:
//!      * custom node handlers (`on_node` with `{ kind: "node", ... }`),
//!      * tool `execute` fns (`on_node` with `{ kind: "tool", ... }`),
//!      * named condition predicates (`on_condition`),
//!      * a fire-and-forget run-event sink (`on_event`).
//!
//!    These entry points are **async** napi fns: each returns a JS `Promise`, so the
//!    event loop stays free to service the [`ThreadsafeFunction`] callbacks while the
//!    Rust run future is parked on a oneshot. A synchronous fn would block the JS
//!    main thread and deadlock (the callbacks could never run). The run future is
//!    `Send` (see `graph-runtime`), which is what lets napi's tokio runtime drive it.
//!
//! The JS seam callbacks are **async**: `on_node` and `on_condition` return a
//! `Promise`, and Rust *awaits* it. `ThreadsafeFunction::call_async::<Promise<T>>`
//! resolves the synchronously-returned promise object, and a further `.await` drives
//! that promise to its JS-resolved value (napi's `Promise<T>` implements both
//! `FromNapiValue` and `Future`). So `on_node` resolves to a JSON string and
//! `on_condition` to a boolean-ish JSON string, each *after* any JS `await` inside
//! the callback. `on_event` stays fire-and-forget (no return awaited).

#![deny(clippy::all)]

mod bridge;
mod spec;

use std::collections::BTreeMap;

use adriane_graph_adriane::compile_graph_yaml;
use adriane_graph_core::{validate_graph, GraphDefinition};
use adriane_llm_gateway::{LlmGateway, LlmRequest};
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction};
use napi_derive::napi;
use serde_json::Value;

/// A JS callback taking one string argument. Declared as a napi `ThreadsafeFunction`
/// (which is `Send + Sync`) so the async run future can capture it across `.await`
/// points — a bare `JsFunction` is `!Send` and would make the future non-`Send`.
/// `ErrorStrategy::Fatal` means JS receives `(payloadString)` with no leading error
/// argument, and a JS throw surfaces as a fatal napi exception.
type StringCallback = ThreadsafeFunction<String, ErrorStrategy::Fatal>;

/// Version of the bound Rust engine.
#[napi]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Validate a graph definition (JSON). Returns a JSON array of validation errors —
/// empty when the graph is structurally sound. Errors on malformed JSON.
#[napi]
pub fn validate_graph_json(definition_json: String) -> napi::Result<String> {
    let definition: GraphDefinition = serde_json::from_str(&definition_json)
        .map_err(|error| napi::Error::from_reason(format!("invalid graph JSON: {error}")))?;
    let errors = validate_graph(&definition);
    serde_json::to_string(&errors).map_err(|error| napi::Error::from_reason(error.to_string()))
}

/// Compile graph DSL YAML into a validated `GraphDefinition` (JSON string).
/// Throws with a clear message on parse, DSL, or structural validation failure.
#[napi]
pub fn compile_graph_yaml_json(yaml: String) -> napi::Result<String> {
    let definition =
        compile_graph_yaml(&yaml).map_err(|error| napi::Error::from_reason(error.to_string()))?;
    serde_json::to_string(&definition).map_err(|error| napi::Error::from_reason(error.to_string()))
}

/// One-shot LLM completion over the Rust gateway (ADR 0031 — backs the SDK `Model.invoke()`
/// overlay). `request_json` is a serialized `LlmRequest` (provider / model / messages / …);
/// `provider_keys_json` is a `{ "<provider>": "<key>" }` map (may be `"{}"` → env keys, else a
/// deterministic mock). Resolves to a serialized `LlmResponse`. The HTTP happens in Rust — no
/// TS provider client, one engine.
#[napi(ts_return_type = "Promise<string>")]
pub async fn llm_complete(
    request_json: String,
    provider_keys_json: String,
) -> napi::Result<String> {
    let request: LlmRequest = serde_json::from_str(&request_json)
        .map_err(|error| napi::Error::from_reason(format!("invalid LLM request JSON: {error}")))?;
    let keys: BTreeMap<String, String> =
        serde_json::from_str(&provider_keys_json).map_err(|error| {
            napi::Error::from_reason(format!("invalid provider keys JSON: {error}"))
        })?;
    let model = if request.model.is_empty() {
        None
    } else {
        Some(request.model.clone())
    };
    let gateway = bridge::build_standalone_gateway(request.provider, model, &keys);
    let response = gateway
        .complete(request)
        .await
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
    serde_json::to_string(&response).map_err(|error| napi::Error::from_reason(error.to_string()))
}

/// Start a fresh run of a graph on the Rust engine.
///
/// `spec_json` is an [`crate::spec::EngineSpec`]: the graph, the run id, optional
/// initial channel data, the agent configs, and the ids/names whose handlers live
/// in JS. The three callbacks bridge back to JS:
/// - `on_node(payloadJson) -> Promise<updateJson>` for JS node handlers and JS tools,
/// - `on_condition(payloadJson) -> Promise<"true"|"false">` for named conditions,
/// - `on_event(payloadJson)` (fire-and-forget) for run-lifecycle events.
///
/// Resolves to a JSON [`crate::bridge::RunOutcome`] (final state + any pending
/// approvals + the serialized state needed for `engine_approve_and_resume`).
#[napi(
    ts_args_type = "specJson: string, onNode: (payloadJson: string) => string | Promise<string>, onCondition: (payloadJson: string) => boolean | string | Promise<boolean | string>, onEvent: (payloadJson: string) => void",
    ts_return_type = "Promise<string>"
)]
pub async fn engine_run(
    spec_json: String,
    on_node: StringCallback,
    on_condition: StringCallback,
    on_event: StringCallback,
) -> napi::Result<String> {
    let callbacks = bridge::JsCallbacks::new(on_node, on_condition, on_event);
    bridge::run(spec_json, callbacks, bridge::Entry::Start).await
}

/// Resume a previously suspended run from its serialized state (carried in
/// `spec_json.state`). Same callbacks as [`engine_run`].
#[napi(
    ts_args_type = "specJson: string, onNode: (payloadJson: string) => string | Promise<string>, onCondition: (payloadJson: string) => boolean | string | Promise<boolean | string>, onEvent: (payloadJson: string) => void",
    ts_return_type = "Promise<string>"
)]
pub async fn engine_resume(
    spec_json: String,
    on_node: StringCallback,
    on_condition: StringCallback,
    on_event: StringCallback,
) -> napi::Result<String> {
    let callbacks = bridge::JsCallbacks::new(on_node, on_condition, on_event);
    bridge::run(spec_json, callbacks, bridge::Entry::Resume).await
}

/// Grant the approved tools carried in `spec_json.approvedTools`, write them into
/// the resumed state's `__approvedTools` channel, then resume. Same callbacks as
/// [`engine_run`].
#[napi(
    ts_args_type = "specJson: string, onNode: (payloadJson: string) => string | Promise<string>, onCondition: (payloadJson: string) => boolean | string | Promise<boolean | string>, onEvent: (payloadJson: string) => void",
    ts_return_type = "Promise<string>"
)]
pub async fn engine_approve_and_resume(
    spec_json: String,
    on_node: StringCallback,
    on_condition: StringCallback,
    on_event: StringCallback,
) -> napi::Result<String> {
    let callbacks = bridge::JsCallbacks::new(on_node, on_condition, on_event);
    bridge::run(spec_json, callbacks, bridge::Entry::Approve).await
}

/// Deliver an external signal to a suspended run, then resume it. `signalName` is the
/// signal a `waitForSignal` node is blocked on; `payloadJson` is its payload (injected
/// into the run's `__signals[signalName]` channel). The run advances PAST the waiting
/// node. `specJson.state` carries the serialized suspended `GraphState`; callbacks are
/// the same as [`engine_run`].
#[napi(
    ts_args_type = "specJson: string, signalName: string, payloadJson: string, onNode: (payloadJson: string) => string | Promise<string>, onCondition: (payloadJson: string) => boolean | string | Promise<boolean | string>, onEvent: (payloadJson: string) => void",
    ts_return_type = "Promise<string>"
)]
pub async fn engine_signal(
    spec_json: String,
    signal_name: String,
    payload_json: String,
    on_node: StringCallback,
    on_condition: StringCallback,
    on_event: StringCallback,
) -> napi::Result<String> {
    let payload: Value = serde_json::from_str(&payload_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid signal payload JSON: {error}"))
    })?;
    let callbacks = bridge::JsCallbacks::new(on_node, on_condition, on_event);
    bridge::run(
        spec_json,
        callbacks,
        bridge::Entry::Signal {
            name: signal_name,
            payload,
        },
    )
    .await
}
