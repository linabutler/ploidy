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
    Var(&'a CodegenIdentRef),
    Method(&'a CodegenIdentRef),
}

impl Display for CodegenIdentUsage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(name)
            | Self::Field(name)
            | Self::Param(name)
            | Self::Method(name)
            | Self::Var(name) => {
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
