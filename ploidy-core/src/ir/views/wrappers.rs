use petgraph::graph::NodeIndex;

use crate::ir::{
    graph::{IrGraph, IrGraphNode},
    types::IrType,
};

use super::{IrTypeView, ViewNode};

/// A graph-aware view of an array type.
#[derive(Debug)]
pub struct IrArrayView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex,
    inner: &'a IrType<'a>,
}

impl<'a> IrArrayView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex,
        inner: &'a IrType<'a>,
    ) -> Self {
        Self {
            graph,
            index,
            inner,
        }
    }

    /// Returns a view of this array's element type.
    #[inline]
    pub fn inner(&self) -> IrTypeView<'a> {
        let node = IrGraphNode::from_ref(self.graph.spec, self.inner.as_ref());
        IrTypeView::new(self.graph, self.graph.indices[&node])
    }
}

impl<'a> ViewNode<'a> for IrArrayView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex {
        self.index
    }
}

/// A graph-aware view of a map type.
#[derive(Debug)]
pub struct IrMapView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex,
    inner: &'a IrType<'a>,
}

impl<'a> IrMapView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex,
        inner: &'a IrType<'a>,
    ) -> Self {
        Self {
            graph,
            index,
            inner,
        }
    }

    /// Returns a view of this map's value type.
    #[inline]
    pub fn inner(&self) -> IrTypeView<'a> {
        let node = IrGraphNode::from_ref(self.graph.spec, self.inner.as_ref());
        IrTypeView::new(self.graph, self.graph.indices[&node])
    }
}

impl<'a> ViewNode<'a> for IrMapView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex {
        self.index
    }
}

/// A graph-aware view of an optional value.
#[derive(Debug)]
pub struct IrOptionalView<'a> {
    graph: &'a IrGraph<'a>,
    index: NodeIndex,
    inner: &'a IrType<'a>,
}

impl<'a> IrOptionalView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex,
        inner: &'a IrType<'a>,
    ) -> Self {
        Self {
            graph,
            index,
            inner,
        }
    }

    /// Returns a view of the inner type.
    #[inline]
    pub fn inner(&self) -> IrTypeView<'a> {
        let node = IrGraphNode::from_ref(self.graph.spec, self.inner.as_ref());
        IrTypeView::new(self.graph, self.graph.indices[&node])
    }
}

impl<'a> ViewNode<'a> for IrOptionalView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        self.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex {
        self.index
    }
}
