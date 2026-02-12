use itertools::Itertools;
use ploidy_core::ir::{InlineIrTypeView, IrOperationView, SchemaIrTypeView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature,
    enum_::CodegenEnum,
    naming::{CodegenTypeName, CodegenTypeNameSortKey},
    struct_::CodegenStruct,
    tagged::CodegenTagged,
    untagged::CodegenUntagged,
};

/// Generates an inline `mod types`, with definitions for all the inline types
/// that are reachable from a resource or schema type.
///
/// Inline types nested _within_ referenced schemas are excluded; those are
/// emitted by [`CodegenSchemaType`](crate::CodegenSchemaType) instead.
#[derive(Clone, Copy, Debug)]
pub enum CodegenInlines<'a> {
    Resource(&'a [IrOperationView<'a>]),
    Schema(&'a SchemaIrTypeView<'a>),
}

impl ToTokens for CodegenInlines<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Resource(ops) => {
                let items = CodegenInlineItems(IncludeCfgFeatures::Include, ops);
                items.to_tokens(tokens);
            }
            &Self::Schema(ty) => {
                let items = CodegenInlineItems(IncludeCfgFeatures::Omit, std::slice::from_ref(ty));
                items.to_tokens(tokens);
            }
        }
    }
}

#[derive(Debug)]
struct CodegenInlineItems<'a, V>(IncludeCfgFeatures, &'a [V]);

impl<'a, V> ToTokens for CodegenInlineItems<'a, V>
where
    V: View<'a>,
{
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut inlines = self.1.iter().flat_map(|op| op.inlines()).collect_vec();
        inlines.sort_by(|a, b| {
            CodegenTypeNameSortKey::for_inline(a).cmp(&CodegenTypeNameSortKey::for_inline(b))
        });

        let mut items = inlines.into_iter().filter_map(|view| {
            let name = CodegenTypeName::Inline(&view);
            let ty = match &view {
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
                InlineIrTypeView::Container(..)
                | InlineIrTypeView::Primitive(..)
                | InlineIrTypeView::Any(..) => {
                    // Container types, primitive types, and untyped values
                    // are emitted directly; they don't need type aliases.
                    return None;
                }
            };
            Some(match self.0 {
                IncludeCfgFeatures::Include => {
                    // Wrap each type in an inner inline module, so that
                    // the `#[cfg(...)]` applies to all items (types and `impl`s).
                    let cfg = CfgFeature::for_inline_type(&view);
                    let mod_name = name.into_module_name();
                    quote! {
                        #cfg
                        mod #mod_name {
                            #ty
                        }
                        #cfg
                        pub use #mod_name::*;
                    }
                }
                IncludeCfgFeatures::Omit => ty,
            })
        });

        if let Some(first) = items.next() {
            tokens.append_all(quote! {
                pub mod types {
                    #first
                    #(#items)*
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum IncludeCfgFeatures {
    Include,
    Omit,
}

#[cfg(test)]
mod tests {
    use super::*;

    use itertools::Itertools;
    use ploidy_core::{
        ir::{IrGraph, IrSpec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::graph::CodegenGraph;

    #[test]
    fn test_includes_inline_types_from_operation_parameters() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: filter
                      in: query
                      schema:
                        type: object
                        properties:
                          status:
                            type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_filter {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsFilter {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub status: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_filter::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_excludes_inline_types_from_referenced_schemas() {
        // The operation references `Item`, which has an inline type `Details`.
        // `Details` should _not_ be emitted by `CodegenInlines`; it belongs in
        // the schema's module instead.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Item'
            components:
              schemas:
                Item:
                  type: object
                  properties:
                    details:
                      type: object
                      properties:
                        description:
                          type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        // No inline types should be emitted, since the only inline (`Details`)
        // belongs to the referenced schema.
        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {};
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_sorts_inline_types_alphabetically() {
        // Parameters defined in reverse order: zebra, mango, apple.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: zebra
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                    - name: mango
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                    - name: apple
                      in: query
                      schema:
                        type: object
                        properties:
                          value:
                            type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        // Types should be sorted: Apple, Mango, Zebra.
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_apple {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsApple {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_apple::*;
                mod get_items_mango {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsMango {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_mango::*;
                mod get_items_zebra {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsZebra {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_zebra::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_no_output_when_no_inline_types() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: limit
                      in: query
                      schema:
                        type: integer
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {};
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_optionals() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: config
                      in: query
                      schema:
                        nullable: true
                        type: object
                        properties:
                          enabled:
                            type: boolean
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_config {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsConfig {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub enabled: ::ploidy_util::absent::AbsentOr<bool>,
                    }
                }
                pub use get_items_config::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_arrays() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: filters
                      in: query
                      schema:
                        type: array
                        items:
                          type: object
                          properties:
                            field:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_filters_item {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsFiltersItem {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub field: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_filters_item::*;
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_finds_inline_types_within_maps() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /items:
                get:
                  operationId: getItems
                  parameters:
                    - name: metadata
                      in: query
                      schema:
                        type: object
                        additionalProperties:
                          type: object
                          properties:
                            value:
                              type: string
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let ops = graph.operations().collect_vec();
        let inlines = CodegenInlines::Resource(&ops);

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_metadata_value {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                    #[serde(crate = "::ploidy_util::serde")]
                    pub struct GetItemsMetadataValue {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_metadata_value::*;
            }
        };
        assert_eq!(actual, expected);
    }
}
