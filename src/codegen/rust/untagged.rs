use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::ir::{IrUntagged, IrUntaggedVariant};

use super::{
    context::CodegenContext,
    derives::ExtraDerive,
    doc_attrs,
    naming::{CodegenTypeName, CodegenUntaggedVariantName},
    ref_::CodegenRef,
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenUntagged<'a> {
    context: &'a CodegenContext<'a>,
    name: CodegenTypeName<'a>,
    ty: &'a IrUntagged<'a>,
}

impl<'a> CodegenUntagged<'a> {
    pub fn new(
        context: &'a CodegenContext,
        name: CodegenTypeName<'a>,
        ty: &'a IrUntagged<'a>,
    ) -> Self {
        Self { context, name, ty }
    }
}

impl ToTokens for CodegenUntagged<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut variants = Vec::new();

        for variant in &self.ty.variants {
            match variant {
                IrUntaggedVariant::Some(name, ty) => {
                    let variant_name = CodegenUntaggedVariantName(*name);
                    let rust_type = CodegenRef::new(self.context, ty);
                    variants.push(quote! { #variant_name(#rust_type) });
                }
                IrUntaggedVariant::Null => variants.push(quote! { None }),
            }
        }

        let type_name_ident = &self.name;
        let doc_attrs = self.ty.description.map(doc_attrs);

        let mut extra_derives = vec![];
        let is_hashable = self.ty.variants.iter().all(|variant| match variant {
            IrUntaggedVariant::Some(_, ty) => self.context.hashable(ty),
            IrUntaggedVariant::Null => true,
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::serde::Serialize, ::serde::Deserialize)]
            #[serde(untagged)]
            pub enum #type_name_ident {
                #(#variants),*
            }
        })
    }
}
