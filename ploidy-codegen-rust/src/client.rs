use ploidy_core::codegen::IntoCode;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

use super::{
    cfg::CfgFeature,
    graph::CodegenGraph,
    naming::{CodegenIdentUsage, ResourceGroup},
};

/// Generates the `client/mod.rs` source file.
#[derive(Debug)]
pub struct CodegenClientModule<'a> {
    graph: &'a CodegenGraph<'a>,
    resources: &'a [ResourceGroup<'a>],
}

impl<'a> CodegenClientModule<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>, resources: &'a [ResourceGroup<'a>]) -> Self {
        Self { graph, resources }
    }
}

impl ToTokens for CodegenClientModule<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let client_doc = self.graph.info().label().map(|label| {
            let doc = match label.version {
                Some(version) => format!("API client for {} (version {version})", label.title),
                None => format!("API client for {}", label.title),
            };
            quote! { #[doc = #doc] }
        });

        let mods = ResourceModules(self.resources);

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
                ///
                /// The client never follows redirects, so operations can
                /// surface documented 3xx responses. Use
                /// [`Self::with_reqwest_client`] to supply a client with a
                /// different redirect policy.
                pub fn new(base_url: impl AsRef<str>) -> Result<Self, crate::error::Error> {
                    Ok(Self::with_reqwest_client(
                        ::ploidy_util::reqwest::Client::builder()
                            .redirect(::ploidy_util::reqwest::redirect::Policy::none())
                            .build()?,
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
                        .map_err(crate::error::Error::bad_header_name)?;
                    let value = value
                        .try_into()
                        .map_err(|err| crate::error::Error::bad_header_value(name.clone(), err))?;
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
                        .map_err(crate::error::Error::bad_header_name)?;
                    let mut value: ::ploidy_util::http::HeaderValue = value
                        .try_into()
                        .map_err(|err| crate::error::Error::bad_header_value(name.clone(), err))?;
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
                /// to the base URL's path and query. The path can be relative or
                /// absolute; its segments are appended to the base path.
                /// Appended query parameters are not deduplicated.
                ///
                /// For example, if this client's base URL is
                /// `https://api.example.com/v1` and `path_and_query` is
                /// `/pets/list?limit=10`, the request URL is
                /// `https://api.example.com/v1/pets/list?limit=10`.
                /// Prefer using the builder's [`query`] method to append
                /// dynamic query parameters; use `path_and_query` for static
                /// parameters.
                ///
                /// The request includes the client's default headers.
                ///
                /// Use this for requests that the client's operation methods
                /// don't cover.
                ///
                /// [`RequestBuilder`]: crate::util::reqwest::RequestBuilder
                /// [`query`]: crate::util::reqwest::RequestBuilder::query
                pub fn request(
                    &self,
                    method: crate::util::reqwest::Method,
                    path_and_query: &str,
                ) -> Result<crate::util::reqwest::RequestBuilder, crate::error::Error> {
                    let url = ::ploidy_util::url::UrlExt::with_path_and_query(
                        self.base_url.clone(),
                        path_and_query,
                    )?;
                    Ok(self.client
                        .request(method, url)
                        .headers(self.headers.clone()))
                }
            }

            #mods
        });
    }
}

impl IntoCode for CodegenClientModule<'_> {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/client/mod.rs", self.into_token_stream())
    }
}

#[derive(Debug)]
struct ResourceModules<'a>(&'a [ResourceGroup<'a>]);

impl ToTokens for ResourceModules<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(self.0.iter().map(|ident| match ident {
            &ResourceGroup::Named(name) => {
                let cfg = CfgFeature::Single(name);
                let mod_name = CodegenIdentUsage::Module(name);
                quote! {
                    #cfg
                    pub mod #mod_name;
                }
            }
            ResourceGroup::Default => quote!(
                pub mod default;
            ),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        arena::Arena,
        ir::{RawGraph, Spec},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::naming::UniqueIdents;

    #[test]
    fn test_resource_modules_gates_named_resources_and_keeps_default_ungated() {
        let arena = Arena::new();
        let mut scope = UniqueIdents::new(&arena);
        let resources = [
            ResourceGroup::Default,
            ResourceGroup::Named(scope.claim("customer_profiles")),
        ];
        let modules = ResourceModules(&resources);

        let actual: syn::File = parse_quote!(#modules);
        let expected: syn::File = parse_quote! {
            pub mod default;

            #[cfg(feature = "customer-profiles")]
            pub mod customer_profiles;
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_client_new_disables_redirects() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
        "})
        .unwrap();

        let arena = Arena::new();
        let spec = Spec::from_doc(&arena, &doc).unwrap();
        let graph = CodegenGraph::new(RawGraph::new(&arena, &spec).cook());

        let module = CodegenClientModule::new(&graph, &[]);
        let file: syn::File = parse_quote!(#module);
        let new_fn = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(impl_) => impl_.items.iter().find_map(|item| match item {
                    syn::ImplItem::Fn(f) if f.sig.ident == "new" => Some(f),
                    _ => None,
                }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected `fn new` in `impl Client`; got `{file:?}`"));

        let expected: syn::ImplItemFn = parse_quote! {
            /// Creates a new client.
            ///
            /// The client never follows redirects, so operations can
            /// surface documented 3xx responses. Use
            /// [`Self::with_reqwest_client`] to supply a client with a
            /// different redirect policy.
            pub fn new(base_url: impl AsRef<str>) -> Result<Self, crate::error::Error> {
                Ok(Self::with_reqwest_client(
                    ::ploidy_util::reqwest::Client::builder()
                        .redirect(::ploidy_util::reqwest::redirect::Policy::none())
                        .build()?,
                    base_url.as_ref().parse()?,
                ))
            }
        };
        assert_eq!(*new_fn, expected);
    }
}
