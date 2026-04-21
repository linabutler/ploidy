use itertools::Itertools;
use rustc_hash::FxHashMap;

use crate::{
    arena::Arena,
    ir::JsonF64,
    parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty},
};

use super::types::{
    Enum, EnumVariant, InlineTypePath, InlineTypePathRoot, InlineTypePathSegment, PrimitiveType,
    SpecContainer, SpecInlineType, SpecInner, SpecSchemaType, SpecStruct, SpecStructField,
    SpecTagged, SpecTaggedVariant, SpecType, SpecUntagged, SpecUntaggedVariant, StructFieldName,
    StructFieldNameHint, TypeInfo, UntaggedVariantNameHint,
};

#[inline]
pub fn transform<'a>(
    arena: &'a Arena,
    doc: &'a Document,
    name: impl Into<TypeInfo<'a>>,
    schema: &'a Schema,
) -> SpecType<'a> {
    let context = TransformContext::new(arena, doc);
    transform_with_context(&context, name.into(), schema)
}

/// Context for the [`IrTransformer`].
#[derive(Debug)]
pub struct TransformContext<'a> {
    pub arena: &'a Arena,
    /// The document being transformed.
    pub doc: &'a Document,
}

impl<'a> TransformContext<'a> {
    /// Creates a new context for the given document.
    pub fn new(arena: &'a Arena, doc: &'a Document) -> Self {
        Self { arena, doc }
    }
}

fn transform_with_context<'context, 'a>(
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
            TypeInfo::Inline(path) => SpecInlineType::Tagged(path, tagged).into(),
        })
    }

    fn try_untagged(self) -> Result<SpecType<'a>, Self> {
        let Some(one_of) = &self.schema.one_of else {
            return Err(self);
        };

        // Unwrap single-variant untagged unions before collecting,
        // so that the schema identity is preserved through the
        // inner transform.
        match &**one_of {
            [] => {
                return Ok(match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                    TypeInfo::Inline(path) => SpecInlineType::Any(path).into(),
                });
            }
            [schema] => {
                return Ok(match schema {
                    RefOrSchema::Ref(r) => SpecType::Ref(r),
                    RefOrSchema::Inline(s) if matches!(&*s.ty, [Ty::Null]) => match self.name {
                        TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                        TypeInfo::Inline(path) => SpecInlineType::Any(path).into(),
                    },
                    RefOrSchema::Inline(schema) => {
                        transform_with_context(self.context, self.name, schema)
                    }
                });
            }
            _ => {}
        }

        let variants = one_of
            .iter()
            .enumerate()
            .map(|(index, schema)| (index + 1, schema))
            .map(|(index, schema)| {
                let ty = match schema {
                    RefOrSchema::Ref(r) => Some(SpecType::Ref(r)),
                    RefOrSchema::Inline(s) if matches!(&*s.ty, [Ty::Null]) => None,
                    RefOrSchema::Inline(schema) => {
                        let segment = InlineTypePathSegment::Variant(index);
                        let path = match self.name {
                            TypeInfo::Schema(info) => InlineTypePath {
                                root: InlineTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        Some(transform_with_context(self.context, path, schema))
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
            .collect_vec();

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
                    TypeInfo::Inline(path) => SpecInlineType::Container(path, container).into(),
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
                    TypeInfo::Inline(path) => SpecInlineType::Untagged(path, untagged).into(),
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
                        // For references, use the referenced type's name
                        // as the field name. For example, a pointer like
                        // `#/components/schemas/Address` becomes `address`.
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
                        // For inline schemas, we don't have a name that we can use,
                        // so use its index in `anyOf` as a naming hint.
                        let name = StructFieldName::Hint(StructFieldNameHint::Index(index + 1));
                        let segment = InlineTypePathSegment::Field(name);
                        let path = match self.name {
                            TypeInfo::Schema(info) => InlineTypePath {
                                root: InlineTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        let ty: &_ =
                            self.arena()
                                .alloc(transform_with_context(self.context, path, schema));
                        let desc = schema.description.as_deref();
                        (name, ty, desc)
                    }
                };
                // Flattened `anyOf` fields are always optional.
                let segment = InlineTypePathSegment::Field(field_name);
                let path = match self.name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: self.arena().alloc_slice_copy(&[segment]),
                    },
                    TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                };
                let ty: &_ = self.arena().alloc(
                    SpecInlineType::Container(
                        path,
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
            TypeInfo::Inline(path) => SpecInlineType::Struct(path, ty).into(),
        })
    }

    fn try_enum(self) -> Result<SpecType<'a>, Self> {
        let Some(values) = &self.schema.variants else {
            return Err(self);
        };
        let variants = self.arena().alloc_slice(values.iter().filter_map(|value| {
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
        }));
        let ty = Enum {
            description: self.schema.description.as_deref(),
            variants,
        };
        Ok(match self.name {
            TypeInfo::Schema(info) => SpecSchemaType::Enum(info, ty).into(),
            TypeInfo::Inline(path) => SpecInlineType::Enum(path, ty).into(),
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
            TypeInfo::Inline(path) => SpecInlineType::Struct(path, ty).into(),
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
                            let segment = InlineTypePathSegment::ArrayItem;
                            let path = match self.name {
                                TypeInfo::Schema(info) => InlineTypePath {
                                    root: InlineTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            transform_with_context(self.context, path, schema)
                        }
                        None => {
                            let segment = InlineTypePathSegment::ArrayItem;
                            let path = match self.name {
                                TypeInfo::Schema(info) => InlineTypePath {
                                    root: InlineTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            SpecInlineType::Any(path).into()
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
                            let segment = InlineTypePathSegment::MapValue;
                            let path = match self.name {
                                TypeInfo::Schema(info) => InlineTypePath {
                                    root: InlineTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            Some(SpecInner {
                                description: self.schema.description.as_deref(),
                                ty: self.arena().alloc(transform_with_context(
                                    self.context,
                                    path,
                                    schema,
                                )),
                            })
                        }
                        Some(AdditionalProperties::Bool(true)) => {
                            let segment = InlineTypePathSegment::MapValue;
                            let path = match self.name {
                                TypeInfo::Schema(info) => InlineTypePath {
                                    root: InlineTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            Some(SpecInner {
                                description: self.schema.description.as_deref(),
                                ty: self
                                    .arena()
                                    .alloc(SpecType::Inline(SpecInlineType::Any(path))),
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
                TypeInfo::Inline(path) => SpecInlineType::Any(path).into(),
            },

            // A `null` variant becomes `Any`.
            ([], true) => match self.name {
                TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => SpecInlineType::Any(path).into(),
            },

            // A union with a single, non-`null` variant unwraps to
            // the type of that variant.
            ([variant], false) => variant.to_type(),

            // A two-variant union, with one type T and one `null` variant,
            // simplifies to `Optional(T)`.
            ([variant], true) => {
                let container = SpecContainer::Optional(SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self.arena().alloc(variant.to_inline_type()),
                });
                match self.name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(path) => SpecInlineType::Container(path, container).into(),
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
                            self.arena().alloc(variant.to_inline_type()),
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
                    TypeInfo::Inline(path) => SpecInlineType::Untagged(path, untagged).into(),
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
            .map(move |(index, parent)| &*match parent {
                RefOrSchema::Ref(r) => self.arena().alloc(SpecType::Ref(r)),
                RefOrSchema::Inline(schema) => {
                    let segment = InlineTypePathSegment::Parent(index);
                    let path = match self.name {
                        TypeInfo::Schema(info) => InlineTypePath {
                            root: InlineTypePathRoot::Type(info.name),
                            segments: self.arena().alloc_slice_copy(&[segment]),
                        },
                        TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                    };
                    self.arena()
                        .alloc(transform_with_context(self.context, path, schema))
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
                        let segment =
                            InlineTypePathSegment::Field(StructFieldName::Name(field_name));
                        let path = match self.name {
                            TypeInfo::Schema(info) => InlineTypePath {
                                root: InlineTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        self.arena()
                            .alloc(transform_with_context(self.context, path, schema))
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
                    let segment = InlineTypePathSegment::Field(StructFieldName::Name(field_name));
                    let path = match self.name {
                        TypeInfo::Schema(info) => InlineTypePath {
                            root: InlineTypePathRoot::Type(info.name),
                            segments: self.arena().alloc_slice_copy(&[segment]),
                        },
                        TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                    };
                    self.arena().alloc(SpecType::from(SpecInlineType::Container(
                        path,
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
        let path = match self.name {
            TypeInfo::Schema(info) => InlineTypePath {
                root: InlineTypePathRoot::Type(info.name),
                segments: self
                    .arena()
                    .alloc_slice_copy(&[InlineTypePathSegment::Field(name)]),
            },
            TypeInfo::Inline(path) => {
                path.join(self.arena(), &[InlineTypePathSegment::Field(name)])
            }
        };

        let inner = match &self.schema.additional_properties {
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => SpecInner {
                description: self.schema.description.as_deref(),
                ty: self.arena().alloc(SpecType::Ref(r)),
            },
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Inline(schema))) => {
                let path = path.join(self.arena(), &[InlineTypePathSegment::MapValue]);
                SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self
                        .arena()
                        .alloc(transform_with_context(self.context, path, schema)),
                }
            }
            Some(AdditionalProperties::Bool(true)) => {
                let path = path.join(self.arena(), &[InlineTypePathSegment::MapValue]);
                SpecInner {
                    description: self.schema.description.as_deref(),
                    ty: self
                        .arena()
                        .alloc(SpecType::Inline(SpecInlineType::Any(path))),
                }
            }
            _ => return None,
        };

        let ty: &_ = self.arena().alloc(SpecType::from(SpecInlineType::Container(
            path,
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
                TypeInfo::Inline(path) => SpecInlineType::Primitive(path, p).into(),
            },
            Self::Array(name, inner) => {
                let container = SpecContainer::Array(inner);
                match name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(path) => SpecInlineType::Container(path, container).into(),
                }
            }
            Self::Map(name, inner) => {
                let container = SpecContainer::Map(inner);
                match name {
                    TypeInfo::Schema(info) => SpecSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(path) => SpecInlineType::Container(path, container).into(),
                }
            }
            Self::Any(name) => match name {
                TypeInfo::Schema(info) => SpecSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => SpecInlineType::Any(path).into(),
            },
        }
    }

    /// Converts this variant to an inline [`SpecType`].
    ///
    /// This is used to rewrite `[T, null]` unions as `Optional(T)`.
    fn to_inline_type(self) -> SpecType<'a> {
        match self {
            Self::Primitive(name, p) => {
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    TypeInfo::Inline(path) => path,
                };
                SpecInlineType::Primitive(path, p).into()
            }
            Self::Array(name, inner) => {
                let container = SpecContainer::Array(inner);
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    TypeInfo::Inline(path) => path,
                };
                SpecInlineType::Container(path, container).into()
            }
            Self::Map(name, inner) => {
                let container = SpecContainer::Map(inner);
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    TypeInfo::Inline(path) => path,
                };
                SpecInlineType::Container(path, container).into()
            }
            Self::Any(name) => {
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    TypeInfo::Inline(path) => path,
                };
                SpecInlineType::Any(path).into()
            }
        }
    }
}
