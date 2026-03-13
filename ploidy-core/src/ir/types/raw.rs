//! IR types before cooking, where type references are
//! [`&RawType`][RawType] pointers.

use crate::parse::ComponentRef;

use super::shape::{
    Container, InlineType, Inner, Operation, Parameter, ParameterInfo, Request, Response,
    SchemaType, Struct, StructField, Tagged, TaggedVariant, Untagged, UntaggedVariant,
};

/// A schema type or reference.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RawType<'a> {
    /// A reference to another named schema type.
    Ref(&'a ComponentRef),
    /// A named schema type.
    Schema(RawSchemaType<'a>),
    /// An inline type defined within a schema.
    Inline(RawInlineType<'a>),
}

impl<'a> From<RawSchemaType<'a>> for RawType<'a> {
    fn from(ty: RawSchemaType<'a>) -> Self {
        Self::Schema(ty)
    }
}

impl<'a> From<RawInlineType<'a>> for RawType<'a> {
    fn from(ty: RawInlineType<'a>) -> Self {
        Self::Inline(ty)
    }
}

/// A named schema type with [`RawType`] references.
pub type RawSchemaType<'a> = SchemaType<'a, &'a RawType<'a>>;

/// An array, map, or optional type with [`RawType`] references.
pub type RawContainer<'a> = Container<'a, &'a RawType<'a>>;

/// A struct type with [`RawType`] references.
pub type RawStruct<'a> = Struct<'a, &'a RawType<'a>>;

/// A struct field with [`RawType`] references.
pub type RawStructField<'a> = StructField<'a, &'a RawType<'a>>;

/// A tagged union with [`RawType`] references.
pub type RawTagged<'a> = Tagged<'a, &'a RawType<'a>>;

/// A variant of a tagged union with [`RawType`] references.
pub type RawTaggedVariant<'a> = TaggedVariant<'a, &'a RawType<'a>>;

/// An untagged union with [`RawType`] references.
pub type RawUntagged<'a> = Untagged<'a, &'a RawType<'a>>;

/// A variant of an untagged union with [`RawType`] references.
pub type RawUntaggedVariant<'a> = UntaggedVariant<&'a RawType<'a>>;

/// An inline type with [`RawType`] references.
pub type RawInlineType<'a> = InlineType<'a, &'a RawType<'a>>;

/// The inner type of a [`Container`] with [`RawType`] references.
pub type RawInner<'a> = Inner<'a, &'a RawType<'a>>;

/// An operation with [`RawType`] references.
pub type RawOperation<'a> = Operation<'a, &'a RawType<'a>>;

/// An operation parameter with [`RawType`] references.
pub type RawParameter<'a> = Parameter<'a, &'a RawType<'a>>;

/// Information about a [`Parameter`] with [`RawType`] references.
pub type RawParameterInfo<'a> = ParameterInfo<'a, &'a RawType<'a>>;

/// An operation request body with [`RawType`] references.
pub type RawRequest<'a> = Request<&'a RawType<'a>>;

/// An operation response with [`RawType`] references.
pub type RawResponse<'a> = Response<&'a RawType<'a>>;
