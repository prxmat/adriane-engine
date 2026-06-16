//! [`RunnableParallel`] — the Rust port of the TS `RunnableParallel`.
//!
//! The TS version takes a `Record<string, Runnable<TInput, unknown>>` and, on
//! `invoke`, runs every branch against the **same** input and collects the
//! results into an object keyed by branch name. The Rust port mirrors that
//! shape: branches are keyed by [`String`], every branch is a
//! `Runnable<Value, Value>`, and the result is a
//! [`BTreeMap<String, Value>`](std::collections::BTreeMap) (a JSON object). Using
//! `serde_json::Value` for the branch I/O is what lets a heterogeneous set of
//! branches share one input and merge into one map, exactly as the TS object
//! type erases each branch's output to `unknown`.
//!
//! Branches run concurrently via [`futures::future::join_all`], and the first
//! branch error short-circuits the whole call — faithful to the TS
//! `await Promise.all(...)`.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::join_all;
use serde_json::Value;

use crate::error::RunnableError;
use crate::runnable::Runnable;

/// A branch of a [`RunnableParallel`]: a runnable from a JSON value to a JSON
/// value, shared so it can be moved into a concurrent task.
type Branch = Arc<dyn Runnable<Value, Value>>;

/// Runs a set of named branches against the same input and merges their outputs
/// into one map.
///
/// Faithful to the TS `RunnableParallel`: each branch receives an identical copy
/// of the input and the outputs are keyed by branch name.
#[derive(Default)]
pub struct RunnableParallel {
    branches: BTreeMap<String, Branch>,
}

impl RunnableParallel {
    /// Construct an empty parallel runnable.
    pub fn new() -> Self {
        RunnableParallel {
            branches: BTreeMap::new(),
        }
    }

    /// Register a branch under `name`.
    ///
    /// Returns `self` for builder-style chaining. A duplicate name replaces the
    /// prior branch, matching JS object key assignment.
    pub fn with_branch(
        mut self,
        name: impl Into<String>,
        runnable: impl Runnable<Value, Value> + 'static,
    ) -> Self {
        self.branches.insert(name.into(), Arc::new(runnable));
        self
    }

    /// Number of registered branches.
    pub fn len(&self) -> usize {
        self.branches.len()
    }

    /// Whether there are no branches.
    pub fn is_empty(&self) -> bool {
        self.branches.is_empty()
    }
}

#[async_trait]
impl Runnable<Value, BTreeMap<String, Value>> for RunnableParallel {
    async fn invoke(&self, input: Value) -> Result<BTreeMap<String, Value>, RunnableError> {
        // Each branch receives an identical copy of the input — the TS object
        // passes the same `input` reference to every `runnable.invoke`.
        let futures = self.branches.iter().map(|(name, runnable)| {
            let name = name.clone();
            let runnable = Arc::clone(runnable);
            let input = input.clone();
            async move {
                let output = runnable.invoke(input).await?;
                Ok::<(String, Value), RunnableError>((name, output))
            }
        });

        let results = join_all(futures).await;

        let mut merged = BTreeMap::new();
        for result in results {
            let (name, output) = result?;
            merged.insert(name, output);
        }
        Ok(merged)
    }
}
