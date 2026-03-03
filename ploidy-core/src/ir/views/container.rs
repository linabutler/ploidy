use petgraph::graph::NodeIndex;

use crate::ir::{
    InlineIrType, SchemaIrType,
    graph::{IrGraph, IrGraphNode},
    types::Container,
};

use super::{IrTypeView, ViewNode};

/// A graph-aware view of a container type.
#[derive(Debug)]
pub enum ContainerView<'a> {
    Array(InnerView<'a>),
    Map(InnerView<'a>),
    Optional(InnerView<'a>),
}

impl<'a> ContainerView<'a> {
    /// Returns a type view of this container type.
    #[inline]
    pub fn ty(&self) -> IrTypeView<'a> {
        IrTypeView::new(self.graph(), self.index())
    }
}

impl<'a> ViewNode<'a> for ContainerView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        let (Self::Array(c) | Self::Map(c) | Self::Optional(c)) = self;
        c.graph
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        let (Self::Array(c) | Self::Map(c) | Self::Optional(c)) = self;
        c.container
    }
}

/// A graph-aware view of the inner type of a [`Container`].
#[derive(Debug)]
pub struct InnerView<'a> {
    graph: &'a IrGraph<'a>,
    container: NodeIndex<usize>,
    inner: NodeIndex<usize>,
}

impl<'a> InnerView<'a> {
    /// Returns a view of the contained type.
    #[inline]
    pub fn ty(&self) -> IrTypeView<'a> {
        IrTypeView::new(self.graph, self.inner)
    }

    /// Returns a human-readable description of the contained type, if present.
    #[inline]
    pub fn description(&self) -> Option<&'a str> {
        match self.graph.g[self.container] {
            IrGraphNode::Schema(SchemaIrType::Container(_, container))
            | IrGraphNode::Inline(InlineIrType::Container(_, container)) => {
                container.inner().description
            }
            _ => None,
        }
    }
}

impl<'a> ContainerView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        container: &'a Container<'a>,
    ) -> Self {
        let node = graph.resolve_type(container.inner().ty.as_ref().as_ref());
        let inner = InnerView {
            graph,
            container: index,
            inner: graph.indices[&node],
        };
        match container {
            Container::Array(_) => Self::Array(inner),
            Container::Map(_) => Self::Map(inner),
            Container::Optional(_) => Self::Optional(inner),
        }
    }

    /// Returns an iterator over all the types that this container depends on.
    #[inline]
    pub fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        let (Self::Array(view) | Self::Map(view) | Self::Optional(view)) = self;
        let inner = IrTypeView::new(view.graph, view.inner);
        let dependencies = inner.dependencies();
        std::iter::once(inner).chain(dependencies)
    }
}
