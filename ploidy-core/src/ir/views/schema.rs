use petgraph::graph::NodeIndex;

use crate::ir::{SchemaTypeInfo, graph::IrGraph, types::SchemaIrType};

use super::{
    ViewNode, enum_::IrEnumView, struct_::IrStructView, tagged::IrTaggedView,
    untagged::IrUntaggedView,
};

/// A graph-aware view of a [`SchemaIrType`].
#[derive(Debug)]
pub enum SchemaIrTypeView<'a> {
    Enum(SchemaTypeInfo<'a>, IrEnumView<'a>),
    Struct(SchemaTypeInfo<'a>, IrStructView<'a>),
    Tagged(SchemaTypeInfo<'a>, IrTaggedView<'a>),
    Untagged(SchemaTypeInfo<'a>, IrUntaggedView<'a>),
}

impl<'a> SchemaIrTypeView<'a> {
    pub(in crate::ir) fn new(
        graph: &'a IrGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a SchemaIrType<'a>,
    ) -> Self {
        match ty {
            SchemaIrType::Enum(info, ty) => Self::Enum(*info, IrEnumView::new(graph, index, ty)),
            SchemaIrType::Struct(info, ty) => {
                Self::Struct(*info, IrStructView::new(graph, index, ty))
            }
            SchemaIrType::Tagged(info, ty) => {
                Self::Tagged(*info, IrTaggedView::new(graph, index, ty))
            }
            SchemaIrType::Untagged(info, ty) => {
                Self::Untagged(*info, IrUntaggedView::new(graph, index, ty))
            }
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(SchemaTypeInfo { name, .. }, ..)
        | Self::Struct(SchemaTypeInfo { name, .. }, ..)
        | Self::Tagged(SchemaTypeInfo { name, .. }, ..)
        | Self::Untagged(SchemaTypeInfo { name, .. }, ..)) = self;
        name
    }

    /// Returns whether this type transitively depends on `other`.
    #[inline]
    pub fn depends_on(&self, other: &SchemaIrTypeView<'a>) -> bool {
        self.graph()
            .metadata
            .schemas
            .get(&self.index())
            .is_some_and(|meta| meta.dependencies.contains(other.index().index()))
    }

    /// Returns the resource name that this schema type declares
    /// in its `x-resourceId` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        let (&Self::Enum(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Struct(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Tagged(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Untagged(SchemaTypeInfo { resource, .. }, ..)) = self;
        resource
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
    fn index(&self) -> NodeIndex<usize> {
        match self {
            Self::Enum(_, view) => view.index(),
            Self::Struct(_, view) => view.index(),
            Self::Tagged(_, view) => view.index(),
            Self::Untagged(_, view) => view.index(),
        }
    }
}
