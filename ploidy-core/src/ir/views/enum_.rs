use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::CookedGraph,
    types::{IrEnum, IrEnumVariant},
};

use super::ViewNode;

/// A graph-aware view of an [`IrEnum`].
#[derive(Debug)]
pub struct IrEnumView<'a> {
    cooked: &'a CookedGraph<'a>,
    index: NodeIndex<usize>,
    ty: &'a IrEnum<'a>,
}

impl<'a> IrEnumView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a IrEnum<'a>,
    ) -> Self {
        Self { cooked, index, ty }
    }

    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        self.ty.description
    }

    #[inline]
    pub fn variants(&self) -> &'a [IrEnumVariant<'a>] {
        &self.ty.variants
    }
}

impl<'a> ViewNode<'a> for IrEnumView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
        self.cooked
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.index
    }
}
