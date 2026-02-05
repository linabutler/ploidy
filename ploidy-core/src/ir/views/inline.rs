use petgraph::graph::NodeIndex;

use crate::ir::{InlineIrTypePath, graph::IrGraph, types::InlineIrType};

use super::{
    ViewNode, container::ContainerView, enum_::IrEnumView, struct_::IrStructView,
    tagged::IrTaggedView, untagged::IrUntaggedView,
};

/// A graph-aware view of an [`InlineIrType`].
#[derive(Debug)]
pub enum InlineIrTypeView<'a> {
    Enum(&'a InlineIrTypePath<'a>, IrEnumView<'a>),
    Struct(&'a InlineIrTypePath<'a>, IrStructView<'a>),
    Tagged(&'a InlineIrTypePath<'a>, IrTaggedView<'a>),
    Untagged(&'a InlineIrTypePath<'a>, IrUntaggedView<'a>),
    Container(&'a InlineIrTypePath<'a>, ContainerView<'a>),
}

impl<'a> InlineIrTypeView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a InlineIrType<'a>,
    ) -> Self {
        match ty {
            InlineIrType::Enum(path, ty) => Self::Enum(path, IrEnumView::new(graph, index, ty)),
            InlineIrType::Struct(path, ty) => {
                Self::Struct(path, IrStructView::new(graph, index, ty))
            }
            InlineIrType::Tagged(path, ty) => {
                Self::Tagged(path, IrTaggedView::new(graph, index, ty))
            }
            InlineIrType::Untagged(path, ty) => {
                Self::Untagged(path, IrUntaggedView::new(graph, index, ty))
            }
            InlineIrType::Container(path, container) => {
                Self::Container(path, ContainerView::new(graph, index, container))
            }
        }
    }

    #[inline]
    pub fn path(&self) -> &'a InlineIrTypePath<'a> {
        let (Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)
        | Self::Container(path, _)) = self;
        path
    }
}

impl<'a> ViewNode<'a> for InlineIrTypeView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        match self {
            Self::Enum(_, view) => view.graph(),
            Self::Struct(_, view) => view.graph(),
            Self::Tagged(_, view) => view.graph(),
            Self::Untagged(_, view) => view.graph(),
            Self::Container(_, view) => view.graph(),
        }
    }

    fn index(&self) -> NodeIndex<usize> {
        match self {
            Self::Enum(_, view) => view.index(),
            Self::Struct(_, view) => view.index(),
            Self::Tagged(_, view) => view.index(),
            Self::Untagged(_, view) => view.index(),
            Self::Container(_, view) => view.index(),
        }
    }
}
