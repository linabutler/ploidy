//! Language-agnostic intermediate representation types.

use serde_json::Number;

use crate::parse::{ComponentRef, Method, path::PathSegment};

/// A schema type ready for code generation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IrType<'a> {
    /// A reference to another named schema type.
    Ref(&'a ComponentRef),
    /// A named schema type.
    Schema(SchemaIrType<'a>),
    /// An inline type defined within a schema.
    Inline(InlineIrType<'a>),
}

impl IrType<'_> {
    pub fn as_ref(&self) -> IrTypeRef<'_> {
        match self {
            Self::Schema(ty) => IrTypeRef::Schema(ty),
            Self::Inline(ty) => IrTypeRef::Inline(ty),
            Self::Ref(r) => IrTypeRef::Ref(r),
        }
    }
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

/// A reference to a schema type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrTypeRef<'a> {
    Ref(&'a ComponentRef),
    Schema(&'a SchemaIrType<'a>),
    Inline(&'a InlineIrType<'a>),
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Container<'a> {
    /// An array of items.
    Array(Inner<'a>),
    /// A map with string keys.
    Map(Inner<'a>),
    /// A nullable value, or an optional struct field.
    Optional(Inner<'a>),
}

impl<'a> Container<'a> {
    /// Returns a reference to the inner type of this container.
    #[inline]
    pub fn inner(&self) -> &Inner<'a> {
        let (Self::Array(inner) | Self::Map(inner) | Self::Optional(inner)) = self;
        inner
    }
}

/// The inner type of a [`Container`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Inner<'a> {
    pub description: Option<&'a str>,
    pub ty: Box<IrType<'a>>,
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SchemaIrType<'a> {
    /// An enum with named variants.
    Enum(SchemaTypeInfo<'a>, IrEnum<'a>),
    /// A struct with fields.
    Struct(SchemaTypeInfo<'a>, IrStruct<'a>),
    /// A tagged union.
    Tagged(SchemaTypeInfo<'a>, IrTagged<'a>),
    /// An untagged union.
    Untagged(SchemaTypeInfo<'a>, IrUntagged<'a>),
    /// A named container.
    Container(SchemaTypeInfo<'a>, Container<'a>),
    /// A primitive type.
    Primitive(SchemaTypeInfo<'a>, PrimitiveIrType),
    /// Any JSON value.
    Any(SchemaTypeInfo<'a>),
}

impl<'a> SchemaIrType<'a> {
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum InlineIrType<'a> {
    Enum(InlineIrTypePath<'a>, IrEnum<'a>),
    Struct(InlineIrTypePath<'a>, IrStruct<'a>),
    Tagged(InlineIrTypePath<'a>, IrTagged<'a>),
    Untagged(InlineIrTypePath<'a>, IrUntagged<'a>),
    Container(InlineIrTypePath<'a>, Container<'a>),
    Primitive(InlineIrTypePath<'a>, PrimitiveIrType),
    Any(InlineIrTypePath<'a>),
}

impl<'a> InlineIrType<'a> {
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
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineIrTypePath<'a> {
    pub root: InlineIrTypePathRoot<'a>,
    pub segments: Vec<InlineIrTypePathSegment<'a>>,
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
}

/// An enum type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrEnum<'a> {
    pub description: Option<&'a str>,
    pub variants: Vec<IrEnumVariant<'a>>,
}

/// A variant of an enum.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IrEnumVariant<'a> {
    String(&'a str),
    Number(Number),
    Bool(bool),
}

/// A struct, created from a schema with named properties.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrStruct<'a> {
    pub description: Option<&'a str>,
    pub fields: Vec<IrStructField<'a>>,
    /// Immediate parent types from `allOf`, in declaration order.
    pub parents: Vec<IrType<'a>>,
    /// The discriminator property name, if this struct defines one.
    pub discriminator: Option<&'a str>,
}

/// A field in a struct.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrStructField<'a> {
    pub name: IrStructFieldName<'a>,
    pub ty: IrType<'a>,
    pub required: bool,
    pub description: Option<&'a str>,
    pub flattened: bool,
}

/// A tagged union, created from a `oneOf` schema
/// with an explicit `discriminator`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrTagged<'a> {
    pub description: Option<&'a str>,
    pub tag: &'a str,
    pub variants: Vec<IrTaggedVariant<'a>>,
}

/// A variant of a tagged union.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrTaggedVariant<'a> {
    pub name: &'a str,
    pub aliases: Vec<&'a str>,
    pub ty: IrType<'a>,
}

/// An untagged union, created from a `oneOf` schema
/// without a discriminator, or an OpenAPI 3.1 schema
/// with multiple types in its `type` field.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct IrUntagged<'a> {
    pub description: Option<&'a str>,
    pub variants: Vec<IrUntaggedVariant<'a>>,
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
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum IrUntaggedVariant<'a> {
    Some(IrUntaggedVariantNameHint, IrType<'a>),
    Null,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct IrOperation<'a> {
    pub id: &'a str,
    pub method: Method,
    pub path: Vec<PathSegment<'a>>,
    pub resource: Option<&'a str>,
    pub description: Option<&'a str>,
    pub params: Vec<IrParameter<'a>>,
    pub request: Option<IrRequest<'a>>,
    pub response: Option<IrResponse<'a>>,
}

impl<'a> IrOperation<'a> {
    /// Returns an iterator over all the types that this operation
    /// references directly.
    pub fn types(&self) -> impl Iterator<Item = &IrType<'a>> {
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

#[derive(Clone, Debug)]
pub enum IrResponse<'a> {
    Json(IrType<'a>),
}

#[derive(Clone, Debug)]
pub enum IrRequest<'a> {
    Json(IrType<'a>),
    Multipart,
}

#[derive(Clone, Debug)]
pub enum IrParameter<'a> {
    Path(IrParameterInfo<'a>),
    Query(IrParameterInfo<'a>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IrParameterStyle {
    Form { exploded: bool },
    PipeDelimited,
    SpaceDelimited,
    DeepObject,
}

#[derive(Clone, Debug)]
pub struct IrParameterInfo<'a> {
    pub name: &'a str,
    pub ty: IrType<'a>,
    pub required: bool,
    pub description: Option<&'a str>,
    pub style: Option<IrParameterStyle>,
}
