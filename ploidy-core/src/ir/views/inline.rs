//! Inline types.
//!
//! [`InlineTypeView`] mirrors [`SchemaTypeView`][schema] for anonymous schemas
//! that are nested inside other schemas or operations. Each view variant
//! pairs an OpenAPI type view with an [`InlineTypeId`] for identity and an
//! [`InlineTrace`] for codegen naming.
//!
//! [schema]: super::schema::SchemaTypeView

use petgraph::graph::NodeIndex;

use crate::ir::{
    InlineTypeId,
    graph::CookedGraph,
    types::{GraphInlineType, InlineTrace},
};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::EnumView, primitive::PrimitiveView,
    struct_::StructView, tagged::TaggedView, untagged::UntaggedView,
};

/// A graph-aware view of an [inline type][GraphInlineType].
#[derive(Debug)]
pub enum InlineTypeView<'graph, 'a> {
    Enum(InlineTypeId, InlineTrace<'a>, EnumView<'graph, 'a>),
    Struct(InlineTypeId, InlineTrace<'a>, StructView<'graph, 'a>),
    Tagged(InlineTypeId, InlineTrace<'a>, TaggedView<'graph, 'a>),
    Untagged(InlineTypeId, InlineTrace<'a>, UntaggedView<'graph, 'a>),
    Container(InlineTypeId, InlineTrace<'a>, ContainerView<'graph, 'a>),
    Primitive(InlineTypeId, InlineTrace<'a>, PrimitiveView<'graph, 'a>),
    Any(InlineTypeId, InlineTrace<'a>, AnyView<'graph, 'a>),
}

impl<'graph, 'a> InlineTypeView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        trace: InlineTrace<'a>,
        ty: GraphInlineType<'a>,
    ) -> Self {
        let id = ty.id();
        match ty {
            GraphInlineType::Enum(_, ty) => Self::Enum(id, trace, EnumView::new(cooked, index, ty)),
            GraphInlineType::Struct(_, ty) => {
                Self::Struct(id, trace, StructView::new(cooked, index, ty))
            }
            GraphInlineType::Tagged(_, ty) => {
                Self::Tagged(id, trace, TaggedView::new(cooked, index, ty))
            }
            GraphInlineType::Untagged(_, ty) => {
                Self::Untagged(id, trace, UntaggedView::new(cooked, index, ty))
            }
            GraphInlineType::Container(_, container) => {
                Self::Container(id, trace, ContainerView::new(cooked, index, container))
            }
            GraphInlineType::Primitive(_, p) => {
                Self::Primitive(id, trace, PrimitiveView::new(cooked, index, p))
            }
            GraphInlineType::Any(_) => Self::Any(id, trace, AnyView::new(cooked, index)),
        }
    }

    /// Returns the opaque identity for this inline type node.
    #[inline]
    pub fn id(&self) -> InlineTypeId {
        let (Self::Enum(id, ..)
        | Self::Struct(id, ..)
        | Self::Tagged(id, ..)
        | Self::Untagged(id, ..)
        | Self::Container(id, ..)
        | Self::Primitive(id, ..)
        | Self::Any(id, ..)) = self;
        *id
    }

    /// Returns the canonical trace from the root context to this
    /// inline type.
    #[inline]
    pub fn trace(&self) -> InlineTrace<'a> {
        let (Self::Enum(_, trace, ..)
        | Self::Struct(_, trace, ..)
        | Self::Tagged(_, trace, ..)
        | Self::Untagged(_, trace, ..)
        | Self::Container(_, trace, ..)
        | Self::Primitive(_, trace, ..)
        | Self::Any(_, trace, ..)) = self;
        *trace
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for InlineTypeView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        match self {
            Self::Enum(_, _, view) => view.cooked(),
            Self::Struct(_, _, view) => view.cooked(),
            Self::Tagged(_, _, view) => view.cooked(),
            Self::Untagged(_, _, view) => view.cooked(),
            Self::Container(_, _, view) => view.cooked(),
            Self::Primitive(_, _, view) => view.cooked(),
            Self::Any(_, _, view) => view.cooked(),
        }
    }

    #[inline]
    fn index(&self) -> NodeIndex<usize> {
        match self {
            Self::Enum(_, _, view) => view.index(),
            Self::Struct(_, _, view) => view.index(),
            Self::Tagged(_, _, view) => view.index(),
            Self::Untagged(_, _, view) => view.index(),
            Self::Container(_, _, view) => view.index(),
            Self::Primitive(_, _, view) => view.index(),
            Self::Any(_, _, view) => view.index(),
        }
    }
}
