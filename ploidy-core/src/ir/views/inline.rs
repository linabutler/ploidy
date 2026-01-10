use petgraph::graph::NodeIndex;

use crate::ir::{InlineIrTypePath, graph::IrGraph, types::InlineIrType};

use super::{ViewNode, enum_::IrEnumView, struct_::IrStructView, untagged::IrUntaggedView};

/// A graph-aware view of an [`InlineIrType`].
#[derive(Debug)]
pub enum InlineIrTypeView<'a> {
    Enum(&'a InlineIrTypePath<'a>, IrEnumView<'a>),
    Struct(&'a InlineIrTypePath<'a>, IrStructView<'a>),
    Untagged(&'a InlineIrTypePath<'a>, IrUntaggedView<'a>),
}

impl<'a> InlineIrTypeView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex,
        ty: &'a InlineIrType<'a>,
    ) -> Self {
        match ty {
            InlineIrType::Enum(name, ty) => Self::Enum(name, IrEnumView::new(graph, index, ty)),
            InlineIrType::Struct(name, ty) => {
                Self::Struct(name, IrStructView::new(graph, index, ty))
            }
            InlineIrType::Untagged(name, ty) => {
                Self::Untagged(name, IrUntaggedView::new(graph, index, ty))
            }
        }
    }

    #[inline]
    pub fn path(&self) -> &'a InlineIrTypePath<'a> {
        let (Self::Enum(path, _) | Self::Struct(path, _) | Self::Untagged(path, _)) = self;
        path
    }
}

impl<'a> ViewNode<'a> for InlineIrTypeView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        match self {
            Self::Enum(_, view) => view.graph(),
            Self::Struct(_, view) => view.graph(),
            Self::Untagged(_, view) => view.graph(),
        }
    }

    fn index(&self) -> NodeIndex {
        match self {
            Self::Enum(_, view) => view.index(),
            Self::Struct(_, view) => view.index(),
            Self::Untagged(_, view) => view.index(),
        }
    }
}
