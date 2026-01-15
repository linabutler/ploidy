use std::{borrow::Cow, fmt::Display, ops::Deref};

use heck::{AsPascalCase, AsSnekCase};
use itertools::Itertools;
use ploidy_core::{
    codegen::{
        UniqueNames,
        unique::{UniqueNamesScope, WordSegments},
    },
    ir::{
        InlineIrTypePathSegment, InlineIrTypeView, IrStructFieldName, IrStructFieldNameHint,
        IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrTypeView, View,
    },
};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt, format_ident};
use ref_cast::{RefCastCustom, ref_cast_custom};

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

#[derive(Clone, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a SchemaIrTypeView<'a>),
    Inline(&'a InlineIrTypeView<'a>),
}

impl ToTokens for CodegenTypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Schema(view) => {
                let ident = view.extensions().get::<CodegenIdent>().unwrap();
                tokens.append_all(CodegenIdentUsage::Type(&ident).to_token_stream())
            }
            Self::Inline(view) => {
                let ident = view
                    .path()
                    .segments
                    .iter()
                    .map(CodegenTypePathSegment)
                    .map(|segment| format_ident!("{}", segment))
                    .reduce(|a, b| format_ident!("{}{}", a, b))
                    .unwrap();
                tokens.append(ident);
            }
        }
    }
}

/// A string that's statically guaranteed to be valid for any
/// [`CodegenIdentUsage`].
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Copy, Debug)]
pub enum CodegenIdentUsage<'a> {
    Module(&'a CodegenIdentRef),
    Type(&'a CodegenIdentRef),
    Field(&'a CodegenIdentRef),
    Variant(&'a CodegenIdentRef),
    Param(&'a CodegenIdentRef),
    Method(&'a CodegenIdentRef),
}

impl Display for CodegenIdentUsage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(name) | Self::Field(name) | Self::Param(name) | Self::Method(name) => {
                if name.0.starts_with(unicode_ident::is_xid_start) {
                    write!(f, "{}", AsSnekCase(&name.0))
                } else {
                    // `name` doesn't start with `XID_Start` (e.g., "1099KStatus"),
                    // so prefix it with `_`; everything after is known to be
                    // `XID_Continue`.
                    write!(f, "_{}", AsSnekCase(&name.0))
                }
            }
            Self::Type(name) | Self::Variant(name) => {
                if name.0.starts_with(unicode_ident::is_xid_start) {
                    write!(f, "{}", AsPascalCase(&name.0))
                } else {
                    write!(f, "_{}", AsPascalCase(&name.0))
                }
            }
        }
    }
}

impl ToTokens for CodegenIdentUsage<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let s = self.to_string();
        let ident = syn::parse_str(&s).unwrap_or_else(|_| Ident::new_raw(&s, Span::call_site()));
        tokens.append(ident);
    }
}

/// A scope for generating unique, valid Rust identifiers.
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
            std::iter::once("")
        )))
    }

    /// Cleans the input string and returns a name that's unique
    /// within this scope, and valid for any [`CodegenIdentUsage`].
    pub fn uniquify(&mut self, name: &str) -> CodegenIdent {
        CodegenIdent(self.0.uniquify(&clean(name)).into_owned())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenUntaggedVariantName(pub IrUntaggedVariantNameHint);

impl ToTokens for CodegenUntaggedVariantName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        use IrUntaggedVariantNameHint::*;
        let s = match self.0 {
            Primitive(PrimitiveIrType::String) => "String".into(),
            Primitive(PrimitiveIrType::I32) => "I32".into(),
            Primitive(PrimitiveIrType::I64) => "I64".into(),
            Primitive(PrimitiveIrType::F32) => "F32".into(),
            Primitive(PrimitiveIrType::F64) => "F64".into(),
            Primitive(PrimitiveIrType::Bool) => "Bool".into(),
            Primitive(PrimitiveIrType::DateTime) => "DateTime".into(),
            Primitive(PrimitiveIrType::Date) => "Date".into(),
            Primitive(PrimitiveIrType::Url) => "Url".into(),
            Primitive(PrimitiveIrType::Uuid) => "Uuid".into(),
            Primitive(PrimitiveIrType::Bytes) => "Bytes".into(),
            Array => "Array".into(),
            Map => "Map".into(),
            Index(index) => Cow::Owned(format!("V{index}")),
        };
        tokens.append(Ident::new(&s, Span::call_site()));
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenStructFieldName(pub IrStructFieldNameHint);

impl ToTokens for CodegenStructFieldName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.0 {
            IrStructFieldNameHint::Index(index) => {
                CodegenIdentUsage::Field(&CodegenIdent(format!("variant_{index}")))
                    .to_tokens(tokens)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenTypePathSegment<'a>(&'a InlineIrTypePathSegment<'a>);

impl IdentFragment for CodegenTypePathSegment<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use InlineIrTypePathSegment::*;
        match self.0 {
            // Segments are part of an inline type path that always has a root prefix,
            // so we don't need to check for `XID_Start`.
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
            MapValue => f.write_str("Value"),
            ArrayItem => f.write_str("Item"),
            Variant(index) => write!(f, "V{index}"),
        }
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

    // MARK: Usages

    #[test]
    fn test_codegen_ident_type() {
        let ident = CodegenIdent::new("pet_store");
        let usage = CodegenIdentUsage::Type(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(PetStore);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_field() {
        let ident = CodegenIdent::new("petStore");
        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(pet_store);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_module() {
        let ident = CodegenIdent::new("MyModule");
        let usage = CodegenIdentUsage::Module(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(my_module);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_variant() {
        let ident = CodegenIdent::new("http_error");
        let usage = CodegenIdentUsage::Variant(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(HttpError);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_param() {
        let ident = CodegenIdent::new("userId");
        let usage = CodegenIdentUsage::Param(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(user_id);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_method() {
        let ident = CodegenIdent::new("getUserById");
        let usage = CodegenIdentUsage::Method(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(get_user_by_id);
        assert_eq!(actual, expected);
    }

    // MARK: Special characters

    #[test]
    fn test_codegen_ident_handles_rust_keywords() {
        let ident = CodegenIdent::new("type");
        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(r#type);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_invalid_start_chars() {
        let ident = CodegenIdent::new("123foo");
        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_123_foo);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_special_chars() {
        let ident = CodegenIdent::new("foo-bar-baz");
        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(foo_bar_baz);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_codegen_ident_handles_number_prefix() {
        let ident = CodegenIdent::new("1099KStatus");

        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099_k_status);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_1099KStatus);
        assert_eq!(actual, expected);
    }

    // MARK: Untagged variant names

    #[test]
    fn test_untagged_variant_name_string() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(
            PrimitiveIrType::String,
        ));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(String);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_i32() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::I32));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(I32);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_i64() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::I64));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(I64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_f32() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::F32));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(F32);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_f64() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::F64));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(F64);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_bool() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::Bool));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Bool);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_datetime() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(
            PrimitiveIrType::DateTime,
        ));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(DateTime);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_date() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::Date));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Date);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_url() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::Url));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Url);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_uuid() {
        let variant_name =
            CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::Uuid));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Uuid);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_bytes() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Primitive(
            PrimitiveIrType::Bytes,
        ));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Bytes);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_index() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Index(0));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(V0);
        assert_eq!(actual, expected);

        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Index(42));
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(V42);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_array() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Array);
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Array);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_variant_name_map() {
        let variant_name = CodegenUntaggedVariantName(IrUntaggedVariantNameHint::Map);
        let actual: syn::Ident = parse_quote!(#variant_name);
        let expected: syn::Ident = parse_quote!(Map);
        assert_eq!(actual, expected);
    }

    // MARK: Struct field names

    #[test]
    fn test_struct_field_name_index() {
        let field_name = CodegenStructFieldName(IrStructFieldNameHint::Index(0));
        let actual: syn::Ident = parse_quote!(#field_name);
        let expected: syn::Ident = parse_quote!(variant_0);
        assert_eq!(actual, expected);

        let field_name = CodegenStructFieldName(IrStructFieldNameHint::Index(5));
        let actual: syn::Ident = parse_quote!(#field_name);
        let expected: syn::Ident = parse_quote!(variant_5);
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
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let ident = scope.uniquify("");

        let usage = CodegenIdentUsage::Field(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);

        let usage = CodegenIdentUsage::Type(&ident);
        let actual: syn::Ident = parse_quote!(#usage);
        let expected: syn::Ident = parse_quote!(_2);
        assert_eq!(actual, expected);
    }
}
