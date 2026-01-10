use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{
    codegen::IntoCode,
    ir::{InlineIrTypeView, SchemaIrTypeView, View},
};

use super::{
    enum_::CodegenEnum, naming::CodegenTypeName, struct_::CodegenStruct, tagged::CodegenTagged,
    untagged::CodegenUntagged,
};

/// Generates a module for a named schema type.
#[derive(Debug)]
pub struct CodegenSchemaType<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a SchemaIrTypeView<'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a SchemaIrTypeView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenSchemaType<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = self.name;
        let code = match self.ty {
            SchemaIrTypeView::Struct(_, view) => CodegenStruct::new(name, view).into_token_stream(),
            SchemaIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).into_token_stream(),
            SchemaIrTypeView::Tagged(_, view) => CodegenTagged::new(name, view).into_token_stream(),
            SchemaIrTypeView::Untagged(_, view) => {
                CodegenUntagged::new(name, view).into_token_stream()
            }
        };
        let mut inlines = self.ty.inlines().map(|view| match view {
            InlineIrTypeView::Enum(path, view) => {
                CodegenEnum::new(CodegenTypeName::Inline(path), &view).into_token_stream()
            }
            InlineIrTypeView::Struct(path, view) => {
                CodegenStruct::new(CodegenTypeName::Inline(path), &view).into_token_stream()
            }
            InlineIrTypeView::Untagged(path, view) => {
                CodegenUntagged::new(CodegenTypeName::Inline(path), &view).into_token_stream()
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
        let name = match self.name {
            CodegenTypeName::Schema(_, ident) => {
                format!("src/types/{}.rs", ident.module().to_token_stream())
            }
            CodegenTypeName::Inline(..) => {
                unreachable!("inline types shouldn't be written to disk")
            }
        };
        (name, self.into_token_stream())
    }
}
