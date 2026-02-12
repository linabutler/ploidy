use petgraph::graph::NodeIndex;

use crate::ir::IrGraph;

use super::ViewNode;

/// A graph-aware view of an untyped JSON value.
#[derive(Debug)]
pub struct AnyView<'a>(&'a IrGraph<'a>, NodeIndex<usize>);

impl<'a> AnyView<'a> {
    #[inline]
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex<usize>) -> Self {
        Self(graph, index)
    }
}

impl<'a> ViewNode<'a> for AnyView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.0
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        self.1
    }
}
