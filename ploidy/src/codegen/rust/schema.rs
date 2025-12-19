use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{
    codegen::IntoCode,
    ir::{InlineIrType, IrType, SchemaIrType},
};

use super::{
    context::CodegenContext, enum_::CodegenEnum, naming::CodegenTypeName, ref_::CodegenRef,
    struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
};

/// Generates a module for a named schema type.
#[derive(Clone, Copy, Debug)]
pub struct CodegenSchemaType<'a> {
    context: &'a CodegenContext<'a>,
    name: CodegenTypeName<'a>,
    ty: &'a SchemaIrType<'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(
        context: &'a CodegenContext<'a>,
        name: CodegenTypeName<'a>,
        ty: &'a SchemaIrType<'a>,
    ) -> Self {
        Self { context, name, ty }
    }
}

impl ToTokens for CodegenSchemaType<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let n = self.name;
        let code = match self.ty {
            SchemaIrType::Struct(_, ty) => {
                CodegenStruct::new(self.context, n, ty).into_token_stream()
            }
            SchemaIrType::Enum(_, ty) => CodegenEnum::new(n, ty).into_token_stream(),
            SchemaIrType::Tagged(_, ty) => {
                CodegenTagged::new(self.context, n, ty).into_token_stream()
            }
            SchemaIrType::Untagged(_, ty) => {
                CodegenUntagged::new(self.context, n, ty).into_token_stream()
            }
        };
        let mut inlines = self.ty.visit().map(|ty: &InlineIrType<'_>| match ty {
            InlineIrType::Enum(path, ty) => {
                CodegenEnum::new(CodegenTypeName::Inline(path), ty).into_token_stream()
            }
            InlineIrType::Struct(path, ty) => {
                CodegenStruct::new(self.context, CodegenTypeName::Inline(path), ty)
                    .into_token_stream()
            }
            InlineIrType::Untagged(path, ty) => {
                CodegenUntagged::new(self.context, CodegenTypeName::Inline(path), ty)
                    .into_token_stream()
            }
        });
        let fields_module = inlines.next().map(|head| {
            quote! {
                pub mod fields {
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
            CodegenTypeName::Schema(name, _) => {
                let info = &self.context.map.0[name];
                format!("src/types/{}.rs", info.module)
            }
            CodegenTypeName::Inline(..) => {
                unreachable!("inline types shouldn't be written to disk")
            }
        };
        (name, self.into_token_stream())
    }
}

/// Generates a module for a named schema type alias.
#[derive(Clone, Copy, Debug)]
pub struct CodegenSchemaTypeAlias<'a> {
    context: &'a CodegenContext<'a>,
    name: CodegenTypeName<'a>,
    ty: &'a IrType<'a>,
}

impl<'a> CodegenSchemaTypeAlias<'a> {
    pub fn new(
        context: &'a CodegenContext<'a>,
        name: CodegenTypeName<'a>,
        ty: &'a IrType<'a>,
    ) -> Self {
        Self { context, name, ty }
    }
}

impl ToTokens for CodegenSchemaTypeAlias<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ty_name = self.name;
        let ty_v = CodegenRef::new(self.context, self.ty);
        tokens.append_all(quote! {
            pub type #ty_name = #ty_v;
        });
    }
}

impl IntoCode for CodegenSchemaTypeAlias<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        let name = match self.name {
            CodegenTypeName::Schema(name, _) => {
                let info = &self.context.map.0[name];
                format!("src/types/{}.rs", info.module)
            }
            CodegenTypeName::Inline(..) => {
                unreachable!("inline types shouldn't be written to disk")
            }
        };
        (name, self.into_token_stream())
    }
}
