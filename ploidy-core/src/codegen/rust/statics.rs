use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::codegen::IntoCode;

#[derive(Clone, Copy, Debug)]
pub struct CodegenLibrary;

impl ToTokens for CodegenLibrary {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            pub mod types;
            pub mod client;
            pub mod error;

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
            /// Transport-level error types.
            #[derive(Debug, thiserror::Error)]
            pub enum Error {
                /// Network or connection error.
                #[error("Network error")]
                Network(#[from] reqwest::Error),

                /// Invalid JSON in request or response.
                #[error("Malformed JSON")]
                Json(#[from] JsonError),

                /// Invalid URL.
                #[error("Malformed URL")]
                Url(#[from] url::ParseError),

                /// URL can't be used as a base.
                #[error("Can't use URL as base URL")]
                UrlCannotBeABase,

                /// Invalid query parameter.
                #[error("Invalid query parameter")]
                QueryParam(#[from] ::ploidy_util::QueryParamError),

                /// Invalid HTTP header name.
                #[error("invalid header name")]
                BadHeaderName(#[source] http::Error),

                /// Invalid HTTP header value.
                #[error("invalid value for header `{0}`")]
                BadHeaderValue(http::HeaderName, #[source] http::Error),
            }

            /// Invalid or unexpected JSON, with or without a path
            /// to the unexpected section.
            #[derive(Debug, thiserror::Error)]
            pub enum JsonError {
                #[error(transparent)]
                Json(#[from] serde_json::Error),
                #[error(transparent)]
                JsonWithPath(#[from] serde_path_to_error::Error<serde_json::Error>),
            }
        });
    }
}

impl IntoCode for CodegenErrorModule {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/error.rs", self.into_token_stream())
    }
}
