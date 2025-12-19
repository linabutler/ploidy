use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::parse_quote;

use crate::{
    codegen::rust::CodegenIdent,
    ir::{InlineIrTypePathRoot, IrType, PrimitiveIrType},
};

use super::{context::CodegenContext, naming::CodegenTypeName};

#[derive(Clone, Copy, Debug)]
pub struct CodegenRef<'a> {
    context: &'a CodegenContext<'a>,
    ty: &'a IrType<'a>,
}

impl<'a> CodegenRef<'a> {
    pub fn new(context: &'a CodegenContext<'a>, ty: &'a IrType<'a>) -> Self {
        Self { context, ty }
    }
}

impl ToTokens for CodegenRef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self.ty {
            &IrType::Primitive(PrimitiveIrType::String) => quote! { ::std::string::String },
            &IrType::Primitive(PrimitiveIrType::I32) => quote! { i32 },
            &IrType::Primitive(PrimitiveIrType::I64) => quote! { i64 },
            &IrType::Primitive(PrimitiveIrType::F32) => quote! { f32 },
            &IrType::Primitive(PrimitiveIrType::F64) => quote! { f64 },
            &IrType::Primitive(PrimitiveIrType::Bool) => quote! { bool },
            &IrType::Primitive(PrimitiveIrType::DateTime) => {
                quote! { ::ploidy_util::date_time::UnixMilliseconds }
            }
            &IrType::Primitive(PrimitiveIrType::Date) => quote! { ::chrono::NaiveDate },
            &IrType::Primitive(PrimitiveIrType::Url) => quote! { ::url::Url },
            &IrType::Primitive(PrimitiveIrType::Uuid) => quote! { ::uuid::Uuid },
            &IrType::Primitive(PrimitiveIrType::Bytes) => quote! { ::bytes::Bytes },
            IrType::Array(ty) => {
                let ty = CodegenRef::new(self.context, ty.as_ref());
                quote! { ::std::vec::Vec<#ty> }
            }
            IrType::Map(ty) => {
                let ty = CodegenRef::new(self.context, ty.as_ref());
                quote! { ::std::collections::BTreeMap<::std::string::String, #ty> }
            }
            IrType::Ref(name) => {
                let name = self.context.map.ty(name);
                quote! { crate::types::#name }
            }
            IrType::Nullable(ty) => {
                let ty = CodegenRef::new(self.context, ty.as_ref());
                quote! { ::std::option::Option<#ty> }
            }
            IrType::Any => quote! { ::serde_json::Value },
            IrType::Inline(ty) => {
                let path = ty.path();
                let root: syn::Path = match &path.root {
                    InlineIrTypePathRoot::Resource(a) => {
                        let name = format_ident!("{}", a.to_snake_case());
                        parse_quote!(crate::client::#name::types)
                    }
                    InlineIrTypePathRoot::Type(a) => {
                        let m = CodegenIdent::Module(a);
                        parse_quote!(crate::types::#m::fields)
                    }
                };
                let name = CodegenTypeName::Inline(path);
                parse_quote!(#root::#name)
            }
            IrType::Schema(s) => {
                let name = self.context.map.ty(s.name());
                quote! { crate::types::#name }
            }
        })
    }
}

/// A reference from one type to another type that may require boxing.
#[derive(Clone, Copy, Debug)]
pub struct CodegenBoxedRef<'a> {
    context: &'a CodegenContext<'a>,
    from: CodegenTypeName<'a>,
    to: &'a IrType<'a>,
}

impl<'a> CodegenBoxedRef<'a> {
    pub fn new(context: &'a CodegenContext, from: CodegenTypeName<'a>, to: &'a IrType<'a>) -> Self {
        Self { context, from, to }
    }
}

impl ToTokens for CodegenBoxedRef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if let CodegenTypeName::Schema(from, _) = &self.from
            && let Some(this) = self.context.spec.lookup(from)
            && let IrType::Ref(to) = &self.to
            && let Some(other) = self.context.spec.lookup(to)
            && this.requires_indirection_to(other)
        {
            let inner = CodegenRef::new(self.context, other.ty());
            tokens.append_all(quote! { ::std::boxed::Box<#inner> });
        } else {
            CodegenRef::new(self.context, self.to).to_tokens(tokens);
        }
    }
}
