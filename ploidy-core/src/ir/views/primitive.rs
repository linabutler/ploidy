use petgraph::graph::NodeIndex;

use crate::ir::{CookedGraph, PrimitiveIrType};

use super::ViewNode;

/// A graph-aware view of a primitive type.
#[derive(Debug)]
pub struct IrPrimitiveView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: PrimitiveIrType,
}

impl<'a> IrPrimitiveView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: PrimitiveIrType,
    ) -> Self {
        Self { cooked, index, ty }
    }

    /// Returns the primitive type.
    #[inline]
    pub fn ty(&self) -> PrimitiveIrType {
        self.ty
    }
}

impl<'a> ViewNode<'a> for IrPrimitiveView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
