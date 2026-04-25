use std::{
    fmt::{Display, Write},
    ops::Deref,
};

use heck::{AsKebabCase, AsPascalCase, AsSnekCase};
use itertools::Itertools;
use ploidy_core::{
    arena::Arena,
    codegen::{UniqueNames, unique::WordSegments},
    ir::{
        InlineTypePath, InlineTypePathSegment, PrimitiveType, StructFieldName, StructFieldNameHint,
        UntaggedVariantNameHint,
    },
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt};
use ref_cast::{RefCastCustom, ref_cast_custom};

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// A name for a schema or inline type in generated Rust code.
///
/// Implements [`ToTokens`] to produce PascalCase identifiers
/// (e.g., `Pet`, `GetItemsFilter`) in [`quote`] macros.
/// Use [`into_module`](Self::into_module) for the corresponding
/// module name.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CodegenTypeIdent<'a> {
    Schema(&'a UniqueIdent),
    Inline(InlineTypePath<'a>),
}

impl<'a> CodegenTypeIdent<'a> {
    #[inline]
    pub fn into_module(self) -> CodegenModuleIdent<'a> {
        CodegenModuleIdent(self)
    }
}

impl ToTokens for CodegenTypeIdent<'_> {
    #[inline]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match *self {
            Self::Schema(ident) => {
                CodegenIdentUsage::Type(ident).to_tokens(tokens);
            }
            Self::Inline(path) => {
                let name = CodegenTypePathSegment::join(path.segments);
                CodegenIdentUsage::Type(CodegenIdent::new(&name)).to_tokens(tokens);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CodegenResourceIdent<'a>(pub &'a UniqueIdent);

impl Default for CodegenResourceIdent<'_> {
    fn default() -> Self {
        Self(UniqueIdent::new("default"))
    }
}

impl Deref for CodegenResourceIdent<'_> {
    type Target = UniqueIdent;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

/// A name for an operation in generated Rust code.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CodegenOperationIdent<'a>(pub &'a UniqueIdent);

impl Deref for CodegenOperationIdent<'_> {
    type Target = UniqueIdent;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

/// A module name derived from a [`CodegenTypeIdent`].
///
/// Implements [`ToTokens`] to produce a snake_case identifier. For
/// string interpolation (e.g., file paths), use [`display`](Self::display),
/// which returns an `impl Display` that can be used with `format!`.
#[derive(Clone, Copy, Debug)]
pub struct CodegenModuleIdent<'a>(CodegenTypeIdent<'a>);

impl<'a> CodegenModuleIdent<'a> {
    #[inline]
    pub fn into_type(self) -> CodegenTypeIdent<'a> {
        self.0
    }

    /// Returns a formattable representation of this module name.
    ///
    /// [`CodegenModuleIdent`] doesn't implement [`Display`] directly, to help catch
    /// context mismatches: using `.display()` in a [`quote`] macro, or
    /// `.to_token_stream()` in a [`format`] string, stands out during review.
    pub fn display(&self) -> impl Display {
        struct DisplayModuleName<'a>(CodegenTypeIdent<'a>);
        impl Display for DisplayModuleName<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    CodegenTypeIdent::Schema(ident) => {
                        write!(f, "{}", CodegenIdentUsage::Module(ident).display())
                    }
                    CodegenTypeIdent::Inline(path) => {
                        let name = CodegenTypePathSegment::join(path.segments);
                        write!(
                            f,
                            "{}",
                            CodegenIdentUsage::Module(CodegenIdent::new(&name)).display()
                        )
                    }
                }
            }
        }
        DisplayModuleName(self.0)
    }
}

impl ToTokens for CodegenModuleIdent<'_> {
    #[inline]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.0 {
            CodegenTypeIdent::Schema(ident) => {
                CodegenIdentUsage::Module(ident).to_tokens(tokens);
            }
            CodegenTypeIdent::Inline(path) => {
                let name = CodegenTypePathSegment::join(path.segments);
                CodegenIdentUsage::Module(CodegenIdent::new(&name)).to_tokens(tokens);
            }
        }
    }
}

/// A cleaned string that's valid for use as a Rust identifier.
///
/// Use [`CodegenIdentUsage`] to transform the identifier into
/// the correct idiomatic case.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdent(str);

impl CodegenIdent {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
}

/// An identifier that has been uniquified within a
/// [`UniqueIdents`] scope.
///
/// Only a scope can construct these, ensuring that identifiers
/// used as fields, variants, and parameters are collision-free.
/// Pass to [`CodegenIdentUsage`] variants to select the
/// appropriate case transformation.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct UniqueIdent(str);

impl UniqueIdent {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
}

impl Deref for UniqueIdent {
    type Target = CodegenIdent;

    #[inline]
    fn deref(&self) -> &Self::Target {
        CodegenIdent::new(&self.0)
    }
}

/// A Cargo feature for conditionally compiling generated code.
///
/// Feature names appear in the `Cargo.toml` `[features]` table,
/// and in `#[cfg(feature = "...")]` attributes. The special `default` feature
/// enables all other features.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CargoFeature {
    #[default]
    Default,
    Named(String),
}

impl CargoFeature {
    #[inline]
    pub fn from_name(name: &str) -> Self {
        match name {
            // `default` can't be used as a literal feature name; ignore it.
            "default" => Self::Default,
            name => Self::Named(clean(name)),
        }
    }

    #[inline]
    pub fn display(&self) -> impl Display {
        match self {
            Self::Named(name) => AsKebabCase(name.as_str()),
            Self::Default => AsKebabCase("default"),
        }
    }
}

/// Emits a [`CodegenIdent`] as an idiomatic Rust identifier.
///
/// Each [`CodegenIdentUsage`] variant determines the case transformation
/// applied to the identifier: module, field, parameter, and method names
/// become snake_case; type and enum variant names become PascalCase.
///
/// Implements [`ToTokens`] for use in [`quote`] macros. For string interpolation,
/// use [`display`](Self::display).
#[derive(Clone, Copy, Debug)]
pub enum CodegenIdentUsage<'a> {
    Module(&'a CodegenIdent),
    Type(&'a CodegenIdent),
    Field(&'a UniqueIdent),
    Variant(&'a UniqueIdent),
    Param(&'a UniqueIdent),
    Method(&'a UniqueIdent),
}

impl<'a> CodegenIdentUsage<'a> {
    /// Returns a formattable representation of this identifier.
    ///
    /// [`CodegenIdentUsage`] doesn't implement [`Display`] directly, to help catch
    /// context mismatches: using `.display()` in a [`quote`] macro, or
    /// `.to_token_stream()` in a [`format`] string, stands out during review.
    pub fn display(self) -> impl Display {
        struct DisplayUsage<'a>(CodegenIdentUsage<'a>);
        impl Display for DisplayUsage<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let s = self.0.as_str();
                if !s.starts_with(unicode_ident::is_xid_start) {
                    f.write_char('_')?;
                }
                match self.0 {
                    CodegenIdentUsage::Type(_) | CodegenIdentUsage::Variant(_) => {
                        write!(f, "{}", AsPascalCase(s))
                    }
                    CodegenIdentUsage::Module(_)
                    | CodegenIdentUsage::Field(_)
                    | CodegenIdentUsage::Param(_)
                    | CodegenIdentUsage::Method(_) => write!(f, "{}", AsSnekCase(s)),
                }
            }
        }
        DisplayUsage(self)
    }

    #[inline]
    fn as_str(&self) -> &str {
        match self {
            CodegenIdentUsage::Type(s) => &s.0,
            CodegenIdentUsage::Variant(s) => &s.0,
            CodegenIdentUsage::Module(s) => &s.0,
            CodegenIdentUsage::Field(s) => &s.0,
            CodegenIdentUsage::Param(s) => &s.0,
            CodegenIdentUsage::Method(s) => &s.0,
        }
    }
}

impl IdentFragment for CodegenIdentUsage<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl ToTokens for CodegenIdentUsage<'_> {
    #[inline]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let s = self.display().to_string();
        let ident = syn::parse_str(&s).unwrap_or_else(|_| Ident::new_raw(&s, Span::call_site()));
        tokens.append(ident);
    }
}

/// A scope for generating unique, valid Rust identifiers.
#[derive(Debug)]
pub struct UniqueIdents<'a>(UniqueNames<'a>);

impl<'a> UniqueIdents<'a> {
    /// Creates a new identifier scope that's backed by the given arena.
    #[inline]
    pub fn new(arena: &'a Arena) -> Self {
        Self::with_reserved(arena, &[])
    }

    /// Creates a new identifier scope that's backed by the given arena,
    /// with additional pre-reserved names.
    #[inline]
    pub fn with_reserved(arena: &'a Arena, reserved: &[&str]) -> Self {
        Self(UniqueNames::with_reserved(
            arena,
            itertools::chain!(
                reserved.iter().copied(),
                KEYWORDS.iter().copied(),
                std::iter::once("")
            ),
        ))
    }

    /// Cleans the input string and returns a name that's unique
    /// within this scope, and valid for any [`CodegenIdentUsage`].
    #[inline]
    pub fn name(&mut self, name: &str) -> &'a UniqueIdent {
        UniqueIdent::new(self.0.uniquify(&clean(name)))
    }

    /// Uniquifies a struct field name from a [`StructFieldNameHint`].
    pub fn field_name_hint(&mut self, hint: StructFieldNameHint) -> &'a UniqueIdent {
        use StructFieldNameHint::*;
        UniqueIdent::new(match hint {
            Index(index) => self.0.uniquify(&format!("variant_{index}")),
            AdditionalProperties => self.0.uniquify("additional_properties"),
        })
    }

    /// Uniquifies an untagged union variant name from an
    /// [`UntaggedVariantNameHint`].
    pub fn variant_name_hint(&mut self, hint: UntaggedVariantNameHint) -> &'a UniqueIdent {
        use {PrimitiveType::*, UntaggedVariantNameHint::*};
        UniqueIdent::new(match hint {
            Primitive(String) => self.0.uniquify("String"),
            Primitive(I8) => self.0.uniquify("I8"),
            Primitive(U8) => self.0.uniquify("U8"),
            Primitive(I16) => self.0.uniquify("I16"),
            Primitive(U16) => self.0.uniquify("U16"),
            Primitive(I32) => self.0.uniquify("I32"),
            Primitive(U32) => self.0.uniquify("U32"),
            Primitive(I64) => self.0.uniquify("I64"),
            Primitive(U64) => self.0.uniquify("U64"),
            Primitive(F32) => self.0.uniquify("F32"),
            Primitive(F64) => self.0.uniquify("F64"),
            Primitive(Bool) => self.0.uniquify("Bool"),
            Primitive(DateTime) => self.0.uniquify("DateTime"),
            Primitive(UnixTime) => self.0.uniquify("UnixTime"),
            Primitive(Date) => self.0.uniquify("Date"),
            Primitive(Url) => self.0.uniquify("Url"),
            Primitive(Uuid) => self.0.uniquify("Uuid"),
            Primitive(Bytes) => self.0.uniquify("Bytes"),
            Primitive(Binary) => self.0.uniquify("Binary"),
            Array => self.0.uniquify("Array"),
            Map => self.0.uniquify("Map"),
            Index(index) => self.0.uniquify(&format!("V{index}")),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenTypePathSegment<'a>(&'a InlineTypePathSegment<'a>);

impl<'a> CodegenTypePathSegment<'a> {
    /// Formats an inline type path as a string suitable for use as
    /// a Rust identifier.
    #[inline]
    fn join(segments: &[InlineTypePathSegment<'_>]) -> String {
        segments
            .iter()
            .map(CodegenTypePathSegment)
            .format_with("", |segment, f| f(&segment.display()))
            .to_string()
    }

    pub fn display(&self) -> impl Display {
        struct DisplaySegment<'a>(&'a InlineTypePathSegment<'a>);
        impl Display for DisplaySegment<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use InlineTypePathSegment::*;
                match self.0 {
                    // Segments are always part of an identifier, never emitted directly;
                    // so we don't need to check for `XID_Start`.
                    Operation(name) => write!(f, "{}", AsPascalCase(clean(name))),
                    Parameter(name) => write!(f, "{}", AsPascalCase(clean(name))),
                    Request => f.write_str("Request"),
                    Response => f.write_str("Response"),
                    Field(StructFieldName::Name(name)) => {
                        write!(f, "{}", AsPascalCase(clean(name)))
                    }
                    Field(StructFieldName::Hint(StructFieldNameHint::Index(index))) => {
                        write!(f, "Variant{index}")
                    }
                    Field(StructFieldName::Hint(StructFieldNameHint::AdditionalProperties)) => {
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

/// Makes a string suitable for inclusion within a Rust identifier.
///
/// Cleaning segments the string on word boundaries, collapses all
/// non-`XID_Continue` characters into new boundaries, and
/// reassembles the string. This makes the string resilient to
/// case transformations, which also collapse boundaries, and so
/// can produce duplicates in some cases.
///
/// Note that the result may not itself be a valid Rust identifier,
/// because Rust identifiers must start with `XID_Start`.
/// This is checked and handled in [`CodegenIdentUsage`].
#[inline]
fn clean(s: &str) -> String {
    WordSegments::new(s)
        .flat_map(|s| s.split(|c| !unicode_ident::is_xid_continue(c)))
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    // MARK: Cargo features

    #[test]
    fn test_feature_from_name() {
        let feature = CargoFeature::from_name("customers");
        assert_eq!(feature.display().to_string(), "customers");
    }

    #[test]
    fn test_feature_default() {
        let feature = CargoFeature::Default;
        assert_eq!(feature.display().to_string(), "default");

        let feature = CargoFeature::from_name("default");
        assert_eq!(feature, CargoFeature::Default);
    }

    #[test]
    fn test_features_from_multiple_words() {
        let feature = CargoFeature::from_name("foo_bar");
        assert_eq!(feature.display().to_string(), "foo-bar");

        let feature = CargoFeature::from_name("foo.bar");
        assert_eq!(feature.display().to_string(), "foo-bar");

        let feature = CargoFeature::from_name("fooBar");
        assert_eq!(feature.display().to_string(), "foo-bar");

        let feature = CargoFeature::from_name("FooBar");
        assert_eq!(feature.display().to_string(), "foo-bar");
    }

    // MARK: Usages

    #[test]
    fn test_codegen_ident_type() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("pet_store");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(PetStore);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_field() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("petStore");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(pet_store);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_module() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("MyModule");
        let usage = CodegenIdentUsage::Module(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(my_module);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_variant() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("http_error");
        let usage = CodegenIdentUsage::Variant(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(HttpError);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_param() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("userId");
        let usage = CodegenIdentUsage::Param(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(user_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("getUserById");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(get_user_by_id);
        assert_eq!(actual, expected);
    }

    // MARK: Special characters

    #[test]
    fn test_codegen_ident_handles_rust_keywords() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("type");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(r#type);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("123foo");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_123_foo);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(foo_bar_baz);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("1099KStatus");

        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099_k_status);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099KStatus);
        assert_eq!(actual, expected);
    }

    // MARK: Untagged variant names

    #[test]
    fn test_untagged_variant_name_index() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.variant_name_hint(UntaggedVariantNameHint::Index(0));
        assert_eq!(&ident.0, "V0");

        let ident = scope.variant_name_hint(UntaggedVariantNameHint::Index(42));
        assert_eq!(&ident.0, "V42");
    }

    // MARK: Struct field names

    #[test]
    fn test_struct_field_name_index() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident0 = scope.field_name_hint(StructFieldNameHint::Index(0));
        let usage = CodegenIdentUsage::Field(ident0);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(variant_0);
        assert_eq!(actual, expected);

        let ident5 = scope.field_name_hint(StructFieldNameHint::Index(5));
        let usage = CodegenIdentUsage::Field(ident5);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(variant_5);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_struct_field_name_additional_properties() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.field_name_hint(StructFieldNameHint::AdditionalProperties);
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(additional_properties);
        assert_eq!(actual, expected);
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

        assert_eq!(clean("_foo"), "foo");
        assert_eq!(clean("__foo"), "foo");

        // Digits are in `XID_Continue`, so they should be preserved.
        assert_eq!(clean("123foo"), "123_foo");
        assert_eq!(clean("9bar"), "9_bar");

        // Non-ASCII characters that are valid in identifiers should be preserved;
        // characters that aren't should be replaced.
        assert_eq!(clean("café"), "café");
        assert_eq!(clean("foo™bar"), "foo_bar");

        // Invalid characters should be collapsed.
        assert_eq!(clean("foo---bar"), "foo_bar");
        assert_eq!(clean("foo...bar"), "foo_bar");
    }

    // MARK: Scopes

    #[test]
    fn test_codegen_ident_scope_handles_empty() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.name("");

        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);
    }
}
