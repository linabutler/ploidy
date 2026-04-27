use std::{collections::BTreeSet, fmt::Write, ops::Deref};

use ploidy_core::{
    ir::{
        ContainerView, CookedGraph, EnumVariant, EnumView, HasResource, HasTypeId,
        InlineTypePathRoot, InlineTypePathSegment, InlineTypePathView, InlineTypeView, OperationId,
        OperationUsage, SchemaTypeView, StructFieldName, StructView, TaggedView, TypeId, TypeView,
        UntaggedView, View,
    },
    parse::ParameterLocation,
};
use rustc_hash::FxHashMap;

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{CodegenIdentUsage, ResourceGroup, UniqueIdent, UniqueIdents},
};

/// A [`CookedGraph`] decorated with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a> {
    cooked: CookedGraph<'a>,
    idents: IdentMap<'a>,
    date_time_format: DateTimeFormat,
}

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    #[inline]
    pub fn new(cooked: CookedGraph<'a>) -> Self {
        Self::with_config(cooked, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    #[inline]
    pub fn with_config(cooked: CookedGraph<'a>, config: &CodegenConfig) -> Self {
        let idents = ident_map(&cooked);
        Self {
            cooked,
            idents,
            date_time_format: config.date_time_format,
        }
    }

    /// Returns the unique Rust identifier for a schema, operation, parameter,
    /// field, or variant.
    #[inline]
    pub fn ident(&self, key: impl Into<IdentMapping<'a>>) -> &'a UniqueIdent {
        use {IdentMapKey as Key, IdentMapping::*};
        match key.into() {
            Operation(op) => self.idents[&Key::Operation(op)],
            Path(op, name) => self.idents[&Key::Parameter(op, ParameterLocation::Path, name)],
            Query(op, name) => self.idents[&Key::Parameter(op, ParameterLocation::Query, name)],
            Type(id) => match self.cooked.view(id) {
                TypeView::Schema(s) => self.idents[&Key::Schema(s.name())],
                TypeView::Inline(i) => self.idents[&Key::Inline(i.id())],
            },
            StructField(id, name) => self.idents[&Key::StructField(id, name)],
            EnumVariant(id, name) => self.idents[&Key::EnumVariant(id, name)],
            TaggedVariant(id, name) => self.idents[&Key::TaggedVariant(id, name)],
            UntaggedVariant(id, index) => self.idents[&Key::UntaggedVariant(id, index)],
            Resource(name) => self.idents[&IdentMapKey::Resource(name)],
        }
    }

    /// Returns the resource that contains the given view.
    #[inline]
    pub fn resource_for(&self, view: &impl HasResource<'a>) -> ResourceGroup<'a> {
        view.resource()
            .map(|name| ResourceGroup::Named(self.idents[&IdentMapKey::Resource(name)]))
            .unwrap_or_default()
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

/// An item with a uniquified Rust identifier in a [`CodegenGraph`].
pub enum IdentMapping<'a> {
    /// A schema or inline type.
    Type(TypeId),

    /// An operation method.
    Operation(&'a OperationId),

    /// A path parameter for an operation.
    Path(&'a OperationId, &'a str),

    /// A query parameter for an operation.
    Query(&'a OperationId, &'a str),

    /// A struct field.
    StructField(TypeId, StructFieldName<'a>),

    /// A string enum variant.
    EnumVariant(TypeId, &'a str),

    /// A tagged union variant.
    TaggedVariant(TypeId, &'a str),

    /// An untagged union variant.
    UntaggedVariant(TypeId, usize),

    /// ...
    Resource(&'a str),
}

impl<'a> From<&'a OperationId> for IdentMapping<'a> {
    #[inline]
    fn from(id: &'a OperationId) -> Self {
        Self::Operation(id)
    }
}

impl<'a> From<TypeId> for IdentMapping<'a> {
    #[inline]
    fn from(id: TypeId) -> Self {
        Self::Type(id)
    }
}

/// Builds the identifier table for every name that Rust code generation emits.
///
/// Names are assigned in dependency order. Schema, operation, parameter, field,
/// and variant identifiers are uniquified first; inline type names are composed
/// from those earlier identifiers.
fn ident_map<'a>(cooked: &CookedGraph<'a>) -> IdentMap<'a> {
    let mut idents = FxHashMap::default();
    idents.extend({
        let mut scope = UniqueIdents::new(cooked.arena());
        cooked
            .schemas()
            .map(move |ty| (IdentMapKey::Schema(ty.name()), scope.ident(ty.name())))
    });
    idents.extend({
        let mut scope = UniqueIdents::new(cooked.arena());
        cooked
            .operations()
            .map(move |op| (IdentMapKey::Operation(op.id()), scope.ident(op.id())))
    });
    idents.extend({
        let resources: BTreeSet<_> = cooked
            .operations()
            .filter_map(|op| op.resource())
            .chain(cooked.schemas().filter_map(|ty| ty.resource()))
            .collect();
        // Resources become feature names; `default` is a special feature name.
        let mut scope = UniqueIdents::with_reserved(cooked.arena(), &["default"]);
        resources
            .into_iter()
            .map(move |name| (IdentMapKey::Resource(name), scope.ident(name)))
    });
    for op in cooked.operations() {
        {
            // Path parameters become arguments, so we need to reserve
            // local variable and argument names that we use in the
            // generated operation method body.
            let mut scope = UniqueIdents::with_reserved(
                cooked.arena(),
                &["query", "request", "form", "url", "response"],
            );
            for param in op.path().params() {
                let ident = scope.ident(param.name());
                idents.insert(
                    IdentMapKey::Parameter(op.id(), ParameterLocation::Path, param.name()),
                    ident,
                );
            }
        }
        {
            // Query parameters become regular struct fields.
            let mut scope = UniqueIdents::new(cooked.arena());
            for param in op.query() {
                let ident = scope.ident(param.name());
                idents.insert(
                    IdentMapKey::Parameter(op.id(), ParameterLocation::Query, param.name()),
                    ident,
                );
            }
        }
    }

    {
        let domains = cooked
            .schemas()
            .filter_map(MemberIdentDomain::from_schema_type)
            .chain(
                cooked
                    .schemas()
                    .flat_map(|s| s.inlines())
                    .chain(cooked.operations().flat_map(|op| op.inlines()))
                    .filter_map(MemberIdentDomain::from_inline_type),
            );
        for domain in domains {
            match domain {
                // Own, inherited, and synthesized struct fields.
                MemberIdentDomain::Struct(id, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for field in view.fields() {
                        let ident = match field.name() {
                            StructFieldName::Name(n) => scope.ident(n),
                            StructFieldName::Hint(hint) => scope.field_name_hint(hint),
                        };
                        idents.insert(IdentMapKey::StructField(id, field.name()), ident);
                    }
                }
                // Common fields inherited by all untagged variants. The struct arm
                // uniquifies them for fields; this arm uniquifies them for
                // naming inline types.
                MemberIdentDomain::Untagged(id, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for (index, variant) in view.variants().enumerate() {
                        let ident = match variant.ty() {
                            Some(variant) => scope.variant_name_hint(variant.hint),
                            None => scope.ident("None"),
                        };
                        idents.insert(IdentMapKey::UntaggedVariant(id, index), ident);
                    }
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for field in view.fields() {
                        let ident = match field.name() {
                            StructFieldName::Name(n) => scope.ident(n),
                            StructFieldName::Hint(hint) => scope.field_name_hint(hint),
                        };
                        idents.insert(IdentMapKey::StructField(id, field.name()), ident);
                    }
                }
                // String enum variants.
                MemberIdentDomain::Enum(id, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for variant in view.variants() {
                        if let EnumVariant::String(name) = variant {
                            let ident = scope.ident(name);
                            idents.insert(IdentMapKey::EnumVariant(id, name), ident);
                        }
                    }
                }
                // Tagged variant names and common fields form different scopes:
                // variant names must be unique within the generated enum;
                // common fields are for naming inline types.
                MemberIdentDomain::Tagged(id, view) => {
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for variant in view.variants() {
                        let ident = scope.ident(variant.name());
                        idents.insert(IdentMapKey::TaggedVariant(id, variant.name()), ident);
                    }
                    let mut scope = UniqueIdents::new(cooked.arena());
                    for field in view.fields() {
                        let ident = match field.name() {
                            StructFieldName::Name(n) => scope.ident(n),
                            StructFieldName::Hint(hint) => scope.field_name_hint(hint),
                        };
                        idents.insert(IdentMapKey::StructField(id, field.name()), ident);
                    }
                }
            }
        }
    }

    // Inline type names depend on uniquified path segments, so build them after
    // all schemas, operations, parameters, fields, and variants have names.
    {
        let inlines = cooked
            .schemas()
            .flat_map(|schema| schema.inlines())
            .chain(cooked.operations().flat_map(|op| op.inlines()))
            .filter(|ty| {
                // Optional types are invisible for naming.
                !matches!(ty, InlineTypeView::Container(_, ContainerView::Optional(_)))
            });

        let mut scopes = FxHashMap::default();
        for inline in inlines {
            let path = inline.path();
            let domain = match path.root() {
                InlineTypePathRoot::Schema(id) => InlineTypeIdentDomain::Schema(id),
                InlineTypePathRoot::Operation { resource, .. } => InlineTypeIdentDomain::Resource(
                    resource
                        .map(|name| ResourceGroup::Named(idents[&IdentMapKey::Resource(name)]))
                        .unwrap_or_default(),
                ),
            };
            let name = inline_type_candidate_name(&idents, &path);
            let scope = scopes
                .entry(domain)
                .or_insert_with(|| UniqueIdents::new(cooked.arena()));
            idents.insert(IdentMapKey::Inline(inline.id()), scope.ident(&name));
        }
    }
    idents
}

type IdentMap<'a> = FxHashMap<IdentMapKey<'a>, &'a UniqueIdent>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum IdentMapKey<'a> {
    Schema(&'a str),
    Inline(TypeId),
    Operation(&'a OperationId),
    Parameter(&'a OperationId, ParameterLocation, &'a str),
    Resource(&'a str),
    StructField(TypeId, StructFieldName<'a>),
    EnumVariant(TypeId, &'a str),
    TaggedVariant(TypeId, &'a str),
    UntaggedVariant(TypeId, usize),
}

/// A uniqueness domain for inline type identifiers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum InlineTypeIdentDomain<'a> {
    Schema(TypeId),
    Resource(ResourceGroup<'a>),
}

/// A uniqueness domain for field and variant identifiers.
enum MemberIdentDomain<'graph, 'a> {
    Struct(TypeId, StructView<'graph, 'a>),
    Enum(TypeId, EnumView<'graph, 'a>),
    Tagged(TypeId, TaggedView<'graph, 'a>),
    Untagged(TypeId, UntaggedView<'graph, 'a>),
}

impl<'graph, 'a> MemberIdentDomain<'graph, 'a> {
    fn from_schema_type(schema: SchemaTypeView<'graph, 'a>) -> Option<Self> {
        let id = schema.id();
        Some(match schema {
            SchemaTypeView::Struct(_, v) => Self::Struct(id, v),
            SchemaTypeView::Enum(_, v) => Self::Enum(id, v),
            SchemaTypeView::Tagged(_, v) => Self::Tagged(id, v),
            SchemaTypeView::Untagged(_, v) => Self::Untagged(id, v),
            _ => return None,
        })
    }

    fn from_inline_type(inline: InlineTypeView<'graph, 'a>) -> Option<Self> {
        let id = inline.id();
        Some(match inline {
            InlineTypeView::Struct(_, v) => Self::Struct(id, v),
            InlineTypeView::Enum(_, v) => Self::Enum(id, v),
            InlineTypeView::Tagged(_, v) => Self::Tagged(id, v),
            InlineTypeView::Untagged(_, v) => Self::Untagged(id, v),
            _ => return None,
        })
    }
}

fn inline_type_candidate_name<'a>(
    idents: &IdentMap<'a>,
    path: &InlineTypePathView<'_, 'a>,
) -> String {
    let mut name = String::new();

    match path.root() {
        InlineTypePathRoot::Schema(_) => {}
        InlineTypePathRoot::Operation { id, usage, .. } => {
            let ident = idents[&IdentMapKey::Operation(id)];
            write!(name, "{}", CodegenIdentUsage::Type(ident).display()).unwrap();

            match usage {
                OperationUsage::Path(param) => {
                    let ident = idents[&IdentMapKey::Parameter(id, ParameterLocation::Path, param)];
                    write!(name, "Path{}", CodegenIdentUsage::Type(ident).display()).unwrap();
                }
                OperationUsage::Query(param) => {
                    let ident =
                        idents[&IdentMapKey::Parameter(id, ParameterLocation::Query, param)];
                    write!(name, "Query{}", CodegenIdentUsage::Type(ident).display()).unwrap();
                }
                OperationUsage::Request => name.push_str("Request"),
                OperationUsage::Response => name.push_str("Response"),
            }
        }
    }

    for segment in path.segments() {
        match segment {
            InlineTypePathSegment::Field(parent, field_name) => {
                let ident = idents[&IdentMapKey::StructField(parent, field_name)];
                write!(name, "{}", CodegenIdentUsage::Type(ident).display()).unwrap();
            }
            InlineTypePathSegment::TaggedVariant(parent, variant_name) => {
                let ident = idents[&IdentMapKey::TaggedVariant(parent, variant_name)];
                write!(name, "{}", CodegenIdentUsage::Variant(ident).display()).unwrap();
            }
            InlineTypePathSegment::UntaggedVariant(index) => {
                write!(name, "V{index}").unwrap();
            }
            InlineTypePathSegment::ArrayItem => name.push_str("Item"),
            InlineTypePathSegment::MapValue => name.push_str("Value"),
            InlineTypePathSegment::Optional => {
                // Optional types are invisible for naming.
            }
            InlineTypePathSegment::Inherits(index) => {
                write!(name, "P{index}").unwrap();
            }
        }
    }

    if name.is_empty() {
        name.push_str("Value");
    }

    name
}
