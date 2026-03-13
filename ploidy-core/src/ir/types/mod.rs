//! Language-agnostic intermediate representation types.

use serde_json::Number;

use crate::arena::Arena;

pub use self::{cooked::*, raw::*};

mod cooked;
mod raw;
pub mod shape;

/// Metadata about a type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeInfo<'a> {
    Schema(SchemaTypeInfo<'a>),
    Inline(InlineTypePath<'a>),
}

impl<'a> From<&'a str> for TypeInfo<'a> {
    fn from(name: &'a str) -> Self {
        Self::Schema(SchemaTypeInfo {
            name,
            resource: None,
        })
    }
}

impl<'a> From<SchemaTypeInfo<'a>> for TypeInfo<'a> {
    fn from(info: SchemaTypeInfo<'a>) -> Self {
        Self::Schema(info)
    }
}

impl<'a> From<InlineTypePath<'a>> for TypeInfo<'a> {
    fn from(path: InlineTypePath<'a>) -> Self {
        Self::Inline(path)
    }
}

/// Metadata for a named schema type.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SchemaTypeInfo<'a> {
    /// The name of the schema type.
    pub name: &'a str,
    /// The `x-resourceId` extension value, if present.
    pub resource: Option<&'a str>,
}

/// A path to an inline type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineTypePath<'a> {
    pub root: InlineTypePathRoot<'a>,
    pub segments: &'a [InlineTypePathSegment<'a>],
}

impl<'a> InlineTypePath<'a> {
    /// Returns a new path with the suffix appended to the segments.
    pub fn join(
        self,
        arena: &'a Arena,
        suffix: &[InlineTypePathSegment<'a>],
    ) -> InlineTypePath<'a> {
        match suffix {
            [] => self,
            suffix => InlineTypePath {
                root: self.root,
                segments: arena.alloc_slice(self.segments.iter().chain(suffix).copied()),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InlineTypePathRoot<'a> {
    Resource(Option<&'a str>),
    Type(&'a str),
}

/// A segment of an inline type path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InlineTypePathSegment<'a> {
    Operation(&'a str),
    Parameter(&'a str),
    Request,
    Response,
    Field(StructFieldName<'a>),
    MapValue,
    ArrayItem,
    Variant(usize),
    Parent(usize),
    TaggedVariant(&'a str),
}

/// A primitive type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrimitiveType {
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

/// An enum type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Enum<'a> {
    pub description: Option<&'a str>,
    pub variants: &'a [EnumVariant<'a>],
}

/// A variant of an enum.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum EnumVariant<'a> {
    String(&'a str),
    Number(Number),
    Bool(bool),
}

/// A hint that's used to generate a more descriptive name
/// for an untagged union variant. These are emitted for
/// `oneOf` schemas without a discriminator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntaggedVariantNameHint {
    Primitive(PrimitiveType),
    Array,
    Map,
    Index(usize),
}

/// A struct field name, either explicit or generated from a hint.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructFieldName<'a> {
    /// Explicit name from a schema or reference.
    Name(&'a str),
    /// Generated name, deferred until generation time.
    Hint(StructFieldNameHint),
}

/// A hint that's used to generate a name for a struct field.
/// These are emitted for inline `anyOf` schemas and
/// additional properties fields.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StructFieldNameHint {
    Index(usize),
    AdditionalProperties,
}

/// The serialization style for query parameters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ParameterStyle {
    Form { exploded: bool },
    PipeDelimited,
    SpaceDelimited,
    DeepObject,
}
