use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::IrGraph,
    types::{IrEnum, IrEnumVariant},
};

use super::ViewNode;

/// A graph-aware view of an [`IrEnum`].
#[derive(Debug)]
pub struct IrEnumView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex,
    ty: &'a IrEnum<'a>,
}

impl<'a> IrEnumView<'a> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex, ty: &'a IrEnum<'a>) -> Self {
        Self { graph, index, ty }
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
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex {
        self.index
    }
}
