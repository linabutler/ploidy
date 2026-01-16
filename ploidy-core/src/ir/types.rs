//! Language-agnostic intermediate representation types.

use serde_json::Number;

use crate::parse::{ComponentRef, Method, path::PathSegment};

/// A schema type ready for code generation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IrType<'a> {
    /// A primitive type.
    Primitive(PrimitiveIrType),
    /// An array of items.
    Array(Box<IrType<'a>>),
    /// A map with string keys.
    Map(Box<IrType<'a>>),
    /// A nullable value, or an optional struct field.
    Optional(Box<IrType<'a>>),
    /// A reference to another named schema type.
    Ref(&'a ComponentRef),
    /// A named schema type.
    Schema(SchemaIrType<'a>),
    /// An inline type defined within a schema.
    Inline(InlineIrType<'a>),
    /// Any JSON value.
    Any,
}

impl IrType<'_> {
    pub fn as_ref(&self) -> IrTypeRef<'_> {
        match self {
            Self::Schema(ty) => IrTypeRef::Schema(ty),
            Self::Inline(ty) => IrTypeRef::Inline(ty),
            Self::Array(ty) => IrTypeRef::Array(ty),
            Self::Map(ty) => IrTypeRef::Map(ty),
            Self::Optional(ty) => IrTypeRef::Optional(ty),
            Self::Ref(r) => IrTypeRef::Ref(r),
            &Self::Primitive(ty) => IrTypeRef::Primitive(ty),
            Self::Any => IrTypeRef::Any,
        }
    }
}

impl From<PrimitiveIrType> for IrType<'_> {
    fn from(ty: PrimitiveIrType) -> Self {
        Self::Primitive(ty)
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
    Primitive(PrimitiveIrType),
    Array(&'a IrType<'a>),
    Map(&'a IrType<'a>),
    Optional(&'a IrType<'a>),
    Ref(&'a ComponentRef),
    Schema(&'a SchemaIrType<'a>),
    Inline(&'a InlineIrType<'a>),
    Any,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrimitiveIrType {
    String,
    I32,
    I64,
    F32,
    F64,
    Bool,
    DateTime,
    UnixTime,
    Date,
    Url,
    Uuid,
    Bytes,
}

/// A named schema type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SchemaIrType<'a> {
    /// An enum with named variants.
    Enum(&'a str, IrEnum<'a>),
    /// A struct with fields.
    Struct(&'a str, IrStruct<'a>),
    /// A tagged union.
    Tagged(&'a str, IrTagged<'a>),
    /// An untagged union.
    Untagged(&'a str, IrUntagged<'a>),
}

impl<'a> SchemaIrType<'a> {
    #[inline]
    pub fn name(&self) -> &'a str {
        let (Self::Enum(name, _)
        | Self::Struct(name, _)
        | Self::Tagged(name, _)
        | Self::Untagged(name, _)) = self;
        name
    }
}

/// An inline schema type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum InlineIrType<'a> {
    Enum(InlineIrTypePath<'a>, IrEnum<'a>),
    Struct(InlineIrTypePath<'a>, IrStruct<'a>),
    Tagged(InlineIrTypePath<'a>, IrTagged<'a>),
    Untagged(InlineIrTypePath<'a>, IrUntagged<'a>),
}

impl<'a> InlineIrType<'a> {
    #[inline]
    pub fn path(&self) -> &InlineIrTypePath<'a> {
        let (Self::Enum(path, _)
        | Self::Struct(path, _)
        | Self::Tagged(path, _)
        | Self::Untagged(path, _)) = self;
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
    Resource(&'a str),
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
}

/// A field in a struct.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IrStructField<'a> {
    pub name: IrStructFieldName<'a>,
    pub ty: IrType<'a>,
    pub required: bool,
    pub description: Option<&'a str>,
    pub inherited: bool,
    pub discriminator: bool,
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
/// These are emitted for inline `anyOf` schemas.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IrStructFieldNameHint {
    Index(usize),
}

/// A variant of an untagged union.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum IrUntaggedVariant<'a> {
    Some(IrUntaggedVariantNameHint, IrType<'a>),
    Null,
}

impl From<PrimitiveIrType> for IrUntaggedVariant<'_> {
    fn from(ty: PrimitiveIrType) -> Self {
        Self::Some(IrUntaggedVariantNameHint::Primitive(ty), ty.into())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IrTypeName<'a> {
    Schema(&'a str),
    Inline(InlineIrTypePath<'a>),
}

impl<'a> From<InlineIrTypePath<'a>> for IrTypeName<'a> {
    fn from(path: InlineIrTypePath<'a>) -> Self {
        Self::Inline(path)
    }
}

#[derive(Clone, Debug)]
pub struct IrOperation<'a> {
    pub resource: &'a str,
    pub id: &'a str,
    pub method: Method,
    pub path: Vec<PathSegment<'a>>,
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
