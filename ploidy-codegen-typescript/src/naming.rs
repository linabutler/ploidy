use std::{borrow::Cow, cmp::Ordering, fmt::Display};

use heck::{AsLowerCamelCase, AsPascalCase, AsSnekCase};
use itertools::Itertools;
use ploidy_core::{
    codegen::{
        UniqueNames,
        unique::{UniqueNamesScope, WordSegments},
    },
    ir::{
        ExtendableView, InlineIrTypePathSegment, InlineIrTypeView, IrStructFieldName,
        IrStructFieldNameHint, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrTypeView,
    },
};

/// TypeScript reserved words that can't be used as identifiers.
const KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    // Strict mode reserved words.
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "yield",
];

/// A name for a schema or an inline type, used in generated TypeScript code.
///
/// [`CodegenTypeName`] produces PascalCase type names (e.g., `Pet`,
/// `GetItemsFilter`). Use [`display_file_name`](Self::display_file_name)
/// for the corresponding file name (kebab-case), and
/// [`into_sort_key`](Self::into_sort_key) for deterministic sorting.
#[derive(Clone, Copy, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a SchemaIrTypeView<'a>),
    Inline(&'a InlineIrTypeView<'a>),
}

impl<'a> CodegenTypeName<'a> {
    /// Returns the PascalCase type name as a string.
    pub fn type_name(&self) -> String {
        match self {
            Self::Schema(view) => {
                let ident = view.extensions().get::<CodegenIdent>().unwrap();
                format!("{}", AsPascalCase(&ident.0))
            }
            Self::Inline(view) => {
                let ident = CodegenIdent::from_segments(&view.path().segments);
                format!("{}", AsPascalCase(&ident.0))
            }
        }
    }

    /// Returns the snake_case file name (without extension).
    pub fn display_file_name(&self) -> impl Display {
        struct DisplayFileName(String);
        impl Display for DisplayFileName {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", AsSnekCase(&self.0))
            }
        }
        let ident = match self {
            Self::Schema(view) => view.extensions().get::<CodegenIdent>().unwrap().clone(),
            Self::Inline(view) => CodegenIdent::from_segments(&view.path().segments),
        };
        DisplayFileName(ident.0)
    }

    #[inline]
    pub fn into_sort_key(self) -> CodegenTypeNameSortKey<'a> {
        CodegenTypeNameSortKey(self)
    }
}

/// A sort key for deterministic ordering of [`CodegenTypeName`]s.
///
/// Sorts schema types before inline types, then lexicographically by name.
#[derive(Clone, Copy, Debug)]
pub struct CodegenTypeNameSortKey<'a>(CodegenTypeName<'a>);

impl<'a> CodegenTypeNameSortKey<'a> {
    #[inline]
    pub fn for_schema(view: &'a SchemaIrTypeView<'a>) -> Self {
        Self(CodegenTypeName::Schema(view))
    }

    #[inline]
    pub fn for_inline(view: &'a InlineIrTypeView<'a>) -> Self {
        Self(CodegenTypeName::Inline(view))
    }

    #[inline]
    pub fn into_name(self) -> CodegenTypeName<'a> {
        self.0
    }
}

impl Eq for CodegenTypeNameSortKey<'_> {}

impl Ord for CodegenTypeNameSortKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (CodegenTypeName::Schema(a), CodegenTypeName::Schema(b)) => a.name().cmp(b.name()),
            (CodegenTypeName::Inline(a), CodegenTypeName::Inline(b)) => a.path().cmp(b.path()),
            (CodegenTypeName::Schema(_), CodegenTypeName::Inline(_)) => Ordering::Less,
            (CodegenTypeName::Inline(_), CodegenTypeName::Schema(_)) => Ordering::Greater,
        }
    }
}

impl PartialEq for CodegenTypeNameSortKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl PartialOrd for CodegenTypeNameSortKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A cleaned string that's valid for any TypeScript identifier usage.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CodegenIdent(pub(crate) String);

impl CodegenIdent {
    /// Creates an identifier for any usage.
    pub fn new(s: &str) -> Self {
        let s = clean(s);
        if KEYWORDS.contains(&s.as_str()) {
            Self(format!("_{s}"))
        } else {
            Self(s)
        }
    }

    /// Creates an identifier from an inline type path.
    pub fn from_segments(segments: &[InlineIrTypePathSegment<'_>]) -> Self {
        Self(
            segments
                .iter()
                .map(CodegenTypePathSegment)
                .format_with("", |segment, f| f(&segment.display()))
                .to_string(),
        )
    }

    /// Returns the PascalCase type name.
    pub fn to_type_name(&self) -> String {
        if self.0.starts_with(unicode_ident::is_xid_start) {
            format!("{}", AsPascalCase(&self.0))
        } else {
            format!("_{}", AsPascalCase(&self.0))
        }
    }

    /// Returns the camelCase property name.
    pub fn to_property_name(&self) -> String {
        format!("{}", AsLowerCamelCase(&self.0))
    }
}

/// A scope for generating unique, valid TypeScript identifiers.
#[derive(Debug)]
pub struct CodegenIdentScope<'a>(UniqueNamesScope<'a>);

impl<'a> CodegenIdentScope<'a> {
    /// Creates a new identifier scope backed by the given arena.
    pub fn new(arena: &'a UniqueNames) -> Self {
        Self(arena.scope_with_reserved(itertools::chain!(
            KEYWORDS.iter().copied(),
            std::iter::once("")
        )))
    }

    /// Cleans the input string and returns a name that's unique
    /// within this scope.
    pub fn uniquify(&mut self, name: &str) -> CodegenIdent {
        CodegenIdent(self.0.uniquify(&clean(name)).into_owned())
    }
}

/// Returns the camelCase field name for a TypeScript property.
pub fn ts_field_name(name: &str) -> String {
    // For TypeScript, we preserve the original field name from the
    // OpenAPI spec (camelCase is conventional in JSON).
    name.to_owned()
}

/// Returns a valid JavaScript identifier for a parameter name.
///
/// If the name is already a valid identifier, it's returned as-is.
/// Otherwise, it's converted to camelCase.
pub fn ts_param_name(name: &str) -> String {
    use super::emit::is_valid_js_identifier;
    if is_valid_js_identifier(name) {
        name.to_owned()
    } else {
        format!("{}", AsLowerCamelCase(&clean(name)))
    }
}

/// Returns the variant name for an untagged union member.
pub fn ts_untagged_variant_name(hint: IrUntaggedVariantNameHint) -> Cow<'static, str> {
    use IrUntaggedVariantNameHint::*;
    match hint {
        Primitive(PrimitiveIrType::String) => "String".into(),
        Primitive(PrimitiveIrType::I8) => "I8".into(),
        Primitive(PrimitiveIrType::U8) => "U8".into(),
        Primitive(PrimitiveIrType::I16) => "I16".into(),
        Primitive(PrimitiveIrType::U16) => "U16".into(),
        Primitive(PrimitiveIrType::I32) => "I32".into(),
        Primitive(PrimitiveIrType::U32) => "U32".into(),
        Primitive(PrimitiveIrType::I64) => "I64".into(),
        Primitive(PrimitiveIrType::U64) => "U64".into(),
        Primitive(PrimitiveIrType::F32) => "F32".into(),
        Primitive(PrimitiveIrType::F64) => "F64".into(),
        Primitive(PrimitiveIrType::Bool) => "Bool".into(),
        Primitive(PrimitiveIrType::DateTime) => "DateTime".into(),
        Primitive(PrimitiveIrType::UnixTime) => "UnixTime".into(),
        Primitive(PrimitiveIrType::Date) => "Date".into(),
        Primitive(PrimitiveIrType::Url) => "Url".into(),
        Primitive(PrimitiveIrType::Uuid) => "Uuid".into(),
        Primitive(PrimitiveIrType::Bytes) => "Bytes".into(),
        Primitive(PrimitiveIrType::Binary) => "Binary".into(),
        Array => "Array".into(),
        Map => "Map".into(),
        Index(index) => Cow::Owned(format!("V{index}")),
    }
}

/// Returns the name for a struct field hint.
pub fn ts_struct_field_hint_name(hint: IrStructFieldNameHint) -> String {
    match hint {
        IrStructFieldNameHint::Index(index) => format!("variant{index}"),
        IrStructFieldNameHint::AdditionalProperties => "additionalProperties".to_owned(),
    }
}

#[derive(Clone, Copy, Debug)]
struct CodegenTypePathSegment<'a>(&'a InlineIrTypePathSegment<'a>);

impl<'a> CodegenTypePathSegment<'a> {
    fn display(&self) -> impl Display + '_ {
        struct DisplaySegment<'a>(&'a InlineIrTypePathSegment<'a>);
        impl Display for DisplaySegment<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use InlineIrTypePathSegment::*;
                match self.0 {
                    Operation(name) => write!(f, "{}", AsPascalCase(clean(name))),
                    Parameter(name) => write!(f, "{}", AsPascalCase(clean(name))),
                    Request => f.write_str("Request"),
                    Response => f.write_str("Response"),
                    Field(IrStructFieldName::Name(name)) => {
                        write!(f, "{}", AsPascalCase(clean(name)))
                    }
                    Field(IrStructFieldName::Hint(IrStructFieldNameHint::Index(index))) => {
                        write!(f, "Variant{index}")
                    }
                    Field(IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties)) => {
                        f.write_str("AdditionalProperties")
                    }
                    MapValue => f.write_str("Value"),
                    ArrayItem => f.write_str("Item"),
                    Variant(index) => write!(f, "V{index}"),
                    Parent(index) => write!(f, "P{index}"),
                    TaggedVariant(name) => write!(f, "{}", AsPascalCase(clean(name))),
                }
            }
        }
        DisplaySegment(self.0)
    }
}

/// Makes a string suitable for inclusion within a TypeScript identifier.
fn clean(s: &str) -> String {
    WordSegments::new(s)
        .flat_map(|s| s.split(|c| !unicode_ident::is_xid_continue(c)))
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn test_clean() {
        assert_eq!(clean("foo-bar"), "foo_bar");
        assert_eq!(clean("foo.bar"), "foo_bar");
        assert_eq!(clean("FooBar"), "Foo_Bar");
        assert_eq!(clean("123foo"), "123_foo");
    }

    #[test]
    fn test_codegen_ident_new() {
        let ident = CodegenIdent::new("pet_store");
        assert_eq!(ident.to_type_name(), "PetStore");
    }

    #[test]
    fn test_codegen_ident_keyword_escaped() {
        let ident = CodegenIdent::new("class");
        assert_eq!(ident.to_type_name(), "_Class");
    }

    #[test]
    fn test_codegen_ident_number_prefix() {
        let ident = CodegenIdent::new("1099KStatus");
        assert_eq!(ident.to_type_name(), "_1099KStatus");
    }

    #[test]
    fn test_ts_field_name_preserves_original() {
        assert_eq!(ts_field_name("petType"), "petType");
        assert_eq!(ts_field_name("first_name"), "first_name");
    }
}
