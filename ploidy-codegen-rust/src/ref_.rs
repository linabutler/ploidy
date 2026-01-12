use heck::ToSnakeCase;
use ploidy_core::ir::{InlineIrTypePathRoot, IrTypeView, PrimitiveIrType, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};
use syn::parse_quote;

use super::{
    naming::CodegenTypeName,
    naming::{CodegenIdent, SchemaIdent},
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
                    InlineIrTypePathRoot::Resource(a) => {
                        let name = format_ident!("{}", a.to_snake_case());
                        parse_quote!(crate::client::#name::types)
                    }
                    InlineIrTypePathRoot::Type(a) => {
                        let m = CodegenIdent::Module(a);
                        parse_quote!(crate::types::#m::types)
                    }
                };
                let name = CodegenTypeName::Inline(path);
                parse_quote!(#root::#name)
            }
            IrTypeView::Schema(view) => {
                let ext = view.extensions();
                let idents = ext.get::<SchemaIdent>().unwrap();
                let name = idents.ty();
                quote! { crate::types::#name }
            }
        })
    }
}
