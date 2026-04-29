//! Named schema types.
//!
//! A [`SchemaTypeView`] pairs each OpenAPI type view with a [`SchemaTypeInfo`],
//! which carries the schema's name and optional `x-resourceId` for grouping.
//! Ploidy extracts named schema types from the `components/schemas` section
//! of the source [`Spec`][crate::ir::Spec].

use petgraph::graph::NodeIndex;

use crate::ir::{SchemaTypeInfo, graph::CookedGraph, types::GraphSchemaType};

use super::{
    ViewNode, any::AnyView, composition::CompositionView, container::ContainerView,
    enum_::EnumView, primitive::PrimitiveView, struct_::StructView, tagged::TaggedView,
    untagged::UntaggedView,
};

/// A graph-aware view of a [schema type][GraphSchemaType].
#[derive(Debug)]
pub enum SchemaTypeView<'graph, 'a> {
    Composition(SchemaTypeInfo<'a>, CompositionView<'graph, 'a>),
    Enum(SchemaTypeInfo<'a>, EnumView<'graph, 'a>),
    Struct(SchemaTypeInfo<'a>, StructView<'graph, 'a>),
    Tagged(SchemaTypeInfo<'a>, TaggedView<'graph, 'a>),
    Untagged(SchemaTypeInfo<'a>, UntaggedView<'graph, 'a>),
    Container(SchemaTypeInfo<'a>, ContainerView<'graph, 'a>),
    Primitive(SchemaTypeInfo<'a>, PrimitiveView<'graph, 'a>),
    Any(SchemaTypeInfo<'a>, AnyView<'graph, 'a>),
}

impl<'graph, 'a> SchemaTypeView<'graph, 'a> {
    #[inline]
    pub(in crate::ir) fn new(
        cooked: &'graph CookedGraph<'a>,
        index: NodeIndex<usize>,
        ty: GraphSchemaType<'a>,
    ) -> Self {
        match ty {
            GraphSchemaType::Composition(info, ty) => {
                Self::Composition(info, CompositionView::new(cooked, index, ty))
            }
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
        let (Self::Composition(SchemaTypeInfo { name, .. }, ..)
        | Self::Enum(SchemaTypeInfo { name, .. }, ..)
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
    pub fn depends_on(&self, other: &SchemaTypeView<'graph, 'a>) -> bool {
        self.cooked()
            .metadata
            .closure
            .depends_on(self.index(), other.index())
    }

    /// Returns the resource name that this schema type declares
    /// in its `x-resourceId` extension field.
    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        let (&Self::Composition(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Enum(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Struct(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Tagged(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Untagged(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Container(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Primitive(SchemaTypeInfo { resource, .. }, ..)
        | &Self::Any(SchemaTypeInfo { resource, .. }, ..)) = self;
        resource
    }
}

impl<'graph, 'a> ViewNode<'graph, 'a> for SchemaTypeView<'graph, 'a> {
    #[inline]
    fn cooked(&self) -> &'graph CookedGraph<'a> {
        match self {
            Self::Composition(_, view) => view.cooked(),
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
            Self::Composition(_, view) => view.index(),
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
