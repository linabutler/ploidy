use std::{borrow::Cow, collections::BTreeSet, ops::Deref};

use ploidy_core::{
    ir::{
        CookedGraph, EnumVariant, EnumView, InlineTypeView, OperationId, SchemaTypeView,
        StructFieldName, StructView, TaggedView, TypeView, View,
        views::{Identifiable, TypeViewId},
    },
    parse::ParameterLocation,
};
use rustc_hash::FxHashMap;

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{ResourceIdent, UniqueIdent, UniqueIdents, format_inline_type_path},
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
            let resources: BTreeSet<_> = cooked
                .operations()
                .filter_map(|op| op.resource())
                .chain(cooked.schemas().filter_map(|ty| ty.resource()))
                .collect();
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

        let schemas = cooked.schemas().filter_map(|schema| {
            let id = schema.id();
            match schema {
                SchemaTypeView::Struct(_, v) => Some(Uniquifiable::Struct(id, v)),
                SchemaTypeView::Enum(_, v) => Some(Uniquifiable::Enum(id, v)),
                SchemaTypeView::Tagged(_, v) => Some(Uniquifiable::Tagged(id, v)),
                _ => None,
            }
        });

        let inlines = cooked
            .schemas()
            .flat_map(|s| s.inlines())
            .chain(cooked.operations().flat_map(|op| op.inlines()))
            .filter_map(|inline| {
                let id = inline.id();
                match inline {
                    InlineTypeView::Struct(_, v) => Some(Uniquifiable::Struct(id, v)),
                    InlineTypeView::Enum(_, v) => Some(Uniquifiable::Enum(id, v)),
                    InlineTypeView::Tagged(_, v) => Some(Uniquifiable::Tagged(id, v)),
                    _ => None,
                }
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
            Type(id) => match self.cooked.lookup(id) {
                TypeView::Schema(s) => Cow::Borrowed(self.idents[&Key::Schema(s.name())]),
                TypeView::Inline(i) => Cow::Owned(format_inline_type_path(self, i.path())),
            },
            StructField(info, name) => Cow::Borrowed(self.idents[&Key::StructField(info, name)]),
            EnumVariant(info, name) => Cow::Borrowed(self.idents[&Key::EnumVariant(info, name)]),
            TaggedVariant(info, name) => {
                Cow::Borrowed(self.idents[&Key::TaggedVariant(info, name)])
            }
        }
    }

    /// Looks up the Rust type name for a resource name.
    #[inline]
    pub fn resource(&self, name: &str) -> Option<ResourceIdent<'a>> {
        let &ident = self.idents.get(&IdentMappingKey::Resource(name))?;
        Some(ResourceIdent(ident))
    }

    /// Returns the format to use for `date-time` types.
    #[inline]
    pub fn date_time_format(&self) -> DateTimeFormat {
        self.date_time_format
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = CookedGraph<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.cooked
    }
}

pub enum IdentMapping<'a> {
    Type(TypeViewId),
    Operation(&'a OperationId),
    Path(&'a OperationId, &'a str),
    Query(&'a OperationId, &'a str),
    StructField(TypeViewId, StructFieldName<'a>),
    EnumVariant(TypeViewId, &'a str),
    TaggedVariant(TypeViewId, &'a str),
}

impl<'a> From<&'a OperationId> for IdentMapping<'a> {
    fn from(id: &'a OperationId) -> Self {
        Self::Operation(id)
    }
}

impl<'a> From<TypeViewId> for IdentMapping<'a> {
    #[inline]
    fn from(info: TypeViewId) -> Self {
        Self::Type(info)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum IdentMappingKey<'a> {
    Schema(&'a str),
    Operation(&'a OperationId),
    Parameter(&'a OperationId, ParameterLocation, &'a str),
    Resource(&'a str),
    StructField(TypeViewId, StructFieldName<'a>),
    EnumVariant(TypeViewId, &'a str),
    TaggedVariant(TypeViewId, &'a str),
}

// Per-type uniquification pass.
enum Uniquifiable<'graph, 'a> {
    Struct(TypeViewId, StructView<'graph, 'a>),
    Enum(TypeViewId, EnumView<'graph, 'a>),
    Tagged(TypeViewId, TaggedView<'graph, 'a>),
}
