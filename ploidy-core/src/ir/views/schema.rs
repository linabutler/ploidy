use petgraph::graph::NodeIndex;

use crate::ir::{graph::IrGraph, types::SchemaIrType};

use super::{
    ViewNode, enum_::IrEnumView, struct_::IrStructView, tagged::IrTaggedView,
    untagged::IrUntaggedView,
};

/// A graph-aware view of a [`SchemaIrType`].
#[derive(Debug)]
pub enum SchemaIrTypeView<'a> {
    Enum(&'a str, IrEnumView<'a>),
    Struct(&'a str, IrStructView<'a>),
    Tagged(&'a str, IrTaggedView<'a>),
    Untagged(&'a str, IrUntaggedView<'a>),
}

impl<'a> SchemaIrTypeView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex,
        ty: &'a SchemaIrType<'a>,
    ) -> Self {
        match ty {
            SchemaIrType::Enum(name, ty) => Self::Enum(name, IrEnumView::new(graph, index, ty)),
            SchemaIrType::Struct(name, ty) => {
                Self::Struct(name, IrStructView::new(graph, index, ty))
            }
            SchemaIrType::Tagged(name, ty) => {
                Self::Tagged(name, IrTaggedView::new(graph, index, ty))
            }
            SchemaIrType::Untagged(name, ty) => {
                Self::Untagged(name, IrUntaggedView::new(graph, index, ty))
            }
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(name, _)
        | Self::Struct(name, _)
        | Self::Tagged(name, _)
        | Self::Untagged(name, _)) = self;
        name
    }
}

impl<'a> ViewNode<'a> for SchemaIrTypeView<'a> {
    #[inline]
    fn graph(&self) -> &'a IrGraph<'a> {
        match self {
            Self::Enum(_, view) => view.graph(),
            Self::Struct(_, view) => view.graph(),
            Self::Tagged(_, view) => view.graph(),
            Self::Untagged(_, view) => view.graph(),
        }
    }

    #[inline]
    fn index(&self) -> NodeIndex {
        match self {
            Self::Enum(_, view) => view.index(),
            Self::Struct(_, view) => view.index(),
            Self::Tagged(_, view) => view.index(),
            Self::Untagged(_, view) => view.index(),
        }
    }
}
