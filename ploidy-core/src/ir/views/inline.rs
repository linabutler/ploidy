use petgraph::graph::NodeIndex;

use crate::ir::{InlineTypePath, graph::CookedGraph, types::CookedInlineType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::EnumView, primitive::PrimitiveView,
    struct_::StructView, tagged::TaggedView, untagged::UntaggedView,
};

/// A graph-aware view of an [`InlineType`][CookedInlineType].
#[derive(Debug)]
pub enum InlineTypeView<'a> {
    Enum(&'a InlineTypePath<'a>, EnumView<'a>),
    Struct(&'a InlineTypePath<'a>, StructView<'a>),
    Tagged(&'a InlineTypePath<'a>, TaggedView<'a>),
    Untagged(&'a InlineTypePath<'a>, UntaggedView<'a>),
    Container(&'a InlineTypePath<'a>, ContainerView<'a>),
    Primitive(&'a InlineTypePath<'a>, PrimitiveView<'a>),
    Any(&'a InlineTypePath<'a>, AnyView<'a>),
}

impl<'a> InlineTypeView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a CookedInlineType<'a>,
    ) -> Self {
        match ty {
            CookedInlineType::Enum(path, ty) => Self::Enum(path, EnumView::new(cooked, index, ty)),
            CookedInlineType::Struct(path, ty) => {
                Self::Struct(path, StructView::new(cooked, index, ty))
            }
            CookedInlineType::Tagged(path, ty) => {
                Self::Tagged(path, TaggedView::new(cooked, index, ty))
            }
            CookedInlineType::Untagged(path, ty) => {
                Self::Untagged(path, UntaggedView::new(cooked, index, ty))
            }
            CookedInlineType::Container(path, container) => {
                Self::Container(path, ContainerView::new(cooked, index, container))
            }
            CookedInlineType::Primitive(path, p) => {
                Self::Primitive(path, PrimitiveView::new(cooked, index, *p))
            }
            CookedInlineType::Any(path) => Self::Any(path, AnyView::new(cooked, index)),
        }
    }

    #[inline]
    pub fn path(&self) -> &'a InlineTypePath<'a> {
        let (Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)
        | Self::Container(path, _)
        | Self::Primitive(path, _)
        | Self::Any(path, _)) = self;
        path
    }
}

impl<'a> ViewNode<'a> for InlineTypeView<'a> {
    #[inline]
    fn cooked(&self) -> &'a CookedGraph<'a> {
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
