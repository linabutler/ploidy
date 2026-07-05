use std::{collections::BTreeSet, fmt::Write, num::NonZeroUsize, ops::Deref};

use ploidy_core::{
    arena::Arena,
    ir::{
        ContainerView, CookedGraph, EnumVariant, EnumView, HasResource, HasTypeId,
        InlineTypePathRoot, InlineTypePathSegment, InlineTypePathView, InlineTypeView, OperationId,
        OperationUsage, PrimitiveType, SchemaTypeView, StructFieldName, StructView, TaggedView,
        TypeId, TypeView, UntaggedView, View,
    },
    parse::ParameterLocation,
};
use rustc_hash::FxHashMap;

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{CodegenIdentUsage, ResourceGroup, UniqueIdent, UniqueIdents, status_variant_name},
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
    pub fn ident(&self, key: impl Into<IdentMapping<'a>>) -> UniqueIdent<'a> {
        use {IdentMapKey as Key, IdentMapping::*};
        match key.into() {
            Operation(op) => self.idents[&Key::Operation(op)],
            Path(op, name) => self.idents[&Key::Parameter(op, ParameterLocation::Path, name)],
            Query(op, name) => self.idents[&Key::Parameter(op, ParameterLocation::Query, name)],
            Type(id) => self.idents[&Key::Type(id)],
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
    UntaggedVariant(TypeId, NonZeroUsize),
    /// A resource name for a type or an operation.
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
/// Names are assigned in dependency order. Schema types and operations are
/// uniquified first, then inline types are named from their paths, and finally
/// inline type members.
fn ident_map<'a>(cooked: &CookedGraph<'a>) -> IdentMap<'a> {
    let mut idents = FxHashMap::default();
    idents.extend({
        let mut scope = UniqueIdents::new(cooked.arena());
        cooked
            .schemas()
            .map(move |ty| (IdentMapKey::Type(ty.id()), scope.claim(ty.name())))
    });
    idents.extend({
        let mut scope = UniqueIdents::new(cooked.arena());
        cooked
            .operations()
            .map(move |op| (IdentMapKey::Operation(op.id()), scope.claim(op.id())))
    });
    idents.extend({
        let resources: BTreeSet<_> = cooked
            .operations()
            .filter_map(|op| op.resource())
            .chain(cooked.schemas().filter_map(|ty| ty.resource()))
            .collect();
        // Resources become feature names; `default`, `tracing`, and
        // `trace-context` are special feature names.
        let mut scope =
            UniqueIdents::with_reserved(cooked.arena(), &["default", "tracing", "trace-context"]);
        resources
            .into_iter()
            .map(move |name| (IdentMapKey::Resource(name), scope.claim(name)))
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
                let ident = scope.claim(param.name());
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
                let ident = scope.claim(param.name());
                idents.insert(
                    IdentMapKey::Parameter(op.id(), ParameterLocation::Query, param.name()),
                    ident,
                );
            }
        }
    }

    for schema in cooked.schemas() {
        if let Some(domain) = MemberIdentDomain::from_schema_type(schema) {
            let map = domain.into_idents(cooked.arena(), &idents);
            idents.extend(map);
        }
    }

    // Inline type names depend on uniquified path segments. Build each inline
    // type after its parent, then name its members for child path segments.
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
            idents.insert(IdentMapKey::Type(inline.id()), scope.claim(&name));
            if let Some(domain) = MemberIdentDomain::from_inline_type(inline) {
                let map = domain.into_idents(cooked.arena(), &idents);
                idents.extend(map);
            }
        }
    }
    idents
}

type IdentMap<'a> = FxHashMap<IdentMapKey<'a>, UniqueIdent<'a>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum IdentMapKey<'a> {
    Type(TypeId),
    Operation(&'a OperationId),
    Parameter(&'a OperationId, ParameterLocation, &'a str),
    Resource(&'a str),
    StructField(TypeId, StructFieldName<'a>),
    EnumVariant(TypeId, &'a str),
    TaggedVariant(TypeId, &'a str),
    UntaggedVariant(TypeId, NonZeroUsize),
}

/// A uniqueness domain for inline type identifiers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum InlineTypeIdentDomain<'a> {
    Schema(TypeId),
    Resource(ResourceGroup<'a>),
}

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
            SchemaTypeView::Struct(_, view) => Self::Struct(id, view),
            SchemaTypeView::Enum(_, view) => Self::Enum(id, view),
            SchemaTypeView::Tagged(_, view) => Self::Tagged(id, view),
            SchemaTypeView::Untagged(_, view) => Self::Untagged(id, view),
            _ => return None,
        })
    }

    fn from_inline_type(inline: InlineTypeView<'graph, 'a>) -> Option<Self> {
        let id = inline.id();
        Some(match inline {
            InlineTypeView::Struct(_, view) => Self::Struct(id, view),
            InlineTypeView::Enum(_, view) => Self::Enum(id, view),
            InlineTypeView::Tagged(_, view) => Self::Tagged(id, view),
            InlineTypeView::Untagged(_, view) => Self::Untagged(id, view),
            _ => return None,
        })
    }

    fn into_idents(self, arena: &'a Arena, idents: &IdentMap<'a>) -> IdentMap<'a> {
        let mut map = IdentMap::default();
        match self {
            Self::Struct(id, view) => {
                // Own, inherited, and synthesized struct fields.
                let mut scope = UniqueIdents::new(arena);
                for field in view.fields() {
                    let name = field.name();
                    let ident = match name {
                        StructFieldName::Name(name) => scope.claim(name),
                        StructFieldName::Ordinal(ordinal) => {
                            let ident = idents[&IdentMapKey::Type(id)];
                            scope.claim(&format!(
                                "{}_{ordinal}",
                                CodegenIdentUsage::Type(ident).display()
                            ))
                        }
                        StructFieldName::AdditionalProperties => {
                            scope.claim("additional_properties")
                        }
                    };
                    map.insert(IdentMapKey::StructField(id, name), ident);
                }
            }
            Self::Enum(id, view) => {
                let mut scope = UniqueIdents::with_reserved(
                    arena,
                    &[&format!(
                        "Other{}",
                        CodegenIdentUsage::Type(idents[&IdentMapKey::Type(id)]).display()
                    )],
                );
                for &variant in view.variants() {
                    if let EnumVariant::String(name) = variant {
                        map.insert(IdentMapKey::EnumVariant(id, name), scope.claim(name));
                    }
                }
            }
            Self::Tagged(id, view) => {
                // Tagged variant names and common fields form different scopes:
                // variant names must be unique within the generated enum;
                // common fields are for naming inline types.
                let mut scope = UniqueIdents::new(arena);
                for variant in view.variants() {
                    let name = variant.name();
                    let ident = scope.claim(name);
                    map.insert(IdentMapKey::TaggedVariant(id, name), ident);
                }
                let mut scope = UniqueIdents::new(arena);
                for field in view.fields() {
                    let name = field.name();
                    let ident = match name {
                        StructFieldName::Name(name) => scope.claim(name),
                        StructFieldName::Ordinal(ordinal) => {
                            let ident = idents[&IdentMapKey::Type(id)];
                            scope.claim(&format!(
                                "{}_{ordinal}",
                                CodegenIdentUsage::Type(ident).display()
                            ))
                        }
                        StructFieldName::AdditionalProperties => {
                            scope.claim("additional_properties")
                        }
                    };
                    map.insert(IdentMapKey::StructField(id, name), ident);
                }
            }
            Self::Untagged(id, view) => {
                let mut scope = UniqueIdents::new(arena);
                for variant in view.variants() {
                    use {ContainerView::*, InlineTypeView::*, TypeView::*};
                    let ordinal = variant.ordinal();
                    let ident = match variant.ty() {
                        Some(Schema(schema)) => {
                            let ident = idents[&IdentMapKey::Type(schema.id())];
                            scope.adopt(ident)
                        }
                        Some(Inline(Primitive(_, primitive))) => {
                            scope.claim(match primitive.ty() {
                                PrimitiveType::String => "String",
                                PrimitiveType::I8 => "I8",
                                PrimitiveType::U8 => "U8",
                                PrimitiveType::I16 => "I16",
                                PrimitiveType::U16 => "U16",
                                PrimitiveType::I32 => "I32",
                                PrimitiveType::U32 => "U32",
                                PrimitiveType::I64 => "I64",
                                PrimitiveType::U64 => "U64",
                                PrimitiveType::F32 => "F32",
                                PrimitiveType::F64 => "F64",
                                PrimitiveType::Bool => "Bool",
                                PrimitiveType::DateTime => "DateTime",
                                PrimitiveType::UnixTime => "UnixTime",
                                PrimitiveType::Date => "Date",
                                PrimitiveType::Url => "Url",
                                PrimitiveType::Uuid => "Uuid",
                                PrimitiveType::Bytes => "Bytes",
                                PrimitiveType::Binary => "Binary",
                            })
                        }
                        Some(Inline(Container(_, Array(_)))) => scope.claim("Array"),
                        Some(Inline(Container(_, Map(_)))) => scope.claim("Map"),
                        Some(Inline(..)) => {
                            let ident = idents[&IdentMapKey::Type(id)];
                            scope.claim(&format!(
                                "{}_{ordinal}",
                                CodegenIdentUsage::Type(ident).display()
                            ))
                        }
                        None => scope.claim("None"),
                    };
                    map.insert(IdentMapKey::UntaggedVariant(id, ordinal), ident);
                }
                // Common fields inherited by all untagged variants.
                let mut scope = UniqueIdents::new(arena);
                for field in view.fields() {
                    let name = field.name();
                    let ident = match name {
                        StructFieldName::Name(name) => scope.claim(name),
                        StructFieldName::Ordinal(ordinal) => {
                            let ident = idents[&IdentMapKey::Type(id)];
                            scope.claim(&format!(
                                "{}_{ordinal}",
                                CodegenIdentUsage::Type(ident).display()
                            ))
                        }
                        StructFieldName::AdditionalProperties => {
                            scope.claim("additional_properties")
                        }
                    };
                    map.insert(IdentMapKey::StructField(id, name), ident);
                }
            }
        }
        map
    }
}

fn inline_type_candidate_name<'a>(
    idents: &IdentMap<'a>,
    path: &InlineTypePathView<'_, 'a>,
) -> String {
    let mut name = String::new();

    for segment in path.segments() {
        match segment {
            InlineTypePathSegment::Field(parent, field) => {
                let ident = idents[&IdentMapKey::StructField(parent, field)];
                write!(name, "{}", CodegenIdentUsage::Type(ident).display()).unwrap();
            }
            InlineTypePathSegment::TaggedVariant(parent, variant) => {
                let ident = idents[&IdentMapKey::TaggedVariant(parent, variant)];
                write!(name, "{}", CodegenIdentUsage::Variant(ident).display()).unwrap();
            }
            InlineTypePathSegment::UntaggedVariant(parent, ordinal) => {
                let ident = idents[&IdentMapKey::UntaggedVariant(parent, ordinal)];
                write!(name, "{}", CodegenIdentUsage::Variant(ident).display()).unwrap();
            }
            InlineTypePathSegment::ArrayItem => name.push_str("Item"),
            InlineTypePathSegment::MapValue => name.push_str("Value"),
            InlineTypePathSegment::Optional => {
                // Optional types are invisible for naming.
            }
            InlineTypePathSegment::Inherits(parent, ordinal) => {
                let ident = idents[&IdentMapKey::Type(parent)];
                write!(
                    name,
                    "{}_{ordinal}",
                    CodegenIdentUsage::Type(ident).display()
                )
                .unwrap();
            }
        }
    }

    match path.root() {
        InlineTypePathRoot::Schema(id) if name.is_empty() => {
            let ident = idents[&IdentMapKey::Type(id)];
            CodegenIdentUsage::Type(ident).display().to_string()
        }
        InlineTypePathRoot::Schema(..) => name,
        InlineTypePathRoot::Operation { id, usage, .. } => {
            let mut full = String::new();

            let ident = idents[&IdentMapKey::Operation(id)];
            write!(full, "{}", CodegenIdentUsage::Type(ident).display()).unwrap();
            match usage {
                OperationUsage::Path(param) => {
                    let ident = idents[&IdentMapKey::Parameter(id, ParameterLocation::Path, param)];
                    write!(full, "Path{}", CodegenIdentUsage::Type(ident).display()).unwrap();
                }
                OperationUsage::Query(param) => {
                    let ident =
                        idents[&IdentMapKey::Parameter(id, ParameterLocation::Query, param)];
                    write!(full, "Query{}", CodegenIdentUsage::Type(ident).display()).unwrap();
                }
                OperationUsage::Request => full.push_str("Request"),
                OperationUsage::Response { status } => {
                    full.push_str("Response");
                    if let Some(status) = status {
                        full.push_str(&status_variant_name(status));
                    }
                }
            }
            full.push_str(&name);

            full
        }
    }
}
