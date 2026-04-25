use itertools::Itertools;
use ploidy_core::ir::InlineTypeView;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature, enum_::CodegenEnum, graph::CodegenGraph, naming::CodegenTypeIdent,
    struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
};

/// Generates an inline `mod types` block from a pre-collected slice
/// of [`InlineTypeView`]s.
///
/// Sorts the views by name, dispatches each to its codegen type,
/// and wraps the result in `pub mod types { ... }`. Emits nothing
/// if there are no inline types that need definitions (containers,
/// primitives, and untyped values are emitted inline at their use
/// site and skipped here).
///
/// Use [`with_cfg`](Self::with_cfg) for resource modules, where
/// each inline type needs its own `#[cfg(feature = "...")]` gate.
#[derive(Debug)]
pub(crate) struct CodegenInlines<'a> {
    graph: &'a CodegenGraph<'a>,
    inlines: Vec<InlineTypeView<'a, 'a>>,
    cfg: bool,
}

impl<'a> CodegenInlines<'a> {
    /// Creates inline items without feature gates (for schema modules).
    pub fn new(
        graph: &'a CodegenGraph<'a>,
        inlines: impl Iterator<Item = InlineTypeView<'a, 'a>>,
    ) -> Self {
        let mut inlines = inlines.collect_vec();
        inlines.sort_by_key(|view| CodegenTypeIdent::Inline(view.path()));
        Self {
            graph,
            inlines,
            cfg: false,
        }
    }

    /// Creates inline items with feature gates (for resource modules).
    pub fn with_cfg(
        graph: &'a CodegenGraph<'a>,
        inlines: impl Iterator<Item = InlineTypeView<'a, 'a>>,
    ) -> Self {
        let mut inlines = inlines.collect_vec();
        inlines.sort_by_key(|view| CodegenTypeIdent::Inline(view.path()));
        Self {
            graph,
            inlines,
            cfg: true,
        }
    }
}

impl ToTokens for CodegenInlines<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let graph = self.graph;

        let mut items = self.inlines.iter().filter_map(|view| {
            let name = CodegenTypeIdent::Inline(view.path());
            let ty = match view {
                InlineTypeView::Struct(_, view) => {
                    CodegenStruct::new(graph, name, view).into_token_stream()
                }
                InlineTypeView::Enum(_, view) => {
                    CodegenEnum::new(graph, name, view).into_token_stream()
                }
                InlineTypeView::Tagged(_, view) => {
                    CodegenTagged::new(graph, name, view).into_token_stream()
                }
                InlineTypeView::Untagged(_, view) => {
                    CodegenUntagged::new(graph, name, view).into_token_stream()
                }
                InlineTypeView::Container(..)
                | InlineTypeView::Primitive(..)
                | InlineTypeView::Any(..) => {
                    // Container types, primitive types, and untyped values
                    // are emitted directly; they don't need type aliases.
                    return None;
                }
            };
            Some(if self.cfg {
                // Wrap each type in an inner module, so that the
                // `#[cfg(...)]` applies to all items (types and `impl`s).
                let cfg = CfgFeature::for_inline_type(view);
                let mod_name = name.into_module();
                quote! {
                    #cfg
                    mod #mod_name {
                        #ty
                    }
                    #cfg
                    pub use #mod_name::*;
                }
            } else {
                ty
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

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec, View},
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_filter {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsFilter {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

        let actual: syn::File = parse_quote!(#inlines);
        // Types should be sorted: Apple, Mango, Zebra.
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_apple {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsApple {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_apple::*;
                mod get_items_mango {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsMango {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_mango::*;
                mod get_items_zebra {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsZebra {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_config {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsConfig {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_filters_item {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsFiltersItem {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let inlines =
            CodegenInlines::with_cfg(&graph, graph.operations().flat_map(|op| op.inlines()));

        let actual: syn::File = parse_quote!(#inlines);
        let expected: syn::File = parse_quote! {
            pub mod types {
                mod get_items_metadata_value {
                    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                    #[serde(crate = "::ploidy_util::serde")]
                    #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                    pub struct GetItemsMetadataValue {
                        #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                        pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                    }
                }
                pub use get_items_metadata_value::*;
            }
        };
        assert_eq!(actual, expected);
    }
}
