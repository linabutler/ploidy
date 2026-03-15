//! Named schema types.
//!
//! A [`SchemaTypeView`] pairs each OpenAPI type view with a [`SchemaTypeInfo`],
//! which carries the schema's name and optional `x-resourceId` for grouping.
//! Ploidy extracts named schema types from the `components/schemas` section
//! of the source [`Spec`][crate::ir::Spec].

use petgraph::graph::NodeIndex;

use crate::ir::{SchemaTypeInfo, graph::CookedGraph, types::GraphSchemaType};

use super::{
    ViewNode, any::AnyView, container::ContainerView, enum_::EnumView, primitive::PrimitiveView,
    struct_::StructView, tagged::TaggedView, untagged::UntaggedView,
};

/// A graph-aware view of a [schema type][GraphSchemaType].
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
        ty: GraphSchemaType<'a>,
    ) -> Self {
        match ty {
            GraphSchemaType::Enum(info, ty) => Self::Enum(info, EnumView::new(cooked, index, ty)),
            GraphSchemaType::Struct(info, ty) => {
                Self::Struct(info, StructView::new(cooked, index, ty))
            }
            GraphSchemaType::Tagged(info, ty) => {
                Self::Tagged(info, TaggedView::new(cooked, index, ty))
            }
            GraphSchemaType::Untagged(info, ty) => {
                Self::Untagged(info, UntaggedView::new(cooked, index, ty))
            }
            GraphSchemaType::Container(info, container) => {
                Self::Container(info, ContainerView::new(cooked, index, container))
            }
            GraphSchemaType::Primitive(info, p) => {
                Self::Primitive(info, PrimitiveView::new(cooked, index, p))
            }
            GraphSchemaType::Any(info) => Self::Any(info, AnyView::new(cooked, index)),
        }
    }

    /// Returns the schema name from `components/schemas`.
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
