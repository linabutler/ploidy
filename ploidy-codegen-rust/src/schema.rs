use itertools::Itertools;
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
        let mut inlines = self.ty.inlines().collect_vec();
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

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{IrGraph, IrSpec, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::CodegenGraph;

    #[test]
    fn test_schema_inline_types_order() {
        // Inline types are defined in reverse alphabetical order (Zebra, Mango, Apple),
        // to verify that they're sorted in the output.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Container:
                  type: object
                  properties:
                    zebra:
                      type: object
                      properties:
                        name:
                          type: string
                    mango:
                      type: object
                      properties:
                        name:
                          type: string
                    apple:
                      type: object
                      properties:
                        name:
                          type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Container");
        let Some(schema @ SchemaIrTypeView::Struct(_, _)) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(schema);

        let actual: syn::File = parse_quote!(#codegen);
        // The struct fields remain in their original order (`zebra`, `mango`, `apple`),
        // but the inline types in `mod types` should be sorted alphabetically
        // (`Apple`, `Mango`, `Zebra`).
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub zebra: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Zebra>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub mango: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Mango>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub apple: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Apple>,
            }
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
                pub struct Apple {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
                pub struct Mango {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::serde::Serialize, ::serde::Deserialize)]
                pub struct Zebra {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }
}
