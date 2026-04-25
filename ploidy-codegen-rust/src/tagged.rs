use itertools::Itertools;
use ploidy_core::ir::{TaggedView, View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{CodegenTypeIdent, UniqueIdents};

use super::{
    derives::ExtraDerive, doc_attrs, graph::CodegenGraph, naming::CodegenIdentUsage,
    ref_::CodegenRef,
};

/// Generates a tagged union as a Rust enum, with `#[serde(tag = ...)]`
/// and associated data for each variant.
#[derive(Clone, Debug)]
pub struct CodegenTagged<'a> {
    graph: &'a CodegenGraph<'a>,
    ident: CodegenTypeIdent<'a>,
    ty: &'a TaggedView<'a, 'a>,
}

impl<'a> CodegenTagged<'a> {
    pub fn new(
        graph: &'a CodegenGraph<'a>,
        ident: CodegenTypeIdent<'a>,
        ty: &'a TaggedView<'a, 'a>,
    ) -> Self {
        Self { graph, ident, ty }
    }
}

impl ToTokens for CodegenTagged<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut extra_derives = vec![];

        // Derive `Eq` and `Hash` if all variants are transitively hashable.
        if self.ty.hashable() {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        let mut scope = UniqueIdents::new(self.graph.arena());
        let variants = self
            .ty
            .variants()
            .map(|variant| {
                // Look up the proper Rust type name.
                let view = variant.ty();
                let scope_name = scope.name(variant.name());
                let variant_name = CodegenIdentUsage::Variant(scope_name);

                // Add `#[serde(alias = ...)]` attributes for multiple
                // discriminator values that map to the same type.
                let serde_attr = {
                    let mut iter = variant.aliases().iter();
                    match iter.next() {
                        Some(&primary) => {
                            let mut aliases = iter.copied().peekable();
                            Some(if aliases.peek().is_none() {
                                quote! { #[serde(rename = #primary)] }
                            } else {
                                quote! { #[serde(rename = #primary, #(alias = #aliases,)*)] }
                            })
                        }
                        None => None,
                    }
                };

                // Use the primary name for JSON pointer traversal;
                // secondary aliases are only used for deserialization.
                let pointer_attr = variant.aliases().first().map(|&primary| {
                    quote! { #[ploidy(pointer(rename = #primary))] }
                });

                let rust_type_name = CodegenRef::new(self.graph, &view);
                let v = quote! {
                    #serde_attr
                    #pointer_attr
                    #variant_name(#rust_type_name),
                };

                let type_name = &self.ident;
                let from_impl = quote! {
                    impl ::std::convert::From<#rust_type_name> for #type_name {
                        fn from(value: #rust_type_name) -> Self {
                            Self::#variant_name(value)
                        }
                    }
                };

                (v, from_impl)
            })
            .collect_vec();

        let discriminator_field_literal = self.ty.tag();

        let doc_attrs = self.ty.description().map(doc_attrs);

        let vs = variants.iter().map(|(variant, _)| variant);
        let fs = variants.iter().map(|(_, from_impl)| from_impl);
        let type_name = &self.ident;
        let main = quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = #discriminator_field_literal)]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = #discriminator_field_literal))]
            pub enum #type_name {
                #(#vs)*
            }

            #(#fs)*
        };

        tokens.append_all(main);
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
    fn test_tagged_union_serde_tag_attr() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: petType
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "petType")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "petType"))]
            pub enum Pet {
                #[serde(rename = "dog")]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::Dog),
                #[serde(rename = "cat")]
                #[ploidy(pointer(rename = "cat"))]
                Cat(crate::types::Cat),
            }
            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
            impl ::std::convert::From<crate::types::Cat> for Pet {
                fn from(value: crate::types::Cat) -> Self {
                    Self::Cat(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_variant_rename() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      canine: '#/components/schemas/Dog'
                      feline: '#/components/schemas/Cat'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "type")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "type"))]
            pub enum Pet {
                #[serde(rename = "canine")]
                #[ploidy(pointer(rename = "canine"))]
                Dog(crate::types::Dog),
                #[serde(rename = "feline")]
                #[ploidy(pointer(rename = "feline"))]
                Cat(crate::types::Cat),
            }
            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
            impl ::std::convert::From<crate::types::Cat> for Pet {
                fn from(value: crate::types::Cat) -> Self {
                    Self::Cat(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_variant_with_alias() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    bark:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      canine: '#/components/schemas/Dog'
                      puppy: '#/components/schemas/Dog'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "type")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "type"))]
            pub enum Pet {
                #[serde(rename = "dog", alias = "canine", alias = "puppy",)]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::Dog),
            }
            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                Pet:
                  description: Represents different types of pets
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: type
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[doc = "Represents different types of pets"]
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "type")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "type"))]
            pub enum Pet {
                #[serde(rename = "dog")]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::Dog),
                #[serde(rename = "cat")]
                #[ploidy(pointer(rename = "cat"))]
                Cat(crate::types::Cat),
            }
            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
            impl ::std::convert::From<crate::types::Cat> for Pet {
                fn from(value: crate::types::Cat) -> Self {
                    Self::Cat(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_without_mapping() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
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
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: petType
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "petType")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "petType"))]
            pub enum Pet {
                #[serde(rename = "Dog")]
                #[ploidy(pointer(rename = "Dog"))]
                Dog(crate::types::Dog),
                #[serde(rename = "Cat")]
                #[ploidy(pointer(rename = "Cat"))]
                Cat(crate::types::Cat),
            }
            impl ::std::convert::From<crate::types::Dog> for Pet {
                fn from(value: crate::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
            impl ::std::convert::From<crate::types::Cat> for Pet {
                fn from(value: crate::types::Cat) -> Self {
                    Self::Cat(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    // MARK: Inlined variants

    #[test]
    fn test_tagged_union_inlined_variant_wraps_inline_type() {
        // `Dog` is used both inside the `Pet` tagged union _and_ referenced
        // by `Owner.dog`, making it inlinable. After inlining, `Pet::Dog`
        // holds the inlined `Dog`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    kind:
                      type: string
                    bark:
                      type: string
                  required:
                    - kind
                    - bark
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      dog: '#/components/schemas/Dog'
                Owner:
                  type: object
                  properties:
                    dog:
                      $ref: '#/components/schemas/Dog'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let mut raw = RawGraph::new(&arena, &spec);
        raw.inline_tagged_variants();
        let graph = CodegenGraph::new(raw.cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "kind")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "kind"))]
            pub enum Pet {
                #[serde(rename = "dog")]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::pet::types::Dog),
            }
            impl ::std::convert::From<crate::types::pet::types::Dog> for Pet {
                fn from(value: crate::types::pet::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_wraps_non_inlined_variant() {
        // `Dog` is only used inside the `Pet` tagged union, so it's
        // used directly in `Pet::Dog`; not inlined.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    bark:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      dog: '#/components/schemas/Dog'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

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
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tagged_union_mixed_inlined_and_non_inlined() {
        // `Dog` is inlined (referenced by `Owner.dog`); `Cat` is not.
        // Each should be handled independently.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            components:
              schemas:
                Dog:
                  type: object
                  properties:
                    kind:
                      type: string
                    bark:
                      type: string
                  required:
                    - bark
                Cat:
                  type: object
                  properties:
                    meow:
                      type: string
                Pet:
                  oneOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: kind
                    mapping:
                      dog: '#/components/schemas/Dog'
                      cat: '#/components/schemas/Cat'
                Owner:
                  type: object
                  properties:
                    dog:
                      $ref: '#/components/schemas/Dog'
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let mut raw = RawGraph::new(&arena, &spec);
        raw.inline_tagged_variants();
        let graph = CodegenGraph::new(raw.cook());

        let schema = graph.schema("Pet").unwrap();
        let SchemaTypeView::Tagged(_, tagged) = &*schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let codegen = CodegenTagged::new(&graph, schema.ident(), tagged);

        let actual: syn::File = parse_quote!(#codegen);
        // `Dog` is inlined, so `Pet::Dog` holds the inline type.
        // `Cat` isn't inlined, so `Pet::Cat` holds the schema type.
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize, ::ploidy_util::pointer::JsonPointee, ::ploidy_util::pointer::JsonPointerTarget)]
            #[serde(crate = "::ploidy_util::serde", tag = "kind")]
            #[ploidy(pointer(crate = "::ploidy_util::pointer", tag = "kind"))]
            pub enum Pet {
                #[serde(rename = "dog")]
                #[ploidy(pointer(rename = "dog"))]
                Dog(crate::types::pet::types::Dog),
                #[serde(rename = "cat")]
                #[ploidy(pointer(rename = "cat"))]
                Cat(crate::types::Cat),
            }
            impl ::std::convert::From<crate::types::pet::types::Dog> for Pet {
                fn from(value: crate::types::pet::types::Dog) -> Self {
                    Self::Dog(value)
                }
            }
            impl ::std::convert::From<crate::types::Cat> for Pet {
                fn from(value: crate::types::Cat) -> Self {
                    Self::Cat(value)
                }
            }
        };
        assert_eq!(actual, expected);
    }
}
