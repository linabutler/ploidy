use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{
    codegen::IntoCode,
    ir::{InlineIrType, InlineIrTypePathRoot, IrOperationView},
};

use super::{
    context::CodegenContext, enum_::CodegenEnum, naming::CodegenTypeName,
    operation::CodegenOperation, struct_::CodegenStruct, untagged::CodegenUntagged,
};

/// Generates a feature-gated `impl Client` block for a resource,
/// with all its operations.
pub struct CodegenResource<'a> {
    context: &'a CodegenContext<'a>,
    resource: &'a str,
    operations: &'a [IrOperationView<'a>],
}

impl<'a> CodegenResource<'a> {
    pub fn new(
        context: &'a CodegenContext<'a>,
        resource: &'a str,
        operations: &'a [IrOperationView<'a>],
    ) -> Self {
        Self {
            context,
            resource,
            operations,
        }
    }
}

impl ToTokens for CodegenResource<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let feature_name = self.resource;
        let methods: Vec<TokenStream> = self
            .operations
            .iter()
            .map(|view| CodegenOperation::new(self.context, view.op()).into_token_stream())
            .collect();

        let mut inlines = self
            .operations
            .iter()
            .flat_map(|op| op.inlines())
            .filter(|ty| {
                // Only emit Rust definitions for inline types contained
                // within the operation. Inline types contained within schemas
                // that the operation _references_ will be generated as part of
                // `CodegenSchemaType`.
                matches!(ty.path().root, InlineIrTypePathRoot::Resource(r) if r == self.resource)
            })
            .map(|ty| match ty {
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
                pub mod types {
                    #head
                    #(#inlines)*
                }
            }
        });

        tokens.append_all(quote! {
            #[cfg(feature = #feature_name)]
            impl crate::client::Client {
                #(#methods)*
            }
            #fields_module
        });
    }
}

impl IntoCode for CodegenResource<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        (
            format!("src/client/{}.rs", self.resource.to_snake_case()),
            self.into_token_stream(),
        )
    }
}
