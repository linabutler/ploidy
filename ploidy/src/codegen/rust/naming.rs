use std::borrow::Cow;

use heck::{ToPascalCase, ToSnakeCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{IdentFragment, ToTokens, TokenStreamExt, format_ident};

use crate::ir::{
    InlineIrTypePath, InlineIrTypePathSegment, IrUntaggedVariantNameHint, PrimitiveIrType,
};

#[derive(Clone, Copy, Debug)]
pub enum CodegenTypeName<'a> {
    Schema(&'a str, &'a Ident),
    Inline(&'a InlineIrTypePath<'a>),
}

impl ToTokens for CodegenTypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            &Self::Schema(_, ident) => tokens.append(ident.clone()),
            Self::Inline(path) => {
                let ident = path
                    .segments
                    .iter()
                    .map(CodegenTypePathSegment)
                    .fold(None, |ident, segment| {
                        Some(match ident {
                            Some(ident) => format_ident!("{}{}", ident, segment),
                            None => format_ident!("{}", segment),
                        })
                    })
                    .ok_or_else(|| syn::Error::new(Span::call_site(), "empty inline type path"));
                match ident {
                    Ok(ident) => tokens.append(ident),
                    Err(err) => tokens.append_all(err.into_compile_error()),
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CodegenIdent<'a> {
    Module(&'a str),
    Type(&'a str),
    Field(&'a str),
    Variant(&'a str),
    Param(&'a str),
    Method(&'a str),
}

impl<'a> CodegenIdent<'a> {
    fn name(&self) -> &'a str {
        let (Self::Module(s)
        | Self::Type(s)
        | Self::Field(s)
        | Self::Variant(s)
        | Self::Param(s)
        | Self::Method(s)) = self;
        s
    }
}

impl ToTokens for CodegenIdent<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let cased = match self {
            Self::Module(name) | Self::Field(name) | Self::Param(name) | Self::Method(name) => {
                name.to_snake_case()
            }
            Self::Type(name) | Self::Variant(name) => name.to_pascal_case(),
        };
        let cleaned = clean(&cased);
        let ident: syn::Result<Ident> = syn::parse_str(&cleaned)
            .or_else(|_| syn::parse_str(&format!("r#{cleaned}")))
            .or_else(|_| syn::parse_str(&format!("{cleaned}_")))
            .map_err(|_| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "`{}` can't be represented as a Rust identifier",
                        self.name()
                    ),
                )
            });
        match ident {
            Ok(ident) => tokens.append(ident),
            Err(err) => tokens.append_all(err.into_compile_error()),
        }
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
pub struct CodegenTypePathSegment<'a>(&'a InlineIrTypePathSegment<'a>);

impl IdentFragment for CodegenTypePathSegment<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.0 {
            InlineIrTypePathSegment::Operation(name) => f.write_str(&name.to_pascal_case()),
            InlineIrTypePathSegment::Parameter(name) => f.write_str(&name.to_pascal_case()),
            InlineIrTypePathSegment::Request => f.write_str("Request"),
            InlineIrTypePathSegment::Response => f.write_str("Response"),
            InlineIrTypePathSegment::Field(name) => f.write_str(&name.to_pascal_case()),
            InlineIrTypePathSegment::MapValue => f.write_str("Value"),
            InlineIrTypePathSegment::ArrayItem => f.write_str("Item"),
            InlineIrTypePathSegment::Variant(index) => write!(f, "V{index}"),
        }
    }
}

pub fn clean(s: &str) -> String {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut string = String::with_capacity(s.len());
    if first == '_' || unicode_ident::is_xid_start(first) {
        string.push(first);
    } else {
        string.push('_');
        chars = s.chars();
    }
    string.push_str(
        &chars
            .as_str()
            .replace(|next| !unicode_ident::is_xid_continue(next), "_"),
    );
    string
}
