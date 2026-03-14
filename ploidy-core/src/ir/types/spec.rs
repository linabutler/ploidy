//! IR types in a [`Spec`][crate::ir::Spec], where type references are
//! [`&SpecType`][SpecType] pointers.

use crate::parse::ComponentRef;

use super::shape::{
    Container, InlineType, Inner, Operation, Parameter, ParameterInfo, Request, Response,
    SchemaType, Struct, StructField, Tagged, TaggedVariant, Untagged, UntaggedVariant,
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
pub type SpecSchemaType<'a> = SchemaType<'a, &'a SpecType<'a>>;

/// An array, map, or optional type with [`SpecType`] references.
pub type SpecContainer<'a> = Container<'a, &'a SpecType<'a>>;

/// A struct type with [`SpecType`] references.
pub type SpecStruct<'a> = Struct<'a, &'a SpecType<'a>>;

/// A struct field with [`SpecType`] references.
pub type SpecStructField<'a> = StructField<'a, &'a SpecType<'a>>;

/// A tagged union with [`SpecType`] references.
pub type SpecTagged<'a> = Tagged<'a, &'a SpecType<'a>>;

/// A variant of a tagged union with [`SpecType`] references.
pub type SpecTaggedVariant<'a> = TaggedVariant<'a, &'a SpecType<'a>>;

/// An untagged union with [`SpecType`] references.
pub type SpecUntagged<'a> = Untagged<'a, &'a SpecType<'a>>;

/// A variant of an untagged union with [`SpecType`] references.
pub type SpecUntaggedVariant<'a> = UntaggedVariant<&'a SpecType<'a>>;

/// An inline type with [`SpecType`] references.
pub type SpecInlineType<'a> = InlineType<'a, &'a SpecType<'a>>;

/// The type contained within an array, map, or optional type,
/// with [`SpecType`] references.
pub type SpecInner<'a> = Inner<'a, &'a SpecType<'a>>;

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
