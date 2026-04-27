use std::{borrow::Cow, collections::BTreeSet, ops::Deref};

use ploidy_core::{
    ir::{
        CookedGraph, EnumVariant, EnumView, InlineTypeId, InlineTypeView, OperationId,
        SchemaTypeInfo, SchemaTypeView, StructFieldName, StructView, TaggedView, TypeInfo, View,
    },
    parse::ParameterLocation,
};
use rustc_hash::FxHashMap;

use crate::{CodegenResourceIdent, UniqueIdentBuf};

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{UniqueIdent, UniqueIdents},
};

/// A [`CookedGraph`] decorated with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a> {
    cooked: CookedGraph<'a>,
    idents: FxHashMap<IdentMappingKey<'a>, &'a UniqueIdent>,
    date_time_format: DateTimeFormat,
}

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    pub fn new(cooked: CookedGraph<'a>) -> Self {
        Self::with_config(cooked, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    pub fn with_config(cooked: CookedGraph<'a>, config: &CodegenConfig) -> Self {
        let mut idents = FxHashMap::default();
        idents.extend({
            let mut scope = UniqueIdents::new(cooked.arena());
            cooked
                .schemas()
                .map(move |ty| (IdentMappingKey::Schema(ty.name()), scope.ident(ty.name())))
        });
        idents.extend({
            let mut scope = UniqueIdents::new(cooked.arena());
            cooked
                .operations()
                .map(move |op| (IdentMappingKey::Operation(op.id()), scope.ident(op.id())))
        });
        idents.extend({
            let resources: BTreeSet<_> =
                cooked.operations().filter_map(|op| op.resource()).collect();
            let mut scope = UniqueIdents::with_reserved(cooked.arena(), &["default"]);
            resources
                .into_iter()
                .map(move |name| (IdentMappingKey::Resource(name), scope.ident(name)))
        });
        for op in cooked.operations() {
            {
                // Reserve names used as local variables and arguments in
                // the generated operation method body.
                let mut scope = UniqueIdents::with_reserved(
                    cooked.arena(),
                    &["query", "request", "form", "url", "response"],
                );
                for param in op.path().params() {
                    let ident = scope.ident(param.name());
                    idents.insert(
                        IdentMappingKey::Parameter(op.id(), ParameterLocation::Path, param.name()),
                        ident,
                    );
                }
            }
            {
                let mut scope = UniqueIdents::new(cooked.arena());
                for param in op.query() {
                    let ident = scope.ident(param.name());
                    idents.insert(
                        IdentMappingKey::Parameter(op.id(), ParameterLocation::Query, param.name()),
                        ident,
                    );
                }
            }
        }

        let schemas = cooked.schemas().filter_map(|schema| match schema {
            SchemaTypeView::Struct(i, v) => Some(Uniquifiable::Struct(TypeInfo::Schema(i), v)),
            SchemaTypeView::Enum(i, v) => Some(Uniquifiable::Enum(TypeInfo::Schema(i), v)),
            SchemaTypeView::Tagged(i, v) => Some(Uniquifiable::Tagged(TypeInfo::Schema(i), v)),
            _ => None,
        });

        let inlines = cooked
            .schemas()
            .flat_map(|s| s.inlines())
            .chain(cooked.operations().flat_map(|op| op.inlines()))
            .filter_map(|inline| match inline {
                InlineTypeView::Struct(id, _, v) => {
                    Some(Uniquifiable::Struct(TypeInfo::Inline(id), v))
                }
                InlineTypeView::Enum(id, _, v) => Some(Uniquifiable::Enum(TypeInfo::Inline(id), v)),
                InlineTypeView::Tagged(id, _, v) => {
                    Some(Uniquifiable::Tagged(TypeInfo::Inline(id), v))
                }
                _ => None,
            });

        for item in schemas.chain(inlines) {
            match item {
                Uniquifiable::Struct(info, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for field in view.fields() {
                        if field.tag() {
                            continue;
                        }
                        let ident = match field.name() {
                            StructFieldName::Name(n) => scope.ident(n),
                            StructFieldName::Hint(hint) => scope.field_name_hint(hint),
                        };
                        idents.insert(IdentMappingKey::StructField(info, field.name()), ident);
                    }
                }
                Uniquifiable::Enum(info, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for variant in view.variants() {
                        if let EnumVariant::String(name) = variant {
                            let ident = scope.ident(name);
                            idents.insert(IdentMappingKey::EnumVariant(info, name), ident);
                        }
                    }
                }
                Uniquifiable::Tagged(info, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for variant in view.variants() {
                        let ident = scope.ident(variant.name());
                        idents.insert(IdentMappingKey::TaggedVariant(info, variant.name()), ident);
                    }
                }
            }
        }

        Self {
            cooked,
            idents,
            date_time_format: config.date_time_format,
        }
    }

    #[inline]
    pub fn ident(&self, key: impl Into<IdentMapping<'a>>) -> Cow<'a, UniqueIdent> {
        use {IdentMapping::*, IdentMappingKey as Key};
        match key.into() {
            Operation(name) => Cow::Borrowed(self.idents[&Key::Operation(name)]),
            Path(op, name) => {
                Cow::Borrowed(self.idents[&Key::Parameter(op, ParameterLocation::Path, name)])
            }
            Query(op, name) => {
                Cow::Borrowed(self.idents[&Key::Parameter(op, ParameterLocation::Query, name)])
            }
            Type(TypeInfo::Schema(info)) => Cow::Borrowed(self.idents[&Key::Schema(info.name)]),
            Type(TypeInfo::Inline(id)) => Cow::Owned(UniqueIdentBuf::for_inline(self, id)),
            StructField(info, name) => Cow::Borrowed(self.idents[&Key::StructField(info, name)]),
            EnumVariant(info, name) => Cow::Borrowed(self.idents[&Key::EnumVariant(info, name)]),
            TaggedVariant(info, name) => {
                Cow::Borrowed(self.idents[&Key::TaggedVariant(info, name)])
            }
        }
    }

    /// Looks up the Rust type name for a resource name.
    #[inline]
    pub fn resource(&self, name: &str) -> Option<CodegenResourceIdent<'a>> {
        let key = IdentMappingKey::Resource(name);
        let &ident = self.idents.get(&key)?;
        Some(CodegenResourceIdent(ident))
    }

    /// Returns the format to use for `date-time` types.
    #[inline]
    pub fn date_time_format(&self) -> DateTimeFormat {
        self.date_time_format
    }
}

pub enum IdentMapping<'a> {
    Type(TypeInfo<'a>),
    Operation(&'a OperationId),
    Path(&'a OperationId, &'a str),
    Query(&'a OperationId, &'a str),
    StructField(TypeInfo<'a>, StructFieldName<'a>),
    EnumVariant(TypeInfo<'a>, &'a str),
    TaggedVariant(TypeInfo<'a>, &'a str),
}

impl From<InlineTypeId> for IdentMapping<'_> {
    #[inline]
    fn from(id: InlineTypeId) -> Self {
        Self::Type(id.into())
    }
}

impl<'a> From<&'a OperationId> for IdentMapping<'a> {
    fn from(id: &'a OperationId) -> Self {
        Self::Operation(id)
    }
}

impl<'a> From<SchemaTypeInfo<'a>> for IdentMapping<'a> {
    #[inline]
    fn from(info: SchemaTypeInfo<'a>) -> Self {
        Self::Type(info.into())
    }
}

impl<'a> From<TypeInfo<'a>> for IdentMapping<'a> {
    #[inline]
    fn from(info: TypeInfo<'a>) -> Self {
        Self::Type(info)
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = CookedGraph<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.cooked
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum IdentMappingKey<'a> {
    Schema(&'a str),
    Operation(&'a OperationId),
    Parameter(&'a OperationId, ParameterLocation, &'a str),
    Resource(&'a str),
    StructField(TypeInfo<'a>, StructFieldName<'a>),
    EnumVariant(TypeInfo<'a>, &'a str),
    TaggedVariant(TypeInfo<'a>, &'a str),
}

// Per-type uniquification pass.
enum Uniquifiable<'graph, 'a> {
    Struct(TypeInfo<'a>, StructView<'graph, 'a>),
    Enum(TypeInfo<'a>, EnumView<'graph, 'a>),
    Tagged(TypeInfo<'a>, TaggedView<'graph, 'a>),
}
