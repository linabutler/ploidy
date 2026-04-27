use itertools::Itertools;
use ploidy_core::{codegen::IntoCode, ir::Identifiable};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{cfg::CfgFeature, graph::CodegenGraph, naming::CodegenIdentUsage};

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
        let mut tys = self.graph.schemas().collect_vec();
        tys.sort_by_cached_key(|s| self.graph.ident(s.id()));

        let mods = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(self.graph, schema);
            let ident = self.graph.ident(schema.id());
            let mod_name = CodegenIdentUsage::Module(&ident);
            quote! {
                #cfg
                pub mod #mod_name;
            }
        });
        let uses = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(self.graph, schema);
            let ident = self.graph.ident(schema.id());
            let ty_name = CodegenIdentUsage::Type(&ident);
            let mod_name = CodegenIdentUsage::Module(&ident);
            quote! {
                #cfg
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
