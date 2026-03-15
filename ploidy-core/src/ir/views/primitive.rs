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
pub struct PrimitiveView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: PrimitiveType,
}

impl<'a> PrimitiveView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
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

impl<'a> ViewNode<'a> for PrimitiveView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
