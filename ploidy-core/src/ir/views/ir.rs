use either::Either;
use petgraph::{graph::NodeIndex, visit::Bfs};

use crate::ir::graph::{IrGraph, IrGraphNode};

use super::{
    ViewNode,
    inline::InlineIrTypeView,
    schema::SchemaIrTypeView,
    wrappers::{IrArrayView, IrMapView, IrOptionalView, IrPrimitiveView},
};

/// A graph-aware view of an [`IrType`][crate::ir::IrType].
#[derive(Debug)]
pub enum IrTypeView<'a> {
    Any,
    Primitive(IrPrimitiveView<'a>),
    Array(IrArrayView<'a>),
    Map(IrMapView<'a>),
    Optional(IrOptionalView<'a>),
    Schema(SchemaIrTypeView<'a>),
    Inline(InlineIrTypeView<'a>),
}

impl<'a> IrTypeView<'a> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex) -> Self {
        match &graph.g[index] {
            IrGraphNode::Any => IrTypeView::Any,
            &IrGraphNode::Primitive(ty) => {
                IrTypeView::Primitive(IrPrimitiveView::new(graph, index, ty))
            }
            IrGraphNode::Array(inner) => IrTypeView::Array(IrArrayView::new(graph, index, inner)),
            IrGraphNode::Map(inner) => IrTypeView::Map(IrMapView::new(graph, index, inner)),
            IrGraphNode::Optional(inner) => {
                IrTypeView::Optional(IrOptionalView::new(graph, index, inner))
            }
            IrGraphNode::Schema(ty) => Self::Schema(SchemaIrTypeView::new(graph, index, ty)),
            IrGraphNode::Inline(ty) => Self::Inline(InlineIrTypeView::new(graph, index, ty)),
        }
    }

    /// Returns an iterator over all the types that are reachable from this type,
    /// including this type.
    #[inline]
    pub fn reachable(&self) -> impl Iterator<Item = IrTypeView<'a>> {
        fn bfs<'a>(
            graph: &'a IrGraph<'a>,
            index: NodeIndex,
        ) -> impl Iterator<Item = IrTypeView<'a>> {
            let mut bfs = Bfs::new(&graph.g, index);
            std::iter::from_fn(move || bfs.next(&graph.g))
                .map(|index| IrTypeView::new(graph, index))
        }
        match self {
            Self::Any => Either::Left(std::iter::once(IrTypeView::Any)),
            Self::Primitive(p) => Either::Right(bfs(p.graph(), p.index())),
            Self::Array(v) => Either::Right(bfs(v.graph(), v.index())),
            Self::Map(v) => Either::Right(bfs(v.graph(), v.index())),
            Self::Optional(v) => Either::Right(bfs(v.graph(), v.index())),
            Self::Schema(v) => Either::Right(bfs(v.graph(), v.index())),
            Self::Inline(v) => Either::Right(bfs(v.graph(), v.index())),
        }
    }
}
