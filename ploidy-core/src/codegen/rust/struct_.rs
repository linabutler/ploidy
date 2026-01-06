use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{Ident, parse_quote};

use crate::{
    codegen::{
        rust::{CodegenIdent, CodegenStructFieldName},
        unique::UniqueNameSpace,
    },
    ir::{InlineIrType, IrStruct, IrStructFieldName, IrType, SchemaIrType},
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

    fn to_ty(self) -> IrType<'a> {
        match self.name {
            CodegenTypeName::Schema(name, _) => {
                IrType::Schema(SchemaIrType::Struct(name, self.ty.clone()))
            }
            CodegenTypeName::Inline(path) => {
                IrType::Inline(InlineIrType::Struct(path.clone(), self.ty.clone()))
            }
        }
    }
}

impl ToTokens for CodegenStruct<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut space = UniqueNameSpace::new();
        let mut all_optional = true;
        let fields =
            self.ty
                .fields
                .iter()
                .filter(|field| !field.discriminator)
                .map(|field| {
                    let field_name = match field.name {
                        IrStructFieldName::Name(n) => {
                            let name = CodegenIdent::Field(&space.uniquify(n));
                            parse_quote!(#name)
                        }
                        IrStructFieldName::Hint(hint) => {
                            let name = CodegenStructFieldName(hint);
                            parse_quote!(#name)
                        }
                    };
                    if field.required {
                        all_optional = false;
                    }

                    let ty = self.to_ty();
                    let final_type = match (&field.ty, field.required) {
                        (IrType::Nullable(inner), true) => {
                            let inner = CodegenBoxedRef::new(self.context, ty.as_ref(), inner);
                            quote! { ::std::option::Option<#inner> }
                        }
                        (IrType::Nullable(inner), false) => {
                            let inner = CodegenBoxedRef::new(self.context, ty.as_ref(), inner);
                            quote! { ::ploidy_util::absent::AbsentOr<#inner> }
                        }
                        (other, true) => CodegenBoxedRef::new(self.context, ty.as_ref(), other)
                            .into_token_stream(),
                        (other, false) => {
                            let inner = CodegenBoxedRef::new(self.context, ty.as_ref(), other);
                            quote! { ::ploidy_util::absent::AbsentOr<#inner> }
                        }
                    };

                    let serde_attrs = field_serde_attrs(
                        &field.name,
                        &field_name,
                        field.required,
                        matches!(field.ty, IrType::Nullable(_)),
                        field.flattened,
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
fn field_serde_attrs(
    field_name: &IrStructFieldName,
    field_ident: &Ident,
    required: bool,
    nullable: bool,
    flattened: bool,
) -> TokenStream {
    let mut attrs = Vec::new();

    // Add `flatten` xor `rename` (specifying both
    // on the same field isn't meaningful).
    if flattened {
        attrs.push(quote! { flatten });
    } else if let &IrStructFieldName::Name(name) = field_name {
        // `rename` if the field name doesn't match the identifier.
        let f = field_ident.to_string();
        if f.strip_prefix("r#").unwrap_or(&f) != name {
            attrs.push(quote! { rename = #name });
        }
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
