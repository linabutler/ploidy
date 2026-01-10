use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::Ident;

use crate::{
    codegen::unique::UniqueNameSpace,
    ir::{
        IrOperationView, IrParameterStyle, IrParameterView, IrPathParameter, IrQueryParameter,
        IrRequestView, IrResponseView, IrTypeView,
    },
    parse::{Method, path::PathFragment},
};

use super::{doc_attrs, naming::CodegenIdent, ref_::CodegenRef};

/// Generates a single client method for an API operation.
pub struct CodegenOperation<'a> {
    op: &'a IrOperationView<'a>,
}

impl<'a> CodegenOperation<'a> {
    pub fn new(op: &'a IrOperationView<'a>) -> Self {
        Self { op }
    }

    /// Generates code to build the request URL, with path parameters substituted.
    fn url(
        &self,
        url: CodegenIdent<'_>,
        params: &[(CodegenIdent<'_>, &IrParameterView<'_, IrPathParameter>)],
    ) -> TokenStream {
        let segments = self
            .op
            .path()
            .segments()
            .map(|segment| match segment.fragments() {
                [] => quote! { "" },
                [PathFragment::Literal(text)] => quote! { #text },
                [PathFragment::Param(name)] => {
                    let (ident, _) = params
                        .iter()
                        .find(|(_, param)| param.name() == *name)
                        .unwrap();
                    quote!(#ident)
                }
                fragments => {
                    // Build a format string, with placeholders for parameter fragments.
                    let format = fragments.iter().fold(String::new(), |mut f, fragment| {
                        match fragment {
                            PathFragment::Literal(text) => {
                                f.push_str(&text.replace('{', "{{").replace('}', "}}"))
                            }
                            PathFragment::Param(_) => f.push_str("{}"),
                        }
                        f
                    });
                    let args = fragments
                        .iter()
                        .filter_map(|fragment| match fragment {
                            PathFragment::Param(name) => Some(name),
                            PathFragment::Literal(_) => None,
                        })
                        .map(|name| {
                            // `url::PathSegmentsMut::push` percent-encodes the
                            // full segment, so we can interpolate fragments
                            // directly.
                            let (ident, _) = params
                                .iter()
                                .find(|(_, param)| param.name() == *name)
                                .unwrap();
                            ident
                        });
                    quote! { &format!(#format, #(#args),*) }
                }
            });
        quote! {
            let #url = {
                let mut #url = self.base_url.clone();
                #url
                    .path_segments_mut()
                    .map_err(|()| crate::error::Error::UrlCannotBeABase)?
                    .pop_if_empty()
                    #(.push(#segments))*;
                #url
            };
        }
    }

    /// Generates code to append query parameters.
    fn query(
        &self,
        url: CodegenIdent<'_>,
        params: &[(CodegenIdent<'_>, &IrParameterView<'_, IrQueryParameter>)],
    ) -> TokenStream {
        let appends = params
            .iter()
            .map(|(ident, param)| {
                let name = param.name();
                let style = match param.style() {
                    Some(IrParameterStyle::DeepObject) => {
                        quote!(::ploidy_util::QueryStyle::DeepObject)
                    }
                    Some(IrParameterStyle::SpaceDelimited) => {
                        quote!(::ploidy_util::QueryStyle::SpaceDelimited)
                    }
                    Some(IrParameterStyle::PipeDelimited) => {
                        quote!(::ploidy_util::QueryStyle::PipeDelimited)
                    }
                    Some(IrParameterStyle::Form { exploded }) => {
                        quote!(::ploidy_util::QueryStyle::Form { exploded: #exploded })
                    }
                    None => quote!(::ploidy_util::QueryStyle::default()),
                };
                Some(quote! {
                    .style(#style)
                    .append(#name, &#ident)?
                })
            })
            .collect_vec();
        match &*appends {
            [] => quote! {},
            appends => quote! {
                let #url = {
                    let mut #url = #url;
                    let serializer = ::ploidy_util::QuerySerializer::new(&mut #url);
                    serializer #(#appends)*;
                    #url
                };
            },
        }
    }
}

impl ToTokens for CodegenOperation<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let operation_id = self.op.id();
        let method_name = CodegenIdent::Method(operation_id);

        let mut space = UniqueNameSpace::new();
        let mut params = vec![];

        let paths = self
            .op
            .path()
            .params()
            .map(|param| (space.uniquify(param.name()), param))
            .collect_vec();
        let paths = paths
            .iter()
            .map(|(name, param)| (CodegenIdent::Param(name), param))
            .collect_vec();
        for (ident, _) in &paths {
            params.push(quote! { #ident: &str });
        }

        let queries = self
            .op
            .query()
            .map(|param| (space.uniquify(param.name()), param))
            .collect_vec();
        let queries = queries
            .iter()
            .map(|(name, param)| (CodegenIdent::Param(name), param))
            .collect_vec();
        for (ident, param) in &queries {
            let view = param.ty();
            let ty = if param.required() || matches!(view, IrTypeView::Nullable(_)) {
                let path = CodegenRef::new(&view);
                quote!(#path)
            } else {
                let path = CodegenRef::new(&view);
                quote! { ::std::option::Option<#path> }
            };
            params.push(quote! { #ident: #ty });
        }

        // Local variables and parameters that might conflict
        // with path and query parameter names.
        let url_var = CodegenIdent::Var(&space.uniquify("url"));
        let request_param = CodegenIdent::Param(&space.uniquify("request"));
        let form_param = CodegenIdent::Param(&space.uniquify("form"));

        if let Some(request) = self.op.request() {
            match request {
                IrRequestView::Json(view) => {
                    let param_type = CodegenRef::new(&view);
                    params.push(quote! { #request_param: impl Into<#param_type> });
                }
                IrRequestView::Multipart => {
                    params.push(quote! { #form_param: reqwest::multipart::Form });
                }
            }
        }

        let return_type = match self.op.response() {
            Some(response) => match response {
                IrResponseView::Json(view) => CodegenRef::new(&view).into_token_stream(),
            },
            None => quote! { () },
        };

        let build_url = self.url(url_var, &paths);
        let build_query = self.query(url_var, &queries);

        let http_method = CodegenMethod(self.op.method());
        let build_request = match self.op.request() {
            Some(IrRequestView::Json(_)) => quote! {
                let response = self.client
                    .#http_method(#url_var)
                    .headers(self.headers.clone())
                    .json(&#request_param.into())
                    .send()
                    .await?
                    .error_for_status()?;
            },
            Some(IrRequestView::Multipart) => quote! {
                let response = self.client
                    .#http_method(#url_var)
                    .headers(self.headers.clone())
                    .multipart(#form_param)
                    .send()
                    .await?
                    .error_for_status()?;
            },
            None => quote! {
                let response = self.client
                    .#http_method(#url_var)
                    .headers(self.headers.clone())
                    .send()
                    .await?
                    .error_for_status()?;
            },
        };

        let parse_response = if self.op.response().is_some() {
            quote! {
                let body = response.bytes().await?;
                let deserializer = &mut serde_json::Deserializer::from_slice(&body);
                let result = serde_path_to_error::deserialize(deserializer)
                    .map_err(crate::error::JsonError::from)?;
                Ok(result)
            }
        } else {
            quote! {
                let _ = response;
                Ok(())
            }
        };

        let doc = self.op.description().map(doc_attrs);

        tokens.append_all(quote! {
            #doc
            pub async fn #method_name(
                &self,
                #(#params),*
            ) -> Result<#return_type, crate::error::Error> {
                #build_url
                #build_query
                #build_request
                #parse_response
            }
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenMethod(pub Method);

impl ToTokens for CodegenMethod {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(match self.0 {
            Method::Get => Ident::new("get", Span::call_site()),
            Method::Post => Ident::new("post", Span::call_site()),
            Method::Put => Ident::new("put", Span::call_site()),
            Method::Patch => Ident::new("patch", Span::call_site()),
            Method::Delete => Ident::new("delete", Span::call_site()),
        });
    }
}
