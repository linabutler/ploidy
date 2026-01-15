use std::collections::BTreeSet;

use itertools::Itertools;
use ploidy_core::{codegen::IntoCode, ir::View};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    graph::CodegenGraph,
    naming::{CodegenIdent, CodegenIdentUsage},
};

/// Generates the `types/mod.rs` module.
pub struct CodegenTypesModule<'a> {
    graph: &'a CodegenGraph<'a>,
}

impl<'a> CodegenTypesModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>) -> Self {
        Self { graph }
    }
}

impl ToTokens for CodegenTypesModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut tys = self
            .graph
            .schemas()
            .filter_map(|view| {
                let resources: BTreeSet<_> = view.used_by().map(|op| op.resource()).collect();
                let cfg_attr = cfg_attr(&resources)?;
                Some((view.extensions().get::<CodegenIdent>()?.clone(), cfg_attr))
            })
            .collect_vec();
        tys.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mods = tys.iter().map(|(ident, cfg_attr)| {
            let mod_name = CodegenIdentUsage::Module(ident);
            quote! {
                #cfg_attr
                pub mod #mod_name;
            }
        });
        let uses = tys.iter().map(|(ident, cfg_attr)| {
            let mod_name = CodegenIdentUsage::Module(ident);
            let ty_name = CodegenIdentUsage::Type(ident);
            quote! {
                #cfg_attr
                pub use #mod_name::#ty_name;
            }
        });

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
