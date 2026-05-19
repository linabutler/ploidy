//! Inline types.
//!
//! [`InlineTypeView`] mirrors [`SchemaTypeView`][schema] for anonymous schemas
//! that are nested inside other schemas or operations:
//!
//! ```yaml
//! components:
//!   schemas:
//!     Pet:
//!       type: object
//!       properties:
//!         address:
//!           type: object
//!           properties:
//!             street:
//!               type: string
//! ```
//!
//! Here, `address` isn't a named schema in `components/schemas`, so Ploidy
//! assigns it an [inline path] based on its position in the spec.
//!
//! [schema]: super::schema::SchemaTypeView
//! [inline path]: super::path::InlineTypePathView

use petgraph::graph::NodeIndex;

use crate::ir::{
    GraphType, InlineTypeId, InlineTypePathRoot, graph::CookedGraph, types::GraphInlineType,
};

use super::{
    HasResource, ViewNode, any::AnyView, container::ContainerView, enum_::EnumView,
    path::InlineTypePathView, primitive::PrimitiveView, struct_::StructView, tagged::TaggedView,
    untagged::UntaggedView,
};

/// A graph-aware view of an [inline type][GraphInlineType].
#[derive(Debug)]
pub enum InlineTypeView<'graph, 'a> {
    Enum(InlineTypeId, EnumView<'graph, 'a>),
    Struct(InlineTypeId, StructView<'graph, 'a>),
    Tagged(InlineTypeId, TaggedView<'graph, 'a>),
    Untagged(InlineTypeId, UntaggedView<'graph, 'a>),
    Container(InlineTypeId, ContainerView<'graph, 'a>),
    Primitive(InlineTypeId, PrimitiveView<'graph, 'a>),
    Any(InlineTypeId, AnyView<'graph, 'a>),
}

impl<'graph, 'a> InlineTypeView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphInlineType<'a>,
    ) -> Self {
        match ty {
            GraphInlineType::Enum(id, ty) => Self::Enum(id, EnumView::new(cooked, index, ty)),
            GraphInlineType::Struct(id, ty) => Self::Struct(id, StructView::new(cooked, index, ty)),
            GraphInlineType::Tagged(id, ty) => Self::Tagged(id, TaggedView::new(cooked, index, ty)),
            GraphInlineType::Untagged(id, ty) => {
                Self::Untagged(id, UntaggedView::new(cooked, index, ty))
            }
            GraphInlineType::Container(id, container) => {
                Self::Container(id, ContainerView::new(cooked, index, container))
            }
            GraphInlineType::Primitive(id, p) => {
                Self::Primitive(id, PrimitiveView::new(cooked, index, p))
            }
            GraphInlineType::Any(id) => Self::Any(id, AnyView::new(cooked, index)),
        }
    }

    /// Returns the path to this inline type.
    #[inline]
    pub fn path(&self) -> InlineTypePathView<'graph, 'a> {
        InlineTypePathView::new(self.cooked(), self.id())
    }

    #[inline]
    fn id(&self) -> InlineTypeId {
        let &(Self::Enum(id, _)
        | Self::Struct(id, _)
        | Self::Tagged(id, _)
        | Self::Untagged(id, _)
        | Self::Container(id, _)
        | Self::Primitive(id, _)
        | Self::Any(id, _)) = self;
        id
    }
}

impl<'a> HasResource<'a> for InlineTypeView<'_, 'a> {
    /// Returns the name of the resource that this inline type belongs to.
    #[inline]
    fn resource(&self) -> Option<&'a str> {
        let cooked = self.cooked();
        let id = self.id();
        match cooked.metadata.paths[&id].root {
            InlineTypePathRoot::Schema(index) => match cooked.graph[index] {
                GraphType::Schema(schema) => schema.info().resource,
                GraphType::Inline(_) => unreachable!(),
            },
            InlineTypePathRoot::Operation { resource, .. } => resource,
        }
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for InlineTypeView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        match self {
            Self::Enum(_, view) => view.cooked(),
            Self::Struct(_, view) => view.cooked(),
            Self::Tagged(_, view) => view.cooked(),
            Self::Untagged(_, view) => view.cooked(),
            Self::Container(_, view) => view.cooked(),
            Self::Primitive(_, view) => view.cooked(),
            Self::Any(_, view) => view.cooked(),
        }
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        match self {
            Self::Enum(_, view) => view.index(),
            Self::Struct(_, view) => view.index(),
            Self::Tagged(_, view) => view.index(),
            Self::Untagged(_, view) => view.index(),
            Self::Container(_, view) => view.index(),
            Self::Primitive(_, view) => view.index(),
            Self::Any(_, view) => view.index(),
        }
    }
}
