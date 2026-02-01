use ploidy_core::ir::{IrTypeView, IrUntaggedView, PrimitiveIrType, SomeIrUntaggedVariant};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    naming::{CodegenTypeName, CodegenUntaggedVariantName},
    ref_::CodegenRef,
};

#[derive(Clone, Debug)]
pub struct CodegenUntagged<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrUntaggedView<'a>,
}

impl<'a> CodegenUntagged<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrUntaggedView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenUntagged<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut variants = Vec::new();

        for variant in self.ty.variants() {
            match variant.ty() {
                Some(variant) => {
                    let variant_name = CodegenUntaggedVariantName(variant.hint);
                    let rust_type = CodegenRef::new(&variant.view);
                    variants.push(quote! { #variant_name(#rust_type) });
                }
                None => variants.push(quote! { None }),
            }
        }

        let type_name_ident = &self.name;
        let doc_attrs = self.ty.description().map(doc_attrs);

        let mut extra_derives = vec![];
        let is_hashable = self.ty.variants().all(|variant| match variant.ty() {
            Some(SomeIrUntaggedVariant { view, .. }) => view
                .dependencies()
                .chain(std::iter::once(view))
                .all(|view| {
                    if let IrTypeView::Primitive(p) = &view
                        && let PrimitiveIrType::F32 | PrimitiveIrType::F64 = p.ty()
                    {
                        false
                    } else {
                        true
                    }
                }),
            None => true,
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
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
        ir::{IrGraph, IrSpec, SchemaIrTypeView},
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            pub enum StringOrInt {
                String(::std::string::String),
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Animal");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `Animal`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrInt");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrInt`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[doc = "A union that can be either a string or an integer."]
            #[derive(Debug, Clone, PartialEq, Eq, Hash, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrFloat");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrFloat`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "StringOrDouble");
        let Some(schema @ SchemaIrTypeView::Untagged(_, untagged_view)) = &schema else {
            panic!("expected untagged union `StringOrDouble`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let untagged = CodegenUntagged::new(name, untagged_view);

        let actual: syn::ItemEnum = parse_quote!(#untagged);
        let expected: syn::ItemEnum = parse_quote! {
            #[derive(Debug, Clone, PartialEq, ::ploidy_util::serde::Serialize, ::ploidy_util::serde::Deserialize)]
            #[serde(crate = "::ploidy_util::serde", untagged)]
            pub enum StringOrDouble {
                String(::std::string::String),
                F64(f64)
            }
        };
        assert_eq!(actual, expected);
    }
}
