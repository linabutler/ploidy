use ploidy_core::{
    codegen::UniqueNames,
    ir::{UntaggedView, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    naming::{CodegenIdentRef, CodegenIdentScope, CodegenIdentUsage, CodegenTypeName},
    ref_::CodegenRef,
};

#[derive(Clone, Debug)]
pub struct CodegenUntagged<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a UntaggedView<'a>,
}

impl<'a> CodegenUntagged<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a UntaggedView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenUntagged<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let mut variants = Vec::new();

        for variant in self.ty.variants() {
            match variant.ty() {
                Some(variant) => {
                    let base = CodegenIdentRef::from_variant_name_hint(variant.hint);
                    let ident = scope.uniquify_ident(&base);
                    let variant_name = CodegenIdentUsage::Variant(&ident);
                    let rust_type = CodegenRef::new(&variant.view);
                    variants.push(quote! { #variant_name(#rust_type) });
                }
                None => {
                    let ident = scope.uniquify("None");
                    let variant_name = CodegenIdentUsage::Variant(&ident);
                    variants.push(quote! { #variant_name });
                }
            }
        }

        let type_name_ident = &self.name;
        let doc_attrs = self.ty.description().map(doc_attrs);

        let mut extra_derives = vec![];

        // Derive `Eq` and `Hash` if all variants are transitively hashable.
        if self.ty.hashable() {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum #type_name_ident {
                #(#variants),*
            }
        })
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
    fn test_untagged_union_serde_untagged_attr() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrInt:
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum StringOrInt {
                String(::std::string::String),
                I32(i32)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_type_array() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                DateOrUnix:
                  type: [string, integer]
                  format: date-time
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "DateOrUnix");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `DateOrUnix`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum DateOrUnix {
                DateTime(::ploidy_util::chrono::DateTime<::ploidy_util::chrono::Utc>),
                I32(i32)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_with_refs() {
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
                    bark:
                      type: string
                Cat:
                  type: object
                  properties:
                    meow:
                      type: string
                Animal:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "Animal");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum Animal {
                V1(crate::types::Dog),
                V2(crate::types::Cat)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrInt:
                  description: A union that can be either a string or an integer.
                  oneOf:
                    - type: string
                    - type: integer
                      format: int32
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[doc = "A union that can be either a string or an integer."]
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum StringOrInt {
                String(::std::string::String),
                I32(i32)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_not_hashable_with_f32() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrFloat:
                  oneOf:
                    - type: string
                    - type: number
                      format: float
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "StringOrFloat");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrFloat`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum StringOrFloat {
                String(::std::string::String),
                F32(f32)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_not_hashable_with_f64() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                StringOrDouble:
                  oneOf:
                    - type: string
                    - type: number
                      format: double
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "StringOrDouble");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrDouble`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum StringOrDouble {
                String(::std::string::String),
                F64(f64)
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Deduplication

    #[test]
    fn test_untagged_union_deduplicates_array_variants() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Input:
                  oneOf:
                    - type: string
                    - type: array
                      items:
                        type: string
                    - type: array
                      items:
                        type: object
                        properties:
                          kind:
                            type: string
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "Input");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Input`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum Input {
                String(::std::string::String),
                Array(::std::vec::Vec<::std::string::String>),
                Array2(::std::vec::Vec<crate::types::input::types::V3Item>)
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_untagged_union_deduplicates_multiple_null_variants() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.1.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Weird:
                  oneOf:
                    - type: string
                    - type: 'null'
                    - type: 'null'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schemas().find(|s| s.name() == "Weird");
        let Some(schema @ SchemaTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Weird`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", untagged))]
            pub enum Weird {
                String(::std::string::String),
                None,
                None2
            }
        };
        assert_eq!(actual, expected);
    }
}
