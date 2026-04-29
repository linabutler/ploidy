//! IR types with graph node references.

use petgraph::graph::NodeIndex;

use super::{
    Enum, InlineTypePath, PrimitiveType, SchemaTypeInfo, StructFieldName, UntaggedVariantNameHint,
    shape::{Operation, Parameter, ParameterInfo, Request, Response},
    spec::{SpecComposition, SpecContainer, SpecInlineType, SpecSchemaType},
};

/// A type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphType<'a> {
    Schema(GraphSchemaType<'a>),
    Inline(GraphInlineType<'a>),
}

/// A named schema type in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphSchemaType<'a> {
    /// An enum with named variants.
    Enum(SchemaTypeInfo<'a>, Enum<'a>),
    /// A composition of other schemas.
    Composition(SchemaTypeInfo<'a>, GraphComposition<'a>),
    /// A struct with fields.
    Struct(SchemaTypeInfo<'a>, GraphStruct<'a>),
    /// A tagged union.
    Tagged(SchemaTypeInfo<'a>, GraphTagged<'a>),
    /// An untagged union.
    Untagged(SchemaTypeInfo<'a>, GraphUntagged<'a>),
    /// A named container.
    Container(SchemaTypeInfo<'a>, GraphContainer<'a>),
    /// A primitive type.
    Primitive(SchemaTypeInfo<'a>, PrimitiveType),
    /// Any JSON value.
    Any(SchemaTypeInfo<'a>),
}

impl<'a> GraphSchemaType<'a> {
    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(info, ..)
        | Self::Composition(info, ..)
        | Self::Struct(info, ..)
        | Self::Tagged(info, ..)
        | Self::Untagged(info, ..)
        | Self::Container(info, ..)
        | Self::Primitive(info, ..)
        | Self::Any(info)) = self;
        info.name
    }

    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        let (Self::Enum(info, ..)
        | Self::Composition(info, ..)
        | Self::Struct(info, ..)
        | Self::Tagged(info, ..)
        | Self::Untagged(info, ..)
        | Self::Container(info, ..)
        | Self::Primitive(info, ..)
        | Self::Any(info)) = self;
        info.resource
    }
}

impl<'a> From<SpecSchemaType<'a>> for GraphSchemaType<'a> {
    fn from(spec: SpecSchemaType<'a>) -> Self {
        match spec {
            SpecSchemaType::Composition(info, c) => Self::Composition(info, c.into()),
            SpecSchemaType::Enum(info, e) => Self::Enum(info, e),
            SpecSchemaType::Struct(info, s) => Self::Struct(
                info,
                GraphStruct {
                    description: s.description,
                },
            ),
            SpecSchemaType::Tagged(info, t) => Self::Tagged(
                info,
                GraphTagged {
                    description: t.description,
                    tag: t.tag,
                },
            ),
            SpecSchemaType::Untagged(info, u) => Self::Untagged(
                info,
                GraphUntagged {
                    description: u.description,
                },
            ),
            SpecSchemaType::Container(info, c) => Self::Container(info, c.into()),
            SpecSchemaType::Primitive(info, p) => Self::Primitive(info, p),
            SpecSchemaType::Any(info) => Self::Any(info),
        }
    }
}

/// An inline type in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphInlineType<'a> {
    Composition(InlineTypePath<'a>, GraphComposition<'a>),
    Enum(InlineTypePath<'a>, Enum<'a>),
    Struct(InlineTypePath<'a>, GraphStruct<'a>),
    Tagged(InlineTypePath<'a>, GraphTagged<'a>),
    Untagged(InlineTypePath<'a>, GraphUntagged<'a>),
    Container(InlineTypePath<'a>, GraphContainer<'a>),
    Primitive(InlineTypePath<'a>, PrimitiveType),
    Any(InlineTypePath<'a>),
}

impl<'a> GraphInlineType<'a> {
    #[inline]
    pub fn path(&self) -> &InlineTypePath<'a> {
        let (Self::Composition(path, _)
        | Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)
        | Self::Container(path, _)
        | Self::Primitive(path, _)
        | Self::Any(path)) = self;
        path
    }
}

impl<'a> From<SpecInlineType<'a>> for GraphInlineType<'a> {
    fn from(spec: SpecInlineType<'a>) -> Self {
        match spec {
            SpecInlineType::Composition(path, c) => Self::Composition(path, c.into()),
            SpecInlineType::Enum(path, e) => Self::Enum(path, e),
            SpecInlineType::Struct(path, s) => Self::Struct(
                path,
                GraphStruct {
                    description: s.description,
                },
            ),
            SpecInlineType::Tagged(path, t) => Self::Tagged(
                path,
                GraphTagged {
                    description: t.description,
                    tag: t.tag,
                },
            ),
            SpecInlineType::Untagged(path, u) => Self::Untagged(
                path,
                GraphUntagged {
                    description: u.description,
                },
            ),
            SpecInlineType::Container(path, c) => Self::Container(path, c.into()),
            SpecInlineType::Primitive(path, p) => Self::Primitive(path, p),
            SpecInlineType::Any(path) => Self::Any(path),
        }
    }
}

/// A composition in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphComposition<'a> {
    pub description: Option<&'a str>,
}

impl<'a> From<SpecComposition<'a>> for GraphComposition<'a> {
    fn from(spec: SpecComposition<'a>) -> Self {
        Self {
            description: spec.description,
        }
    }
}

/// A struct in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphStruct<'a> {
    pub description: Option<&'a str>,
}

/// A tagged union in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphTagged<'a> {
    pub description: Option<&'a str>,
    pub tag: &'a str,
}

/// An untagged union in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GraphUntagged<'a> {
    pub description: Option<&'a str>,
}

/// A container in the graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphContainer<'a> {
    Array { description: Option<&'a str> },
    Map { description: Option<&'a str> },
    Optional { description: Option<&'a str> },
}

impl<'a> From<SpecContainer<'a>> for GraphContainer<'a> {
    fn from(spec: SpecContainer<'a>) -> Self {
        match spec {
            SpecContainer::Array(inner) => Self::Array {
                description: inner.description,
            },
            SpecContainer::Map(inner) => Self::Map {
                description: inner.description,
            },
            SpecContainer::Optional(inner) => Self::Optional {
                description: inner.description,
            },
        }
    }
}

/// Metadata for a struct or union field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldMeta<'a> {
    pub name: StructFieldName<'a>,
    pub required: bool,
    pub description: Option<&'a str>,
    pub flattened: bool,
}

/// Metadata for a tagged or untagged union variant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VariantMeta<'a> {
    /// A tagged union variant with a discriminator.
    Tagged(TaggedVariantMeta<'a>),
    /// An untagged union variant.
    Untagged(UntaggedVariantMeta),
}

impl<'a> From<TaggedVariantMeta<'a>> for VariantMeta<'a> {
    fn from(meta: TaggedVariantMeta<'a>) -> Self {
        Self::Tagged(meta)
    }
}

impl From<UntaggedVariantMeta> for VariantMeta<'_> {
    fn from(meta: UntaggedVariantMeta) -> Self {
        Self::Untagged(meta)
    }
}

/// Metadata for a tagged union variant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaggedVariantMeta<'a> {
    pub name: &'a str,
    pub aliases: &'a [&'a str],
}

/// Metadata for an untagged union variant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntaggedVariantMeta {
    Type { hint: UntaggedVariantNameHint },
    Null,
}

/// An operation with graph node references.
pub type GraphOperation<'a> = Operation<'a, NodeIndex<usize>>;

/// A path or query parameter with graph node references.
pub type GraphParameter<'a> = Parameter<'a, NodeIndex<usize>>;

/// The name, type, and metadata of an operation parameter,
/// with graph node references.
pub type GraphParameterInfo<'a> = ParameterInfo<'a, NodeIndex<usize>>;

/// A request body with graph node references.
pub type GraphRequest = Request<NodeIndex<usize>>;

/// A response body with graph node references.
pub type GraphResponse = Response<NodeIndex<usize>>;
