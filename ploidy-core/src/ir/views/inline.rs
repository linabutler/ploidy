use petgraph::graph::NodeIndex;

use crate::ir::{InlineIrTypePath, graph::CookedGraph, types::InlineIrType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::IrEnumView,
    primitive::IrPrimitiveView, struct_::IrStructView, tagged::IrTaggedView,
    untagged::IrUntaggedView,
};

/// A graph-aware view of an [`InlineIrType`].
#[derive(Debug)]
pub enum InlineIrTypeView<'a> {
    Enum(&'a InlineIrTypePath<'a>, IrEnumView<'a>),
    Struct(&'a InlineIrTypePath<'a>, IrStructView<'a>),
    Tagged(&'a InlineIrTypePath<'a>, IrTaggedView<'a>),
    Untagged(&'a InlineIrTypePath<'a>, IrUntaggedView<'a>),
    Container(&'a InlineIrTypePath<'a>, ContainerView<'a>),
    Primitive(&'a InlineIrTypePath<'a>, IrPrimitiveView<'a>),
    Any(&'a InlineIrTypePath<'a>, AnyView<'a>),
}

impl<'a> InlineIrTypeView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a InlineIrType<'a, NodeIndex<usize>>,
    ) -> Self {
        match ty {
            InlineIrType::Enum(path, ty) => Self::Enum(path, IrEnumView::new(cooked, index, ty)),
            InlineIrType::Struct(path, ty) => {
                Self::Struct(path, IrStructView::new(cooked, index, ty))
            }
            InlineIrType::Tagged(path, ty) => {
                Self::Tagged(path, IrTaggedView::new(cooked, index, ty))
            }
            InlineIrType::Untagged(path, ty) => {
                Self::Untagged(path, IrUntaggedView::new(cooked, index, ty))
            }
            InlineIrType::Container(path, container) => {
                Self::Container(path, ContainerView::new(cooked, index, container))
            }
            InlineIrType::Primitive(path, p) => {
                Self::Primitive(path, IrPrimitiveView::new(cooked, index, *p))
            }
            InlineIrType::Any(path) => Self::Any(path, AnyView::new(cooked, index)),
        }
    }

    #[inline]
    pub fn path(&self) -> &'a InlineIrTypePath<'a> {
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

impl<'a> ViewNode<'a> for InlineIrTypeView<'a> {
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
