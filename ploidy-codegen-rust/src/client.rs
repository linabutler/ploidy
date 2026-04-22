use ploidy_core::codegen::IntoCode;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature,
    graph::CodegenGraph,
    naming::{CargoFeature, CodegenIdentUsage},
};

/// Generates the `client/mod.rs` source file.
#[derive(Clone, Copy, Debug)]
pub struct CodegenClientModule<'a> {
    graph: &'a CodegenGraph<'a>,
    features: &'a [&'a CargoFeature],
}

impl<'a> CodegenClientModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>, features: &'a [&'a CargoFeature]) -> Self {
        Self { graph, features }
    }
}

impl ToTokens for CodegenClientModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mods = self.features.iter().map(|feature| {
            let cfg = CfgFeature::for_resource_module(feature);
            let mod_name = CodegenIdentUsage::Module(feature.as_ident());
            quote! {
                #cfg
                pub mod #mod_name;
            }
        });

        let client_doc = self.graph.info().label().map(|label| {
            let doc = match label.version {
                Some(version) => format!("API client for {} (version {version})", label.title),
                None => format!("API client for {}", label.title),
            };
            quote! { #[doc = #doc] }
        });

        tokens.append_all(quote! {
            #client_doc
            #[derive(Clone, Debug)]
            pub struct Client {
                client: ::ploidy_util::reqwest::Client,
                headers: ::ploidy_util::http::HeaderMap,
                base_url: ::ploidy_util::url::Url,
            }

            impl Client {
                /// Creates a new client.
                pub fn new(base_url: impl AsRef<str>) -> Result<Self, crate::error::Error> {
                    Ok(Self::with_reqwest_client(
                        ::ploidy_util::reqwest::Client::new(),
                        base_url.as_ref().parse()?,
                    ))
                }

                pub fn with_reqwest_client(
                    client: crate::util::reqwest::Client,
                    base_url: crate::util::url::Url,
                ) -> Self {
                    Self {
                        client,
                        headers: ::ploidy_util::http::HeaderMap::new(),
                        base_url,
                    }
                }

                /// Adds a header to each request.
                pub fn with_header<K, V>(mut self, name: K, value: V) -> Result<Self, crate::error::Error>
                where
                    K: TryInto<crate::util::http::HeaderName>,
                    V: TryInto<crate::util::http::HeaderValue>,
                    K::Error: Into<crate::util::http::Error>,
                    V::Error: Into<crate::util::http::Error>,
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
                    K: TryInto<crate::util::http::HeaderName>,
                    V: TryInto<crate::util::http::HeaderValue>,
                    K::Error: Into<crate::util::http::Error>,
                    V::Error: Into<crate::util::http::Error>,
                {
                    let name = name
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderName(err.into()))?;
                    let mut value: ::ploidy_util::http::HeaderValue = value
                        .try_into()
                        .map_err(|err| crate::error::Error::BadHeaderValue(name.clone(), err.into()))?;
                    value.set_sensitive(true);
                    self.with_header(name, value)
                }

                pub fn with_user_agent<V>(self, value: V) -> Result<Self, crate::error::Error>
                where
                    V: TryInto<crate::util::http::HeaderValue>,
                    V::Error: Into<crate::util::http::Error>,
                {
                    self.with_header(::ploidy_util::http::header::USER_AGENT, value)
                }

                /// Returns a raw [`RequestBuilder`].
                ///
                /// Constructs the request URL by appending `path_and_query`
                /// to the base URL's path and query, respectively. For example,
                /// given a base URL of `https://api.example.com/v1` and a
                /// `path_and_query` of `/pets/list?limit=10`, the request URL is
                /// `https://api.example.com/v1/pets/list?limit=10`.
                ///
                /// The request includes the client's default headers.
                ///
                /// Use this for requests that the typed client methods
                /// don't support.
                ///
                /// [`RequestBuilder`]: crate::util::reqwest::RequestBuilder
                pub fn request(
                    &self,
                    method: crate::util::reqwest::Method,
                    path_and_query: &str,
                ) -> Result<crate::util::reqwest::RequestBuilder, crate::error::Error> {
                    let parts: ::ploidy_util::http::uri::PathAndQuery = path_and_query.parse()?;
                    let mut url = self.base_url.clone();
                    let _ = url
                        .path_segments_mut()
                        .map(|mut segments| {
                            segments.pop_if_empty()
                                .extend(parts.path().split('/'));
                        });
                    if let Some(query) = parts.query() {
                        url.query_pairs_mut()
                            .extend_pairs(::ploidy_util::url::form_urlencoded::parse(query.as_bytes()));
                    }
                    Ok(self.client
                        .request(method, url)
                        .headers(self.headers.clone()))
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
