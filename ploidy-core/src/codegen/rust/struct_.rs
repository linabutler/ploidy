use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{Ident, parse_quote};

use crate::{
    codegen::{rust::CodegenIdent, unique::UniqueNameSpace},
    ir::{IrStruct, IrType},
};

use super::{
    context::CodegenContext, derives::ExtraDerive, doc_attrs, naming::CodegenTypeName,
    ref_::CodegenBoxedRef,
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenStruct<'a> {
    context: &'a CodegenContext<'a>,
    name: CodegenTypeName<'a>,
    ty: &'a IrStruct<'a>,
}

impl<'a> CodegenStruct<'a> {
    pub fn new(
        context: &'a CodegenContext,
        name: CodegenTypeName<'a>,
        ty: &'a IrStruct<'a>,
    ) -> Self {
        Self { context, name, ty }
    }
}

impl ToTokens for CodegenStruct<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut space = UniqueNameSpace::new();
        let mut all_optional = true;
        let fields = self
            .ty
            .fields
            .iter()
            .filter(|field| !field.discriminator)
            .map(|field| {
                let field_name = {
                    let name = CodegenIdent::Field(&space.uniquify(field.name));
                    parse_quote!(#name)
                };
                if field.required {
                    all_optional = false;
                }

                let final_type = match (&field.ty, field.required) {
                    (IrType::Nullable(inner), true) => {
                        let inner = CodegenBoxedRef::new(self.context, self.name, inner);
                        quote! { ::std::option::Option<#inner> }
                    }
                    (IrType::Nullable(inner), false) => {
                        let inner = CodegenBoxedRef::new(self.context, self.name, inner);
                        quote! { ::ploidy_util::absent::AbsentOr<#inner> }
                    }
                    (other, true) => {
                        CodegenBoxedRef::new(self.context, self.name, other).into_token_stream()
                    }
                    (other, false) => {
                        let inner = CodegenBoxedRef::new(self.context, self.name, other);
                        quote! { ::ploidy_util::absent::AbsentOr<#inner> }
                    }
                };

                let serde_attrs = field_serde_attrs(
                    &field_name,
                    field.name,
                    field.required,
                    matches!(field.ty, IrType::Nullable(_)),
                );

                let doc_attrs = field.description.map(doc_attrs);

                quote! {
                    #doc_attrs
                    #serde_attrs
                    pub #field_name: #final_type,
                }
            })
            .collect::<Vec<_>>();

        let mut extra_derives = vec![];
        let is_hashable = self
            .ty
            .fields
            .iter()
            .all(|variant| self.context.hashable(&variant.ty));
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }
        if all_optional && !fields.is_empty() {
            extra_derives.push(ExtraDerive::Default);
        }

        let type_name = &self.name;
        let doc_attrs = self.ty.description.map(doc_attrs);

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::serde::Serialize, ::serde::Deserialize)]
            pub struct #type_name {
                #(#fields)*
            }
        });
    }
}

/// Generates `#[serde(...)]` attributes for a field.
fn field_serde_attrs(ident: &Ident, name: &str, required: bool, nullable: bool) -> TokenStream {
    let mut attrs = Vec::new();

    // `rename` if the field name doesn't match the identifier.
    let f = ident.to_string();
    if f.strip_prefix("r#").unwrap_or(&f) != name {
        attrs.push(quote! { rename = #name });
    }

    match (required, nullable) {
        (false, true) | (false, false) => {
            attrs.push(quote! { default });
            attrs.push(
                quote! { skip_serializing_if = "::ploidy_util::absent::AbsentOr::is_absent" },
            );
        }
        _ => {}
    }

    if attrs.is_empty() {
        quote! {}
    } else {
        quote! { #[serde(#(#attrs,)*)] }
    }
}
