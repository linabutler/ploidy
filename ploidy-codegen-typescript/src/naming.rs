use std::{cmp::Ordering, fmt::Display, ops::Deref};

use heck::{AsKebabCase, AsLowerCamelCase, AsPascalCase};
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
use quasiquodo_ts::swc::ecma_ast::Ident;
use ref_cast::{RefCastCustom, ref_cast_custom};

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
/// `GetItemsFilter`). Use [`display`](Self::display) for the type name
/// string, [`into_module_name`](Self::into_module_name) for the corresponding
/// module name (kebab-case), and [`into_sort_key`](Self::into_sort_key) for
/// deterministic sorting.
#[derive(Clone, Copy, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a SchemaIrTypeView<'a>),
    Inline(&'a InlineIrTypeView<'a>),
}

impl<'a> CodegenTypeName<'a> {
    /// Returns a formattable representation of this type name.
    ///
    /// [`CodegenTypeName`] doesn't implement [`Display`] directly, to
    /// help catch context mismatches: using `.display()` in a
    /// [`ts_quote`] macro, or `.to_string()` in a [`format`] string,
    /// stands out during review.
    pub fn display(&self) -> impl Display {
        struct DisplayTypeName<'a>(CodegenTypeName<'a>);
        impl Display for DisplayTypeName<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    CodegenTypeName::Schema(view) => {
                        let ident = view.extensions().get::<CodegenIdent>().unwrap();
                        write!(f, "{}", CodegenIdentUsage::Type(&ident).display())
                    }
                    CodegenTypeName::Inline(view) => {
                        let ident = CodegenIdent::from_segments(&view.path().segments);
                        write!(f, "{}", CodegenIdentUsage::Type(&ident).display())
                    }
                }
            }
        }
        DisplayTypeName(*self)
    }

    #[inline]
    pub fn into_module_name(self) -> CodegenModuleName<'a> {
        CodegenModuleName(self)
    }

    #[inline]
    pub fn into_sort_key(self) -> CodegenTypeNameSortKey<'a> {
        CodegenTypeNameSortKey(self)
    }
}

/// A module name derived from a [`CodegenTypeName`].
///
/// Produces kebab-case file names (e.g., `create-pet-request`).
/// For string interpolation (e.g., file paths), use
/// [`display`](Self::display), which returns an `impl Display` that
/// can be used with `format!`.
#[derive(Clone, Copy, Debug)]
pub struct CodegenModuleName<'a>(CodegenTypeName<'a>);

impl<'a> CodegenModuleName<'a> {
    #[inline]
    pub fn into_type_name(self) -> CodegenTypeName<'a> {
        self.0
    }

    /// Returns a formattable representation of this module name.
    pub fn display(&self) -> impl Display {
        struct DisplayModuleName<'a>(CodegenTypeName<'a>);
        impl Display for DisplayModuleName<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    CodegenTypeName::Schema(view) => {
                        let ident = view.extensions().get::<CodegenIdent>().unwrap();
                        write!(f, "{}", CodegenIdentUsage::Module(&ident).display())
                    }
                    CodegenTypeName::Inline(view) => {
                        let ident = CodegenIdent::from_segments(&view.path().segments);
                        write!(f, "{}", CodegenIdentUsage::Module(&ident).display())
                    }
                }
            }
        }
        DisplayModuleName(self.0)
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
pub struct CodegenIdent(String);

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
}

impl AsRef<CodegenIdentRef> for CodegenIdent {
    fn as_ref(&self) -> &CodegenIdentRef {
        self
    }
}

impl Deref for CodegenIdent {
    type Target = CodegenIdentRef;

    fn deref(&self) -> &Self::Target {
        CodegenIdentRef::new(&self.0)
    }
}

/// A string slice that's guaranteed to be valid for any
/// [`CodegenIdentUsage`].
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdentRef(str);

impl CodegenIdentRef {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
}

/// A context-aware wrapper for emitting a [`CodegenIdentRef`] as a
/// TypeScript identifier.
///
/// [`CodegenIdentUsage`] is a lower-level building block for generating
/// identifiers. For schema and inline types, prefer [`CodegenTypeName`]
/// instead.
///
/// Each [`CodegenIdentUsage`] variant determines the case
/// transformation applied to the identifier: module names become
/// kebab-case; type and variant names become PascalCase; field,
/// parameter, and method names become camelCase.
///
/// [`CodegenIdentUsage`] doesn't implement [`Display`] directly, to
/// help catch context mismatches. Use [`display`](Self::display).
#[derive(Clone, Copy, Debug)]
pub enum CodegenIdentUsage<'a> {
    Module(&'a CodegenIdentRef),
    Type(&'a CodegenIdentRef),
    Field(&'a CodegenIdentRef),
    Variant(&'a CodegenIdentRef),
    Param(&'a CodegenIdentRef),
    Method(&'a CodegenIdentRef),
}

impl CodegenIdentUsage<'_> {
    /// Returns a formattable representation of this identifier.
    pub fn display(self) -> impl Display {
        struct DisplayUsage<'a>(CodegenIdentUsage<'a>);
        impl Display for DisplayUsage<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use CodegenIdentUsage::*;
                match self.0 {
                    Module(name) => {
                        if name.0.starts_with(Ident::is_valid_start) {
                            write!(f, "{}", AsKebabCase(&name.0))
                        } else {
                            write!(f, "_{}", AsKebabCase(&name.0))
                        }
                    }
                    Field(name) | Param(name) | Method(name) => {
                        if name.0.starts_with(Ident::is_valid_start) {
                            write!(f, "{}", AsLowerCamelCase(&name.0))
                        } else {
                            write!(f, "_{}", AsLowerCamelCase(&name.0))
                        }
                    }
                    Type(name) | Variant(name) => {
                        if name.0.starts_with(Ident::is_valid_start) {
                            write!(f, "{}", AsPascalCase(&name.0))
                        } else {
                            write!(f, "_{}", AsPascalCase(&name.0))
                        }
                    }
                }
            }
        }
        DisplayUsage(self)
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

/// A field name derived from an [`IrStructFieldNameHint`].
#[derive(Clone, Copy, Debug)]
pub struct CodegenStructFieldName(pub IrStructFieldNameHint);

impl CodegenStructFieldName {
    /// Returns a formattable representation of this field name.
    pub fn display(self) -> impl Display {
        struct DisplayFieldName(IrStructFieldNameHint);
        impl Display for DisplayFieldName {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    IrStructFieldNameHint::Index(index) => write!(f, "variant{index}"),
                    IrStructFieldNameHint::AdditionalProperties => {
                        f.write_str("additionalProperties")
                    }
                }
            }
        }
        DisplayFieldName(self.0)
    }
}

/// A variant name for an untagged union member.
#[derive(Clone, Copy, Debug)]
pub struct CodegenUntaggedVariantName(pub IrUntaggedVariantNameHint);

impl CodegenUntaggedVariantName {
    /// Returns a formattable representation of this variant name.
    pub fn display(self) -> impl Display {
        struct DisplayVariantName(IrUntaggedVariantNameHint);
        impl Display for DisplayVariantName {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use IrUntaggedVariantNameHint::*;
                match self.0 {
                    Primitive(PrimitiveIrType::String) => f.write_str("String"),
                    Primitive(PrimitiveIrType::I8) => f.write_str("I8"),
                    Primitive(PrimitiveIrType::U8) => f.write_str("U8"),
                    Primitive(PrimitiveIrType::I16) => f.write_str("I16"),
                    Primitive(PrimitiveIrType::U16) => f.write_str("U16"),
                    Primitive(PrimitiveIrType::I32) => f.write_str("I32"),
                    Primitive(PrimitiveIrType::U32) => f.write_str("U32"),
                    Primitive(PrimitiveIrType::I64) => f.write_str("I64"),
                    Primitive(PrimitiveIrType::U64) => f.write_str("U64"),
                    Primitive(PrimitiveIrType::F32) => f.write_str("F32"),
                    Primitive(PrimitiveIrType::F64) => f.write_str("F64"),
                    Primitive(PrimitiveIrType::Bool) => f.write_str("Bool"),
                    Primitive(PrimitiveIrType::DateTime) => f.write_str("DateTime"),
                    Primitive(PrimitiveIrType::UnixTime) => f.write_str("UnixTime"),
                    Primitive(PrimitiveIrType::Date) => f.write_str("Date"),
                    Primitive(PrimitiveIrType::Url) => f.write_str("Url"),
                    Primitive(PrimitiveIrType::Uuid) => f.write_str("Uuid"),
                    Primitive(PrimitiveIrType::Bytes) => f.write_str("Bytes"),
                    Primitive(PrimitiveIrType::Binary) => f.write_str("Binary"),
                    Array => f.write_str("Array"),
                    Map => f.write_str("Map"),
                    Index(index) => write!(f, "V{index}"),
                }
            }
        }
        DisplayVariantName(self.0)
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
        .flat_map(|s| s.split(|c| !Ident::is_valid_continue(c)))
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    // MARK: `clean()`

    #[test]
    fn test_clean() {
        assert_eq!(clean("foo-bar"), "foo_bar");
        assert_eq!(clean("foo.bar"), "foo_bar");
        assert_eq!(clean("FooBar"), "Foo_Bar");
        assert_eq!(clean("123foo"), "123_foo");
    }

    // MARK: Usages

    #[test]
    fn test_codegen_ident_type() {
        let ident = CodegenIdent::new("pet_store");
        let usage = CodegenIdentUsage::Type(&ident);
        assert_eq!(usage.display().to_string(), "PetStore");
    }

    #[test]
    fn test_codegen_ident_field() {
        let ident = CodegenIdent::new("pet_store");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "petStore");
    }

    #[test]
    fn test_codegen_ident_module() {
        let ident = CodegenIdent::new("MyModule");
        let usage = CodegenIdentUsage::Module(&ident);
        assert_eq!(usage.display().to_string(), "my-module");
    }

    #[test]
    fn test_codegen_ident_variant() {
        let ident = CodegenIdent::new("http_error");
        let usage = CodegenIdentUsage::Variant(&ident);
        assert_eq!(usage.display().to_string(), "HttpError");
    }

    #[test]
    fn test_codegen_ident_param() {
        let ident = CodegenIdent::new("userId");
        let usage = CodegenIdentUsage::Param(&ident);
        assert_eq!(usage.display().to_string(), "userId");
    }

    #[test]
    fn test_codegen_ident_method() {
        let ident = CodegenIdent::new("getUserById");
        let usage = CodegenIdentUsage::Method(&ident);
        assert_eq!(usage.display().to_string(), "getUserById");
    }

    // MARK: Special characters

    #[test]
    fn test_codegen_ident_keyword_escaped() {
        let ident = CodegenIdent::new("class");
        let usage = CodegenIdentUsage::Type(&ident);
        assert_eq!(usage.display().to_string(), "Class");
    }

    #[test]
    fn test_codegen_ident_number_prefix() {
        let ident = CodegenIdent::new("1099KStatus");

        let type_usage = CodegenIdentUsage::Type(&ident);
        assert_eq!(type_usage.display().to_string(), "_1099KStatus");

        let field_usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(field_usage.display().to_string(), "_1099KStatus");
    }

    #[test]
    fn test_codegen_ident_special_chars() {
        let ident = CodegenIdent::new("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "fooBarBaz");
    }
}
