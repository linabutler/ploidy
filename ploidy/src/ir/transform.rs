use std::collections::BTreeMap;

use itertools::Itertools;
use ploidy_pointer::JsonPointee;

use crate::parse::{AdditionalProperties, Document, Format, RefOrSchema, Schema, Ty};

use super::{
    fields::{IrSchemaField, all_fields},
    types::{
        InlineIrType, InlineIrTypePath, InlineIrTypePathRoot, InlineIrTypePathSegment, IrEnum,
        IrEnumVariant, IrStruct, IrStructField, IrTagged, IrTaggedVariant, IrType, IrTypeName,
        IrUntagged, IrUntaggedVariant, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
    },
};

#[inline]
pub fn transform<'a>(
    doc: &'a Document,
    name: impl Into<IrTypeName<'a>>,
    schema: &'a Schema,
) -> IrType<'a> {
    IrTransformer::new(doc, name.into(), schema).transform()
}

#[derive(Debug)]
struct IrTransformer<'a> {
    doc: &'a Document,
    name: IrTypeName<'a>,
    schema: &'a Schema,
}

impl<'a> IrTransformer<'a> {
    fn new(doc: &'a Document, name: IrTypeName<'a>, schema: &'a Schema) -> Self {
        Self { doc, name, schema }
    }

    fn transform(self) -> IrType<'a> {
        self.try_tagged()
            .or_else(Self::try_untagged)
            .or_else(Self::try_enum)
            .or_else(Self::try_struct)
            .unwrap_or_else(Self::other)
    }

    fn try_tagged(self) -> Result<IrType<'a>, Self> {
        let (Some(one_of), Some(discriminator)) = (&self.schema.one_of, &self.schema.discriminator)
        else {
            return Err(self);
        };
        let inverted: BTreeMap<_, Vec<_>> =
            discriminator
                .mapping
                .iter()
                .fold(BTreeMap::new(), |mut mapping, (tag, reference)| {
                    mapping.entry(reference).or_default().push(tag.as_str());
                    mapping
                });
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
            IrTypeName::Inline(_path) => todo!(),
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
            .map(|(index, schema)| match schema {
                RefOrSchema::Ref(r) => IrUntaggedVariant::Some(
                    IrUntaggedVariantNameHint::Index(index),
                    IrType::Ref(&r.path),
                ),
                RefOrSchema::Other(s) if matches!(&*s.ty, [Ty::Null]) => IrUntaggedVariant::Null,
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
                    IrUntaggedVariant::Some(
                        IrUntaggedVariantNameHint::Index(index),
                        transform(self.doc, path, schema),
                    )
                }
            })
            .collect_vec();
        Ok(match &*variants {
            [] => IrType::Any,
            [IrUntaggedVariant::Null] => IrType::Any,
            [IrUntaggedVariant::Some(_, ty)] => ty.clone(),
            [IrUntaggedVariant::Some(_, ty), IrUntaggedVariant::Null] => {
                IrType::Nullable(ty.clone().into())
            }
            [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                IrType::Nullable(ty.clone().into())
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

    fn try_enum(self) -> Result<IrType<'a>, Self> {
        let Some(values) = &self.schema.variants else {
            return Err(self);
        };
        let variants = values
            .iter()
            .filter_map(|value| value.as_str())
            .map(IrEnumVariant::String)
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
        let all = all_fields(self.doc, self.schema);
        let fields = all
            .map(|(field_name, field)| {
                let info = field.info();
                let ty = match info.schema {
                    RefOrSchema::Ref(reference) => IrType::Ref(&reference.path),
                    RefOrSchema::Other(schema) => {
                        let path = match &self.name {
                            IrTypeName::Schema(name) => InlineIrTypePath {
                                root: InlineIrTypePathRoot::Type(name),
                                segments: vec![InlineIrTypePathSegment::Field(field_name)],
                            },
                            IrTypeName::Inline(path) => {
                                let mut path = path.clone();
                                path.segments
                                    .push(InlineIrTypePathSegment::Field(field_name));
                                path
                            }
                        };
                        transform(self.doc, path, schema)
                    }
                };
                let description = match info.schema {
                    RefOrSchema::Other(schema) => schema.description.as_deref(),
                    RefOrSchema::Ref(r) => self
                        .doc
                        .resolve(r.path.pointer().clone())
                        .ok()
                        .and_then(|p| p.downcast_ref::<Schema>())
                        .and_then(|schema| schema.description.as_deref()),
                };
                let nullable = match info.schema {
                    RefOrSchema::Other(schema) if schema.nullable => true,
                    RefOrSchema::Ref(r) => {
                        if let Ok(resolved) = self.doc.resolve(r.path.pointer().clone())
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
                let ty = if nullable {
                    IrType::Nullable(ty.into())
                } else {
                    ty
                };
                IrStructField {
                    name: field_name,
                    ty,
                    required: info.required,
                    description,
                    inherited: matches!(field, IrSchemaField::Inherited(_)),
                    discriminator: info.discriminator,
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
                (Ty::Integer, Some(Format::Int32) | _) => PrimitiveIrType::I32.into(),
                (Ty::Number, Some(Format::Float)) => PrimitiveIrType::F32.into(),
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
                            transform(self.doc, path, schema)
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
                            IrType::Map(transform(self.doc, path, schema).into())
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
                IrType::Nullable(ty.clone().into())
            }
            [IrUntaggedVariant::Null, IrUntaggedVariant::Some(_, ty)] => {
                IrType::Nullable(ty.clone().into())
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
