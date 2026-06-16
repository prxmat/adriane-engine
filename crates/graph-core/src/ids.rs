//! Branded identifier newtypes — the Rust equivalent of the TS `Brand<string, …>`
//! types. Distinct types prevent mixing a `NodeId` where a `GraphId` is expected,
//! while serializing transparently as plain strings.

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Borrow the underlying string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                $name(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                $name(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(NodeId, "Identifier of a node within a graph.");
string_id!(EdgeId, "Identifier of an edge within a graph.");
string_id!(GraphId, "Identifier of a graph definition.");
string_id!(RunId, "Identifier of a single graph execution.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_transparently_as_a_string() {
        let id = NodeId::from("start");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"start\"");
        let back: NodeId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
