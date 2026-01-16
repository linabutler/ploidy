use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNames,
    ir::{IrTaggedView, IrTypeView, PrimitiveIrType, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{CodegenIdentScope, CodegenTypeName};

use super::{derives::ExtraDerive, doc_attrs, naming::CodegenIdentUsage, ref_::CodegenRef};

/// Generates a tagged union as a Rust enum, with `#[serde(tag = ...)]`
/// and associated data for each variant.
#[derive(Clone, Debug)]
pub struct CodegenTagged<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrTaggedView<'a>,
}

impl<'a> CodegenTagged<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrTaggedView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenTagged<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut extra_derives = vec![];
        let is_hashable = self.ty.variants().all(|variant| {
            variant.reachable().all(|view| {
                if let IrTypeView::Primitive(p) = &view
                    && let PrimitiveIrType::F32 | PrimitiveIrType::F64 = p.ty()
                {
                    false
                } else {
                    true
                }
            })
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let variants = self
            .ty
            .variants()
            .map(|variant| {
                // Look up the proper Rust type name.
                let view = variant.ty();
                let variant_name = CodegenIdentUsage::Variant(&scope.uniquify(variant.name()));
                let rust_type_name = CodegenRef::new(&view);

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

                let v = quote! {
                    #serde_attr
                    #variant_name(#rust_type_name),
                };

                let type_name = &self.name;
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
        let type_name = &self.name;
        let main = quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = #discriminator_field_literal)]
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
        ir::{IrGraph, IrSpec, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::{CodegenGraph, CodegenTypeName};

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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenTagged::new(name, tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = "petType")]
            pub enum Pet {
                #[serde(rename = "dog")]
                Dog(crate::types::Dog),
                #[serde(rename = "cat")]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenTagged::new(name, tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = "type")]
            pub enum Pet {
                #[serde(rename = "canine")]
                Dog(crate::types::Dog),
                #[serde(rename = "feline")]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenTagged::new(name, tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = "type")]
            pub enum Pet {
                #[serde(rename = "dog", alias = "canine", alias = "puppy",)]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Tagged(_, tagged)) = &schema else {
            panic!("expected tagged union `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let codegen = CodegenTagged::new(name, tagged);

        let actual: syn::File = parse_quote!(#codegen);
        let expected: syn::File = parse_quote! {
            #[doc = "Represents different types of pets"]
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = "type")]
            pub enum Pet {
                #[serde(rename = "dog")]
                Dog(crate::types::Dog),
                #[serde(rename = "cat")]
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
}
