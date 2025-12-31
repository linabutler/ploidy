use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::Ident;

use crate::{
    codegen::rust::doc_attrs,
    ir::{IrOperation, IrParameter, IrParameterInfo, IrRequest, IrResponse, IrType},
    parse::{Method, path::PathFragment},
};

use super::{context::CodegenContext, naming::CodegenIdent, ref_::CodegenRef};

/// Generates a single client method for an API operation.
pub struct CodegenOperation<'a> {
    context: &'a CodegenContext<'a>,
    op: &'a IrOperation<'a>,
}

impl<'a> CodegenOperation<'a> {
    pub fn new(context: &'a CodegenContext<'a>, op: &'a IrOperation<'a>) -> Self {
        Self { context, op }
    }

    /// Generates code to build the request URL, with path parameters substituted.
    fn url(&self, params: &[&IrParameterInfo<'_>]) -> TokenStream {
        let segments = &self.op.path;
        let segs = segments.iter().map(|segment| match segment.fragments() {
            [] => quote! { "" },
            [PathFragment::Literal(text)] => quote! { #text },
            [PathFragment::Param(name)] => {
                let info = params.iter().find(|param| param.name == *name).unwrap();
                let value = CodegenIdent::Param(info.name);
                quote! { #value }
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
                        let info = params.iter().find(|param| param.name == *name).unwrap();
                        CodegenIdent::Param(info.name)
                    });
                quote! { &format!(#format, #(#args),*) }
            }
        });
        quote! {
            let url = {
                let mut url = self.base_url.clone();
                url
                    .path_segments_mut()
                    .map_err(|()| crate::error::Error::UrlCannotBeABase)?
                    .pop_if_empty()
                    #(.push(#segs))*;
                url
            };
        }
    }

    /// Generates code to append query parameters.
    fn query(&self, params: &[&IrParameterInfo<'_>]) -> TokenStream {
        if params.is_empty() {
            return quote! {};
        }

        let mut param_serializations = Vec::new();

        for param in params {
            let param_name_str = &param.name;
            let param_ident = CodegenIdent::Param(param.name);

            let serialization = match (&param.ty, param.required) {
                (IrType::Array(_), true) => {
                    quote! {
                        for value in &#param_ident {
                            url.query_pairs_mut()
                                .append_pair(#param_name_str, &value.to_string());
                        }
                    }
                }
                (IrType::Array(_), false) => {
                    quote! {
                        if let Some(ref values) = #param_ident {
                            for value in values {
                                url.query_pairs_mut()
                                    .append_pair(#param_name_str, &value.to_string());
                            }
                        }
                    }
                }
                (_, true) => {
                    quote! {
                        url.query_pairs_mut()
                            .append_pair(#param_name_str, &#param_ident.to_string());
                    }
                }
                (_, false) => {
                    quote! {
                        if let Some(ref value) = #param_ident {
                            url.query_pairs_mut()
                                .append_pair(#param_name_str, &value.to_string());
                        }
                    }
                }
            };

            param_serializations.push(serialization);
        }

        quote! {
            let url = {
                let mut url = url;
                #(#param_serializations)*
                url
            };
        }
    }
}

impl ToTokens for CodegenOperation<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let operation_id = self.op.id;
        let method_name = CodegenIdent::Method(operation_id);

        let mut params = Vec::new();

        let path_params = self
            .op
            .params
            .iter()
            .filter_map(|param| match param {
                IrParameter::Path(info) => Some(info),
                _ => None,
            })
            .collect_vec();
        for param in &path_params {
            let param_name = CodegenIdent::Param(param.name);
            params.push(quote! { #param_name: &str });
        }

        let query_params = self
            .op
            .params
            .iter()
            .filter_map(|param| match param {
                IrParameter::Query(info) => Some(info),
                _ => None,
            })
            .collect_vec();
        for param in &query_params {
            let param_name = CodegenIdent::Param(param.name);
            let base_type = CodegenRef::new(self.context, &param.ty);
            let param_type = if param.required || matches!(param.ty, IrType::Nullable(_)) {
                quote!(#base_type)
            } else {
                quote! { ::std::option::Option<#base_type> }
            };
            params.push(quote! { #param_name: #param_type });
        }

        if let Some(body_info) = &self.op.request {
            match body_info {
                IrRequest::Json(ty) => {
                    let param_type = CodegenRef::new(self.context, ty);
                    params.push(quote! { request: impl Into<#param_type> });
                }
                IrRequest::Multipart => {
                    params.push(quote! { form: reqwest::multipart::Form });
                }
            }
        }

        let return_type = match &self.op.response {
            Some(IrResponse::Json(ty)) => CodegenRef::new(self.context, ty).into_token_stream(),
            None => quote! { () },
        };

        let build_url = self.url(&path_params);
        let build_query = self.query(&query_params);

        let http_method = CodegenMethod(self.op.method);
        let build_request = match &self.op.request {
            Some(IrRequest::Json(_)) => quote! {
                let response = self.client
                    .#http_method(url)
                    .headers(self.headers.clone())
                    .json(&request.into())
                    .send()
                    .await?
                    .error_for_status()?;
            },
            Some(IrRequest::Multipart) => quote! {
                let response = self.client
                    .#http_method(url)
                    .headers(self.headers.clone())
                    .multipart(form)
                    .send()
                    .await?
                    .error_for_status()?;
            },
            None => quote! {
                let response = self.client
                    .#http_method(url)
                    .headers(self.headers.clone())
                    .send()
                    .await?
                    .error_for_status()?;
            },
        };

        let parse_response = if self.op.response.is_some() {
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

        let doc = self.op.description.map(doc_attrs);

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
