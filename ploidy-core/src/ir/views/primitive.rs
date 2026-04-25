//! Primitives: scalar types.
//!
//! Primitives are leaf nodes that don't reference other types in the graph.
//! Codegen maps each [`PrimitiveType`] variant to a language-specific type:
//! for example, [`PrimitiveType::String`] becomes a [`String`] in Rust.
//! See [`PrimitiveType`] for the full list of variants.

use petgraph::graph::NodeIndex;

use crate::ir::{CookedGraph, PrimitiveType};

use super::ViewNode;

/// A graph-aware view of a [primitive type][PrimitiveType].
#[derive(Debug)]
pub struct PrimitiveView<'graph, 'a> {
    cooked: &'graph CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: PrimitiveType,
}

impl<'graph, 'a> PrimitiveView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: PrimitiveType,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the primitive type.
    #[inline]
    pub fn ty(&self) -> PrimitiveType {
        self.ty
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for PrimitiveView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
