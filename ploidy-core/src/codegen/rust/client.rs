use itertools::Itertools;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use crate::codegen::{IntoCode, rust::CodegenIdent};

use super::context::CodegenContext;

/// Generates the `client/mod.rs` source file.
#[derive(Clone, Copy, Debug)]
pub struct CodegenClientModule<'a> {
    context: &'a CodegenContext<'a>,
    resources: &'a [&'a str],
}

impl<'a> CodegenClientModule<'a> {
    pub fn new(context: &'a CodegenContext<'a>, resources: &'a [&'a str]) -> Self {
        Self { context, resources }
    }
}

impl ToTokens for CodegenClientModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mods = self
            .resources
            .iter()
            .map(|resource| {
                let mod_name = CodegenIdent::Module(resource);
                quote! {
                    #[cfg(feature = #resource)]
                    pub mod #mod_name;
                }
            })
            .collect_vec();

        let client_doc = {
            let info = self.context.graph.spec().info;
            format!("API client for {} (version {})", info.title, info.version)
        };

        tokens.append_all(quote! {
            #[doc = #client_doc]
            #[derive(Clone, Debug)]
            pub struct Client {
                client: ::reqwest::Client,
                headers: ::http::HeaderMap,
                base_url: ::url::Url,
            }

            impl Client {
                /// Create a new client.
                pub fn new(base_url: impl AsRef<str>) -> Result<Self, crate::error::Error> {
                    Ok(Self::with_reqwest_client(
                        ::reqwest::Client::new(),
                        base_url.as_ref().parse()?,
                    ))
                }

                pub fn with_reqwest_client(client: ::reqwest::Client, base_url: ::url::Url) -> Self {
                    Self {
                        client,
                        headers: ::http::HeaderMap::new(),
                        base_url,
                    }
                }

                /// Adds a header to each request.
                pub fn with_header<K, V>(mut self, name: K, value: V) -> Result<Self, crate::error::Error>
                where
                    K: TryInto<::http::HeaderName>,
                    V: TryInto<::http::HeaderValue>,
                    K::Error: Into<::http::Error>,
                    V::Error: Into<::http::Error>,
                {
                    let name = name
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderName(err.into()))?;
                    let value = value
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderValue(name.clone(), err.into()))?;
                    self.headers.insert(name, value);
                    Ok(Self {
                        client: self.client,
                        headers: self.headers,
                        base_url: self.base_url,
                    })
                }

                /// Adds a sensitive header to each request, like a password or a bearer token.
                /// Sensitive headers won't appear in `Debug` output, and may be treated specially
                /// by the underlying HTTP stack.
                ///
                /// # Example
                ///
                /// ```rust,ignore
                /// use reqwest::header::AUTHORIZATION;
                ///
                /// let client = Client::new("https://api.example.com")?
                ///     .with_sensitive_header(AUTHORIZATION, "Bearer decafbadcafed00d")?;
                /// ```
                pub fn with_sensitive_header<K, V>(self, name: K, value: V) -> Result<Self, crate::error::Error>
                where
                    K: TryInto<::http::HeaderName>,
                    V: TryInto<::http::HeaderValue>,
                    K::Error: Into<::http::Error>,
                    V::Error: Into<::http::Error>,
                {
                    let name = name
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderName(err.into()))?;
                    let mut value: ::http::HeaderValue = value
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderValue(name.clone(), err.into()))?;
                    value.set_sensitive(true);
                    self.with_header(name, value)
                }

                pub fn with_user_agent<V>(self, value: V) -> Result<Self, crate::error::Error>
                where
                    V: TryInto<::http::HeaderValue>,
                    V::Error: Into<::http::Error>,
                {
                    self.with_header(::http::header::USER_AGENT, value)
                }
            }

            #(#mods)*
        });
    }
}

impl IntoCode for CodegenClientModule<'_> {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/client/mod.rs", self.into_token_stream())
    }
}
