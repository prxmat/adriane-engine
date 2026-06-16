//! Errors raised while building component handlers from a `{ kind, params }`
//! declaration.

use thiserror::Error;

/// Why a component handler could not be built from its `{ kind, params }`
/// declaration.
///
/// Building a handler validates the component `kind` and its `params` up front,
/// so a graph that references a component fails fast (at registry/build time)
/// rather than at run time inside the node.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ComponentError {
    /// No component is registered under the requested `kind`.
    #[error("unknown component kind: {0}")]
    UnknownKind(String),

    /// A required parameter is missing for this component.
    #[error("component {kind}: missing required param `{param}`")]
    MissingParam {
        /// The component kind whose params failed validation.
        kind: String,
        /// The name of the missing parameter.
        param: String,
    },

    /// A parameter was present but had the wrong shape (e.g. a string was
    /// expected but an object was given).
    #[error("component {kind}: param `{param}` is invalid: {reason}")]
    InvalidParam {
        /// The component kind whose params failed validation.
        kind: String,
        /// The name of the offending parameter.
        param: String,
        /// A human-readable reason the parameter was rejected.
        reason: String,
    },
}
