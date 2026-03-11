//! Language-agnostic intermediate representation types.

use serde_json::Number;

use crate::{
    arena::Arena,
    parse::{ComponentRef, Method, path::PathSegment},
};

/// A schema type ready for code generation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrType<'a> {
    /// A reference to another named schema type.
    Ref(&'a ComponentRef),
    /// A named schema type.
    Schema(SchemaIrType<'a>),
    /// An inline type defined within a schema.
    Inline(InlineIrType<'a>),
}

impl<'a> From<SchemaIrType<'a>> for IrType<'a> {
    fn from(ty: SchemaIrType<'a>) -> Self {
        Self::Schema(ty)
    }
}

impl<'a> From<InlineIrType<'a>> for IrType<'a> {
    fn from(ty: InlineIrType<'a>) -> Self {
        Self::Inline(ty)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrimitiveIrType {
    String,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bool,
    DateTime,
    UnixTime,
    Date,
    Url,
    Uuid,
    Bytes,
    Binary,
}

/// A container type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Container<'a, Ty = &'a IrType<'a>> {
    /// An array of items.
    Array(Inner<'a, Ty>),
    /// A map with string keys.
    Map(Inner<'a, Ty>),
    /// A nullable value, or an optional struct field.
    Optional(Inner<'a, Ty>),
}

impl<'a, Ty> Container<'a, Ty> {
    /// Returns a reference to the inner type of this container.
    #[inline]
    pub fn inner(&self) -> &Inner<'a, Ty> {
        let (Self::Array(inner) | Self::Map(inner) | Self::Optional(inner)) = self;
        inner
    }
}

/// The inner type of a [`Container`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Inner<'a, Ty = &'a IrType<'a>> {
    pub description: Option<&'a str>,
    pub ty: Ty,
}

/// Metadata for a named schema type.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SchemaTypeInfo<'a> {
    /// The name of the schema type.
    pub name: &'a str,
    /// The `x-resourceId` extension value, if present.
    pub resource: Option<&'a str>,
}

/// A named schema type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaIrType<'a, Ty = &'a IrType<'a>> {
    /// An enum with named variants.
    Enum(SchemaTypeInfo<'a>, IrEnum<'a>),
    /// A struct with fields.
    Struct(SchemaTypeInfo<'a>, IrStruct<'a, Ty>),
    /// A tagged union.
    Tagged(SchemaTypeInfo<'a>, IrTagged<'a, Ty>),
    /// An untagged union.
    Untagged(SchemaTypeInfo<'a>, IrUntagged<'a, Ty>),
    /// A named container.
    Container(SchemaTypeInfo<'a>, Container<'a, Ty>),
    /// A primitive type.
    Primitive(SchemaTypeInfo<'a>, PrimitiveIrType),
    /// Any JSON value.
    Any(SchemaTypeInfo<'a>),
}

impl<'a, Ty> SchemaIrType<'a, Ty> {
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

/// An inline schema type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineIrType<'a, Ty = &'a IrType<'a>> {
    Enum(InlineIrTypePath<'a>, IrEnum<'a>),
    Struct(InlineIrTypePath<'a>, IrStruct<'a, Ty>),
    Tagged(InlineIrTypePath<'a>, IrTagged<'a, Ty>),
    Untagged(InlineIrTypePath<'a>, IrUntagged<'a, Ty>),
    Container(InlineIrTypePath<'a>, Container<'a, Ty>),
    Primitive(InlineIrTypePath<'a>, PrimitiveIrType),
    Any(InlineIrTypePath<'a>),
}

impl<'a, Ty> InlineIrType<'a, Ty> {
    #[inline]
    pub fn path(&self) -> &InlineIrTypePath<'a> {
        let (Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)
        | Self::Container(path, _)
        | Self::Primitive(path, _)
        | Self::Any(path)) = self;
        path
    }
}

/// A path to an inline type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineIrTypePath<'a> {
    pub root: InlineIrTypePathRoot<'a>,
    pub segments: &'a [InlineIrTypePathSegment<'a>],
}

impl<'a> InlineIrTypePath<'a> {
    /// Returns a new path with the suffix appended to the segments.
    pub fn join(
        self,
        arena: &'a Arena,
        suffix: &[InlineIrTypePathSegment<'a>],
    ) -> InlineIrTypePath<'a> {
        match suffix {
            [] => self,
            suffix => InlineIrTypePath {
                root: self.root,
                segments: arena.alloc_slice(self.segments.iter().chain(suffix).copied()),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InlineIrTypePathRoot<'a> {
    Resource(Option<&'a str>),
    Type(&'a str),
}

/// A segment of an inline type path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InlineIrTypePathSegment<'a> {
    Operation(&'a str),
    Parameter(&'a str),
    Request,
    Response,
    Field(IrStructFieldName<'a>),
    MapValue,
    ArrayItem,
    Variant(usize),
    Parent(usize),
    TaggedVariant(&'a str),
}

/// An enum type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrEnum<'a> {
    pub description: Option<&'a str>,
    pub variants: &'a [IrEnumVariant<'a>],
}

/// A variant of an enum.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IrEnumVariant<'a> {
    String(&'a str),
    Number(Number),
    Bool(bool),
}

/// A struct, created from a schema with named properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrStruct<'a, Ty = &'a IrType<'a>> {
    pub description: Option<&'a str>,
    pub fields: &'a [IrStructField<'a, Ty>],
    /// Immediate parent types from `allOf`, in declaration order.
    pub parents: &'a [Ty],
}

/// A field in a struct.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrStructField<'a, Ty = &'a IrType<'a>> {
    pub name: IrStructFieldName<'a>,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    pub flattened: bool,
}

/// A tagged union, created from a `oneOf` schema
/// with an explicit `discriminator`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrTagged<'a, Ty = &'a IrType<'a>> {
    pub description: Option<&'a str>,
    pub tag: &'a str,
    pub variants: &'a [IrTaggedVariant<'a, Ty>],
}

/// A variant of a tagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrTaggedVariant<'a, Ty = &'a IrType<'a>> {
    pub name: &'a str,
    pub aliases: &'a [&'a str],
    pub ty: Ty,
}

/// An untagged union, created from a `oneOf` schema
/// without a discriminator, or an OpenAPI 3.1 schema
/// with multiple types in its `type` field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IrUntagged<'a, Ty = &'a IrType<'a>> {
    pub description: Option<&'a str>,
    pub variants: &'a [IrUntaggedVariant<Ty>],
}

/// A hint that's used to generate a more descriptive name
/// for an untagged union variant. These are emitted for
/// `oneOf` schemas without a discriminator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrUntaggedVariantNameHint {
    Primitive(PrimitiveIrType),
    Array,
    Map,
    Index(usize),
}

/// A struct field name, either explicit or generated from a hint.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IrStructFieldName<'a> {
    /// Explicit name from a schema or reference.
    Name(&'a str),
    /// Generated name, deferred until generation time.
    Hint(IrStructFieldNameHint),
}

/// A hint that's used to generate a name for a struct field.
/// These are emitted for inline `anyOf` schemas and
/// additional properties fields.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IrStructFieldNameHint {
    Index(usize),
    AdditionalProperties,
}

/// A variant of an untagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrUntaggedVariant<Ty> {
    Some(IrUntaggedVariantNameHint, Ty),
    Null,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrTypeName<'a> {
    Schema(SchemaTypeInfo<'a>),
    Inline(InlineIrTypePath<'a>),
}

impl<'a> From<&'a str> for IrTypeName<'a> {
    fn from(name: &'a str) -> Self {
        Self::Schema(SchemaTypeInfo {
            name,
            resource: None,
        })
    }
}

impl<'a> From<SchemaTypeInfo<'a>> for IrTypeName<'a> {
    fn from(info: SchemaTypeInfo<'a>) -> Self {
        Self::Schema(info)
    }
}

impl<'a> From<InlineIrTypePath<'a>> for IrTypeName<'a> {
    fn from(path: InlineIrTypePath<'a>) -> Self {
        Self::Inline(path)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IrOperation<'a, Ty = &'a IrType<'a>> {
    pub id: &'a str,
    pub method: Method,
    pub path: &'a [PathSegment<'a>],
    pub resource: Option<&'a str>,
    pub description: Option<&'a str>,
    pub params: &'a [IrParameter<'a, Ty>],
    pub request: Option<IrRequest<Ty>>,
    pub response: Option<IrResponse<Ty>>,
}

impl<'a, Ty> IrOperation<'a, Ty> {
    /// Returns an iterator over all the types that this operation
    /// references directly.
    pub fn types(&self) -> impl Iterator<Item = &Ty> {
        itertools::chain!(
            self.params.iter().map(|param| match param {
                IrParameter::Path(info) => &info.ty,
                IrParameter::Query(info) => &info.ty,
            }),
            self.request.as_ref().and_then(|request| match request {
                IrRequest::Json(ty) => Some(ty),
                IrRequest::Multipart => None,
            }),
            self.response.as_ref().map(|response| match response {
                IrResponse::Json(ty) => ty,
            })
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IrResponse<Ty> {
    Json(Ty),
}

#[derive(Clone, Copy, Debug)]
pub enum IrRequest<Ty> {
    Json(Ty),
    Multipart,
}

#[derive(Clone, Copy, Debug)]
pub enum IrParameter<'a, Ty = &'a IrType<'a>> {
    Path(IrParameterInfo<'a, Ty>),
    Query(IrParameterInfo<'a, Ty>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrParameterStyle {
    Form { exploded: bool },
    PipeDelimited,
    SpaceDelimited,
    DeepObject,
}

#[derive(Clone, Copy, Debug)]
pub struct IrParameterInfo<'a, Ty = &'a IrType<'a>> {
    pub name: &'a str,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    pub style: Option<IrParameterStyle>,
}
