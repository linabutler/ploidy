use either::Either;
use petgraph::graph::NodeIndex;

use crate::ir::graph::{IrGraph, IrGraphNode};

use super::{
    View,
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
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex<usize>) -> Self {
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

    /// If this is a view of a named schema type, returns the view for that type.
    #[inline]
    pub fn as_schema(self) -> Option<SchemaIrTypeView<'a>> {
        match self {
            Self::Schema(view) => Some(view),
            _ => None,
        }
    }

    /// Returns an iterator over all the types that this type transitively depends on.
    pub fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        match self {
            Self::Any => Either::Left(std::iter::empty()),
            Self::Primitive(v) => Either::Right(Either::Left(v.dependencies())),
            Self::Array(v) => Either::Right(Either::Right(Either::Left(v.dependencies()))),
            Self::Map(v) => {
                Either::Right(Either::Right(Either::Right(Either::Left(v.dependencies()))))
            }
            Self::Optional(v) => Either::Right(Either::Right(Either::Right(Either::Right(
                Either::Left(v.dependencies()),
            )))),
            Self::Schema(v) => Either::Right(Either::Right(Either::Right(Either::Right(
                Either::Right(Either::Left(v.dependencies())),
            )))),
            Self::Inline(v) => Either::Right(Either::Right(Either::Right(Either::Right(
                Either::Right(Either::Right(v.dependencies())),
            )))),
        }
    }
}
