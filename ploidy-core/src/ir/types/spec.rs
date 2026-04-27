//! IR types in a [`Spec`][crate::ir::Spec], where type references are
//! [`&SpecType`][SpecType] pointers.

use crate::parse::ComponentRef;

use super::{
    Enum, InlineTypeId, PrimitiveType, SchemaTypeInfo, StructFieldName, UntaggedVariantNameHint,
    shape::{Operation, Parameter, ParameterInfo, Request, Response},
};

/// A type or reference in an OpenAPI spec.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpecType<'a> {
    /// A reference to another named schema type.
    Ref(&'a ComponentRef),
    /// A named schema type.
    Schema(SpecSchemaType<'a>),
    /// An inline type defined within a schema.
    Inline(SpecInlineType<'a>),
}

impl<'a> From<SpecSchemaType<'a>> for SpecType<'a> {
    fn from(ty: SpecSchemaType<'a>) -> Self {
        Self::Schema(ty)
    }
}

impl<'a> From<SpecInlineType<'a>> for SpecType<'a> {
    fn from(ty: SpecInlineType<'a>) -> Self {
        Self::Inline(ty)
    }
}

/// A named schema type with [`SpecType`] references.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpecSchemaType<'a> {
    /// An enum with named variants.
    Enum(SchemaTypeInfo<'a>, Enum<'a>),
    /// A struct with fields.
    Struct(SchemaTypeInfo<'a>, SpecStruct<'a>),
    /// A tagged union.
    Tagged(SchemaTypeInfo<'a>, SpecTagged<'a>),
    /// An untagged union.
    Untagged(SchemaTypeInfo<'a>, SpecUntagged<'a>),
    /// A named container.
    Container(SchemaTypeInfo<'a>, SpecContainer<'a>),
    /// A primitive type.
    Primitive(SchemaTypeInfo<'a>, PrimitiveType),
    /// Any JSON value.
    Any(SchemaTypeInfo<'a>),
}

impl<'a> SpecSchemaType<'a> {
    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(info, ..)
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
        | Self::Struct(info, ..)
        | Self::Tagged(info, ..)
        | Self::Untagged(info, ..)
        | Self::Container(info, ..)
        | Self::Primitive(info, ..)
        | Self::Any(info)) = self;
        info.resource
    }
}

/// An inline schema type with [`SpecType`] references.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpecInlineType<'a> {
    Enum(InlineTypeId, Enum<'a>),
    Struct(InlineTypeId, SpecStruct<'a>),
    Tagged(InlineTypeId, SpecTagged<'a>),
    Untagged(InlineTypeId, SpecUntagged<'a>),
    Container(InlineTypeId, SpecContainer<'a>),
    Primitive(InlineTypeId, PrimitiveType),
    Any(InlineTypeId),
}

impl<'a> SpecInlineType<'a> {
    /// Returns the opaque identity for this inline type node.
    #[inline]
    pub fn id(&self) -> InlineTypeId {
        let (Self::Enum(id, _)
        | Self::Struct(id, _)
        | Self::Tagged(id, _)
        | Self::Untagged(id, _)
        | Self::Container(id, _)
        | Self::Primitive(id, _)
        | Self::Any(id)) = *self;
        id
    }
}

/// A struct, created from a schema with named properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecStruct<'a> {
    pub description: Option<&'a str>,
    pub fields: &'a [SpecStructField<'a>],
    /// Immediate parent types from `allOf`, in declaration order.
    pub parents: &'a [&'a SpecType<'a>],
}

/// A field in a spec struct.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecStructField<'a> {
    pub name: StructFieldName<'a>,
    pub ty: &'a SpecType<'a>,
    pub required: bool,
    pub description: Option<&'a str>,
    pub flattened: bool,
}

/// A tagged union, created from a `oneOf` schema
/// with an explicit `discriminator`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecTagged<'a> {
    pub description: Option<&'a str>,
    pub tag: &'a str,
    pub variants: &'a [SpecTaggedVariant<'a>],
    /// Own fields that the union declares as `properties`.
    pub fields: &'a [SpecStructField<'a>],
}

/// A variant of a tagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecTaggedVariant<'a> {
    pub name: &'a str,
    pub aliases: &'a [&'a str],
    pub ty: &'a SpecType<'a>,
}

/// An untagged union, created from a `oneOf` schema
/// without a discriminator, or an OpenAPI 3.1 schema
/// with multiple types in its `type` field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecUntagged<'a> {
    pub description: Option<&'a str>,
    pub variants: &'a [SpecUntaggedVariant<'a>],
    /// Own fields that the union declares as `properties`.
    pub fields: &'a [SpecStructField<'a>],
}

/// A variant of an untagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpecUntaggedVariant<'a> {
    Some(UntaggedVariantNameHint, &'a SpecType<'a>),
    Null,
}

/// An array, map, or optional type with [`SpecType`] references.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpecContainer<'a> {
    /// An array of items.
    Array(SpecInner<'a>),
    /// A map with string keys.
    Map(SpecInner<'a>),
    /// A nullable value, or an optional struct field.
    Optional(SpecInner<'a>),
}

impl<'a> SpecContainer<'a> {
    /// Returns a reference to the inner type of this container.
    #[inline]
    pub fn inner(&self) -> &SpecInner<'a> {
        let (Self::Array(inner) | Self::Map(inner) | Self::Optional(inner)) = self;
        inner
    }
}

/// The inner type of a [`SpecContainer`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpecInner<'a> {
    pub description: Option<&'a str>,
    pub ty: &'a SpecType<'a>,
}

/// An operation with [`SpecType`] references.
pub type SpecOperation<'a> = Operation<'a, &'a SpecType<'a>>;

/// A path or query parameter with [`SpecType`] references.
pub type SpecParameter<'a> = Parameter<'a, &'a SpecType<'a>>;

/// The name, type, and metadata of an operation parameter,
/// with [`SpecType`] references.
pub type SpecParameterInfo<'a> = ParameterInfo<'a, &'a SpecType<'a>>;

/// A request body with [`SpecType`] references.
pub type SpecRequest<'a> = Request<&'a SpecType<'a>>;

/// A response body with [`SpecType`] references.
pub type SpecResponse<'a> = Response<&'a SpecType<'a>>;
