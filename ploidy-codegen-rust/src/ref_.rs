use ploidy_core::ir::{InlineIrTypePathRoot, IrTypeView, PrimitiveIrType, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::parse_quote;

use super::{
    naming::CodegenTypeName,
    naming::{CodegenIdent, CodegenIdentUsage},
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenRef<'a> {
    ty: &'a IrTypeView<'a>,
}

impl<'a> CodegenRef<'a> {
    pub fn new(ty: &'a IrTypeView<'a>) -> Self {
        Self { ty }
    }
}

impl ToTokens for CodegenRef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(match self.ty {
            &IrTypeView::Primitive(PrimitiveIrType::String) => {
                quote! { ::std::string::String }
            }
            &IrTypeView::Primitive(PrimitiveIrType::I32) => {
                quote! { i32 }
            }
            &IrTypeView::Primitive(PrimitiveIrType::I64) => {
                quote! { i64 }
            }
            &IrTypeView::Primitive(PrimitiveIrType::F32) => {
                quote! { f32 }
            }
            &IrTypeView::Primitive(PrimitiveIrType::F64) => {
                quote! { f64 }
            }
            &IrTypeView::Primitive(PrimitiveIrType::Bool) => {
                quote! { bool }
            }
            &IrTypeView::Primitive(PrimitiveIrType::DateTime) => {
                quote! { ::ploidy_util::date_time::UnixMilliseconds }
            }
            &IrTypeView::Primitive(PrimitiveIrType::Date) => {
                quote! { ::chrono::NaiveDate }
            }
            &IrTypeView::Primitive(PrimitiveIrType::Url) => {
                quote! { ::url::Url }
            }
            &IrTypeView::Primitive(PrimitiveIrType::Uuid) => {
                quote! { ::uuid::Uuid }
            }
            &IrTypeView::Primitive(PrimitiveIrType::Bytes) => {
                quote! { ::bytes::Bytes }
            }
            IrTypeView::Array(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::vec::Vec<#ty> }
            }
            IrTypeView::Map(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::collections::BTreeMap<::std::string::String, #ty> }
            }
            IrTypeView::Nullable(ty) => {
                let inner = ty.inner();
                let ty = CodegenRef::new(&inner);
                quote! { ::std::option::Option<#ty> }
            }
            IrTypeView::Any => quote! { ::serde_json::Value },
            IrTypeView::Inline(ty) => {
                let path = ty.path();
                let root: syn::Path = match &path.root {
                    InlineIrTypePathRoot::Resource(name) => {
                        let ident = CodegenIdent::new(name);
                        let usage = CodegenIdentUsage::Module(&ident);
                        parse_quote!(crate::client::#usage::types)
                    }
                    InlineIrTypePathRoot::Type(name) => {
                        let ident = CodegenIdent::new(name);
                        let usage = CodegenIdentUsage::Module(&ident);
                        parse_quote!(crate::types::#usage::types)
                    }
                };
                let name = CodegenTypeName::Inline(ty);
                parse_quote!(#root::#name)
            }
            IrTypeView::Schema(view) => {
                let ext = view.extensions();
                let ident = ext.get::<CodegenIdent>().unwrap();
                let usage = CodegenIdentUsage::Type(&ident);
                quote! { crate::types::#usage }
            }
        })
    }
}
