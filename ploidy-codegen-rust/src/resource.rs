use heck::ToSnakeCase;
use itertools::Itertools;
use ploidy_core::{
    codegen::IntoCode,
    ir::{InlineIrTypePathRoot, InlineIrTypeView, IrOperationView},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    enum_::CodegenEnum, naming::CodegenTypeName, operation::CodegenOperation,
    struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
};

/// Generates a feature-gated `impl Client` block for a resource,
/// with all its operations.
pub struct CodegenResource<'a> {
    resource: &'a str,
    operations: &'a [IrOperationView<'a>],
}

impl<'a> CodegenResource<'a> {
    pub fn new(resource: &'a str, operations: &'a [IrOperationView<'a>]) -> Self {
        Self {
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
            .map(|view| CodegenOperation::new(view).into_token_stream())
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
            .collect_vec();
        inlines.sort_by(|a, b| {
            CodegenTypeName::Inline(a)
                .into_sort_key()
                .cmp(&CodegenTypeName::Inline(b).into_sort_key())
        });
        let mut inlines = inlines.into_iter().map(|view| {
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
