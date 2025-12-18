use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::codegen::IntoCode;

use super::context::CodegenContext;

/// Generates the `types/mod.rs` module.
pub struct CodegenTypesModule<'a> {
    context: &'a CodegenContext<'a>,
    resources_by_type: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> CodegenTypesModule<'a> {
    pub fn new(context: &'a CodegenContext<'a>) -> Self {
        let mut resources_by_type = BTreeMap::<&str, BTreeSet<&str>>::new();
        for view in context.spec.operations() {
            let resource = view.op().resource;
            for v in view.refs() {
                resources_by_type
                    .entry(v.name())
                    .or_default()
                    .insert(resource);
            }
        }

        Self {
            context,
            resources_by_type,
        }
    }
}

impl ToTokens for CodegenTypesModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut mods = Vec::new();
        let mut uses = Vec::new();

        for (name, info) in self.context.map.iter() {
            let Some(resources) = self.resources_by_type.get(name) else {
                continue;
            };

            let cfg_attr = cfg_attr(resources);

            let module = &info.module;
            mods.push(quote! {
                #cfg_attr
                pub mod #module;
            });

            let ty = &info.ty;
            uses.push(quote! {
                #cfg_attr
                pub use #module::#ty;
            });
        }

        tokens.append_all(quote! {
            #(#mods)*

            #(#uses)*
        });
    }
}

impl IntoCode for CodegenTypesModule<'_> {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/types/mod.rs", self.into_token_stream())
    }
}

/// Generates a `#[cfg(feature = "...")]` or `#[cfg(any(feature = "...", ...))]`
/// attribute for the given resources.
fn cfg_attr(resources: &BTreeSet<&str>) -> Option<TokenStream> {
    let mut features = resources.iter().peekable();
    let first = features.next()?;
    Some(match features.next() {
        Some(next) => {
            let rest = features.map(|f| quote! { feature = #f });
            quote! { #[cfg(any(feature = #first, feature = #next, #(#rest),*))] }
        }
        None => quote! { #[cfg(feature = #first)] },
    })
}
