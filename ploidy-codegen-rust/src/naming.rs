use std::{
    fmt::{Display, Write},
    ops::Deref,
};

use heck::{AsKebabCase, AsPascalCase, AsSnekCase};
use itertools::Itertools;
use ploidy_core::{
    arena::Arena,
    codegen::{UniqueNames, unique::WordSegments},
};

use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt};
use ref_cast::{RefCastCustom, ref_cast_custom};

/// Static identifiers for type and field names.
pub mod idents {
    use super::CodegenIdent;

    pub const ADDITIONAL_PROPERTIES: &CodegenIdent = CodegenIdent::new("additional_properties");
    pub const ARRAY: &CodegenIdent = CodegenIdent::new("Array");
    pub const BINARY: &CodegenIdent = CodegenIdent::new("Binary");
    pub const BOOL: &CodegenIdent = CodegenIdent::new("Bool");
    pub const BYTES: &CodegenIdent = CodegenIdent::new("Bytes");
    pub const DATE: &CodegenIdent = CodegenIdent::new("Date");
    pub const DATE_TIME: &CodegenIdent = CodegenIdent::new("DateTime");
    pub const F32: &CodegenIdent = CodegenIdent::new("F32");
    pub const F64: &CodegenIdent = CodegenIdent::new("F64");
    pub const I8: &CodegenIdent = CodegenIdent::new("I8");
    pub const I16: &CodegenIdent = CodegenIdent::new("I16");
    pub const I32: &CodegenIdent = CodegenIdent::new("I32");
    pub const I64: &CodegenIdent = CodegenIdent::new("I64");
    pub const MAP: &CodegenIdent = CodegenIdent::new("Map");
    pub const NONE: &CodegenIdent = CodegenIdent::new("None");
    pub const STRING: &CodegenIdent = CodegenIdent::new("String");
    pub const U8: &CodegenIdent = CodegenIdent::new("U8");
    pub const U16: &CodegenIdent = CodegenIdent::new("U16");
    pub const U32: &CodegenIdent = CodegenIdent::new("U32");
    pub const U64: &CodegenIdent = CodegenIdent::new("U64");
    pub const UNIX_TIME: &CodegenIdent = CodegenIdent::new("UnixTime");
    pub const URL: &CodegenIdent = CodegenIdent::new("Url");
    pub const UUID: &CodegenIdent = CodegenIdent::new("Uuid");
}

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// A cleaned string that's valid for use as a Rust identifier.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdent(str);

impl CodegenIdent {
    #[ref_cast_custom]
    const fn new(s: &str) -> &Self;
}

/// An identifier that's unique within its [`UniqueIdents`] scope.
///
/// Only a scope can construct these, ensuring that identifiers won't collide
/// within that scope. Pass a [`UniqueIdent`] to a [`CodegenIdentUsage`] variant
/// to emit it as an [`Ident`] token.
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
                    // `s` is an identifier fragment; ensure it starts with
                    // `XID_Start` to make it a valid identifier.
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
        // Assume `s` is a keyword that must be rendered as a raw identifier
        // if `parse_str` fails. A string that's not a valid identifier here
        // is a logic error.
        let ident = syn::parse_str(&s).unwrap_or_else(|_| Ident::new_raw(&s, Span::call_site()));
        tokens.append(ident);
    }
}

/// A key used to group a resource's operations into modules
/// and derive Cargo features for resource operations and types.
///
/// [`Named`] wraps a uniquified resource name; [`Default`] represents
/// operations and types without a resource name.
///
/// [`Named`]: Self::Named
/// [`Default`]: Self::Default
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResourceGroup<'a> {
    Named(&'a UniqueIdent),
    #[default]
    Default,
}

impl<'a> ResourceGroup<'a> {
    /// Returns the resource name for a [`Named`][Self::Named] group.
    #[inline]
    pub fn name(self) -> Option<&'a UniqueIdent> {
        match self {
            Self::Named(name) => Some(name),
            Self::Default => None,
        }
    }

    /// Returns whether this group represents operations and types
    /// without a resource name.
    #[inline]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}

/// Formats a uniquified resource name as a Cargo feature name.
#[derive(Clone, Copy, Debug)]
pub struct AsFeatureName<'a>(pub &'a UniqueIdent);

impl Display for AsFeatureName<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", AsKebabCase(&self.0.0))
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
            reserved.iter().chain(KEYWORDS).copied(),
        ))
    }

    /// Cleans and uniquifies an identifier.
    #[inline]
    pub fn reserve(&mut self, name: &str) -> &'a UniqueIdent {
        UniqueIdent::new(self.0.uniquify(&clean(name)))
    }

    /// Uniquifies an already-cleaned identifier.
    #[inline]
    pub fn reserve_ident(&mut self, name: &CodegenIdent) -> &'a UniqueIdent {
        UniqueIdent::new(self.0.uniquify(&name.0))
    }
}

/// Makes a valid Rust identifier fragment from a string.
///
/// Cleaning segments the string on word boundaries and collapses all
/// non-`XID_Continue` characters into new boundaries. This makes the fragment
/// resilient to Heck's case transformations, which also collapse boundaries,
/// and so can produce duplicates.
///
/// The result is a valid identifier fragment, but may not be a valid [`Ident`],
/// because Rust identifiers must start with `XID_Start`.
#[inline]
fn clean(s: &str) -> String {
    WordSegments::new(s)
        .flat_map(|(_, s)| s.split(|c| !unicode_ident::is_xid_continue(c)))
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    // MARK: Usages

    #[test]
    fn test_codegen_ident_type() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("pet_store");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(PetStore);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_field() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("petStore");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(pet_store);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_module() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("MyModule");
        let usage = CodegenIdentUsage::Module(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(my_module);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_variant() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("http_error");
        let usage = CodegenIdentUsage::Variant(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(HttpError);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_param() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("userId");
        let usage = CodegenIdentUsage::Param(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(user_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("getUserById");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(get_user_by_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method_preserves_numeric_boundary() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("get_fees1");

        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(get_fees_1);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(GetFees1);
        assert_eq!(actual, expected);
    }

    // MARK: Special characters

    #[test]
    fn test_codegen_ident_handles_rust_keywords() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("type");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(r#type);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("123foo");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_123_foo);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(foo_bar_baz);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.reserve("1099KStatus");

        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099_k_status);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099KStatus);
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
        assert_eq!(clean("foo123"), "foo_123");
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
        let ident = scope.reserve("");

        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_scope_handles_numeric_names() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.reserve("0");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1);
        assert_eq!(actual, expected);

        let ident = scope.reserve("1");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_scope_handles_reserved_suffixes() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.reserve("crate");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate_2);
        assert_eq!(actual, expected);

        let ident = scope.reserve("crate2");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate_3);
        assert_eq!(actual, expected);
    }
}
