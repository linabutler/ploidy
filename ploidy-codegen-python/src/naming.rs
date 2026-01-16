//! Python identifier naming and case conversion.
//!
//! This module handles converting OpenAPI schema names into valid Python
//! identifiers following PEP 8 naming conventions:
//! - Classes: `PascalCase`
//! - Functions/methods/fields: `snake_case`
//! - Constants/enum variants: `SCREAMING_SNAKE_CASE`
//! - Modules: `snake_case`

use std::{borrow::Cow, cmp::Ordering, fmt::Display, ops::Deref};

use heck::{AsPascalCase, AsShoutySnekCase, AsSnekCase};
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
use ref_cast::{RefCastCustom, ref_cast_custom};

/// Python reserved keywords that can't be used as identifiers.
const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Python soft keywords (context-dependent, but best avoided).
const SOFT_KEYWORDS: &[&str] = &["match", "case", "type", "_"];

/// Pydantic BaseModel attributes that shouldn't be shadowed by field names.
/// These cause `UserWarning: Field name "X" shadows an attribute in parent "BaseModel"`.
const PYDANTIC_RESERVED: &[&str] = &[
    // Deprecated v1 methods that still exist in v2.
    "schema",
    "schema_json",
    "json",
    "dict",
    "copy",
    "parse_obj",
    "parse_raw",
    "parse_file",
    "construct",
    "validate",
    // Current v2 methods and attributes.
    "model_config",
    "model_fields",
    "model_computed_fields",
    "model_extra",
    "model_fields_set",
    "model_construct",
    "model_copy",
    "model_dump",
    "model_dump_json",
    "model_json_schema",
    "model_parametrized_name",
    "model_post_init",
    "model_rebuild",
    "model_validate",
    "model_validate_json",
    "model_validate_strings",
];

/// Python built-in names that shouldn't be shadowed.
const BUILTINS: &[&str] = &[
    "abs",
    "all",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

#[derive(Clone, Copy, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a SchemaIrTypeView<'a>),
    Inline(&'a InlineIrTypeView<'a>),
}

impl<'a> CodegenTypeName<'a> {
    #[inline]
    pub fn into_sort_key(self) -> CodegenTypeNameSortKey<'a> {
        CodegenTypeNameSortKey(self)
    }

    /// Returns the Python class name as a string.
    pub fn as_class_name(&self) -> String {
        match self {
            Self::Schema(view) => {
                let ident = view.extensions().get::<CodegenIdent>().unwrap();
                CodegenIdentUsage::Class(&ident).display().to_string()
            }
            Self::Inline(view) => view
                .path()
                .segments
                .iter()
                .map(CodegenTypePathSegment)
                .join(""),
        }
    }
}

impl Display for CodegenTypeName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_class_name())
    }
}

/// A comparator that sorts type names lexicographically.
#[derive(Clone, Copy, Debug)]
pub struct CodegenTypeNameSortKey<'a>(CodegenTypeName<'a>);

impl<'a> CodegenTypeNameSortKey<'a> {
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

/// A string that's statically guaranteed to be valid for any
/// [`CodegenIdentUsage`].
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CodegenIdent(String);

impl CodegenIdent {
    /// Creates an identifier for any usage.
    pub fn new(s: &str) -> Self {
        let s = clean(s);
        if is_reserved(&s) {
            Self(format!("{s}_"))
        } else {
            Self(s)
        }
    }
}

impl Deref for CodegenIdent {
    type Target = CodegenIdentRef;

    fn deref(&self) -> &Self::Target {
        CodegenIdentRef::new(&self.0)
    }
}

/// A string slice that's guaranteed to be valid for any [`CodegenIdentUsage`].
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdentRef(str);

impl CodegenIdentRef {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
}

/// Represents different usages of a Python identifier, determining the
/// appropriate casing.
#[derive(Clone, Copy, Debug)]
pub enum CodegenIdentUsage<'a> {
    /// Module name (snake_case).
    Module(&'a CodegenIdentRef),
    /// Class name (PascalCase).
    Class(&'a CodegenIdentRef),
    /// Field/attribute name (snake_case).
    Field(&'a CodegenIdentRef),
    /// Enum variant name (SCREAMING_SNAKE_CASE).
    Variant(&'a CodegenIdentRef),
    /// Method name (snake_case).
    Method(&'a CodegenIdentRef),
}

impl<'a> CodegenIdentUsage<'a> {
    /// Returns a displayable value that formats the identifier with the
    /// appropriate casing for this usage.
    pub fn display(&self) -> impl Display + '_ {
        struct DisplayIdent<'a>(&'a CodegenIdentUsage<'a>);

        impl Display for DisplayIdent<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    CodegenIdentUsage::Module(name) | CodegenIdentUsage::Method(name) => {
                        let snake = AsSnekCase(&name.0).to_string();
                        let snake = if name.0.ends_with('_') && !snake.ends_with('_') {
                            format!("{snake}_")
                        } else {
                            snake
                        };
                        if snake.starts_with(|c: char| c.is_ascii_digit()) {
                            write!(f, "_{snake}")
                        } else {
                            write!(f, "{snake}")
                        }
                    }
                    CodegenIdentUsage::Field(name) => {
                        let snake = AsSnekCase(&name.0).to_string();
                        let snake = if name.0.ends_with('_') && !snake.ends_with('_') {
                            format!("{snake}_")
                        } else {
                            snake
                        };
                        // Use `f_` prefix (not `_`) because Pydantic treats
                        // `_`-prefixed fields as private attributes.
                        if snake.starts_with(|c: char| c.is_ascii_digit()) {
                            write!(f, "f_{snake}")
                        } else {
                            write!(f, "{snake}")
                        }
                    }
                    CodegenIdentUsage::Class(name) => {
                        let pascal = AsPascalCase(&name.0).to_string();
                        let pascal = if name.0.ends_with('_') && !pascal.ends_with('_') {
                            format!("{pascal}_")
                        } else {
                            pascal
                        };
                        if pascal.starts_with(|c: char| c.is_ascii_digit()) {
                            write!(f, "_{pascal}")
                        } else {
                            write!(f, "{pascal}")
                        }
                    }
                    CodegenIdentUsage::Variant(name) => {
                        let shouty = AsShoutySnekCase(&name.0).to_string();
                        let shouty = if name.0.ends_with('_') && !shouty.ends_with('_') {
                            format!("{shouty}_")
                        } else {
                            shouty
                        };
                        if shouty.starts_with(|c: char| c.is_ascii_digit()) {
                            write!(f, "_{shouty}")
                        } else {
                            write!(f, "{shouty}")
                        }
                    }
                }
            }
        }

        DisplayIdent(self)
    }
}

/// A scope for generating unique, valid Python identifiers.
#[derive(Debug)]
pub struct CodegenIdentScope<'a>(UniqueNamesScope<'a>);

impl<'a> CodegenIdentScope<'a> {
    /// Creates a new identifier scope that's backed by the given arena.
    pub fn new(arena: &'a UniqueNames) -> Self {
        Self::with_reserved(arena, &[])
    }

    /// Creates a new identifier scope that's backed by the given arena,
    /// with additional pre-reserved names.
    pub fn with_reserved(arena: &'a UniqueNames, reserved: &[&str]) -> Self {
        Self(arena.scope_with_reserved(itertools::chain!(
            reserved.iter().copied(),
            KEYWORDS.iter().copied(),
            SOFT_KEYWORDS.iter().copied(),
            BUILTINS.iter().copied(),
            PYDANTIC_RESERVED.iter().copied(),
            std::iter::once("")
        )))
    }

    /// Cleans the input string and returns a name that's unique within this
    /// scope, and valid for any [`CodegenIdentUsage`].
    pub fn uniquify(&mut self, name: &str) -> CodegenIdent {
        CodegenIdent(self.0.uniquify(&clean(name)).into_owned())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenUntaggedVariantName(pub IrUntaggedVariantNameHint);

impl Display for CodegenUntaggedVariantName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use IrUntaggedVariantNameHint::*;
        let s = match self.0 {
            Primitive(PrimitiveIrType::String) => "Str".into(),
            Primitive(PrimitiveIrType::I8) => "Int8".into(),
            Primitive(PrimitiveIrType::U8) => "Uint8".into(),
            Primitive(PrimitiveIrType::I16) => "Int16".into(),
            Primitive(PrimitiveIrType::U16) => "Uint16".into(),
            Primitive(PrimitiveIrType::I32) => "Int32".into(),
            Primitive(PrimitiveIrType::U32) => "Uint32".into(),
            Primitive(PrimitiveIrType::I64) => "Int64".into(),
            Primitive(PrimitiveIrType::U64) => "Uint64".into(),
            Primitive(PrimitiveIrType::F32) => "Float32".into(),
            Primitive(PrimitiveIrType::F64) => "Float64".into(),
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
        };
        f.write_str(&s)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenStructFieldName(pub IrStructFieldNameHint);

impl Display for CodegenStructFieldName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            IrStructFieldNameHint::Index(index) => {
                write!(f, "variant_{index}")
            }
            IrStructFieldNameHint::AdditionalProperties => {
                write!(f, "additional_properties")
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenTypePathSegment<'a>(&'a InlineIrTypePathSegment<'a>);

impl Display for CodegenTypePathSegment<'_> {
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
            Parent(index) => write!(f, "Parent{index}"),
        }
    }
}

/// Returns `true` if the name is a Python reserved word or built-in.
///
/// Note: Pydantic reserved names are handled by the scope's uniquifier, not
/// here, so they get numeric suffixes (e.g., `schema0`) instead of trailing
/// underscores.
fn is_reserved(s: &str) -> bool {
    KEYWORDS.contains(&s) || SOFT_KEYWORDS.contains(&s) || BUILTINS.contains(&s)
}

/// Makes a string suitable for inclusion within a Python identifier.
///
/// Cleaning segments the string on word boundaries, collapses all
/// non-identifier characters into new boundaries, and reassembles the string.
fn clean(s: &str) -> String {
    WordSegments::new(s)
        .flat_map(|s| s.split(|c: char| !c.is_alphanumeric() && c != '_'))
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    // MARK: Usages

    #[test]
    fn test_codegen_ident_class() {
        let ident = CodegenIdent::new("pet_store");
        let usage = CodegenIdentUsage::Class(&ident);
        assert_eq!(usage.display().to_string(), "PetStore");
    }

    #[test]
    fn test_codegen_ident_field() {
        let ident = CodegenIdent::new("petStore");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "pet_store");
    }

    #[test]
    fn test_codegen_ident_module() {
        let ident = CodegenIdent::new("MyModule");
        let usage = CodegenIdentUsage::Module(&ident);
        assert_eq!(usage.display().to_string(), "my_module");
    }

    #[test]
    fn test_codegen_ident_variant() {
        let ident = CodegenIdent::new("http_error");
        let usage = CodegenIdentUsage::Variant(&ident);
        assert_eq!(usage.display().to_string(), "HTTP_ERROR");
    }

    #[test]
    fn test_codegen_ident_method() {
        let ident = CodegenIdent::new("getUserById");
        let usage = CodegenIdentUsage::Method(&ident);
        assert_eq!(usage.display().to_string(), "get_user_by_id");
    }

    // MARK: Special characters

    #[test]
    fn test_codegen_ident_handles_python_keywords() {
        let ident = CodegenIdent::new("class");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "class_");
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        // Fields use `f_` prefix (not `_`) because Pydantic treats `_` prefixed
        // fields as private attributes.
        let ident = CodegenIdent::new("123foo");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "f_123_foo");
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let ident = CodegenIdent::new("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "foo_bar_baz");
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let ident = CodegenIdent::new("1099KStatus");

        // Fields use `f_` prefix (not `_`) because Pydantic treats `_` prefixed
        // fields as private attributes.
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "f_1099_k_status");

        // Classes use `_` prefix.
        let usage = CodegenIdentUsage::Class(&ident);
        assert_eq!(usage.display().to_string(), "_1099KStatus");
    }

    #[test]
    fn test_codegen_ident_handles_builtins() {
        let ident = CodegenIdent::new("list");
        let usage = CodegenIdentUsage::Field(&ident);
        assert_eq!(usage.display().to_string(), "list_");
    }

    #[test]
    fn test_codegen_ident_scope_handles_pydantic_reserved() {
        // Pydantic reserved names like `schema` and `json` are handled by the
        // scope's uniquifier, which assigns numeric suffixes.
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);

        // `schema` shadows BaseModel.schema (deprecated but still exists).
        // The uniquifier finds the first available suffix.
        let ident = scope.uniquify("schema");
        let usage = CodegenIdentUsage::Field(&ident);
        assert!(
            usage.display().to_string().starts_with("schema"),
            "expected schema with suffix, got: {}",
            usage.display()
        );
        assert_ne!(
            usage.display().to_string(),
            "schema",
            "should have a suffix"
        );

        // `json` shadows BaseModel.json (deprecated but still exists).
        let ident = scope.uniquify("json");
        let usage = CodegenIdentUsage::Field(&ident);
        assert!(
            usage.display().to_string().starts_with("json"),
            "expected json with suffix, got: {}",
            usage.display()
        );
        assert_ne!(usage.display().to_string(), "json", "should have a suffix");
    }

    // MARK: Untagged variant names

    #[test]
    fn test_untagged_variant_name_string() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(
            PrimitiveIrType::String,
        ));
        assert_eq!(variant_name.to_string(), "Str");
    }

    #[test]
    fn test_untagged_variant_name_i32() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::I32));
        assert_eq!(variant_name.to_string(), "Int32");
    }

    #[test]
    fn test_untagged_variant_name_index() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Index(0));
        assert_eq!(variant_name.to_string(), "V0");

        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Index(42));
        assert_eq!(variant_name.to_string(), "V42");
    }

    // MARK: `clean()`

    #[test]
    fn test_clean() {
        assert_eq!(clean("foo-bar"), "foo_bar");
        assert_eq!(clean("foo.bar"), "foo_bar");
        assert_eq!(clean("foo bar"), "foo_bar");
        assert_eq!(clean("foo@bar"), "foo_bar");
        assert_eq!(clean("foo#bar"), "foo_bar");
        assert_eq!(clean("foo!bar"), "foo_bar");

        assert_eq!(clean("foo_bar"), "foo_bar");
        assert_eq!(clean("FooBar"), "Foo_Bar");
        assert_eq!(clean("foo123"), "foo123");
        assert_eq!(clean("_foo"), "foo");

        assert_eq!(clean("123foo"), "123_foo");
        assert_eq!(clean("9bar"), "9_bar");
    }

    // MARK: Scopes

    #[test]
    fn test_codegen_ident_scope_handles_empty() {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let ident = scope.uniquify("");

        let usage = CodegenIdentUsage::Field(&ident);
        // Empty string gets a numeric suffix (e.g., "0"), which starts with a
        // digit, so it gets `f_` prefix (not `_`) because Pydantic treats `_`
        // prefixed fields as private attributes.
        assert!(usage.display().to_string().starts_with("f_"));
    }
}
