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
//! assigns it the inline path `Type("Pet") / Field("address")`. Each
//! [`InlineTypeView`] variant pairs an OpenAPI type view with an
//! [`InlineTypePath`] like this one, which codegen uses to derive a
//! stable generated name.
//!
//! [schema]: super::schema::SchemaTypeView

use petgraph::graph::NodeIndex;

use crate::ir::{InlineTypePath, graph::CookedGraph, types::GraphInlineType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::EnumView, primitive::PrimitiveView,
    struct_::StructView, tagged::TaggedView, untagged::UntaggedView,
};

/// A graph-aware view of an [inline type][GraphInlineType].
#[derive(Debug)]
pub enum InlineTypeView<'graph, 'a> {
    Enum(InlineTypePath<'a>, EnumView<'graph, 'a>),
    Struct(InlineTypePath<'a>, StructView<'graph, 'a>),
    Tagged(InlineTypePath<'a>, TaggedView<'graph, 'a>),
    Untagged(InlineTypePath<'a>, UntaggedView<'graph, 'a>),
    Container(InlineTypePath<'a>, ContainerView<'graph, 'a>),
    Primitive(InlineTypePath<'a>, PrimitiveView<'graph, 'a>),
    Any(InlineTypePath<'a>, AnyView<'graph, 'a>),
}

impl<'graph, 'a> InlineTypeView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphInlineType<'a>,
    ) -> Self {
        match ty {
            GraphInlineType::Enum(path, ty) => Self::Enum(path, EnumView::new(cooked, index, ty)),
            GraphInlineType::Struct(path, ty) => {
                Self::Struct(path, StructView::new(cooked, index, ty))
            }
            GraphInlineType::Tagged(path, ty) => {
                Self::Tagged(path, TaggedView::new(cooked, index, ty))
            }
            GraphInlineType::Untagged(path, ty) => {
                Self::Untagged(path, UntaggedView::new(cooked, index, ty))
            }
            GraphInlineType::Container(path, container) => {
                Self::Container(path, ContainerView::new(cooked, index, container))
            }
            GraphInlineType::Primitive(path, p) => {
                Self::Primitive(path, PrimitiveView::new(cooked, index, p))
            }
            GraphInlineType::Any(path) => Self::Any(path, AnyView::new(cooked, index)),
        }
    }

    /// Returns the path describing where this inline type was found
    /// in the spec.
    #[inline]
    pub fn path(&self) -> InlineTypePath<'a> {
        let (Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)
        | Self::Container(path, _)
        | Self::Primitive(path, _)
        | Self::Any(path, _)) = self;
        *path
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
