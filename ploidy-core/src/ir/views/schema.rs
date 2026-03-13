use petgraph::graph::NodeIndex;

use crate::ir::{SchemaTypeInfo, graph::CookedGraph, types::CookedSchemaType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::EnumView, primitive::PrimitiveView,
    struct_::StructView, tagged::TaggedView, untagged::UntaggedView,
};

/// A graph-aware view of a [`SchemaType`][CookedSchemaType].
#[derive(Debug)]
pub enum SchemaTypeView<'a> {
    Enum(SchemaTypeInfo<'a>, EnumView<'a>),
    Struct(SchemaTypeInfo<'a>, StructView<'a>),
    Tagged(SchemaTypeInfo<'a>, TaggedView<'a>),
    Untagged(SchemaTypeInfo<'a>, UntaggedView<'a>),
    Container(SchemaTypeInfo<'a>, ContainerView<'a>),
    Primitive(SchemaTypeInfo<'a>, PrimitiveView<'a>),
    Any(SchemaTypeInfo<'a>, AnyView<'a>),
}

impl<'a> SchemaTypeView<'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'a CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: &'a CookedSchemaType<'a>,
    ) -> Self {
        match ty {
            CookedSchemaType::Enum(info, ty) => Self::Enum(*info, EnumView::new(cooked, index, ty)),
            CookedSchemaType::Struct(info, ty) => {
                Self::Struct(*info, StructView::new(cooked, index, ty))
            }
            CookedSchemaType::Tagged(info, ty) => {
                Self::Tagged(*info, TaggedView::new(cooked, index, ty))
            }
            CookedSchemaType::Untagged(info, ty) => {
                Self::Untagged(*info, UntaggedView::new(cooked, index, ty))
            }
            CookedSchemaType::Container(info, container) => {
                Self::Container(*info, ContainerView::new(cooked, index, container))
            }
            &CookedSchemaType::Primitive(info, p) => {
                Self::Primitive(info, PrimitiveView::new(cooked, index, p))
            }
            &CookedSchemaType::Any(info) => Self::Any(info, AnyView::new(cooked, index)),
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
    pub fn depends_on(&self, other: &SchemaTypeView<'a>) -> bool {
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

impl<'a> ViewNode<'a> for SchemaTypeView<'a> {
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
