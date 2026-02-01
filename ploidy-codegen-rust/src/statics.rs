use ploidy_core::codegen::IntoCode;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

#[derive(Clone, Copy, Debug)]
pub struct CodegenLibrary;

impl ToTokens for CodegenLibrary {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            pub mod types;
            pub mod client;
            pub mod error;

            // Re-export `ploidy-util`, so that consumers don't need to
            // depend on it directly.
            pub use ::ploidy_util as util;

            pub use client::Client;
            pub use error::Error;
        });
    }
}

impl IntoCode for CodegenLibrary {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/lib.rs", self.into_token_stream())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenErrorModule;

impl ToTokens for CodegenErrorModule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            pub use ::ploidy_util::error::*;
        });
    }
}

impl IntoCode for CodegenErrorModule {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/error.rs", self.into_token_stream())
    }
}
