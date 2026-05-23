use ploidy_core::{
    codegen::IntoCode,
    ir::{ContainerView, HasTypeId, SchemaTypeView, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    doc_attrs, enum_::CodegenEnum, graph::CodegenGraph, inlines::CodegenInlines,
    naming::CodegenIdentUsage, primitive::CodegenPrimitive, ref_::CodegenRef,
    struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
};

/// Generates a module for a named schema type.
#[derive(Debug)]
pub struct CodegenSchemaType<'a> {
    graph: &'a CodegenGraph<'a>,
    ty: &'a SchemaTypeView<'a, 'a>,
}

impl<'a> CodegenSchemaType<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>, ty: &'a SchemaTypeView<'a, 'a>) -> Self {
        Self { graph, ty }
    }
}

impl ToTokens for CodegenSchemaType<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ty = match self.ty {
            SchemaTypeView::Struct(_, view) => {
                CodegenStruct::new(self.graph, view).into_token_stream()
            }
            SchemaTypeView::Enum(_, view) => CodegenEnum::new(self.graph, view).into_token_stream(),
            SchemaTypeView::Tagged(_, view) => {
                CodegenTagged::new(self.graph, view).into_token_stream()
            }
            SchemaTypeView::Untagged(_, view) => {
                CodegenUntagged::new(self.graph, view).into_token_stream()
            }
            SchemaTypeView::Container(_, ContainerView::Array(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let type_name = CodegenIdentUsage::Type(self.graph.ident(self.ty.id()));
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(self.graph, &inner_ty);
                quote! {
                    #doc_attrs
                    pub type #type_name = ::std::vec::Vec<#inner_ref>;
                }
            }
            SchemaTypeView::Container(_, ContainerView::Map(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let type_name = CodegenIdentUsage::Type(self.graph.ident(self.ty.id()));
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(self.graph, &inner_ty);
                quote! {
                    #doc_attrs
                    pub type #type_name = ::std::collections::BTreeMap<::std::string::String, #inner_ref>;
                }
            }
            SchemaTypeView::Container(_, ContainerView::Optional(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let type_name = CodegenIdentUsage::Type(self.graph.ident(self.ty.id()));
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(self.graph, &inner_ty);
                quote! {
                    #doc_attrs
                    pub type #type_name = ::std::option::Option<#inner_ref>;
                }
            }
            SchemaTypeView::Primitive(_, view) => {
                let type_name = CodegenIdentUsage::Type(self.graph.ident(self.ty.id()));
                let primitive = CodegenPrimitive::new(self.graph, view);
                quote! {
                    pub type #type_name = #primitive;
                }
            }
            SchemaTypeView::Any(_, _) => {
                let type_name = CodegenIdentUsage::Type(self.graph.ident(self.ty.id()));
                quote! {
                    pub type #type_name = ::ploidy_util::serde_json::Value;
                }
            }
        };
        let inlines = CodegenInlines::for_schema_inlines(self.graph, self.ty.inlines().collect());
        tokens.append_all(quote! {
            #ty
            #inlines
        });
    }
}

impl IntoCode for CodegenSchemaType<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        let mod_name = CodegenIdentUsage::Module(self.graph.ident(self.ty.id()));
        (
            format!("src/types/{}.rs", mod_name.display()),
            self.into_token_stream(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, SchemaTypeView, Spec},
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

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Container").unwrap();
        let SchemaTypeView::Struct(_, _) = &schema else {
            panic!("expected struct `Container`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        // The struct fields remain in their original order (`zebra`, `mango`, `apple`),
        // but the inline types in `mod types` should be sorted alphabetically
        // (`Apple`, `Mango`, `Zebra`).
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub zebra: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Zebra>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub mango: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Mango>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub apple: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Apple>,
            }
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Apple {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Mango {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Zebra {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_container_schema_emits_type_alias_with_inline_types() {
        // A named array of inline structs should emit a type alias for the array,
        // and a `mod types` with the inline type (linabutler/ploidy#30).
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                InvalidParameters:
                  type: array
                  items:
                    type: object
                    required:
                      - name
                      - reason
                    properties:
                      name:
                        type: string
                      reason:
                        type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("InvalidParameters").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `InvalidParameters`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type InvalidParameters = ::std::vec::Vec<crate::types::invalid_parameters::types::Item>;
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Item {
                    pub name: ::std::string::String,
                    pub reason: ::std::string::String,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_container_schema_emits_type_alias_without_inline_types() {
        // A named array of primitives should emit a type alias, and no `mod types`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Tags:
                  type: array
                  items:
                    type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Tags").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `Tags`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type Tags = ::std::vec::Vec<::std::string::String>;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_container_schema_map_emits_type_alias() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Metadata:
                  type: object
                  additionalProperties:
                    type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Metadata").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `Metadata`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type Metadata = ::std::collections::BTreeMap<::std::string::String, ::std::string::String>;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_container_nullable_schema() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                NullableString:
                  type: [string, 'null']
                NullableArray:
                  type: [array, 'null']
                  items:
                    type: string
                NullableMap:
                  type: [object, 'null']
                  additionalProperties:
                    type: string
                NullableOneOf:
                  oneOf:
                    - type: object
                      properties:
                        value:
                          type: string
                    - type: 'null'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        // `type: ["string", "null"]` becomes `Option<String>`.
        let schema = graph.schema("NullableString").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `NullableString`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(&graph, &schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableString = ::std::option::Option<::std::string::String>;
        };
        assert_eq!(actual, expected);

        // `type: ["array", "null"]` becomes `Option<Vec<String>>`.
        let schema = graph.schema("NullableArray").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `NullableArray`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(&graph, &schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableArray = ::std::option::Option<::std::vec::Vec<::std::string::String>>;
        };
        assert_eq!(actual, expected);

        // `type: ["object", "null"]` with `additionalProperties` becomes
        // `Option<BTreeMap<String, String>>`.
        let schema = graph.schema("NullableMap").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `NullableMap`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(&graph, &schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableMap = ::std::option::Option<::std::collections::BTreeMap<::std::string::String, ::std::string::String>>;
        };
        assert_eq!(actual, expected);

        // `oneOf` with an inline schema and `null` becomes an `Option<InlineStruct>`,
        // with the inline struct definition emitted in `mod types`.
        let schema = graph.schema("NullableOneOf").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `NullableOneOf`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(&graph, &schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableOneOf = ::std::option::Option<crate::types::nullable_one_of::types::Value>;
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Value {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_nullable_schema_value_name_collision() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                NullableThing:
                  oneOf:
                    - type: object
                      properties:
                        value:
                          type: object
                          properties:
                            id:
                              type: string
                    - type: 'null'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("NullableThing").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `NullableThing`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableThing = ::std::option::Option<crate::types::nullable_thing::types::Value>;
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Value {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub value: ::ploidy_util::absent::AbsentOr<crate::types::nullable_thing::types::Value2>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Value2 {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub id: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_container_schema_preserves_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Tags:
                  description: A list of tags.
                  type: array
                  items:
                    type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Tags").unwrap();
        let SchemaTypeView::Container(_, _) = &schema else {
            panic!("expected container `Tags`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[doc = "A list of tags."]
            pub type Tags = ::std::vec::Vec<::std::string::String>;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_case_colliding_fields_uniquify_inline_type_names() {
        // `fooBar` and `foo_bar` both normalize to `foo_bar` in Rust,
        // so the field names — and their inline type names — must be
        // uniquified to avoid collisions.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Qux:
                  type: object
                  properties:
                    fooBar:
                      type: array
                      items:
                        type: object
                        properties:
                          zoom:
                            type: string
                    foo_bar:
                      type: array
                      items:
                        type: object
                        properties:
                          blagh:
                            type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Qux").unwrap();
        let SchemaTypeView::Struct(_, _) = &schema else {
            panic!("expected struct `Qux`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Qux {
                #[serde(rename = "fooBar", default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                #[ploidy(pointer(rename = "fooBar"))]
                pub foo_bar: ::ploidy_util::absent::AbsentOr<::std::vec::Vec<crate::types::qux::types::FooBarItem>>,
                #[serde(rename = "foo_bar", default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                #[ploidy(pointer(rename = "foo_bar"))]
                pub foo_bar_2: ::ploidy_util::absent::AbsentOr<::std::vec::Vec<crate::types::qux::types::FooBar2Item>>,
            }
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct FooBar2Item {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub blagh: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct FooBarItem {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub zoom: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_colliding_inline_paths_uniquify_inline_type_names() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Collision API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Qux:
                  type: object
                  properties:
                    fooItem:
                      type: object
                      properties:
                        direct:
                          type: string
                    foo:
                      type: array
                      items:
                        type: object
                        properties:
                          nested:
                            type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Qux").unwrap();
        let SchemaTypeView::Struct(_, _) = &schema else {
            panic!("expected struct `Qux`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
            pub struct Qux {
                #[serde(rename = "fooItem", default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                #[ploidy(pointer(rename = "fooItem"))]
                pub foo_item: ::ploidy_util::absent::AbsentOr<crate::types::qux::types::FooItem>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                pub foo: ::ploidy_util::absent::AbsentOr<::std::vec::Vec<crate::types::qux::types::FooItem2>>,
            }
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct FooItem {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub direct: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct FooItem2 {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub nested: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_common_inline_field_codegen() {
        // `metadata` is a common field on the tagged union itself.
        // Its inline object type is reached through a `Field` edge
        // whose parent is a tagged view, not a struct view.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    kind:
                      type: string
                    bark:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      dog: '#/components/schemas/Dog'
                  properties:
                    metadata:
                      type: object
                      properties:
                        source:
                          type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, _) = &schema else {
            panic!("expected tagged `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(&graph, &schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "kind")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "kind"))]
            pub enum Pet {
                #[serde(rename = "dog")]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::Dog),
            }

            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }

            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
                #[serde(crate = "::ploidy_util::serde")]
                #[ploidy(pointer(crate = "::ploidy_util::pointer"))]
                pub struct Metadata {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent")]
                    pub source: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
            }
        };
        assert_eq!(actual, expected);
    }
}
