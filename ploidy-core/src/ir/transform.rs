use itertools::Itertools;
use ploidy_pointer::JsonPointee;
use rustc_hash::FxHashMap;

use crate::{
    arena::Arena,
    parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty},
};

use super::types::{
    Enum, EnumVariant, InlineTypePath, InlineTypePathRoot, InlineTypePathSegment, PrimitiveType,
    RawContainer, RawInlineType, RawInner, RawSchemaType, RawStruct, RawStructField, RawTagged,
    RawTaggedVariant, RawType, RawUntagged, RawUntaggedVariant, StructFieldName,
    StructFieldNameHint, TypeInfo, UntaggedVariantNameHint,
};

#[inline]
pub fn transform<'a>(
    arena: &'a Arena,
    doc: &'a Document,
    name: impl Into<TypeInfo<'a>>,
    schema: &'a Schema,
) -> RawType<'a> {
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
) -> RawType<'a> {
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

    fn transform(self) -> RawType<'a> {
        self.try_tagged()
            .or_else(Self::try_untagged)
            .or_else(Self::try_any_of)
            .or_else(Self::try_enum)
            .or_else(Self::try_struct)
            .unwrap_or_else(Self::other)
    }

    fn try_tagged(self) -> Result<RawType<'a>, Self> {
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
                        let aliases = inverted.get(&r.path).map(|s| s.as_slice()).unwrap_or(&[]);
                        if aliases.is_empty() {
                            // Variant missing from discriminator mapping;
                            // fall through to `try_untagged`.
                            return Err(self);
                        }
                        variants.push(RawTaggedVariant {
                            name: r.path.name(),
                            ty: &*self.arena().alloc(RawType::Ref(&r.path)),
                            aliases: self.arena().alloc_slice_copy(aliases),
                        });
                    }
                    // An inline schema variant can't have a discriminator mapping;
                    // fall through to `try_untagged`.
                    RefOrSchema::Other(_) => return Err(self),
                }
            }
            variants
        };

        let tagged = RawTagged {
            description: self.schema.description.as_deref(),
            tag: discriminator.property_name.as_str(),
            variants: self.arena().alloc_slice_copy(&variants),
        };

        Ok(match self.name {
            TypeInfo::Schema(info) => RawSchemaType::Tagged(info, tagged).into(),
            TypeInfo::Inline(path) => RawInlineType::Tagged(path, tagged).into(),
        })
    }

    fn try_untagged(self) -> Result<RawType<'a>, Self> {
        let Some(one_of) = &self.schema.one_of else {
            return Err(self);
        };

        let variants = one_of
            .iter()
            .enumerate()
            .map(|(index, schema)| (index + 1, schema))
            .map(|(index, schema)| {
                let ty = match schema {
                    RefOrSchema::Ref(r) => Some(RawType::Ref(&r.path)),
                    RefOrSchema::Other(s) if matches!(&*s.ty, [Ty::Null]) => None,
                    RefOrSchema::Other(schema) => {
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
                    let hint = match &ty {
                        &RawType::Schema(RawSchemaType::Primitive(_, p))
                        | &RawType::Inline(RawInlineType::Primitive(_, p)) => {
                            UntaggedVariantNameHint::Primitive(p)
                        }
                        RawType::Schema(RawSchemaType::Container(_, RawContainer::Array(_)))
                        | RawType::Inline(RawInlineType::Container(_, RawContainer::Array(_))) => {
                            UntaggedVariantNameHint::Array
                        }
                        RawType::Schema(RawSchemaType::Container(_, RawContainer::Map(_)))
                        | RawType::Inline(RawInlineType::Container(_, RawContainer::Map(_))) => {
                            UntaggedVariantNameHint::Map
                        }
                        _ => UntaggedVariantNameHint::Index(index),
                    };
                    RawUntaggedVariant::Some(hint, &*self.arena().alloc(ty))
                })
                .unwrap_or(RawUntaggedVariant::Null)
            })
            .collect_vec();

        Ok(match &*variants {
            [] => match self.name {
                TypeInfo::Schema(info) => RawSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => RawInlineType::Any(path).into(),
            },

            // Unwrap single-variant untagged unions.
            [RawUntaggedVariant::Null] => match self.name {
                TypeInfo::Schema(info) => RawSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => RawInlineType::Any(path).into(),
            },
            [RawUntaggedVariant::Some(_, ty)] => **ty,

            // Simplify two-variant untagged unions, where one is a type
            // and the other is `null`, into optionals.
            [RawUntaggedVariant::Some(_, ty), RawUntaggedVariant::Null]
            | [RawUntaggedVariant::Null, RawUntaggedVariant::Some(_, ty)] => {
                let container = RawContainer::Optional(RawInner {
                    description: self.schema.description.as_deref(),
                    ty: *ty,
                });
                match self.name {
                    TypeInfo::Schema(info) => RawSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(path) => RawInlineType::Container(path, container).into(),
                }
            }

            variants => {
                let untagged = RawUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(variants),
                };
                match self.name {
                    TypeInfo::Schema(info) => RawSchemaType::Untagged(info, untagged).into(),
                    TypeInfo::Inline(path) => RawInlineType::Untagged(path, untagged).into(),
                }
            }
        })
    }

    fn try_any_of(self) -> Result<RawType<'a>, Self> {
        let Some(any_of) = &self.schema.any_of else {
            return Err(self);
        };
        if let [schema] = &**any_of {
            // A single-variant `anyOf` should unwrap to the variant type. This
            // preserves type references that would otherwise become `Any`.
            return Ok(match schema {
                RefOrSchema::Ref(r) => RawType::Ref(&r.path),
                RefOrSchema::Other(schema) => {
                    let path = match self.name {
                        TypeInfo::Schema(info) => InlineTypePath {
                            root: InlineTypePathRoot::Type(info.name),
                            segments: &[],
                        },
                        TypeInfo::Inline(path) => path,
                    };
                    transform_with_context(self.context, path, schema)
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
                        let name = StructFieldName::Name(r.path.name());
                        let ty: &_ = self.arena().alloc(RawType::Ref(&r.path));
                        let desc = self
                            .context
                            .doc
                            .resolve(r.path.pointer().clone())
                            .ok()
                            .and_then(|p| p.downcast_ref::<Schema>())
                            .and_then(|s| s.description.as_deref());
                        (name, ty, desc)
                    }
                    RefOrSchema::Other(schema) => {
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
                    RawInlineType::Container(
                        path,
                        RawContainer::Optional(RawInner { description, ty }),
                    )
                    .into(),
                );
                RawStructField {
                    name: field_name,
                    ty,
                    required: false,
                    description,
                    flattened: true,
                }
            })
            .collect_vec();

        let ty = RawStruct {
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
            TypeInfo::Schema(info) => RawSchemaType::Struct(info, ty).into(),
            TypeInfo::Inline(path) => RawInlineType::Struct(path, ty).into(),
        })
    }

    fn try_enum(self) -> Result<RawType<'a>, Self> {
        let Some(values) = &self.schema.variants else {
            return Err(self);
        };
        let variants = self.arena().alloc_slice(values.iter().filter_map(|value| {
            if let Some(s) = value.as_str() {
                Some(EnumVariant::String(s))
            } else if let Some(n) = value.as_number() {
                Some(EnumVariant::Number(n.clone()))
            } else {
                value.as_bool().map(EnumVariant::Bool)
            }
        }));
        let ty = Enum {
            description: self.schema.description.as_deref(),
            variants,
        };
        Ok(match self.name {
            TypeInfo::Schema(info) => RawSchemaType::Enum(info, ty).into(),
            TypeInfo::Inline(path) => RawInlineType::Enum(path, ty).into(),
        })
    }

    fn try_struct(self) -> Result<RawType<'a>, Self> {
        if self.schema.properties.is_none() && self.schema.all_of.is_none() {
            return Err(self);
        }

        let ty = RawStruct {
            description: self.schema.description.as_deref(),
            fields: self.arena().alloc_slice(itertools::chain!(
                self.properties(),
                self.additional_properties()
            )),
            parents: self.arena().alloc_slice(self.parents()),
        };
        Ok(match self.name {
            TypeInfo::Schema(info) => RawSchemaType::Struct(info, ty).into(),
            TypeInfo::Inline(path) => RawInlineType::Struct(path, ty).into(),
        })
    }

    fn other(self) -> RawType<'a> {
        let mut other = Other {
            variants: Vec::with_capacity(self.schema.ty.len()),
            nullable: false,
        };

        for ty in &self.schema.ty {
            let variant = match (ty, self.schema.format) {
                (Ty::String, Some(Format::DateTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::DateTime)
                }
                (Ty::String, Some(Format::Date)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::Date)
                }
                (Ty::String, Some(Format::Uri)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::Url)
                }
                (Ty::String, Some(Format::Uuid)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::Uuid)
                }
                (Ty::String, Some(Format::Byte)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::Bytes)
                }
                (Ty::String, Some(Format::Binary)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::Binary)
                }
                (Ty::String, _) => OtherVariant::Primitive(&self.name, PrimitiveType::String),

                (Ty::Integer, Some(Format::Int8)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::I8)
                }
                (Ty::Integer, Some(Format::UInt8)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::U8)
                }
                (Ty::Integer, Some(Format::Int16)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::I16)
                }
                (Ty::Integer, Some(Format::UInt16)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::U16)
                }
                (Ty::Integer, Some(Format::Int32)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::I32)
                }
                (Ty::Integer, Some(Format::UInt32)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::U32)
                }
                (Ty::Integer, Some(Format::Int64)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::I64)
                }
                (Ty::Integer, Some(Format::UInt64)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::U64)
                }
                (Ty::Integer, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::UnixTime)
                }
                (Ty::Integer, _) => OtherVariant::Primitive(&self.name, PrimitiveType::I32),

                (Ty::Number, Some(Format::Float)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::F32)
                }
                (Ty::Number, Some(Format::Double)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::F64)
                }
                (Ty::Number, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveType::UnixTime)
                }
                (Ty::Number, _) => OtherVariant::Primitive(&self.name, PrimitiveType::F64),

                (Ty::Boolean, _) => OtherVariant::Primitive(&self.name, PrimitiveType::Bool),

                (Ty::Array, _) => {
                    let items = match &self.schema.items {
                        Some(RefOrSchema::Ref(r)) => RawType::Ref(&r.path),
                        Some(RefOrSchema::Other(schema)) => {
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
                            RawInlineType::Any(path).into()
                        }
                    };
                    OtherVariant::Array(
                        &self.name,
                        RawInner {
                            description: self.schema.description.as_deref(),
                            ty: self.arena().alloc(items),
                        },
                    )
                }

                (Ty::Object, _) => {
                    let inner = match &self.schema.additional_properties {
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => {
                            Some(RawInner {
                                description: self.schema.description.as_deref(),
                                ty: &*self.arena().alloc(RawType::Ref(&r.path)),
                            })
                        }
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                            let segment = InlineTypePathSegment::MapValue;
                            let path = match self.name {
                                TypeInfo::Schema(info) => InlineTypePath {
                                    root: InlineTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            Some(RawInner {
                                description: self.schema.description.as_deref(),
                                ty: &*self.arena().alloc(transform_with_context(
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
                            Some(RawInner {
                                description: self.schema.description.as_deref(),
                                ty: &*self
                                    .arena()
                                    .alloc(RawType::Inline(RawInlineType::Any(path))),
                            })
                        }
                        _ => None,
                    };
                    match inner {
                        Some(inner) => OtherVariant::Map(&self.name, inner),
                        None => OtherVariant::Any(&self.name),
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
                TypeInfo::Schema(info) => RawSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => RawInlineType::Any(path).into(),
            },

            // A `null` variant becomes `Any`.
            ([], true) => match self.name {
                TypeInfo::Schema(info) => RawSchemaType::Any(info).into(),
                TypeInfo::Inline(path) => RawInlineType::Any(path).into(),
            },

            // A union with a single, non-`null` variant unwraps to
            // the type of that variant.
            ([variant], false) => variant.to_type(),

            // A two-variant union, with one type T and one `null` variant,
            // simplifies to `Optional(T)`.
            ([variant], true) => {
                let container = RawContainer::Optional(RawInner {
                    description: self.schema.description.as_deref(),
                    ty: &*self.arena().alloc(variant.to_inline_type()),
                });
                match self.name {
                    TypeInfo::Schema(info) => RawSchemaType::Container(info, container).into(),
                    TypeInfo::Inline(path) => RawInlineType::Container(path, container).into(),
                }
            }

            // Anything else becomes an untagged union.
            (many, nullable) => {
                let mut variants = many
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| (index + 1, variant))
                    .map(|(index, variant)| {
                        RawUntaggedVariant::Some(
                            variant
                                .hint()
                                .unwrap_or(UntaggedVariantNameHint::Index(index)),
                            &*self.arena().alloc(variant.to_type()),
                        )
                    })
                    .collect_vec();
                if nullable {
                    variants.push(RawUntaggedVariant::Null);
                }
                let untagged = RawUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(&variants),
                };
                match self.name {
                    TypeInfo::Schema(info) => RawSchemaType::Untagged(info, untagged).into(),
                    TypeInfo::Inline(path) => RawInlineType::Untagged(path, untagged).into(),
                }
            }
        }
    }

    // MARK: Shared lowering

    /// Lowers immediate parents from `allOf` into a list of types.
    fn parents(&self) -> impl Iterator<Item = &'a RawType<'a>> {
        self.schema
            .all_of
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, parent)| (index + 1, parent))
            .map(move |(index, parent)| match parent {
                RefOrSchema::Ref(r) => &*self.arena().alloc(RawType::Ref(&r.path)),
                RefOrSchema::Other(schema) => {
                    let segment = InlineTypePathSegment::Parent(index);
                    let path = match self.name {
                        TypeInfo::Schema(info) => InlineTypePath {
                            root: InlineTypePathRoot::Type(info.name),
                            segments: self.arena().alloc_slice_copy(&[segment]),
                        },
                        TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                    };
                    &*self
                        .arena()
                        .alloc(transform_with_context(self.context, path, schema))
                }
            })
    }

    /// Lowers regular fields from `properties` into struct fields,
    /// wrapping nullable and optional fields in [`Container::Optional`].
    fn properties(&self) -> impl Iterator<Item = RawStructField<'a>> {
        self.schema
            .properties
            .iter()
            .flatten()
            .map(move |(name, field_schema)| {
                let field_name = name.as_str();
                let required = self.schema.required.contains(name);
                let ty: &_ = match field_schema {
                    RefOrSchema::Ref(r) => &*self.arena().alloc(RawType::Ref(&r.path)),
                    RefOrSchema::Other(schema) => {
                        let segment =
                            InlineTypePathSegment::Field(StructFieldName::Name(field_name));
                        let path = match self.name {
                            TypeInfo::Schema(info) => InlineTypePath {
                                root: InlineTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            TypeInfo::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        &*self
                            .arena()
                            .alloc(transform_with_context(self.context, path, schema))
                    }
                };
                let description = match field_schema {
                    RefOrSchema::Other(schema) => schema.description.as_deref(),
                    RefOrSchema::Ref(r) => self
                        .context
                        .doc
                        .resolve(r.path.pointer().clone())
                        .ok()
                        .and_then(|p| p.downcast_ref::<Schema>())
                        .and_then(|schema| schema.description.as_deref()),
                };
                let nullable = match field_schema {
                    RefOrSchema::Other(schema) if schema.nullable => true,
                    RefOrSchema::Ref(r) => {
                        if let Ok(resolved) = self.context.doc.resolve(r.path.pointer().clone())
                            && let Some(schema) = resolved.downcast_ref::<Schema>()
                            && schema.nullable
                        {
                            true
                        } else {
                            false
                        }
                    }
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
                    self.arena().alloc(RawType::from(RawInlineType::Container(
                        path,
                        RawContainer::Optional(RawInner { description, ty }),
                    )))
                } else {
                    ty
                };
                RawStructField {
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
    fn additional_properties(&self) -> Option<RawStructField<'a>> {
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
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => RawInner {
                description: self.schema.description.as_deref(),
                ty: &*self.arena().alloc(RawType::Ref(&r.path)),
            },
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                let path = path.join(self.arena(), &[InlineTypePathSegment::MapValue]);
                RawInner {
                    description: self.schema.description.as_deref(),
                    ty: &*self
                        .arena()
                        .alloc(transform_with_context(self.context, path, schema)),
                }
            }
            Some(AdditionalProperties::Bool(true)) => {
                let path = path.join(self.arena(), &[InlineTypePathSegment::MapValue]);
                RawInner {
                    description: self.schema.description.as_deref(),
                    ty: &*self
                        .arena()
                        .alloc(RawType::Inline(RawInlineType::Any(path))),
                }
            }
            _ => return None,
        };

        let ty: &_ = self.arena().alloc(RawType::from(RawInlineType::Container(
            path,
            RawContainer::Map(inner),
        )));

        Some(RawStructField {
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
struct Other<'name, 'a> {
    variants: Vec<OtherVariant<'name, 'a>>,
    nullable: bool,
}

/// A variant of an [`Other`] union.
enum OtherVariant<'name, 'a> {
    Primitive(&'name TypeInfo<'a>, PrimitiveType),
    Array(&'name TypeInfo<'a>, RawInner<'a>),
    Map(&'name TypeInfo<'a>, RawInner<'a>),
    Any(&'name TypeInfo<'a>),
}

impl<'name, 'a> OtherVariant<'name, 'a> {
    /// Returns the name hint for this variant when used in an untagged union.
    fn hint(&self) -> Option<UntaggedVariantNameHint> {
        Some(match self {
            &Self::Primitive(_, p) => UntaggedVariantNameHint::Primitive(p),
            Self::Array(..) => UntaggedVariantNameHint::Array,
            Self::Map(..) => UntaggedVariantNameHint::Map,
            Self::Any(_) => return None,
        })
    }

    /// Converts this variant to a [`RawType`].
    ///
    /// This is used to unwrap variants for the single-variant
    /// and untagged union cases.
    fn to_type(&self) -> RawType<'a> {
        match self {
            &Self::Primitive(name, p) => match name {
                TypeInfo::Schema(info) => RawSchemaType::Primitive(*info, p).into(),
                TypeInfo::Inline(path) => RawInlineType::Primitive(*path, p).into(),
            },
            Self::Array(name, inner) => {
                let container = RawContainer::Array(*inner);
                match name {
                    TypeInfo::Schema(info) => RawSchemaType::Container(*info, container).into(),
                    TypeInfo::Inline(path) => RawInlineType::Container(*path, container).into(),
                }
            }
            Self::Map(name, inner) => {
                let container = RawContainer::Map(*inner);
                match name {
                    TypeInfo::Schema(info) => RawSchemaType::Container(*info, container).into(),
                    TypeInfo::Inline(path) => RawInlineType::Container(*path, container).into(),
                }
            }
            Self::Any(name) => match name {
                TypeInfo::Schema(info) => RawSchemaType::Any(*info).into(),
                TypeInfo::Inline(path) => RawInlineType::Any(*path).into(),
            },
        }
    }

    /// Converts this variant to an inline [`RawType`].
    ///
    /// This is used to rewrite `[T, null]` unions as `Optional(T)`.
    fn to_inline_type(&self) -> RawType<'a> {
        match self {
            &Self::Primitive(name, p) => {
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &TypeInfo::Inline(path) => path,
                };
                RawInlineType::Primitive(path, p).into()
            }
            Self::Array(name, inner) => {
                let container = RawContainer::Array(*inner);
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&TypeInfo::Inline(path) => path,
                };
                RawInlineType::Container(path, container).into()
            }
            Self::Map(name, inner) => {
                let container = RawContainer::Map(*inner);
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&TypeInfo::Inline(path) => path,
                };
                RawInlineType::Container(path, container).into()
            }
            Self::Any(name) => {
                let path = match name {
                    TypeInfo::Schema(info) => InlineTypePath {
                        root: InlineTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&TypeInfo::Inline(path) => path,
                };
                RawInlineType::Any(path).into()
            }
        }
    }
}
