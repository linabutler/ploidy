//! Any type: a schema with no type constraints.
//!
//! An [`AnyView`] represents an arbitrary JSON value: a schema without
//! `type`, `properties`, or composition keywords. Codegen maps this to a
//! dynamic type in the target language, like [`serde_json::Value`] in Rust,
//! `Any` in Python, or `any` in TypeScript.

use petgraph::graph::NodeIndex;

use crate::ir::CookedGraph;

use super::ViewNode;

/// A graph-aware view of an untyped JSON value.
#[derive(Debug)]
pub struct AnyView<'a>(&'a CookedGraph<'a>, NodeIndex<usize>);

impl<'a> AnyView<'a> {
    #[inline]
    pub(in crate::ir) fn new(cooked: &'a CookedGraph<'a>, index: NodeIndex<usize>) -> Self {
        Self(cooked, index)
    }
}

impl<'a> ViewNode<'a> for AnyView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.0
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.1
    }
}
