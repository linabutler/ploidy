use std::{
    fmt::{Display, Write},
    ops::Deref,
};

use heck::{AsKebabCase, AsPascalCase, AsSnekCase};
use itertools::Itertools;
use ploidy_core::{
    arena::Arena,
    codegen::{UniqueNames, unique::WordSegments},
    ir::{PrimitiveType, StructFieldNameHint, UntaggedVariantNameHint},
};

use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt};
use ref_cast::{RefCastCustom, ref_cast_custom};

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// A cleaned string that's valid for use as a Rust identifier.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdent(str);

impl CodegenIdent {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
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
    pub fn ident(&mut self, name: &str) -> &'a UniqueIdent {
        UniqueIdent::new(self.0.uniquify(&clean(name)))
    }

    /// Uniquifies a struct field name from a [`StructFieldNameHint`].
    #[inline]
    pub fn field_name_hint(&mut self, hint: StructFieldNameHint) -> &'a UniqueIdent {
        use StructFieldNameHint::*;
        UniqueIdent::new(match hint {
            Index(index) => self.0.uniquify(&format!("variant_{index}")),
            AdditionalProperties => self.0.uniquify("additional_properties"),
        })
    }

    /// Uniquifies an untagged union variant name from an
    /// [`UntaggedVariantNameHint`].
    #[inline]
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
        let ident = scope.ident("pet_store");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(PetStore);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_field() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("petStore");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(pet_store);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_module() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("MyModule");
        let usage = CodegenIdentUsage::Module(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(my_module);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_variant() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("http_error");
        let usage = CodegenIdentUsage::Variant(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(HttpError);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_param() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("userId");
        let usage = CodegenIdentUsage::Param(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(user_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("getUserById");
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
        let ident = scope.ident("type");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(r#type);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("123foo");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_123_foo);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(foo_bar_baz);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.ident("1099KStatus");

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
        assert_eq!(&ident.0, "V");

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
        let expected: syn::Ident = parse_quote!(variant);
        assert_eq!(actual, expected);

        let ident5 = scope.field_name_hint(StructFieldNameHint::Index(5));
        let usage = CodegenIdentUsage::Field(ident5);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(variant5);
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
        let ident = scope.ident("");

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

        let ident = scope.ident("0");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1);
        assert_eq!(actual, expected);

        let ident = scope.ident("1");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_scope_handles_reserved_suffixes() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.ident("crate");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate2);
        assert_eq!(actual, expected);

        let ident = scope.ident("crate2");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate3);
        assert_eq!(actual, expected);
    }
}
