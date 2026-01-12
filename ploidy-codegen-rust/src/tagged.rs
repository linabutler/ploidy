use itertools::Itertools;
use ploidy_core::{
    codegen::UniqueNameSpace,
    ir::{IrTaggedView, IrTypeView, PrimitiveIrType, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    derives::ExtraDerive, doc_attrs, naming::CodegenIdent, naming::CodegenTypeName,
    ref_::CodegenRef,
};

#[derive(Clone, Copy, Debug)]
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
                !matches!(
                    view,
                    IrTypeView::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
                )
            })
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        let mut space = UniqueNameSpace::new();
        let variants = self
            .ty
            .variants()
            .map(|variant| {
                // Look up the proper Rust type name.
                let view = variant.ty();
                let variant_name = CodegenIdent::Variant(&space.uniquify(variant.name()));
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
