use itertools::Itertools;
use ploidy_core::codegen::IntoCode;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{cfg::CfgFeature, graph::CodegenGraph};

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
        tys.sort_by(|a, b| a.ident().cmp(&b.ident()));

        let mods = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(schema);
            let mod_name = schema.ident().into_module();
            quote! {
                #cfg
                pub mod #mod_name;
            }
        });
        let uses = tys.iter().map(|schema| {
            let cfg = CfgFeature::for_schema_type(schema);
            let ty_name = schema.ident();
            let mod_name = ty_name.into_module();
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
