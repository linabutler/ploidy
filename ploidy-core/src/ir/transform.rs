use std::cell::Cell;

use itertools::Itertools;
use rustc_hash::FxHashMap;

use crate::{
    arena::Arena,
    ir::{JsonF64, SchemaTypeInfo},
    parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty},
};

use super::types::{
    Enum, EnumVariant, InlineTypeId, PrimitiveType, SpecContainer, SpecInlineType, SpecInner,
    SpecSchemaType, SpecStruct, SpecStructField, SpecTagged, SpecTaggedVariant, SpecType,
    SpecUntagged, SpecUntaggedVariant, StructFieldName, StructFieldNameHint,
    UntaggedVariantNameHint,
};

/// Metadata about a type in the dependency graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TypeInfo<'a> {
    Schema(SchemaTypeInfo<'a>),
    Inline(InlineTypeId),
}

impl<'a> From<SchemaTypeInfo<'a>> for TypeInfo<'a> {
    fn from(info: SchemaTypeInfo<'a>) -> Self {
        Self::Schema(info)
    }
}

impl From<InlineTypeId> for TypeInfo<'_> {
    fn from(id: InlineTypeId) -> Self {
        Self::Inline(id)
    }
}

/// Context for the [`IrTransformer`].
#[derive(Debug)]
pub struct TransformContext<'a> {
    pub arena: &'a Arena,
    /// The document being transformed.
    pub doc: &'a Document,
    /// Counter for allocating fresh [`InlineTypeId`]s.
    next_inline_id: Cell<usize>,
}

impl<'a> TransformContext<'a> {
    /// Creates a new context for the given document.
    pub fn new(arena: &'a Arena, doc: &'a Document) -> Self {
        Self {
            arena,
            doc,
            next_inline_id: Cell::new(0),
        }
    }

    /// Allocates a fresh [`InlineTypeId`].
    #[inline]
    pub(super) fn next_inline_id(&self) -> InlineTypeId {
        let id = self.next_inline_id.get();
        self.next_inline_id.set(id + 1);
        InlineTypeId::new(id)
    }

    /// Returns the number of inline IDs allocated so far.
    #[inline]
    pub(super) fn inline_id_count(&self) -> usize {
        self.next_inline_id.get()
    }
}

pub(super) fn transform_with_context<'context, 'a>(
    context: &'context TransformContext<'a>,
    name: impl Into<TypeInfo<'a>>,
    schema: &'a Schema,
) -> SpecType<'a> {
    IrTransformer::new(context, name.into(), schema).transform()
}

#[derive(Debug)]
struct IrTransformer<'context, 'a> {
    context: &'context TransformContext<'a>,
    name: TypeInfo<'a>,
    schema: &'a Schema,
}

impl<'context, 'a> IrTransformer<'context, 'a> {
    fn new(
        context: &'context TransformContext<'a>,
        name: TypeInfo<'a>,
        schema: &'a Schema,
    ) -> Self {
        Self {
            context,
            name,
            schema,
        }
    }

    /// Returns a reference to the arena allocator.
    #[inline]
    fn arena(&self) -> &'a Arena {
        self.context.arena
    }

    /// Allocates a fresh [`InlineTypeId`].
    #[inline]
    fn next_inline_id(&self) -> InlineTypeId {
        self.context.next_inline_id()
    }

    fn transform(self) -> SpecType<'a> {
        self.try_tagged()
            .or_else(Self::try_untagged)
            .or_else(Self::try_any_of)
            .or_else(Self::try_enum)
            .or_else(Self::try_struct)
            .unwrap_or_else(Self::other)
    }

    fn try_tagged(self) -> Result<SpecType<'a>, Self> {
        let (Some(one_of), Some(discriminator)) = (&self.schema.one_of, &self.schema.discriminator)
        else {
            return Err(self);
        };

        let variants = {
            let mut inverted = FxHashMap::<_, Vec<_>>::default();
            for (tag, r) in &discriminator.mapping {
                inverted.entry(r).or_default().push(tag.as_str());
            }
            let mut variants = Vec::with_capacity(one_of.len());
            for schema in one_of {
                match schema {
                    RefOrSchema::Ref(r) => {
                        let name: &_ = self.arena().alloc_str(&r.name());
                        let aliases = match inverted.get(r).map(|s| s.as_slice()).unwrap_or(&[]) {
                            // When a discriminator value doesn't have
                            // an explicit `mapping`, use the schema name.
                            [] => &[name],
                            aliases => aliases,
                        };
                        variants.push(SpecTaggedVariant {
                            name,
                            ty: self.arena().alloc(SpecType::Ref(r)),
                            aliases: self.arena().alloc_slice_copy(aliases),
                        });
                    }
                    // An inline schema variant can't have a discriminator mapping;
                    // fall through to `try_untagged`.
                    RefOrSchema::Inline(_) => return Err(self),
                }
            }
            variants
        };

        let tagged = SpecTagged {
            description: self.schema.description.as_deref(),
            tag: discriminator.property_name.as_str(),
            variants: self.arena().alloc_slice_copy(&variants),
            fields: self.arena().alloc_slice(self.properties()),
        };

        Ok(match self.name {
            TypeInfo::Schema(info) => SpecSchemaType::Tagged(info, tagged).into(),
            TypeInfo::Inline(id) => SpecInlineType::Tagged(id, tagged).into(),
        })
    }

    fn try_untagged(self) -> Result<SpecType<'a>, Self> {
        let Some(one_of) = &self.schema.one_of else {
            return Err(self);
        };

        let variants = match &**one_of {
            [] => {
                return Ok(match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Any(id).into(),
                });
            }
            [schema] => {
                // Unwrap single-variant untagged unions.
                return Ok(match schema {
                    RefOrSchema::Ref(r) => SpecType::Ref(r),
                    RefOrSchema::Inline(s) if matches!(&*s.ty, [Ty::Null]) => match self.name {
                        TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                        TypeInfo::Inline(id) => SpecInlineType::Any(id).into(),
                    },
                    RefOrSchema::Inline(schema) => {
                        transform_with_context(self.context, self.name, schema)
                    }
                });
            }
            variants => variants
                .iter()
                .enumerate()
                .map(|(index, schema)| (index + 1, schema))
                .map(|(index, schema)| {
                    let ty = match schema {
                        RefOrSchema::Ref(r) => Some(SpecType::Ref(r)),
                        RefOrSchema::Inline(s) if matches!(&*s.ty, [Ty::Null]) => None,
                        RefOrSchema::Inline(schema) => {
                            let id = self.next_inline_id();
                            Some(transform_with_context(self.context, id, schema))
                        }
                    };
                    ty.map(|ty| {
                        let hint = match ty {
                            SpecType::Schema(SpecSchemaType::Primitive(_, p))
                            | SpecType::Inline(SpecInlineType::Primitive(_, p)) => {
                                UntaggedVariantNameHint::Primitive(p)
                            }
                            SpecType::Schema(SpecSchemaType::Container(
                                _,
                                SpecContainer::Array(_),
                            ))
                            | SpecType::Inline(SpecInlineType::Container(
                                _,
                                SpecContainer::Array(_),
                            )) => UntaggedVariantNameHint::Array,
                            SpecType::Schema(SpecSchemaType::Container(
                                _,
                                SpecContainer::Map(_),
                            ))
                            | SpecType::Inline(SpecInlineType::Container(
                                _,
                                SpecContainer::Map(_),
                            )) => UntaggedVariantNameHint::Map,
                            _ => UntaggedVariantNameHint::Index(index),
                        };
                        SpecUntaggedVariant::Some(hint, self.arena().alloc(ty))
                    })
                    .unwrap_or(SpecUntaggedVariant::Null)
                })
                .collect_vec(),
        };

        Ok(match &*variants {
            // Simplify two-variant untagged unions, where one is a type
            // and the other is `null`, into optionals.
            [SpecUntaggedVariant::Some(_, ty), SpecUntaggedVariant::Null]
            | [SpecUntaggedVariant::Null, SpecUntaggedVariant::Some(_, ty)] => {
                let container = SpecContainer::Optional(SpecInner {
                    description: self.schema.description.as_deref(),
                    ty,
                });
                match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Container(id, container).into(),
                }
            }

            variants => {
                let untagged = SpecUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(variants),
                    fields: self.arena().alloc_slice(self.properties()),
                };
                match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Untagged(info, untagged).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Untagged(id, untagged).into(),
                }
            }
        })
    }

    fn try_any_of(self) -> Result<SpecType<'a>, Self> {
        let Some(any_of) = &self.schema.any_of else {
            return Err(self);
        };
        if let [schema] = &**any_of {
            // A single-variant `anyOf` should unwrap to the variant type. This
            // preserves type references that would otherwise become `Any`.
            return Ok(match schema {
                RefOrSchema::Ref(r) => SpecType::Ref(r),
                RefOrSchema::Inline(schema) => {
                    transform_with_context(self.context, self.name, schema)
                }
            });
        }

        let any_of_fields = any_of
            .iter()
            .enumerate()
            .map(|(index, schema)| {
                let (field_name, ty, description) = match schema {
                    RefOrSchema::Ref(r) => {
                        let name = StructFieldName::Name(self.arena().alloc_str(&r.name()));
                        let ty: &_ = self.arena().alloc(SpecType::Ref(r));
                        let desc = r
                            .pointer()
                            .follow::<&Schema>(self.context.doc)
                            .ok()
                            .and_then(|s| s.description.as_deref());
                        (name, ty, desc)
                    }
                    RefOrSchema::Inline(schema) => {
                        let name = StructFieldName::Hint(StructFieldNameHint::Index(index + 1));
                        let id = self.next_inline_id();
                        let ty: &_ =
                            self.arena()
                                .alloc(transform_with_context(self.context, id, schema));
                        let desc = schema.description.as_deref();
                        (name, ty, desc)
                    }
                };
                // Flattened `anyOf` fields are always optional.
                let id = self.next_inline_id();
                let ty: &_ = self.arena().alloc(
                    SpecInlineType::Container(
                        id,
                        SpecContainer::Optional(SpecInner { description, ty }),
                    )
                    .into(),
                );
                SpecStructField {
                    name: field_name,
                    ty,
                    required: false,
                    description,
                    flattened: true,
                }
            })
            .collect_vec();

        let ty = SpecStruct {
            description: self.schema.description.as_deref(),
            fields: self.arena().alloc_slice({
                // Combine all the fields: regular properties first,
                // followed by the flattened `anyOf` fields. This ordering
                // ensures that regular properties take precedence during
                // (de)serialization.
                itertools::chain!(self.properties(), any_of_fields)
            }),
            parents: self.arena().alloc_slice(self.parents()),
        };

        Ok(match self.name {
            TypeInfo::Schema(info) => SpecSchemaType::Struct(info, ty).into(),
            TypeInfo::Inline(id) => SpecInlineType::Struct(id, ty).into(),
        })
    }

    fn try_enum(self) -> Result<SpecType<'a>, Self> {
        let Some(values) = &self.schema.variants else {
            return Err(self);
        };
        // JSON Schema Validation (draft-bhutton-json-schema-validation-01)
        // recommends unique enum values, but specs in the wild repeat values.
        let variants = self.arena().alloc_slice(
            values
                .iter()
                .filter_map(|value| {
                    if let Some(s) = value.as_str() {
                        Some(EnumVariant::String(s))
                    } else if let Some(n) = value.as_number() {
                        if let Some(n) = n.as_i64() {
                            Some(EnumVariant::I64(n))
                        } else if let Some(n) = n.as_u64() {
                            Some(EnumVariant::U64(n))
                        } else {
                            n.as_f64().map(|f| EnumVariant::F64(JsonF64::new(f)))
                        }
                    } else {
                        value.as_bool().map(EnumVariant::Bool)
                    }
                })
                .unique(),
        );
        let ty = Enum {
            description: self.schema.description.as_deref(),
            variants,
        };
        Ok(match self.name {
            TypeInfo::Schema(info) => SpecSchemaType::Enum(info, ty).into(),
            TypeInfo::Inline(id) => SpecInlineType::Enum(id, ty).into(),
        })
    }

    fn try_struct(self) -> Result<SpecType<'a>, Self> {
        if self.schema.properties.is_none() && self.schema.all_of.is_none() {
            return Err(self);
        }

        let ty = SpecStruct {
            description: self.schema.description.as_deref(),
            fields: self.arena().alloc_slice(itertools::chain!(
                self.properties(),
                self.additional_properties()
            )),
            parents: self.arena().alloc_slice(self.parents()),
        };
        Ok(match self.name {
            TypeInfo::Schema(info) => SpecSchemaType::Struct(info, ty).into(),
            TypeInfo::Inline(id) => SpecInlineType::Struct(id, ty).into(),
        })
    }

    fn other(self) -> SpecType<'a> {
        let mut other = Other {
            variants: Vec::with_capacity(self.schema.ty.len()),
            nullable: false,
        };

        for ty in &self.schema.ty {
            let variant = match (ty, self.schema.format) {
                (Ty::String, Some(Format::DateTime)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::DateTime)
                }
                (Ty::String, Some(Format::Date)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::Date)
                }
                (Ty::String, Some(Format::Uri)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::Url)
                }
                (Ty::String, Some(Format::Uuid)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::Uuid)
                }
                (Ty::String, Some(Format::Byte)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::Bytes)
                }
                (Ty::String, Some(Format::Binary)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::Binary)
                }
                (Ty::String, _) => OtherVariant::Primitive(self.name, PrimitiveType::String),

                (Ty::Integer, Some(Format::Int8)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::I8)
                }
                (Ty::Integer, Some(Format::UInt8)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::U8)
                }
                (Ty::Integer, Some(Format::Int16)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::I16)
                }
                (Ty::Integer, Some(Format::UInt16)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::U16)
                }
                (Ty::Integer, Some(Format::Int32)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::I32)
                }
                (Ty::Integer, Some(Format::UInt32)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::U32)
                }
                (Ty::Integer, Some(Format::Int64)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::I64)
                }
                (Ty::Integer, Some(Format::UInt64)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::U64)
                }
                (Ty::Integer, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::UnixTime)
                }
                (Ty::Integer, _) => OtherVariant::Primitive(self.name, PrimitiveType::I32),

                (Ty::Number, Some(Format::Float)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::F32)
                }
                (Ty::Number, Some(Format::Double)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::F64)
                }
                (Ty::Number, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(self.name, PrimitiveType::UnixTime)
                }
                (Ty::Number, _) => OtherVariant::Primitive(self.name, PrimitiveType::F64),

                (Ty::Boolean, _) => OtherVariant::Primitive(self.name, PrimitiveType::Bool),

                (Ty::Array, _) => {
                    let items = match &self.schema.items {
                        Some(RefOrSchema::Ref(r)) => SpecType::Ref(r),
                        Some(RefOrSchema::Inline(schema)) => {
                            let id = self.next_inline_id();
                            transform_with_context(self.context, id, schema)
                        }
                        None => {
                            let id = self.next_inline_id();
                            SpecInlineType::Any(id).into()
                        }
                    };
                    OtherVariant::Array(
                        self.name,
                        SpecInner {
                            description: self.schema.description.as_deref(),
                            ty: self.arena().alloc(items),
                        },
                    )
                }

                (Ty::Object, _) => {
                    let inner = match &self.schema.additional_properties {
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => {
                            Some(SpecInner {
                                description: self.schema.description.as_deref(),
                                ty: self.arena().alloc(SpecType::Ref(r)),
                            })
                        }
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Inline(schema))) => {
                            let id = self.next_inline_id();
                            Some(SpecInner {
                                description: self.schema.description.as_deref(),
                                ty: self.arena().alloc(transform_with_context(
                                    self.context,
                                    id,
                                    schema,
                                )),
                            })
                        }
                        Some(AdditionalProperties::Bool(true)) => {
                            let id = self.next_inline_id();
                            Some(SpecInner {
                                description: self.schema.description.as_deref(),
                                ty: self
                                    .arena()
                                    .alloc(SpecType::Inline(SpecInlineType::Any(id))),
                            })
                        }
                        _ => None,
                    };
                    match inner {
                        Some(inner) => OtherVariant::Map(self.name, inner),
                        None => OtherVariant::Any(self.name),
                    }
                }

                (Ty::Null, _) => {
                    other.nullable = true;
                    continue;
                }
            };
            other.variants.push(variant);
        }

        match (&*other.variants, other.nullable) {
            // An empty `type` array is invalid in JSON Schema,
            // but we treat it as "any type".
            ([], false) => match self.name {
                TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                TypeInfo::Inline(id) => SpecInlineType::Any(id).into(),
            },

            // A `null` variant becomes `Any`.
            ([], true) => match self.name {
                TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                TypeInfo::Inline(id) => SpecInlineType::Any(id).into(),
            },

            // A union with a single, non-`null` variant unwraps to
            // the type of that variant.
            ([variant], false) => variant.to_type(),

            // A two-variant union, with one type T and one `null` variant,
            // simplifies to `Optional(T)`.
            ([variant], true) => {
                let container = SpecContainer::Optional(SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self
                        .arena()
                        .alloc(variant.to_inline_type(self.next_inline_id())),
                });
                match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Container(id, container).into(),
                }
            }

            // Anything else becomes an untagged union.
            (many, nullable) => {
                let mut variants = many
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| (index + 1, variant))
                    .map(|(index, variant)| {
                        SpecUntaggedVariant::Some(
                            variant
                                .hint()
                                .unwrap_or(UntaggedVariantNameHint::Index(index)),
                            self.arena()
                                .alloc(variant.to_inline_type(self.next_inline_id())),
                        )
                    })
                    .collect_vec();
                if nullable {
                    variants.push(SpecUntaggedVariant::Null);
                }
                let untagged = SpecUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(&variants),
                    fields: &[],
                };
                match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Untagged(info, untagged).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Untagged(id, untagged).into(),
                }
            }
        }
    }

    // MARK: Shared lowering

    /// Lowers immediate parents from `allOf` into a list of types.
    fn parents(&self) -> impl Iterator<Item = &'a SpecType<'a>> {
        self.schema
            .all_of
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, parent)| (index + 1, parent))
            .map(move |(_index, parent)| &*match parent {
                RefOrSchema::Ref(r) => self.arena().alloc(SpecType::Ref(r)),
                RefOrSchema::Inline(schema) => {
                    let id = self.next_inline_id();
                    self.arena()
                        .alloc(transform_with_context(self.context, id, schema))
                }
            })
    }

    /// Lowers regular fields from `properties` into struct fields,
    /// wrapping nullable and optional fields in [`Container::Optional`].
    fn properties(&self) -> impl Iterator<Item = SpecStructField<'a>> {
        self.schema
            .properties
            .iter()
            .flatten()
            .map(move |(name, field_schema)| {
                let field_name = name.as_str();
                let required = self.schema.required.contains(name);
                let ty: &_ = match field_schema {
                    RefOrSchema::Ref(r) => self.arena().alloc(SpecType::Ref(r)),
                    RefOrSchema::Inline(schema) => {
                        let id = self.next_inline_id();
                        self.arena()
                            .alloc(transform_with_context(self.context, id, schema))
                    }
                };
                let description = match field_schema {
                    RefOrSchema::Inline(schema) => schema.description.as_deref(),
                    RefOrSchema::Ref(r) => r
                        .pointer()
                        .follow::<&Schema>(self.context.doc)
                        .ok()
                        .and_then(|schema| schema.description.as_deref()),
                };
                let nullable = match field_schema {
                    RefOrSchema::Inline(schema) if schema.nullable => true,
                    RefOrSchema::Ref(r) => r
                        .pointer()
                        .follow::<&Schema>(self.context.doc)
                        .is_ok_and(|schema| schema.nullable),
                    _ => false,
                };
                // Wrap the type in `Optional` if the field is either
                // explicitly nullable, or implicitly optional. The `required`
                // flag distinguishes between the two for codegen.
                let ty: &_ = if nullable || !required {
                    let id = self.next_inline_id();
                    self.arena().alloc(SpecType::from(SpecInlineType::Container(
                        id,
                        SpecContainer::Optional(SpecInner { description, ty }),
                    )))
                } else {
                    ty
                };
                SpecStructField {
                    name: StructFieldName::Name(field_name),
                    ty,
                    required,
                    description,
                    flattened: false,
                }
            })
    }

    /// Lowers `additionalProperties` into a struct field definition,
    /// if the schema specifies them.
    fn additional_properties(&self) -> Option<SpecStructField<'a>> {
        let name = StructFieldName::Hint(StructFieldNameHint::AdditionalProperties);

        let inner = match &self.schema.additional_properties {
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => SpecInner {
                description: self.schema.description.as_deref(),
                ty: self.arena().alloc(SpecType::Ref(r)),
            },
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Inline(schema))) => {
                let id = self.next_inline_id();
                SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self
                        .arena()
                        .alloc(transform_with_context(self.context, id, schema)),
                }
            }
            Some(AdditionalProperties::Bool(true)) => {
                let id = self.next_inline_id();
                SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self
                        .arena()
                        .alloc(SpecType::Inline(SpecInlineType::Any(id))),
                }
            }
            _ => return None,
        };

        let map_id = self.next_inline_id();
        let ty: &_ = self.arena().alloc(SpecType::from(SpecInlineType::Container(
            map_id,
            SpecContainer::Map(inner),
        )));

        Some(SpecStructField {
            name,
            ty,
            required: true,
            description: None,
            flattened: true,
        })
    }
}

/// A union of variants for representing OpenAPI 3.1-style
/// `type` arrays.
struct Other<'a> {
    variants: Vec<OtherVariant<'a>>,
    nullable: bool,
}

/// A variant of an [`Other`] union.
#[derive(Clone, Copy)]
enum OtherVariant<'a> {
    Primitive(TypeInfo<'a>, PrimitiveType),
    Array(TypeInfo<'a>, SpecInner<'a>),
    Map(TypeInfo<'a>, SpecInner<'a>),
    Any(TypeInfo<'a>),
}

impl<'a> OtherVariant<'a> {
    /// Returns the name hint for this variant when used in an untagged union.
    fn hint(self) -> Option<UntaggedVariantNameHint> {
        Some(match self {
            Self::Primitive(_, p) => UntaggedVariantNameHint::Primitive(p),
            Self::Array(..) => UntaggedVariantNameHint::Array,
            Self::Map(..) => UntaggedVariantNameHint::Map,
            Self::Any(_) => return None,
        })
    }

    /// Converts this variant to a [`SpecType`].
    ///
    /// This is used to unwrap variants for the single-variant
    /// and untagged union cases.
    fn to_type(self) -> SpecType<'a> {
        match self {
            Self::Primitive(name, p) => match name {
                TypeInfo::Schema(info) => SpecSchemaType::Primitive(info, p).into(),
                TypeInfo::Inline(id) => SpecInlineType::Primitive(id, p).into(),
            },
            Self::Array(name, inner) => {
                let container = SpecContainer::Array(inner);
                match name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Container(id, container).into(),
                }
            }
            Self::Map(name, inner) => {
                let container = SpecContainer::Map(inner);
                match name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(id) => SpecInlineType::Container(id, container).into(),
                }
            }
            Self::Any(name) => match name {
                TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                TypeInfo::Inline(id) => SpecInlineType::Any(id).into(),
            },
        }
    }

    /// Converts this variant to an inline [`SpecType`], using
    /// `new_id` for schema-rooted types that need a fresh
    /// [`InlineTypeId`].
    fn to_inline_type(self, new_id: InlineTypeId) -> SpecType<'a> {
        match self {
            Self::Primitive(name, p) => {
                let id = match name {
                    TypeInfo::Schema(_) => new_id,
                    TypeInfo::Inline(id) => id,
                };
                SpecInlineType::Primitive(id, p).into()
            }
            Self::Array(name, inner) => {
                let id = match name {
                    TypeInfo::Schema(_) => new_id,
                    TypeInfo::Inline(id) => id,
                };
                SpecInlineType::Container(id, SpecContainer::Array(inner)).into()
            }
            Self::Map(name, inner) => {
                let id = match name {
                    TypeInfo::Schema(_) => new_id,
                    TypeInfo::Inline(id) => id,
                };
                SpecInlineType::Container(id, SpecContainer::Map(inner)).into()
            }
            Self::Any(name) => {
                let id = match name {
                    TypeInfo::Schema(_) => new_id,
                    TypeInfo::Inline(id) => id,
                };
                SpecInlineType::Any(id).into()
            }
        }
    }
}
