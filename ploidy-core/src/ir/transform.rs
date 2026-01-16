use itertools::Itertools;
use ploidy_pointer::JsonPointee;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::parse::{AdditionalProperties, ComponentRef, Document, Format, RefOrSchema, Schema, Ty};

use super::{
    fields::{IrSchemaField, all_fields},
    types::{
        InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrEnum,
        IrEnumVariant, IrStruct, IrStructField, IrStructFieldName, IrStructFieldNameHint, IrTagged,
        IrTaggedVariant, IrType, IrTypeName, IrUntagged, IrUntaggedVariant,
        IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
    },
};

#[inline]
pub fn transform<'a>(
    doc: &'a Document,
    name: impl Into<IrTypeName<'a>>,
    schema: &'a Schema,
) -> IrType<'a> {
    let context = TransformContext::new(doc);
    transform_with_context(&context, name, schema)
}

/// Context for the [`IrTransformer`].
#[derive(Debug)]
pub struct TransformContext<'a> {
    /// The document being transformed.
    pub doc: &'a Document,

    /// The set of schema references to skip when traversing `allOf` references.
    /// These are already being processed by a transformation further up the stack,
    /// and should be skipped to avoid infinite recursion.
    pub skip_refs: FxHashSet<&'a ComponentRef>,
}

impl<'a> TransformContext<'a> {
    /// Creates a new context for the given document.
    pub fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            skip_refs: FxHashSet::default(),
        }
    }

    /// Creates a new context with the same document, and
    /// additional schema references to skip.
    pub fn with_followed(&self, followed: &FxHashSet<&'a ComponentRef>) -> Self {
        Self {
            doc: self.doc,
            skip_refs: self.skip_refs.union(followed).copied().collect(),
        }
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
        let inverted: FxHashMap<_, Vec<_>> = discriminator.mapping.iter().fold(
            FxHashMap::default(),
            |mut mapping, (tag, reference)| {
                mapping.entry(reference).or_default().push(tag.as_str());
                mapping
            },
        );
        let variants = one_of
            .iter()
            .filter_map(|schema| match schema {
                RefOrSchema::Ref(r) => {
                    let aliases = inverted.get(&r.path).cloned().unwrap_or_default();
                    Some(IrTaggedVariant {
                        name: r.path.name(),
                        ty: IrType::Ref(&r.path),
                        aliases,
                    })
                }
                RefOrSchema::Other(_) => None,
            })
            .filter(|v| !v.aliases.is_empty())
            .collect();
        let tagged = IrTagged {
            description: self.schema.description.as_deref(),
            tag: discriminator.property_name.as_str(),
            variants,
        };
        Ok(match self.name {
            IrTypeName::Schema(name) => SchemaIrType::Tagged(name, tagged).into(),
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
                            IrTypeName::Schema(name) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(name),
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
                        IrType::Array(_) => IrUntaggedVariantNameHint::Array,
                        IrType::Map(_) => IrUntaggedVariantNameHint::Map,
                        _ => IrUntaggedVariantNameHint::Index(index),
                    };
                    IrUntaggedVariant::Some(hint, ty)
                })
                .unwrap_or(IrUntaggedVariant::Null)
            })
            .collect_vec();
        Ok(match &*variants {
            [] => IrType::Any,
            [IrUntaggedVariant::Null] => IrType::Any,
            [IrUntaggedVariant::Some(_, ty)] => ty.clone(),
            [IrUntaggedVariant::Some(_, ty), IrUntaggedVariant::Null] => {
                IrType::Optional(ty.clone().into())
            }
            [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                IrType::Optional(ty.clone().into())
            }
            _ => {
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants,
                };
                match self.name {
                    IrTypeName::Schema(name) => SchemaIrType::Untagged(name, untagged).into(),
                    IrTypeName::Inline(path) => InlineIrType::Untagged(path, untagged).into(),
                }
            }
        })
    }

    fn try_any_of(self) -> Result<IrType<'a>, Self> {
        let Some(any_of) = &self.schema.any_of else {
            return Err(self);
        };
        if any_of.len() == 1 {
            // A single-variant `anyOf` should unwrap to the variant type.
            return Err(self);
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
                            IrTypeName::Schema(n) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(n),
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
                let ty = IrType::Optional(ty.into());
                IrStructField {
                    name: field_name,
                    ty,
                    required: false,
                    description,
                    inherited: false,
                    discriminator: false,
                    flattened: true,
                }
            })
            .collect_vec();

        // Collect inherited and own fields from `allOf` and `properties`,
        // if present.
        let (inner_context, field_infos) = all_fields(self.context, self.schema);

        let regular_fields = field_infos
            .into_iter()
            .map(|(field_name, field)| {
                let info = field.info();
                let ty = match info.schema {
                    RefOrSchema::Ref(r) => IrType::Ref(&r.path),
                    RefOrSchema::Other(schema) => {
                        let path = match &self.name {
                            IrTypeName::Schema(name) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(name),
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
                        transform_with_context(&inner_context, path, schema)
                    }
                };
                let description = match info.schema {
                    RefOrSchema::Other(schema) => schema.description.as_deref(),
                    RefOrSchema::Ref(r) => self
                        .context
                        .doc
                        .resolve(r.path.pointer().clone())
                        .ok()
                        .and_then(|p| p.downcast_ref::<Schema>())
                        .and_then(|schema| schema.description.as_deref()),
                };
                let nullable = match info.schema {
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
                let ty = if nullable || !info.required {
                    IrType::Optional(ty.into())
                } else {
                    ty
                };
                IrStructField {
                    name: IrStructFieldName::Name(field_name),
                    ty,
                    required: info.required,
                    description,
                    inherited: matches!(field, IrSchemaField::Inherited(_)),
                    discriminator: info.discriminator,
                    flattened: info.flattened,
                }
            })
            .collect_vec();

        // Combine all the fields: regular properties first,
        // followed by the flattened `anyOf` fields. This ordering
        // ensures that regular properties take precedence during
        // (de)serialization.
        let all_fields = itertools::chain!(regular_fields, any_of_fields).collect();

        let ty = IrStruct {
            description: self.schema.description.as_deref(),
            fields: all_fields,
        };

        Ok(match self.name {
            IrTypeName::Schema(name) => SchemaIrType::Struct(name, ty).into(),
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
            IrTypeName::Schema(name) => SchemaIrType::Enum(name, ty).into(),
            IrTypeName::Inline(path) => InlineIrType::Enum(path, ty).into(),
        })
    }

    fn try_struct(self) -> Result<IrType<'a>, Self> {
        if self
            .schema
            .additional_properties
            .as_ref()
            .is_some_and(|additional| {
                matches!(
                    additional,
                    AdditionalProperties::RefOrSchema(_) | AdditionalProperties::Bool(true)
                )
            })
        {
            return Err(self);
        }
        if self.schema.properties.is_none() && self.schema.all_of.is_none() {
            return Err(self);
        }

        let (inner_context, field_infos) = all_fields(self.context, self.schema);

        let fields = field_infos
            .into_iter()
            .map(|(field_name, field)| {
                let info = field.info();
                let ty = match info.schema {
                    RefOrSchema::Ref(reference) => IrType::Ref(&reference.path),
                    RefOrSchema::Other(schema) => {
                        let path = match &self.name {
                            IrTypeName::Schema(name) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(name),
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
                        transform_with_context(&inner_context, path, schema)
                    }
                };
                let description = match info.schema {
                    RefOrSchema::Other(schema) => schema.description.as_deref(),
                    RefOrSchema::Ref(r) => self
                        .context
                        .doc
                        .resolve(r.path.pointer().clone())
                        .ok()
                        .and_then(|p| p.downcast_ref::<Schema>())
                        .and_then(|schema| schema.description.as_deref()),
                };
                let nullable = match info.schema {
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
                let ty = if nullable || !info.required {
                    IrType::Optional(ty.into())
                } else {
                    ty
                };
                IrStructField {
                    name: IrStructFieldName::Name(field_name),
                    ty,
                    required: info.required,
                    description,
                    inherited: matches!(field, IrSchemaField::Inherited(_)),
                    discriminator: info.discriminator,
                    flattened: false,
                }
            })
            .collect_vec();
        let ty = IrStruct {
            description: self.schema.description.as_deref(),
            fields,
        };
        Ok(match self.name {
            IrTypeName::Schema(name) => SchemaIrType::Struct(name, ty).into(),
            IrTypeName::Inline(path) => InlineIrType::Struct(path, ty).into(),
        })
    }

    fn other(self) -> IrType<'a> {
        let variants = self
            .schema
            .ty
            .iter()
            .map(|&ty| match (ty, self.schema.format) {
                (Ty::String, Some(Format::DateTime)) => PrimitiveIrType::DateTime.into(),
                (Ty::String, Some(Format::Date)) => PrimitiveIrType::Date.into(),
                (Ty::String, Some(Format::Uri)) => PrimitiveIrType::Url.into(),
                (Ty::String, Some(Format::Uuid)) => PrimitiveIrType::Uuid.into(),
                (Ty::String, Some(Format::Byte) | Some(Format::Binary)) => {
                    PrimitiveIrType::Bytes.into()
                }
                (Ty::String, _) => PrimitiveIrType::String.into(),
                (Ty::Integer, Some(Format::Int64)) => PrimitiveIrType::I64.into(),
                (Ty::Integer, Some(Format::UnixTime)) => PrimitiveIrType::UnixTime.into(),
                (Ty::Integer, Some(Format::Int32) | _) => PrimitiveIrType::I32.into(),
                (Ty::Number, Some(Format::Float)) => PrimitiveIrType::F32.into(),
                (Ty::Number, Some(Format::UnixTime)) => PrimitiveIrType::UnixTime.into(),
                (Ty::Number, Some(Format::Double) | _) => PrimitiveIrType::F64.into(),
                (Ty::Boolean, _) => PrimitiveIrType::Bool.into(),
                (Ty::Array, _) => {
                    let items = match &self.schema.items {
                        Some(RefOrSchema::Ref(r)) => IrType::Ref(&r.path),
                        Some(RefOrSchema::Other(schema)) => {
                            let path = match &self.name {
                                IrTypeName::Schema(name) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(name),
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
                    let ty = IrType::Array(items.into());
                    IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Array, ty)
                }
                (Ty::Object, _) => {
                    let ty = match &self.schema.additional_properties {
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Ref(r))) => {
                            IrType::Map(IrType::Ref(&r.path).into())
                        }
                        Some(AdditionalProperties::RefOrSchema(RefOrSchema::Other(schema))) => {
                            let path = match &self.name {
                                IrTypeName::Schema(name) => InlineIrTypePath {
                                    root: InlineIrTypePathRoot::Type(name),
                                    segments: vec![InlineIrTypePathSegment::MapValue],
                                },
                                IrTypeName::Inline(path) => {
                                    let mut path = path.clone();
                                    path.segments.push(InlineIrTypePathSegment::MapValue);
                                    path
                                }
                            };
                            IrType::Map(transform_with_context(self.context, path, schema).into())
                        }
                        Some(AdditionalProperties::Bool(true)) => IrType::Map(IrType::Any.into()),
                        _ => IrType::Any,
                    };
                    IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Map, ty)
                }
                (Ty::Null, _) => IrUntaggedVariant::Null,
            })
            .collect_vec();

        match &*variants {
            [] => IrType::Any,
            [IrUntaggedVariant::Null] => IrType::Any,
            [IrUntaggedVariant::Some(_, ty)] => ty.clone(),
            [IrUntaggedVariant::Some(_, ty), IrUntaggedVariant::Null] => {
                IrType::Optional(ty.clone().into())
            }
            [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                IrType::Optional(ty.clone().into())
            }
            _ => {
                let untagged = IrUntagged {
                    description: self.schema.description.as_deref(),
                    variants,
                };
                match self.name {
                    IrTypeName::Schema(name) => SchemaIrType::Untagged(name, untagged).into(),
                    IrTypeName::Inline(path) => InlineIrType::Untagged(path, untagged).into(),
                }
            }
        }
    }
}
