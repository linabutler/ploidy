use itertools::Itertools;
use ploidy_pointer::JsonPointee;
use rustc_hash::FxHashMap;

use crate::parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty};

use super::types::{
    Container, InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment,
    Inner, IrEnum, IrEnumVariant, IrStruct, IrStructField, IrStructFieldName,
    IrStructFieldNameHint, IrTagged, IrTaggedVariant, IrType, IrTypeName, IrUntagged,
    IrUntaggedVariant, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
};

#[inline]
pub fn transform<'a>(
    doc: &'a Document,
    name: impl Into<IrTypeName<'a>>,
    schema: &'a Schema,
) -> IrType<'a> {
    let context = TransformContext::new(doc);
    transform_with_context(&context, name.into(), schema)
}

/// Context for the [`IrTransformer`].
#[derive(Debug)]
pub struct TransformContext<'a> {
    /// The document being transformed.
    pub doc: &'a Document,
}

impl<'a> TransformContext<'a> {
    /// Creates a new context for the given document.
    pub fn new(doc: &'a Document) -> Self {
        Self { doc }
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
                            ty: IrType::Ref(&r.path),
                            aliases: aliases.to_vec(),
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
            variants,
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
                        let path = match &self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: vec![InlineIrTypePathSegment::Variant(index)],
                            },
                            IrTypeName::Inline(path) => {
                                let mut path = path.clone();
                                path.segments.push(InlineIrTypePathSegment::Variant(index));
                                path
                            }
                        };
                        Some(transform_with_context(self.context, path, schema))
                    }
                };
                ty.map(|ty| {
                    let hint = match &ty {
                        &IrType::Primitive(ty) => IrUntaggedVariantNameHint::Primitive(ty),
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
                    IrUntaggedVariant::Some(hint, ty)
                })
                .unwrap_or(IrUntaggedVariant::Null)
            })
            .collect_vec();

        Ok(match &*variants {
            [] => IrType::Any,

            // Unwrap single-variant untagged unions.
            [IrUntaggedVariant::Null] => IrType::Any,
            [IrUntaggedVariant::Some(_, ty)] => ty.clone(),

            // Simplify two-variant untagged unions, where one is a type
            // and the other is `null`, into optionals.
            [IrUntaggedVariant::Some(_, ty), IrUntaggedVariant::Null]
            | [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                let container = Container::Optional(Inner {
                    description: self.schema.description.as_deref(),
                    ty: ty.clone().into(),
                });
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(info, container).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Container(path.clone(), container).into()
                    }
                }
            }

            _ => {
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants,
                };
                match &self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Untagged(*info, untagged).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Untagged(path.clone(), untagged).into()
                    }
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
                            segments: vec![],
                        },
                        IrTypeName::Inline(path) => path.clone(),
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
                        let ty = IrType::Ref(&r.path);
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
                        let path = match &self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: vec![InlineIrTypePathSegment::Field(name)],
                            },
                            IrTypeName::Inline(path) => {
                                let mut path = path.clone();
                                path.segments.push(InlineIrTypePathSegment::Field(name));
                                path
                            }
                        };
                        let ty = transform_with_context(self.context, path, schema);
                        let desc = schema.description.as_deref();
                        (name, ty, desc)
                    }
                };
                // Flattened `anyOf` fields are always optional.
                let path = match &self.name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: vec![InlineIrTypePathSegment::Field(field_name)],
                    },
                    IrTypeName::Inline(path) => {
                        let mut path = path.clone();
                        path.segments
                            .push(InlineIrTypePathSegment::Field(field_name));
                        path
                    }
                };
                let ty = InlineIrType::Container(
                    path,
                    Container::Optional(Inner {
                        description,
                        ty: ty.into(),
                    }),
                )
                .into();
                IrStructField {
                    name: field_name,
                    ty,
                    required: false,
                    description,
                    flattened: true,
                }
            })
            .collect_vec();

        let parents = self.parents().collect();
        let regular_fields = self.properties();

        // Combine all the fields: regular properties first,
        // followed by the flattened `anyOf` fields. This ordering
        // ensures that regular properties take precedence during
        // (de)serialization.
        let all_fields = itertools::chain!(regular_fields, any_of_fields).collect();

        let ty = IrStruct {
            description: self.schema.description.as_deref(),
            fields: all_fields,
            parents,
            discriminator: self
                .schema
                .discriminator
                .as_ref()
                .map(|d| d.property_name.as_str()),
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
        let variants = values
            .iter()
            .filter_map(|value| {
                if let Some(s) = value.as_str() {
                    Some(IrEnumVariant::String(s))
                } else if let Some(n) = value.as_number() {
                    Some(IrEnumVariant::Number(n.clone()))
                } else {
                    value.as_bool().map(IrEnumVariant::Bool)
                }
            })
            .collect();
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

        let parents = self.parents().collect();
        let fields = itertools::chain!(self.properties(), self.additional_properties()).collect();
        let ty = IrStruct {
            description: self.schema.description.as_deref(),
            fields,
            parents,
            discriminator: self
                .schema
                .discriminator
                .as_ref()
                .map(|d| d.property_name.as_str()),
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
                    OtherVariant::Primitive(PrimitiveIrType::DateTime)
                }
                (Ty::String, Some(Format::Date)) => OtherVariant::Primitive(PrimitiveIrType::Date),
                (Ty::String, Some(Format::Uri)) => OtherVariant::Primitive(PrimitiveIrType::Url),
                (Ty::String, Some(Format::Uuid)) => OtherVariant::Primitive(PrimitiveIrType::Uuid),
                (Ty::String, Some(Format::Byte)) => OtherVariant::Primitive(PrimitiveIrType::Bytes),
                (Ty::String, Some(Format::Binary)) => {
                    OtherVariant::Primitive(PrimitiveIrType::Binary)
                }
                (Ty::String, _) => OtherVariant::Primitive(PrimitiveIrType::String),

                (Ty::Integer, Some(Format::Int8)) => OtherVariant::Primitive(PrimitiveIrType::I8),
                (Ty::Integer, Some(Format::UInt8)) => OtherVariant::Primitive(PrimitiveIrType::U8),
                (Ty::Integer, Some(Format::Int16)) => OtherVariant::Primitive(PrimitiveIrType::I16),
                (Ty::Integer, Some(Format::UInt16)) => {
                    OtherVariant::Primitive(PrimitiveIrType::U16)
                }
                (Ty::Integer, Some(Format::Int32)) => OtherVariant::Primitive(PrimitiveIrType::I32),
                (Ty::Integer, Some(Format::UInt32)) => {
                    OtherVariant::Primitive(PrimitiveIrType::U32)
                }
                (Ty::Integer, Some(Format::Int64)) => OtherVariant::Primitive(PrimitiveIrType::I64),
                (Ty::Integer, Some(Format::UInt64)) => {
                    OtherVariant::Primitive(PrimitiveIrType::U64)
                }
                (Ty::Integer, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(PrimitiveIrType::UnixTime)
                }
                (Ty::Integer, _) => OtherVariant::Primitive(PrimitiveIrType::I32),

                (Ty::Number, Some(Format::Float)) => OtherVariant::Primitive(PrimitiveIrType::F32),
                (Ty::Number, Some(Format::Double)) => OtherVariant::Primitive(PrimitiveIrType::F64),
                (Ty::Number, Some(Format::UnixTime)) => {
                    OtherVariant::Primitive(PrimitiveIrType::UnixTime)
                }
                (Ty::Number, _) => OtherVariant::Primitive(PrimitiveIrType::F64),

                (Ty::Boolean, _) => OtherVariant::Primitive(PrimitiveIrType::Bool),

                (Ty::Array, _) => {
                    let items = match &self.schema.items {
                        Some(RefOrSchema::Ref(r)) => IrType::Ref(&r.path),
                        Some(RefOrSchema::Other(schema)) => {
                            let path = match &self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: vec![InlineIrTypePathSegment::ArrayItem],
                                },
                                IrTypeName::Inline(path) => {
                                    let mut path = path.clone();
                                    path.segments.push(InlineIrTypePathSegment::ArrayItem);
                                    path
                                }
                            };
                            transform_with_context(self.context, path, schema)
                        }
                        None => IrType::Any,
                    };
                    OtherVariant::Array(
                        &self.name,
                        Inner {
                            description: self.schema.description.as_deref(),
                            ty: items.into(),
                        },
                    )
                }

                (Ty::Object, _) => {
                    let inner = match &self.schema.additional_properties {
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => {
                            Some(Inner {
                                description: self.schema.description.as_deref(),
                                ty: IrType::Ref(&r.path).into(),
                            })
                        }
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                            let path = match &self.name {
                                IrTypeName::Schema(info) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(info.name),
                                    segments: vec![InlineIrTypePathSegment::MapValue],
                                },
                                IrTypeName::Inline(path) => {
                                    let mut path = path.clone();
                                    path.segments.push(InlineIrTypePathSegment::MapValue);
                                    path
                                }
                            };
                            Some(Inner {
                                description: self.schema.description.as_deref(),
                                ty: transform_with_context(self.context, path, schema).into(),
                            })
                        }
                        Some(AdditionalProperties::Bool(true)) => Some(Inner {
                            description: self.schema.description.as_deref(),
                            ty: IrType::Any.into(),
                        }),
                        _ => None,
                    };
                    match inner {
                        Some(inner) => OtherVariant::Map(&self.name, inner),
                        None => OtherVariant::Any,
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
            ([], false) => IrType::Any,

            // A `null` variant becomes `Any`.
            ([], true) => IrType::Any,

            // A union with a single, non-`null` variant unwraps to
            // the type of that variant.
            ([variant], false) => variant.to_type(),

            // A two-variant union, with one type T and one `null` variant,
            // simplifies to `Optional(T)`.
            ([variant], true) => {
                let container = Container::Optional(Inner {
                    description: self.schema.description.as_deref(),
                    ty: variant.to_inline_type().into(),
                });
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(info, container).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Container(path.clone(), container).into()
                    }
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
                            variant.to_type(),
                        )
                    })
                    .collect_vec();
                if nullable {
                    variants.push(IrUntaggedVariant::Null);
                }
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants,
                };
                match self.name {
                    IrTypeName::Schema(info) => SchemaIrType::Untagged(info, untagged).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Untagged(path.clone(), untagged).into()
                    }
                }
            }
        }
    }

    // MARK: Shared lowering

    /// Lowers immediate parents from `allOf` into a list of types.
    fn parents(&self) -> impl Iterator<Item = IrType<'a>> {
        self.schema
            .all_of
            .iter()
            .flatten()
            .enumerate()
            .map(|(index, parent)| (index + 1, parent))
            .map(|(index, parent)| match parent {
                RefOrSchema::Ref(r) => IrType::Ref(&r.path),
                RefOrSchema::Other(schema) => {
                    let path = match &self.name {
                        IrTypeName::Schema(info) => InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: vec![InlineIrTypePathSegment::Parent(index)],
                        },
                        IrTypeName::Inline(path) => {
                            let mut path = path.clone();
                            path.segments.push(InlineIrTypePathSegment::Parent(index));
                            path
                        }
                    };
                    transform_with_context(self.context, path, schema)
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
            .map(|(name, field_schema)| {
                let field_name = name.as_str();
                let required = self.schema.required.contains(name);
                let ty = match field_schema {
                    RefOrSchema::Ref(r) => IrType::Ref(&r.path),
                    RefOrSchema::Other(schema) => {
                        let path = match &self.name {
                            IrTypeName::Schema(info) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(info.name),
                                segments: vec![InlineIrTypePathSegment::Field(
                                    IrStructFieldName::Name(field_name),
                                )],
                            },
                            IrTypeName::Inline(path) => {
                                let mut path = path.clone();
                                path.segments.push(InlineIrTypePathSegment::Field(
                                    IrStructFieldName::Name(field_name),
                                ));
                                path
                            }
                        };
                        transform_with_context(self.context, path, schema)
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
                let ty = if nullable || !required {
                    let path = match &self.name {
                        IrTypeName::Schema(info) => InlineIrTypePath {
                            root: InlineIrTypePathRoot::Type(info.name),
                            segments: vec![InlineIrTypePathSegment::Field(
                                IrStructFieldName::Name(field_name),
                            )],
                        },
                        IrTypeName::Inline(path) => {
                            let mut path = path.clone();
                            path.segments.push(InlineIrTypePathSegment::Field(
                                IrStructFieldName::Name(field_name),
                            ));
                            path
                        }
                    };
                    InlineIrType::Container(
                        path,
                        Container::Optional(Inner {
                            description,
                            ty: ty.into(),
                        }),
                    )
                    .into()
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
        let path = match &self.name {
            IrTypeName::Schema(info) => InlineIrTypePath {
                root: InlineIrTypePathRoot::Type(info.name),
                segments: vec![InlineIrTypePathSegment::Field(name)],
            },
            IrTypeName::Inline(path) => {
                let mut path = path.clone();
                path.segments.push(InlineIrTypePathSegment::Field(name));
                path
            }
        };

        let inner = match &self.schema.additional_properties {
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => Inner {
                description: self.schema.description.as_deref(),
                ty: IrType::Ref(&r.path).into(),
            },
            Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                let mut path = path.clone();
                path.segments.push(InlineIrTypePathSegment::MapValue);
                Inner {
                    description: self.schema.description.as_deref(),
                    ty: transform_with_context(self.context, path, schema).into(),
                }
            }
            Some(AdditionalProperties::Bool(true)) => Inner {
                description: self.schema.description.as_deref(),
                ty: IrType::Any.into(),
            },
            _ => return None,
        };

        let ty = InlineIrType::Container(path, Container::Map(inner));

        Some(IrStructField {
            name,
            ty: ty.into(),
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
    Primitive(PrimitiveIrType),
    Array(&'name IrTypeName<'a>, Inner<'a>),
    Map(&'name IrTypeName<'a>, Inner<'a>),
    Any,
}

impl<'name, 'a> OtherVariant<'name, 'a> {
    /// Returns the name hint for this variant when used in an untagged union.
    fn hint(&self) -> Option<IrUntaggedVariantNameHint> {
        Some(match self {
            &Self::Primitive(p) => IrUntaggedVariantNameHint::Primitive(p),
            Self::Array(..) => IrUntaggedVariantNameHint::Array,
            Self::Map(..) => IrUntaggedVariantNameHint::Map,
            Self::Any => return None,
        })
    }

    /// Converts this variant to an [`IrType`].
    ///
    /// This is used to unwrap variants for the single-variant
    /// and untagged union cases.
    fn to_type(&self) -> IrType<'a> {
        match self {
            &Self::Primitive(p) => IrType::Primitive(p),
            Self::Array(name, inner) => {
                let container = Container::Array(inner.clone());
                match name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(*info, container).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Container(path.clone(), container).into()
                    }
                }
            }
            Self::Map(name, inner) => {
                let container = Container::Map(inner.clone());
                match name {
                    IrTypeName::Schema(info) => SchemaIrType::Container(*info, container).into(),
                    IrTypeName::Inline(path) => {
                        InlineIrType::Container(path.clone(), container).into()
                    }
                }
            }
            Self::Any => IrType::Any,
        }
    }

    /// Converts this variant to an inline [`IrType`].
    ///
    /// This is used to rewrite `[T, null]` unions as `Optional(T)`.
    fn to_inline_type(&self) -> IrType<'a> {
        match self {
            &Self::Primitive(p) => IrType::Primitive(p),
            Self::Array(name, inner) => {
                let container = Container::Array(inner.clone());
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: vec![],
                    },
                    IrTypeName::Inline(path) => path.clone(),
                };
                InlineIrType::Container(path, container).into()
            }
            Self::Map(name, inner) => {
                let container = Container::Map(inner.clone());
                let path = match name {
                    IrTypeName::Schema(info) => InlineIrTypePath {
                        root: InlineIrTypePathRoot::Type(info.name),
                        segments: vec![],
                    },
                    IrTypeName::Inline(path) => path.clone(),
                };
                InlineIrType::Container(path, container).into()
            }
            Self::Any => IrType::Any,
        }
    }
}
