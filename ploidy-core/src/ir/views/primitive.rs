use petgraph::graph::NodeIndex;

use crate::ir::{IrGraph, PrimitiveIrType};

use super::ViewNode;

/// A graph-aware view of a primitive type.
#[derive(Debug)]
pub struct IrPrimitiveView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex<usize>,
    ty: PrimitiveIrType,
}

impl<'a> IrPrimitiveView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: PrimitiveIrType,
    ) -> Self {
        Self { graph, index, ty }
    }

    /// Returns the primitive type.
    #[inline]
    pub fn ty(&self) -> PrimitiveIrType {
        self.ty
    }
}

impl<'a> ViewNode<'a> for IrPrimitiveView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
