use itertools::Itertools;
use ploidy_pointer::JsonPointee;
use rustc_hash::FxHashMap;

use crate::{
    arena::Arena,
    parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty},
};

use super::types::{
    Container, InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment,
    Inner, IrEnum, IrEnumVariant, IrStruct, IrStructField, IrStructFieldName,
    IrStructFieldNameHint, IrTagged, IrTaggedVariant, IrType, IrTypeName, IrUntagged,
    IrUntaggedVariant, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
};

#[inline]
pub fn transform<'a>(
    arena: &'a Arena,
    doc: &'a Document,
    name: impl Into<IrTypeName<'a>>,
    schema: &'a Schema,
) -> IrType<'a> {
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
    name: impl Into<IrTypeName<'a>>,
    schema: &'a Schema,
) -> IrType<'a> {
    IrTransformer::new(context, name.into(), schema).transform()
}

#[derive(Debug)]
struct IrTransformer<'context, 'a> {
    context: &'context TransformContext<'a>,
    name: IrTypeName<'a>,
    schema: &'a Schema,
}

impl<'context, 'a> IrTransformer<'context, 'a> {
    fn new(
        context: &'context TransformContext<'a>,
        name: IrTypeName<'a>,
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

    fn transform(self) -> IrType<'a> {
        self.try_tagged()
            .or_else(Self::try_untagged)
            .or_else(Self::try_any_of)
            .or_else(Self::try_enum)
            .or_else(Self::try_struct)
            .unwrap_or_else(Self::other)
    }

    fn try_tagged(self) -> Result<IrType<'a>, Self> {
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
                        variants.push(IrTaggedVariant {
                            name: r.path.name(),
                            ty: &*self.arena().alloc(IrType::Ref(&r.path)),
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

        let tagged = IrTagged {
            description: self.schema.description.as_deref(),
            tag: discriminator.property_name.as_str(),
            variants: self.arena().alloc_slice_copy(&variants),
        };

        Ok(match self.name {
            IrTypeName::Schema(info) => SchemaIrType::Tagged(info, tagged).into(),
            IrTypeName::Inline(path) => InlineIrType::Tagged(path, tagged).into(),
        })
    }

    fn try_untagged(self) -> Result<IrType<'a>, Self> {
        let Some(one_of) = &self.schema.one_of else {
            return Err(self);
        };

        let variants = one_of
            .iter()
            .enumerate()
            .map(|(index, schema)| (index + 1, schema))
            .map(|(index, schema)| {
                let ty = match schema {
                    RefOrSchema::Ref(r) => Some(IrType::Ref(&r.path)),
                    RefOrSchema::Other(s) if matches!(&*s.ty, [Ty::Null]) => None,
                    RefOrSchema::Other(schema) => {
                        let segment = InlineIrTypePathSegment::Variant(index);
                        let path = match self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        Some(transform_with_context(self.context, path, schema))
                    }
                };
                ty.map(|ty| {
                    let hint = match &ty {
                        IrType::Schema(SchemaIrType::Primitive(_, p))
                        | IrType::Inline(InlineIrType::Primitive(_, p)) => {
                            IrUntaggedVariantNameHint::Primitive(*p)
                        }
                        IrType::Inline(InlineIrType::Container(_, Container::Array(_)))
                        | IrType::Schema(SchemaIrType::Container(_, Container::Array(_))) => {
                            IrUntaggedVariantNameHint::Array
                        }
                        IrType::Inline(InlineIrType::Container(_, Container::Map(_)))
                        | IrType::Schema(SchemaIrType::Container(_, Container::Map(_))) => {
                            IrUntaggedVariantNameHint::Map
                        }
                        _ => IrUntaggedVariantNameHint::Index(index),
                    };
                    IrUntaggedVariant::Some(hint, &*self.arena().alloc(ty))
                })
                .unwrap_or(IrUntaggedVariant::Null)
            })
            .collect_vec();

        Ok(match &*variants {
            [] => match self.name {
                IrTypeName::Schema(info) => SchemaIrType::Any(info).into(),
                IrTypeName::Inline(path) => InlineIrType::Any(path).into(),
            },

            // Unwrap single-variant untagged unions.
            [IrUntaggedVariant::Null] => match self.name {
                IrTypeName::Schema(info) => SchemaIrType::Any(info).into(),
                IrTypeName::Inline(path) => InlineIrType::Any(path).into(),
            },
            [IrUntaggedVariant::Some(_, ty)] => **ty,

            // Simplify two-variant untagged unions, where one is a type
            // and the other is `null`, into optionals.
            [IrUntaggedVariant::Some(_, ty), IrUntaggedVariant::Null]
            | [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                let container = Container::Optional(Inner {
                    description: self.schema.description.as_deref(),
                    ty: *ty,
                });
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(info, container).into(),
                    IrTypeName::Inline(path) => InlineIrType::Container(path, container).into(),
                }
            }

            variants => {
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(variants),
                };
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Untagged(info, untagged).into(),
                    IrTypeName::Inline(path) => InlineIrType::Untagged(path, untagged).into(),
                }
            }
        })
    }

    fn try_any_of(self) -> Result<IrType<'a>, Self> {
        let Some(any_of) = &self.schema.any_of else {
            return Err(self);
        };
        if let [schema] = &**any_of {
            // A single-variant `anyOf` should unwrap to the variant type. This
            // preserves type references that would otherwise become `Any`.
            return Ok(match schema {
                RefOrSchema::Ref(r) => IrType::Ref(&r.path),
                RefOrSchema::Other(schema) => {
                    let path = match self.name {
                        IrTypeName::Schema(info) => InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: &[],
                        },
                        IrTypeName::Inline(path) => path,
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
                        let name = IrStructFieldName::Name(r.path.name());
                        let ty: &_ = self.arena().alloc(IrType::Ref(&r.path));
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
                        let name = IrStructFieldName::Hint(IrStructFieldNameHint::Index(index + 1));
                        let segment = InlineIrTypePathSegment::Field(name);
                        let path = match self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                        };
                        let ty: &_ =
                            self.arena()
                                .alloc(transform_with_context(self.context, path, schema));
                        let desc = schema.description.as_deref();
                        (name, ty, desc)
                    }
                };
                // Flattened `anyOf` fields are always optional.
                let segment = InlineIrTypePathSegment::Field(field_name);
                let path = match self.name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: self.arena().alloc_slice_copy(&[segment]),
                    },
                    IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                };
                let ty: &_ = self.arena().alloc(IrType::from(InlineIrType::Container(
                    path,
                    Container::Optional(Inner { description, ty }),
                )));
                IrStructField {
                    name: field_name,
                    ty,
                    required: false,
                    description,
                    flattened: true,
                }
            })
            .collect_vec();

        let ty = IrStruct {
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
            IrTypeName::Schema(info) => SchemaIrType::Struct(info, ty).into(),
            IrTypeName::Inline(path) => InlineIrType::Struct(path, ty).into(),
        })
    }

    fn try_enum(self) -> Result<IrType<'a>, Self> {
        let Some(values) = &self.schema.variants else {
            return Err(self);
        };
        let variants = self.arena().alloc_slice(values.iter().filter_map(|value| {
            if let Some(s) = value.as_str() {
                Some(IrEnumVariant::String(s))
            } else if let Some(n) = value.as_number() {
                Some(IrEnumVariant::Number(n.clone()))
            } else {
                value.as_bool().map(IrEnumVariant::Bool)
            }
        }));
        let ty = IrEnum {
            description: self.schema.description.as_deref(),
            variants,
        };
        Ok(match self.name {
            IrTypeName::Schema(info) => SchemaIrType::Enum(info, ty).into(),
            IrTypeName::Inline(path) => InlineIrType::Enum(path, ty).into(),
        })
    }

    fn try_struct(self) -> Result<IrType<'a>, Self> {
        if self.schema.properties.is_none() && self.schema.all_of.is_none() {
            return Err(self);
        }

        let ty = IrStruct {
            description: self.schema.description.as_deref(),
            fields: self.arena().alloc_slice(itertools::chain!(
                self.properties(),
                self.additional_properties()
            )),
            parents: self.arena().alloc_slice(self.parents()),
        };
        Ok(match self.name {
            IrTypeName::Schema(info) => SchemaIrType::Struct(info, ty).into(),
            IrTypeName::Inline(path) => InlineIrType::Struct(path, ty).into(),
        })
    }

    fn other(self) -> IrType<'a> {
        let mut other = Other {
            variants: Vec::with_capacity(self.schema.ty.len()),
            nullable: false,
        };

        for ty in &self.schema.ty {
            let variant = match (ty, self.schema.format) {
                (Ty::String, Some(Format::DateTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::DateTime)
                }
                (Ty::String, Some(Format::Date)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::Date)
                }
                (Ty::String, Some(Format::Uri)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::Url)
                }
                (Ty::String, Some(Format::Uuid)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::Uuid)
                }
                (Ty::String, Some(Format::Byte)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::Bytes)
                }
                (Ty::String, Some(Format::Binary)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::Binary)
                }
                (Ty::String, _) => OtherVariant::Primitive(&self.name, PrimitiveIrType::String),

                (Ty::Integer, Some(Format::Int8)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::I8)
                }
                (Ty::Integer, Some(Format::UInt8)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::U8)
                }
                (Ty::Integer, Some(Format::Int16)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::I16)
                }
                (Ty::Integer, Some(Format::UInt16)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::U16)
                }
                (Ty::Integer, Some(Format::Int32)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::I32)
                }
                (Ty::Integer, Some(Format::UInt32)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::U32)
                }
                (Ty::Integer, Some(Format::Int64)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::I64)
                }
                (Ty::Integer, Some(Format::UInt64)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::U64)
                }
                (Ty::Integer, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::UnixTime)
                }
                (Ty::Integer, _) => OtherVariant::Primitive(&self.name, PrimitiveIrType::I32),

                (Ty::Number, Some(Format::Float)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::F32)
                }
                (Ty::Number, Some(Format::Double)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::F64)
                }
                (Ty::Number, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(&self.name, PrimitiveIrType::UnixTime)
                }
                (Ty::Number, _) => OtherVariant::Primitive(&self.name, PrimitiveIrType::F64),

                (Ty::Boolean, _) => OtherVariant::Primitive(&self.name, PrimitiveIrType::Bool),

                (Ty::Array, _) => {
                    let items = match &self.schema.items {
                        Some(RefOrSchema::Ref(r)) => IrType::Ref(&r.path),
                        Some(RefOrSchema::Other(schema)) => {
                            let segment = InlineIrTypePathSegment::ArrayItem;
                            let path = match self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            transform_with_context(self.context, path, schema)
                        }
                        None => {
                            let segment = InlineIrTypePathSegment::ArrayItem;
                            let path = match self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            InlineIrType::Any(path).into()
                        }
                    };
                    OtherVariant::Array(
                        &self.name,
                        Inner {
                            description: self.schema.description.as_deref(),
                            ty: self.arena().alloc(items),
                        },
                    )
                }

                (Ty::Object, _) => {
                    let inner = match &self.schema.additional_properties {
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => {
                            Some(Inner {
                                description: self.schema.description.as_deref(),
                                ty: &*self.arena().alloc(IrType::Ref(&r.path)),
                            })
                        }
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                            let segment = InlineIrTypePathSegment::MapValue;
                            let path = match self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            Some(Inner {
                                description: self.schema.description.as_deref(),
                                ty: &*self.arena().alloc(transform_with_context(
                                    self.context,
                                    path,
                                    schema,
                                )),
                            })
                        }
                        Some(AdditionalProperties::Bool(true)) => {
                            let segment = InlineIrTypePathSegment::MapValue;
                            let path = match self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: self.arena().alloc_slice_copy(&[segment]),
                                },
                                IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                            };
                            Some(Inner {
                                description: self.schema.description.as_deref(),
                                ty: &*self.arena().alloc(IrType::Inline(InlineIrType::Any(path))),
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
                IrTypeName::Schema(info) => SchemaIrType::Any(info).into(),
                IrTypeName::Inline(path) => InlineIrType::Any(path).into(),
            },

            // A `null` variant becomes `Any`.
            ([], true) => match self.name {
                IrTypeName::Schema(info) => SchemaIrType::Any(info).into(),
                IrTypeName::Inline(path) => InlineIrType::Any(path).into(),
            },

            // A union with a single, non-`null` variant unwraps to
            // the type of that variant.
            ([variant], false) => variant.to_type(),

            // A two-variant union, with one type T and one `null` variant,
            // simplifies to `Optional(T)`.
            ([variant], true) => {
                let container = Container::Optional(Inner {
                    description: self.schema.description.as_deref(),
                    ty: &*self.arena().alloc(variant.to_inline_type()),
                });
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(info, container).into(),
                    IrTypeName::Inline(path) => InlineIrType::Container(path, container).into(),
                }
            }

            // Anything else becomes an untagged union.
            (many, nullable) => {
                let mut variants = many
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| (index + 1, variant))
                    .map(|(index, variant)| {
                        IrUntaggedVariant::Some(
                            variant
                                .hint()
                                .unwrap_or(IrUntaggedVariantNameHint::Index(index)),
                            &*self.arena().alloc(variant.to_type()),
                        )
                    })
                    .collect_vec();
                if nullable {
                    variants.push(IrUntaggedVariant::Null);
                }
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants: self.arena().alloc_slice_copy(&variants),
                };
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Untagged(info, untagged).into(),
                    IrTypeName::Inline(path) => InlineIrType::Untagged(path, untagged).into(),
                }
            }
        }
    }

    // MARK: Shared lowering

    /// Lowers immediate parents from `allOf` into a list of types.
    fn parents(&self) -> impl Iterator<Item = &'a IrType<'a>> {
        self.schema
            .all_of
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, parent)| (index + 1, parent))
            .map(move |(index, parent)| match parent {
                RefOrSchema::Ref(r) => &*self.arena().alloc(IrType::Ref(&r.path)),
                RefOrSchema::Other(schema) => {
                    let segment = InlineIrTypePathSegment::Parent(index);
                    let path = match self.name {
                        IrTypeName::Schema(info) => InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: self.arena().alloc_slice_copy(&[segment]),
                        },
                        IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                    };
                    &*self
                        .arena()
                        .alloc(transform_with_context(self.context, path, schema))
                }
            })
    }

    /// Lowers regular fields from `properties` into struct fields,
    /// wrapping nullable and optional fields in [`Container::Optional`].
    fn properties(&self) -> impl Iterator<Item = IrStructField<'a>> {
        self.schema
            .properties
            .iter()
            .flatten()
            .map(move |(name, field_schema)| {
                let field_name = name.as_str();
                let required = self.schema.required.contains(name);
                let ty: &_ = match field_schema {
                    RefOrSchema::Ref(r) => &*self.arena().alloc(IrType::Ref(&r.path)),
                    RefOrSchema::Other(schema) => {
                        let segment =
                            InlineIrTypePathSegment::Field(IrStructFieldName::Name(field_name));
                        let path = match self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: self.arena().alloc_slice_copy(&[segment]),
                            },
                            IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
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
                    let segment =
                        InlineIrTypePathSegment::Field(IrStructFieldName::Name(field_name));
                    let path = match self.name {
                        IrTypeName::Schema(info) => InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: self.arena().alloc_slice_copy(&[segment]),
                        },
                        IrTypeName::Inline(path) => path.join(self.arena(), &[segment]),
                    };
                    self.arena().alloc(IrType::from(InlineIrType::Container(
                        path,
                        Container::Optional(Inner { description, ty }),
                    )))
                } else {
                    ty
                };
                IrStructField {
                    name: IrStructFieldName::Name(field_name),
                    ty,
                    required,
                    description,
                    flattened: false,
                }
            })
    }

    /// Lowers `additionalProperties` into a struct field definition,
    /// if the schema specifies them.
    fn additional_properties(&self) -> Option<IrStructField<'a>> {
        let name = IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties);
        let path = match self.name {
            IrTypeName::Schema(info) => InlineIrTypePath {
                root: InlineIrTypePathRoot::Type(info.name),
                segments: self
                    .arena()
                    .alloc_slice_copy(&[InlineIrTypePathSegment::Field(name)]),
            },
            IrTypeName::Inline(path) => {
                path.join(self.arena(), &[InlineIrTypePathSegment::Field(name)])
            }
        };

        let inner = match &self.schema.additional_properties {
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => Inner {
                description: self.schema.description.as_deref(),
                ty: &*self.arena().alloc(IrType::Ref(&r.path)),
            },
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                let path = path.join(self.arena(), &[InlineIrTypePathSegment::MapValue]);
                Inner {
                    description: self.schema.description.as_deref(),
                    ty: &*self
                        .arena()
                        .alloc(transform_with_context(self.context, path, schema)),
                }
            }
            Some(AdditionalProperties::Bool(true)) => {
                let path = path.join(self.arena(), &[InlineIrTypePathSegment::MapValue]);
                Inner {
                    description: self.schema.description.as_deref(),
                    ty: &*self.arena().alloc(IrType::Inline(InlineIrType::Any(path))),
                }
            }
            _ => return None,
        };

        let ty: &_ = self.arena().alloc(IrType::from(InlineIrType::Container(
            path,
            Container::Map(inner),
        )));

        Some(IrStructField {
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
    Primitive(&'name IrTypeName<'a>, PrimitiveIrType),
    Array(&'name IrTypeName<'a>, Inner<'a>),
    Map(&'name IrTypeName<'a>, Inner<'a>),
    Any(&'name IrTypeName<'a>),
}

impl<'name, 'a> OtherVariant<'name, 'a> {
    /// Returns the name hint for this variant when used in an untagged union.
    fn hint(&self) -> Option<IrUntaggedVariantNameHint> {
        Some(match self {
            &Self::Primitive(_, p) => IrUntaggedVariantNameHint::Primitive(p),
            Self::Array(..) => IrUntaggedVariantNameHint::Array,
            Self::Map(..) => IrUntaggedVariantNameHint::Map,
            Self::Any(_) => return None,
        })
    }

    /// Converts this variant to an [`IrType`].
    ///
    /// This is used to unwrap variants for the single-variant
    /// and untagged union cases.
    fn to_type(&self) -> IrType<'a> {
        match self {
            &Self::Primitive(name, p) => match name {
                IrTypeName::Schema(info) => SchemaIrType::Primitive(*info, p).into(),
                IrTypeName::Inline(path) => InlineIrType::Primitive(*path, p).into(),
            },
            Self::Array(name, inner) => {
                let container = Container::Array(*inner);
                match name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(*info, container).into(),
                    IrTypeName::Inline(path) => InlineIrType::Container(*path, container).into(),
                }
            }
            Self::Map(name, inner) => {
                let container = Container::Map(*inner);
                match name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(*info, container).into(),
                    IrTypeName::Inline(path) => InlineIrType::Container(*path, container).into(),
                }
            }
            Self::Any(name) => match name {
                IrTypeName::Schema(info) => SchemaIrType::Any(*info).into(),
                IrTypeName::Inline(path) => InlineIrType::Any(*path).into(),
            },
        }
    }

    /// Converts this variant to an inline [`IrType`].
    ///
    /// This is used to rewrite `[T, null]` unions as `Optional(T)`.
    fn to_inline_type(&self) -> IrType<'a> {
        match self {
            &Self::Primitive(name, p) => {
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &IrTypeName::Inline(path) => path,
                };
                InlineIrType::Primitive(path, p).into()
            }
            Self::Array(name, inner) => {
                let container = Container::Array(*inner);
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&IrTypeName::Inline(path) => path,
                };
                InlineIrType::Container(path, container).into()
            }
            Self::Map(name, inner) => {
                let container = Container::Map(*inner);
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&IrTypeName::Inline(path) => path,
                };
                InlineIrType::Container(path, container).into()
            }
            Self::Any(name) => {
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: &[],
                    },
                    &&IrTypeName::Inline(path) => path,
                };
                InlineIrType::Any(path).into()
            }
        }
    }
}
