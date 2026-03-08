use petgraph::graph::NodeIndex;

use crate::ir::{SchemaTypeInfo, graph::CookedGraph, types::SchemaIrType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::IrEnumView,
    primitive::IrPrimitiveView, struct_::IrStructView, tagged::IrTaggedView,
    untagged::IrUntaggedView,
};

/// A graph-aware view of a [`SchemaIrType`].
#[derive(Debug)]
pub enum SchemaIrTypeView<'a> {
    Enum(SchemaTypeInfo<'a>, IrEnumView<'a>),
    Struct(SchemaTypeInfo<'a>, IrStructView<'a>),
    Tagged(SchemaTypeInfo<'a>, IrTaggedView<'a>),
    Untagged(SchemaTypeInfo<'a>, IrUntaggedView<'a>),
    Container(SchemaTypeInfo<'a>, ContainerView<'a>),
    Primitive(SchemaTypeInfo<'a>, IrPrimitiveView<'a>),
    Any(SchemaTypeInfo<'a>, AnyView<'a>),
}

impl<'a> SchemaIrTypeView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a SchemaIrType<'a>,
    ) -> Self {
        match ty {
            SchemaIrType::Enum(info, ty) => Self::Enum(*info, IrEnumView::new(cooked, index, ty)),
            SchemaIrType::Struct(info, ty) => {
                Self::Struct(*info, IrStructView::new(cooked, index, ty))
            }
            SchemaIrType::Tagged(info, ty) => {
                Self::Tagged(*info, IrTaggedView::new(cooked, index, ty))
            }
            SchemaIrType::Untagged(info, ty) => {
                Self::Untagged(*info, IrUntaggedView::new(cooked, index, ty))
            }
            SchemaIrType::Container(info, container) => {
                Self::Container(*info, ContainerView::new(cooked, index, container))
            }
            &SchemaIrType::Primitive(info, p) => {
                Self::Primitive(info, IrPrimitiveView::new(cooked, index, p))
            }
            &SchemaIrType::Any(info) => Self::Any(info, AnyView::new(cooked, index)),
        }
    }

    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(SchemaTypeInfo { name, .. }, ..)
        | Self::Struct(SchemaTypeInfo { name, .. }, ..)
        | Self::Tagged(SchemaTypeInfo { name, .. }, ..)
        | Self::Untagged(SchemaTypeInfo { name, .. }, ..)
        | Self::Container(SchemaTypeInfo { name, .. }, ..)
        | Self::Primitive(SchemaTypeInfo { name, .. }, ..)
        | Self::Any(SchemaTypeInfo { name, .. }, ..)) = self;
        name
    }

    /// Returns whether this type transitively depends on `other`.
    #[inline]
    pub fn depends_on(&self, other: &SchemaIrTypeView<'a>) -> bool {
        self.cooked().metadata.schemas[self.index().index()]
            .dependencies
            .contains(other.index().index())
    }

    /// Returns the resource name that this schema type declares
    /// in its `x-resourceId` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        let (&Self::Enum(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Struct(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Tagged(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Untagged(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Container(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Primitive(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Any(SchemaTypeInfo { resource, .. }, ..)) = self;
        resource
    }
}

impl<'a> ViewNode<'a> for SchemaIrTypeView<'a> {
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
