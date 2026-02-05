use ploidy_core::codegen::IntoCode;
use ploidy_core::ir::{ContainerView, SchemaIrTypeView};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    doc_attrs, enum_::CodegenEnum, inlines::CodegenInlines, naming::CodegenTypeName,
    ref_::CodegenRef, struct_::CodegenStruct, tagged::CodegenTagged, untagged::CodegenUntagged,
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
        let ty = match self.ty {
            SchemaIrTypeView::Struct(_, view) => CodegenStruct::new(name, view).into_token_stream(),
            SchemaIrTypeView::Enum(_, view) => CodegenEnum::new(name, view).into_token_stream(),
            SchemaIrTypeView::Tagged(_, view) => CodegenTagged::new(name, view).into_token_stream(),
            SchemaIrTypeView::Untagged(_, view) => {
                CodegenUntagged::new(name, view).into_token_stream()
            }
            SchemaIrTypeView::Container(_, ContainerView::Array(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! {
                    #doc_attrs
                    pub type #name = ::std::vec::Vec<#inner_ref>;
                }
            }
            SchemaIrTypeView::Container(_, ContainerView::Map(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! {
                    #doc_attrs
                    pub type #name = ::std::collections::BTreeMap<::std::string::String, #inner_ref>;
                }
            }
            SchemaIrTypeView::Container(_, ContainerView::Optional(inner)) => {
                let doc_attrs = inner.description().map(doc_attrs);
                let inner_ty = inner.ty();
                let inner_ref = CodegenRef::new(&inner_ty);
                quote! {
                    #doc_attrs
                    pub type #name = ::std::option::Option<#inner_ref>;
                }
            }
        };
        let inlines = CodegenInlines::Schema(self.ty);
        tokens.append_all(quote! {
            #ty
            #inlines
        });
    }
}

impl IntoCode for CodegenSchemaType<'_> {
    type Code = (String, TokenStream);

    fn into_code(self) -> Self::Code {
        let name = CodegenTypeName::Schema(self.ty);
        (
            format!("src/types/{}.rs", name.into_module_name().display()),
            self.into_token_stream(),
        )
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
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde")]
            pub struct Container {
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub zebra: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Zebra>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub mango: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Mango>,
                #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                pub apple: ::ploidy_util::absent::AbsentOr<crate::types::container::types::Apple>,
            }
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                #[serde(crate = "::ploidy_util::serde")]
                pub struct Apple {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                #[serde(crate = "::ploidy_util::serde")]
                pub struct Mango {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub name: ::ploidy_util::absent::AbsentOr<::std::string::String>,
                }
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                #[serde(crate = "::ploidy_util::serde")]
                pub struct Zebra {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "InvalidParameters");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `InvalidParameters`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type InvalidParameters = ::std::vec::Vec<crate::types::invalid_parameters::types::Item>;
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                #[serde(crate = "::ploidy_util::serde")]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Tags");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `Tags`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(schema);

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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Metadata");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `Metadata`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(schema);

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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        // `type: ["string", "null"]` becomes `Option<String>`.
        let schema = graph.schemas().find(|s| s.name() == "NullableString");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `NullableString`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableString = ::std::option::Option<::std::string::String>;
        };
        assert_eq!(actual, expected);

        // `type: ["array", "null"]` becomes `Option<Vec<String>>`.
        let schema = graph.schemas().find(|s| s.name() == "NullableArray");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `NullableArray`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableArray = ::std::option::Option<::std::vec::Vec<::std::string::String>>;
        };
        assert_eq!(actual, expected);

        // `type: ["object", "null"]` with `additionalProperties` becomes
        // `Option<BTreeMap<String, String>>`.
        let schema = graph.schemas().find(|s| s.name() == "NullableMap");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `NullableMap`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableMap = ::std::option::Option<::std::collections::BTreeMap<::std::string::String, ::std::string::String>>;
        };
        assert_eq!(actual, expected);

        // `oneOf` with an inline schema and `null` becomes an `Option<InlineStruct>`,
        // with the inline struct definition emitted in `mod types`.
        let schema = graph.schemas().find(|s| s.name() == "NullableOneOf");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `NullableOneOf`; got `{schema:?}`");
        };
        let codegen = CodegenSchemaType::new(schema);
        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            pub type NullableOneOf = ::std::option::Option<crate::types::nullable_one_of::types::V1>;
            pub mod types {
                #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
                #[serde(crate = "::ploidy_util::serde")]
                pub struct V1 {
                    #[serde(default, skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent",)]
                    pub value: ::ploidy_util::absent::AbsentOr<::std::string::String>,
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Tags");
        let Some(schema @ SchemaIrTypeView::Container(_, _)) = &schema else {
            panic!("expected container `Tags`; got `{schema:?}`");
        };

        let codegen = CodegenSchemaType::new(schema);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[doc = "A list of tags."]
            pub type Tags = ::std::vec::Vec<::std::string::String>;
        };
        assert_eq!(actual, expected);
    }
}
