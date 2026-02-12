use petgraph::graph::NodeIndex;

use crate::ir::graph::{IrGraph, IrGraphNode};

use super::{View, container::ContainerView, inline::InlineIrTypeView, schema::SchemaIrTypeView};

/// A graph-aware view of an [`IrType`][crate::ir::IrType].
#[derive(Debug)]
pub enum IrTypeView<'a> {
    Schema(SchemaIrTypeView<'a>),
    Inline(InlineIrTypeView<'a>),
}

impl<'a> IrTypeView<'a> {
    pub(in crate::ir) fn new(graph: &'a IrGraph<'a>, index: NodeIndex<usize>) -> Self {
        match &graph.g[index] {
            IrGraphNode::Schema(ty) => Self::Schema(SchemaIrTypeView::new(graph, index, ty)),
            IrGraphNode::Inline(ty) => Self::Inline(InlineIrTypeView::new(graph, index, ty)),
        }
    }

    /// If this is a view of a named schema type, returns that schema type;
    /// otherwise, returns an [`Err`] with this view.
    #[inline]
    pub fn into_schema(self) -> Result<SchemaIrTypeView<'a>, Self> {
        match self {
            Self::Schema(view) => Ok(view),
            other => Err(other),
        }
    }

    /// If this is a view of a named or inline container type,
    /// returns the container view.
    #[inline]
    pub fn as_container(&self) -> Option<&ContainerView<'a>> {
        match self {
            Self::Schema(SchemaIrTypeView::Container(_, view)) => Some(view),
            Self::Inline(InlineIrTypeView::Container(_, view)) => Some(view),
            _ => None,
        }
    }

    /// Returns an iterator over all the types that this type transitively depends on.
    #[inline]
    pub fn dependencies(&self) -> impl Iterator<Item = IrTypeView<'a>> + use<'a> {
        either!(match self {
            Self::Schema(v) => v.dependencies(),
            Self::Inline(v) => v.dependencies(),
        })
    }
}
