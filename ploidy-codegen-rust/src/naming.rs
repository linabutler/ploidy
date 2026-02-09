use std::{borrow::Cow, cmp::Ordering, fmt::Display, ops::Deref};

use heck::{AsKebabCase, AsPascalCase, AsSnekCase};
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
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use ref_cast::{RefCastCustom, ref_cast_custom};

// Keywords that can't be used as identifiers, even with `r#`.
const KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// A name for a schema or an inline type, used in generated Rust code.
///
/// [`CodegenTypeName`] is the high-level representation of a type name.
/// For emitting arbitrary identifiers, like fields, parameters, and methods,
/// use [`CodegenIdent`] and [`CodegenIdentUsage`] instead.
///
/// [`CodegenTypeName`] implements [`ToTokens`] to produce PascalCase identifiers
/// (e.g., `Pet`, `GetItemsFilter`) in [`quote`] macros.
/// Use [`into_module_name`](Self::into_module_name) for the corresponding module name,
/// and [`into_sort_key`](Self::into_sort_key) for deterministic sorting.
#[derive(Clone, Copy, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a SchemaIrTypeView<'a>),
    Inline(&'a InlineIrTypeView<'a>),
}

impl<'a> CodegenTypeName<'a> {
    #[inline]
    pub fn into_module_name(self) -> CodegenModuleName<'a> {
        CodegenModuleName(self)
    }

    #[inline]
    pub fn into_sort_key(self) -> CodegenTypeNameSortKey<'a> {
        CodegenTypeNameSortKey(self)
    }
}

impl ToTokens for CodegenTypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Schema(view) => {
                let ident = view.extensions().get::<CodegenIdent>().unwrap();
                CodegenIdentUsage::Type(&ident).to_tokens(tokens);
            }
            Self::Inline(view) => {
                let ident = CodegenIdent::from_segments(&view.path().segments);
                CodegenIdentUsage::Type(&ident).to_tokens(tokens);
            }
        }
    }
}

/// A module name derived from a [`CodegenTypeName`].
///
/// Implements [`ToTokens`] to produce a snake_case identifier. For
/// string interpolation (e.g., file paths), use [`display`](Self::display),
/// which returns an `impl Display` that can be used with `format!`.
#[derive(Clone, Copy, Debug)]
pub struct CodegenModuleName<'a>(CodegenTypeName<'a>);

impl<'a> CodegenModuleName<'a> {
    #[inline]
    pub fn into_type_name(self) -> CodegenTypeName<'a> {
        self.0
    }

    /// Returns a formattable representation of this module name.
    ///
    /// [`CodegenModuleName`] doesn't implement [`Display`] directly, to help catch
    /// context mismatches: using `.display()` in a [`quote`] macro, or
    /// `.to_token_stream()` in a [`format`] string, stands out during review.
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

impl ToTokens for CodegenModuleName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self.0 {
            CodegenTypeName::Schema(view) => {
                let ident = view.extensions().get::<CodegenIdent>().unwrap();
                CodegenIdentUsage::Module(&ident).to_tokens(tokens);
            }
            CodegenTypeName::Inline(view) => {
                let ident = CodegenIdent::from_segments(&view.path().segments);
                CodegenIdentUsage::Module(&ident).to_tokens(tokens);
            }
        }
    }
}

/// A sort key for deterministic ordering of [`CodegenTypeName`]s.
///
/// Sorts schema types before inline types, then lexicographically by name.
/// This ensures that code generation produces stable output regardless of
/// declaration order.
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

/// A string that's statically guaranteed to be valid for any
/// [`CodegenIdentUsage`].
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
        Self(format!(
            "{}",
            segments
                .iter()
                .map(CodegenTypePathSegment)
                .format_with("", |segment, f| f(&segment.display()))
        ))
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

/// A string slice that's guaranteed to be valid for any [`CodegenIdentUsage`].
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, RefCastCustom)]
#[repr(transparent)]
pub struct CodegenIdentRef(str);

impl CodegenIdentRef {
    #[ref_cast_custom]
    fn new(s: &str) -> &Self;
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
    Named(CodegenIdent),
}

impl CargoFeature {
    #[inline]
    pub fn from_name(name: &str) -> Self {
        match name {
            // `default` can't be used as a literal feature name; ignore it.
            "default" => Self::Default,

            // Cargo and crates.io limit which characters can appear in feature names;
            // further, we use feature names as module names for operations, so
            // the feature name needs to be usable as a Rust identifier.
            name => Self::Named(CodegenIdent::new(name)),
        }
    }

    #[inline]
    pub fn as_ident(&self) -> &CodegenIdentRef {
        match self {
            Self::Named(name) => name,
            Self::Default => CodegenIdentRef::new("default"),
        }
    }

    #[inline]
    pub fn display(&self) -> impl Display {
        match self {
            Self::Named(name) => AsKebabCase(name.0.as_str()),
            Self::Default => AsKebabCase("default"),
        }
    }
}

/// A context-aware wrapper for emitting a [`CodegenIdentRef`] as a Rust identifier.
///
/// [`CodegenIdentUsage`] is a lower-level building block for generating
/// identifiers. For schema and inline types, prefer [`CodegenTypeName`] instead.
///
/// Each [`CodegenIdentUsage`] variant determines the case transformation
/// applied to the identifier: module, field, parameter, and method names
/// become snake_case; type and enum variant names become PascalCase.
///
/// Implements [`ToTokens`] for use in [`quote`] macros. For string interpolation,
/// use [`display`](Self::display).
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
    ///
    /// [`CodegenIdentUsage`] doesn't implement [`Display`] directly, to help catch
    /// context mismatches: using `.display()` in a [`quote`] macro, or
    /// `.to_token_stream()` in a [`format`] string, stands out during review.
    pub fn display(self) -> impl Display {
        struct DisplayUsage<'a>(CodegenIdentUsage<'a>);
        impl Display for DisplayUsage<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use CodegenIdentUsage::*;
                match self.0 {
                    Module(name) | Field(name) | Param(name) | Method(name) => {
                        if name.0.starts_with(unicode_ident::is_xid_start) {
                            write!(f, "{}", AsSnekCase(&name.0))
                        } else {
                            // `name` doesn't start with `XID_Start` (e.g., "1099KStatus"),
                            // so prefix it with `_`; everything after is known to be
                            // `XID_Continue`.
                            write!(f, "_{}", AsSnekCase(&name.0))
                        }
                    }
                    Type(name) | Variant(name) => {
                        if name.0.starts_with(unicode_ident::is_xid_start) {
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

impl ToTokens for CodegenIdentUsage<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let s = self.display().to_string();
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
            IrStructFieldNameHint::AdditionalProperties => {
                CodegenIdentUsage::Field(CodegenIdentRef::new("additional_properties"))
                    .to_tokens(tokens)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenTypePathSegment<'a>(&'a InlineIrTypePathSegment<'a>);

impl<'a> CodegenTypePathSegment<'a> {
    pub fn display(&self) -> impl Display {
        struct DisplaySegment<'a>(&'a InlineIrTypePathSegment<'a>);
        impl Display for DisplaySegment<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use InlineIrTypePathSegment::*;
                match self.0 {
                    // Segments are always part of an identifier, never emitted directly;
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
                    Field(IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties)) => {
                        f.write_str("AdditionalProperties")
                    }
                    MapValue => f.write_str("Value"),
                    ArrayItem => f.write_str("Item"),
                    Variant(index) => write!(f, "V{index}"),
                    Parent(index) => write!(f, "P{index}"),
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

    #[test]
    fn test_struct_field_name_additional_properties() {
        let field_name = CodegenStructFieldName(IrStructFieldNameHint::AdditionalProperties);
        let actual: syn::Ident = parse_quote!(#field_name);
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
