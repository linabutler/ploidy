use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::{codegen::IntoCode, ir::View};

use super::{graph::CodegenGraph, naming::SchemaIdent};

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
        let mut mods = Vec::new();
        let mut uses = Vec::new();

        for view in self.graph.schemas() {
            let resources: BTreeSet<_> = view.used_by().map(|op| op.resource()).collect();
            let Some(cfg_attr) = cfg_attr(&resources) else {
                continue;
            };

            let ext = view.extensions();
            let info = ext.get::<SchemaIdent>().unwrap();
            let module = info.module();
            mods.push(quote! {
                #cfg_attr
                pub mod #module;
            });

            let ty = info.ty();
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
