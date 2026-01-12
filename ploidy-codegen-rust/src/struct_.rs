use ploidy_core::{
    codegen::UniqueNameSpace,
    ir::{IrStructFieldName, IrStructFieldView, IrStructView, IrTypeView, PrimitiveIrType, View},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{Ident, parse_quote};

use super::{
    derives::ExtraDerive,
    doc_attrs,
    naming::CodegenTypeName,
    naming::{CodegenIdent, CodegenStructFieldName},
    ref_::CodegenRef,
};

#[derive(Clone, Copy, Debug)]
pub struct CodegenStruct<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrStructView<'a>,
}

impl<'a> CodegenStruct<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrStructView<'a>) -> Self {
        Self { name, ty }
    }
}

impl ToTokens for CodegenStruct<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut space = UniqueNameSpace::new();
        let mut all_optional = true;
        let fields = self
            .ty
            .fields()
            .filter(|field| !field.discriminator())
            .map(|field| {
                let field_name: Ident = match field.name() {
                    IrStructFieldName::Name(n) => {
                        let name = CodegenIdent::Field(&space.uniquify(n));
                        parse_quote!(#name)
                    }
                    IrStructFieldName::Hint(hint) => {
                        let name = CodegenStructFieldName(hint);
                        parse_quote!(#name)
                    }
                };
                if field.required() {
                    all_optional = false;
                }

                let codegen_field = CodegenField::new(&field);
                let final_type = codegen_field.to_token_stream();

                let serde_attrs = field_serde_attrs(
                    &field.name(),
                    &field_name,
                    field.required(),
                    matches!(field.ty(), IrTypeView::Nullable(_)),
                    field.flattened(),
                );

                let doc_attrs = field.description().map(doc_attrs);

                quote! {
                    #doc_attrs
                    #serde_attrs
                    pub #field_name: #final_type,
                }
            })
            .collect::<Vec<_>>();

        let mut extra_derives = vec![];
        let is_hashable = self.ty.reachable().all(|view| {
            !matches!(
                view,
                IrTypeView::Primitive(PrimitiveIrType::F32 | PrimitiveIrType::F64)
            )
        });
        if is_hashable {
            extra_derives.push(ExtraDerive::Eq);
            extra_derives.push(ExtraDerive::Hash);
        }
        if all_optional && !fields.is_empty() {
            extra_derives.push(ExtraDerive::Default);
        }

        let type_name = &self.name;
        let doc_attrs = self.ty.description().map(doc_attrs);

        tokens.append_all(quote! {
            #doc_attrs
            #[derive(Debug, Clone, PartialEq, #(#extra_derives,)* ::serde::Serialize, ::serde::Deserialize)]
            pub struct #type_name {
                #(#fields)*
            }
        });
    }
}

/// A field in a struct, ready for code generation.
#[derive(Debug)]
struct CodegenField<'view, 'a> {
    field: &'a IrStructFieldView<'view, 'a>,
}

impl<'view, 'a> CodegenField<'view, 'a> {
    fn new(field: &'a IrStructFieldView<'view, 'a>) -> Self {
        Self { field }
    }

    fn needs_box(&self) -> bool {
        if matches!(
            self.field.ty(),
            IrTypeView::Array(_) | IrTypeView::Map(_) | IrTypeView::Primitive(_) | IrTypeView::Any
        ) {
            // Leaf types like primitives and `Any` don't contain any references,
            // and arrays (`Vec`) and maps (`BTreeMap`) are heap-allocated,
            // so we never need to box them.
            return false;
        }
        self.field.needs_indirection()
    }
}

impl ToTokens for CodegenField<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let view = self.field.ty();
        let inner_ty = CodegenRef::new(&view);
        let inner = if self.needs_box() {
            quote! { ::std::boxed::Box<#inner_ty> }
        } else {
            quote! { #inner_ty }
        };
        tokens.append_all(match (self.field.ty(), self.field.required()) {
            (IrTypeView::Nullable(_), true) => quote! { ::std::option::Option<#inner_ty> },
            (IrTypeView::Nullable(_), false) => {
                quote! { ::ploidy_util::absent::AbsentOr<#inner_ty> }
            }
            (_, true) => inner,
            (_, false) => quote! { ::ploidy_util::absent::AbsentOr<#inner> },
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
