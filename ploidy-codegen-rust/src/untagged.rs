use ploidy_core::ir::{IrTypeView, IrUntaggedView, PrimitiveIrType, SomeIrUntaggedVariant};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    naming::{CodegenTypeName, CodegenUntaggedVariantName},
    ref_::CodegenRef,
};

#[derive(Clone, Copy, Debug)]
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
            Some(SomeIrUntaggedVariant { view, .. }) => view.reachable().all(|view| {
                !matches!(
                    view,
                    IrTypeView::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
                )
            }),
            None => true,
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
