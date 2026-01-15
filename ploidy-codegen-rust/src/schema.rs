use ploidy_core::{
    codegen::IntoCode,
    ir::{InlineIrTypeView, SchemaIrTypeView, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    enum_::CodegenEnum,
    naming::{CodegenIdent, CodegenIdentUsage, CodegenTypeName},
    struct_::CodegenStruct,
    tagged::CodegenTagged,
    untagged::CodegenUntagged,
};

/// Generates a module for a named schema type.
#[derive(Debug)]
pub struct CodegenSchemaType<'a> {
    ty: &'a SchemaIrTypeView<'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(ty: &'a SchemaIrTypeView<'a>) -> Self {
        Self { ty }
    }
}

impl ToTokens for CodegenSchemaType<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = CodegenTypeName::Schema(self.ty);
        let code = match self.ty {
            SchemaIrTypeView::Struct(_, view) => CodegenStruct::new(name, view).into_token_stream(),
            SchemaIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).into_token_stream(),
            SchemaIrTypeView::Tagged(_, view) => CodegenTagged::new(name, view).into_token_stream(),
            SchemaIrTypeView::Untagged(_, view) => {
                CodegenUntagged::new(name, view).into_token_stream()
            }
        };
        let mut inlines = self.ty.inlines().map(|view| {
            let name = CodegenTypeName::Inline(&view);
            match &view {
                InlineIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).into_token_stream(),
                InlineIrTypeView::Struct(_, view) => {
                    CodegenStruct::new(name, view).into_token_stream()
                }
                InlineIrTypeView::Tagged(_, view) => {
                    CodegenTagged::new(name, view).into_token_stream()
                }
                InlineIrTypeView::Untagged(_, view) => {
                    CodegenUntagged::new(name, view).into_token_stream()
                }
            }
        });
        let fields_module = inlines.next().map(|head| {
            quote! {
                pub mod types {
                    #head
                    #(#inlines)*
                }
            }
        });
        tokens.append_all(quote! {
            #code
            #fields_module
        });
    }
}

impl IntoCode for CodegenSchemaType<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        let ident = self.ty.extensions().get::<CodegenIdent>().unwrap();
        let usage = CodegenIdentUsage::Module(&ident);
        (format!("src/types/{usage}.rs"), self.into_token_stream())
    }
}
