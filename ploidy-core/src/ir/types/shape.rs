//! Generic structural shapes for IR types, parameterized over
//! the type reference representation.
//!
//! Prefer the [raw and cooked type aliases][super], unless
//! you're writing generic code that abstracts over type references.

use crate::parse::{Method, path::PathSegment};

use super::{
    Enum, InlineTypePath, ParameterStyle, PrimitiveType, SchemaTypeInfo, StructFieldName,
    UntaggedVariantNameHint,
};

/// A container type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Container<'a, Ty> {
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
pub struct Inner<'a, Ty> {
    pub description: Option<&'a str>,
    pub ty: Ty,
}

/// A named schema type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SchemaType<'a, Ty> {
    /// An enum with named variants.
    Enum(SchemaTypeInfo<'a>, Enum<'a>),
    /// A struct with fields.
    Struct(SchemaTypeInfo<'a>, Struct<'a, Ty>),
    /// A tagged union.
    Tagged(SchemaTypeInfo<'a>, Tagged<'a, Ty>),
    /// An untagged union.
    Untagged(SchemaTypeInfo<'a>, Untagged<'a, Ty>),
    /// A named container.
    Container(SchemaTypeInfo<'a>, Container<'a, Ty>),
    /// A primitive type.
    Primitive(SchemaTypeInfo<'a>, PrimitiveType),
    /// Any JSON value.
    Any(SchemaTypeInfo<'a>),
}

impl<'a, Ty> SchemaType<'a, Ty> {
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
pub enum InlineType<'a, Ty> {
    Enum(InlineTypePath<'a>, Enum<'a>),
    Struct(InlineTypePath<'a>, Struct<'a, Ty>),
    Tagged(InlineTypePath<'a>, Tagged<'a, Ty>),
    Untagged(InlineTypePath<'a>, Untagged<'a, Ty>),
    Container(InlineTypePath<'a>, Container<'a, Ty>),
    Primitive(InlineTypePath<'a>, PrimitiveType),
    Any(InlineTypePath<'a>),
}

impl<'a, Ty> InlineType<'a, Ty> {
    #[inline]
    pub fn path(&self) -> &InlineTypePath<'a> {
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

/// A struct, created from a schema with named properties.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Struct<'a, Ty> {
    pub description: Option<&'a str>,
    pub fields: &'a [StructField<'a, Ty>],
    /// Immediate parent types from `allOf`, in declaration order.
    pub parents: &'a [Ty],
}

/// A field in a struct.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StructField<'a, Ty> {
    pub name: StructFieldName<'a>,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    pub flattened: bool,
}

/// A tagged union, created from a `oneOf` schema
/// with an explicit `discriminator`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Tagged<'a, Ty> {
    pub description: Option<&'a str>,
    pub tag: &'a str,
    pub variants: &'a [TaggedVariant<'a, Ty>],
}

/// A variant of a tagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaggedVariant<'a, Ty> {
    pub name: &'a str,
    pub aliases: &'a [&'a str],
    pub ty: Ty,
}

/// An untagged union, created from a `oneOf` schema
/// without a discriminator, or an OpenAPI 3.1 schema
/// with multiple types in its `type` field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Untagged<'a, Ty> {
    pub description: Option<&'a str>,
    pub variants: &'a [UntaggedVariant<Ty>],
}

/// A variant of an untagged union.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntaggedVariant<Ty> {
    Some(UntaggedVariantNameHint, Ty),
    Null,
}

#[derive(Clone, Copy, Debug)]
pub struct Operation<'a, Ty> {
    pub id: &'a str,
    pub method: Method,
    pub path: &'a [PathSegment<'a>],
    pub resource: Option<&'a str>,
    pub description: Option<&'a str>,
    pub params: &'a [Parameter<'a, Ty>],
    pub request: Option<Request<Ty>>,
    pub response: Option<Response<Ty>>,
}

impl<'a, Ty> Operation<'a, Ty> {
    /// Returns an iterator over all the types that this operation
    /// references directly.
    pub fn types(&self) -> impl Iterator<Item = &Ty> {
        itertools::chain!(
            self.params.iter().map(|param| match param {
                Parameter::Path(info) => &info.ty,
                Parameter::Query(info) => &info.ty,
            }),
            self.request.as_ref().and_then(|request| match request {
                Request::Json(ty) => Some(ty),
                Request::Multipart => None,
            }),
            self.response.as_ref().map(|response| match response {
                Response::Json(ty) => ty,
            })
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Response<Ty> {
    Json(Ty),
}

#[derive(Clone, Copy, Debug)]
pub enum Request<Ty> {
    Json(Ty),
    Multipart,
}

#[derive(Clone, Copy, Debug)]
pub enum Parameter<'a, Ty> {
    Path(ParameterInfo<'a, Ty>),
    Query(ParameterInfo<'a, Ty>),
}

#[derive(Clone, Copy, Debug)]
pub struct ParameterInfo<'a, Ty> {
    pub name: &'a str,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    pub style: Option<ParameterStyle>,
}
