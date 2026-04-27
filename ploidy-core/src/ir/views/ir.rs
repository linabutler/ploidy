//! An OpenAPI type.
//!
//! A [`TypeView`] represents an arbitrary type in the graph. Each type is
//! either a named top-level [`Schema`][SchemaTypeView] definition from
//! `components/schemas`, or an anonymous [`Inline`][InlineTypeView] schema
//! nested inside another type or operation.

use either::Either;
use petgraph::graph::NodeIndex;

use crate::ir::{graph::CookedGraph, types::GraphType};

use super::{
    Identifiable, TypeViewId, View, container::ContainerView, inline::InlineTypeView,
    schema::SchemaTypeView,
};

/// A graph-aware view of a [schema][crate::ir::GraphSchemaType] or
/// an [inline][crate::ir::GraphInlineType] type.
#[derive(Debug)]
pub enum TypeView<'graph, 'a> {
    Schema(SchemaTypeView<'graph, 'a>),
    Inline(InlineTypeView<'graph, 'a>),
}

impl<'graph, 'a> TypeView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(cooked: &'graph CookedGraph<'a>, index: NodeIndex<usize>) -> Self {
        match cooked.graph[index] {
            GraphType::Schema(ty) => Self::Schema(SchemaTypeView::new(cooked, index, ty)),
            GraphType::Inline(ty) => Self::Inline(InlineTypeView::new(cooked, index, ty)),
        }
    }

    /// Returns an opaque identity for this type.
    #[inline]
    pub fn id(&self) -> TypeViewId {
        match self {
            Self::Schema(ty) => ty.id(),
            Self::Inline(ty) => ty.id(),
        }
    }

    /// If this is a view of a named schema type, returns that type
    /// on the [`Right`]; otherwise, returns this view on the [`Left`].
    ///
    /// [`Right`]: either::Right
    /// [`Left`]: either::Left
    #[inline]
    pub fn into_schema(self) -> Either<Self, SchemaTypeView<'graph, 'a>> {
        match self {
            Self::Schema(view) => Either::Right(view),
            other => Either::Left(other),
        }
    }

    /// If this is a view of a named or inline container type,
    /// returns the container view.
    #[inline]
    pub fn as_container(&self) -> Option<&ContainerView<'graph, 'a>> {
        match self {
            Self::Schema(SchemaTypeView::Container(_, view)) => Some(view),
            Self::Inline(InlineTypeView::Container(_, view)) => Some(view),
            _ => None,
        }
    }

    /// Returns an iterator over all the types that this type transitively depends on.
    #[inline]
    pub fn dependencies(&self) -> impl Iterator<Item = TypeView<'graph, 'a>> + use<'graph, 'a> {
        either!(match self {
            Self::Schema(v) => v.dependencies(),
            Self::Inline(v) => v.dependencies(),
        })
    }
}
