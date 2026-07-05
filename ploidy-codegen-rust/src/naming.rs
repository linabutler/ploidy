use std::fmt::{Display, Formatter, Result as FmtResult, Write};

use ploidy_core::{
    arena::Arena,
    codegen::{AsKebabCase, AsPascalCase, AsSnakeCase, NamePart, UniqueName, UniqueNames},
};

use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt};
use unicode_ident::{is_xid_continue, is_xid_start};

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// An identifier that's unique within its [`UniqueIdents`] scope.
///
/// Only a scope can construct these, ensuring that identifiers won't collide
/// within that scope. Pass a [`UniqueIdent`] to a [`CodegenIdentUsage`] variant
/// to emit it as an [`Ident`] token.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UniqueIdent<'a>(UniqueName<'a>);

/// Emits a [`UniqueIdent`] as an idiomatic Rust identifier.
///
/// Each [`CodegenIdentUsage`] variant determines the case transformation
/// applied to the identifier: module, field, parameter, and method names
/// become snake_case; type and enum variant names become PascalCase.
///
/// Implements [`ToTokens`] for use in [`quote`] macros. For string interpolation,
/// use [`display`](Self::display).
#[derive(Clone, Copy, Debug)]
pub enum CodegenIdentUsage<'a> {
    Module(UniqueIdent<'a>),
    Type(UniqueIdent<'a>),
    Field(UniqueIdent<'a>),
    Variant(UniqueIdent<'a>),
    Param(UniqueIdent<'a>),
    Method(UniqueIdent<'a>),
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
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                let name = self.0.to_name();
                if !name.first_char().is_some_and(is_xid_start) {
                    // Rust identifiers must start with an `XID_Start` character
                    // or `_`. `clean()` explicitly treats `_` as a separator,
                    // so prepending `_` to the unique name here is guaranteed
                    // not to collide with any other identifier.
                    f.write_char('_')?;
                }
                match self.0 {
                    CodegenIdentUsage::Type(_) | CodegenIdentUsage::Variant(_) => {
                        write!(f, "{}", AsPascalCase(name))
                    }
                    CodegenIdentUsage::Module(_)
                    | CodegenIdentUsage::Field(_)
                    | CodegenIdentUsage::Param(_)
                    | CodegenIdentUsage::Method(_) => {
                        write!(f, "{}", AsSnakeCase(name))
                    }
                }
            }
        }
        DisplayUsage(self)
    }

    #[inline]
    fn to_name(self) -> UniqueName<'a> {
        match self {
            CodegenIdentUsage::Type(s) => s.0,
            CodegenIdentUsage::Variant(s) => s.0,
            CodegenIdentUsage::Module(s) => s.0,
            CodegenIdentUsage::Field(s) => s.0,
            CodegenIdentUsage::Param(s) => s.0,
            CodegenIdentUsage::Method(s) => s.0,
        }
    }
}

impl IdentFragment for CodegenIdentUsage<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
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
    Named(UniqueIdent<'a>),
    #[default]
    Default,
}

impl<'a> ResourceGroup<'a> {
    /// Returns the resource name for a [`Named`][Self::Named] group.
    #[inline]
    pub fn name(self) -> Option<UniqueIdent<'a>> {
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
pub struct AsFeatureName<'a>(pub UniqueIdent<'a>);

impl Display for AsFeatureName<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", AsKebabCase(self.0.0))
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
        let names = UniqueNames::with_reserved(
            arena,
            reserved.iter().chain(KEYWORDS).map(|name| clean(name)),
        );
        Self(names)
    }

    /// Uniquifies an identifier fragment.
    #[inline]
    pub fn claim(&mut self, name: &str) -> UniqueIdent<'a> {
        UniqueIdent(self.0.claim(clean(name)))
    }

    /// Uniquifies an identifier from another scope.
    #[inline]
    pub fn adopt(&mut self, ident: UniqueIdent<'a>) -> UniqueIdent<'a> {
        UniqueIdent(self.0.adopt(ident.0))
    }
}

/// Splits an identifier fragment into name parts for [`UniqueNames`].
///
/// Returns non-empty text spans as text parts, with one boundary
/// between adjacent spans. Text spans contain all `XID_Continue` characters
/// except `_`; all others are separators. Leading, trailing, and repeated
/// separators are discarded.
#[inline]
fn clean(s: &str) -> impl Iterator<Item = NamePart<'_>> {
    use itertools::intersperse;
    intersperse(
        s.split(|c| c == '_' || !is_xid_continue(c))
            .filter(|s| !s.is_empty())
            .map(NamePart::Text),
        NamePart::Boundary,
    )
}

/// Returns the UpperCamelCase name for an HTTP status code, used for
/// result-enum variants and status-suffixed inline type names.
///
/// Statuses with well-known reason phrases map to those phrases
/// (`200` → `Ok`, `304` → `NotModified`); anything else falls back
/// to `Status{code}`.
pub(crate) fn status_variant_name(status: u16) -> String {
    match status {
        100 => "Continue".to_owned(),
        101 => "SwitchingProtocols".to_owned(),
        102 => "Processing".to_owned(),
        103 => "EarlyHints".to_owned(),
        200 => "Ok".to_owned(),
        201 => "Created".to_owned(),
        202 => "Accepted".to_owned(),
        203 => "NonAuthoritativeInformation".to_owned(),
        204 => "NoContent".to_owned(),
        205 => "ResetContent".to_owned(),
        206 => "PartialContent".to_owned(),
        207 => "MultiStatus".to_owned(),
        208 => "AlreadyReported".to_owned(),
        226 => "ImUsed".to_owned(),
        300 => "MultipleChoices".to_owned(),
        301 => "MovedPermanently".to_owned(),
        302 => "Found".to_owned(),
        303 => "SeeOther".to_owned(),
        304 => "NotModified".to_owned(),
        305 => "UseProxy".to_owned(),
        307 => "TemporaryRedirect".to_owned(),
        308 => "PermanentRedirect".to_owned(),
        _ => format!("Status{status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    // MARK: Usages

    #[test]
    fn test_codegen_ident_type() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("pet_store");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(PetStore);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_field() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("petStore");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(pet_store);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_module() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("MyModule");
        let usage = CodegenIdentUsage::Module(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(my_module);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_variant() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("http_error");
        let usage = CodegenIdentUsage::Variant(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(HttpError);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_param() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("userId");
        let usage = CodegenIdentUsage::Param(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(user_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("getUserById");
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
        let ident = scope.claim("type");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(r#type);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("123foo");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_123foo);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(foo_bar_baz);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("1099KStatus");

        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099k_status);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099KStatus);
        assert_eq!(actual, expected);
    }

    // MARK: `clean()`

    #[test]
    fn test_clean_classifies_identifier_parts() {
        use NamePart::{Boundary, Text};

        assert_eq!(
            clean("foo-bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
        assert_eq!(
            clean("foo.bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
        assert_eq!(
            clean("foo bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
        assert_eq!(
            clean("foo@bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );

        assert_eq!(
            clean("foo_bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
        assert_eq!(clean("FooBar").collect_vec(), [Text("FooBar")]);
        assert_eq!(clean("foo123").collect_vec(), [Text("foo123")]);
        assert_eq!(clean("_foo").collect_vec(), [Text("foo")]);
        assert_eq!(clean("__foo").collect_vec(), [Text("foo")]);

        assert_eq!(clean("123foo").collect_vec(), [Text("123foo")]);
        assert_eq!(clean("9bar").collect_vec(), [Text("9bar")]);

        assert_eq!(clean("caf\u{e9}").collect_vec(), [Text("caf\u{e9}")]);
        assert_eq!(
            clean("foo\u{2122}bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );

        assert_eq!(
            clean("foo---bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
        assert_eq!(
            clean("foo...bar").collect_vec(),
            [Text("foo"), Boundary, Text("bar")]
        );
    }

    // MARK: Edge cases

    #[test]
    fn test_codegen_ident_empty() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("");

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
    fn test_codegen_ident_numeric_names() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.claim("0");
        let usage = CodegenIdentUsage::Field(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1);
        assert_eq!(actual, expected);

        let ident = scope.claim("1");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_reserved_suffixes() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.claim("crate");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate_2);
        assert_eq!(actual, expected);

        let ident = scope.claim("crate2");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(crate3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_respects_existing_numeric_suffix_boundary() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("get_fees1");

        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(get_fees1);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(GetFees1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_collapses_letter_digit_boundaries() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let ident = scope.claim("s3Upload");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(s3_upload);
        assert_eq!(actual, expected);

        let ident = scope.claim("x509Cert");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(x509_cert);
        assert_eq!(actual, expected);

        let ident = scope.claim("sha256Digest");
        let usage = CodegenIdentUsage::Method(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(sha256_digest);
        assert_eq!(actual, expected);

        let ident = scope.claim("http2Protocol");
        let usage = CodegenIdentUsage::Type(ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Http2Protocol);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_reserves_numeric_suffix_slots() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let first = scope.claim("Response2");
        let usage = CodegenIdentUsage::Type(first);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Response2);
        assert_eq!(actual, expected);

        let second = scope.claim("Response_2");
        let usage = CodegenIdentUsage::Type(second);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Response3);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Method(first);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(response2);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Method(second);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(response_3);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_deduplicates_internal_numeric_boundaries() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let compact = scope.claim("Http2Protocol");
        let usage = CodegenIdentUsage::Type(compact);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Http2Protocol);
        assert_eq!(actual, expected);

        let explicit = scope.claim("Http_2Protocol");
        let usage = CodegenIdentUsage::Type(explicit);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Http2Protocol2);
        assert_eq!(actual, expected);

        let compact = scope.claim("Http2ProtocolVariant");
        let usage = CodegenIdentUsage::Variant(compact);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Http2ProtocolVariant);
        assert_eq!(actual, expected);

        let explicit = scope.claim("Http_2ProtocolVariant");
        let usage = CodegenIdentUsage::Variant(explicit);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Http2ProtocolVariant2);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_adopt_preserves_numeric_suffix_boundary() {
        let arena = Arena::new();
        let mut schema_scope = UniqueIdents::new(&arena);
        let schema_ident = schema_scope.claim("Response_2");

        let mut variant_scope = UniqueIdents::new(&arena);
        let response = variant_scope.claim("Response");
        let variant_ident = variant_scope.adopt(schema_ident);

        let usage = CodegenIdentUsage::Variant(response);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Response);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Variant(variant_ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(Response2);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Method(variant_ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(response_2);
        assert_eq!(actual, expected);
    }

    // MARK: Cargo features

    #[test]
    fn test_feature_name_respects_existing_numeric_suffix_boundary() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let ident = scope.claim("get_fees1");

        assert_eq!(AsFeatureName(ident).to_string(), "get-fees1");
    }

    #[test]
    fn test_feature_name_collapses_letter_digit_boundaries() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);

        let compact = scope.claim("oauth2Token");
        assert_eq!(AsFeatureName(compact).to_string(), "oauth2-token");

        let explicit = scope.claim("oauth_2_token");
        assert_eq!(AsFeatureName(explicit).to_string(), "oauth-2-token-2");
    }
}
