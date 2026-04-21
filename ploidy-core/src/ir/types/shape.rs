//! Generic operation types, parameterized over the type reference
//! representation. Used by both spec and graph layers.

use crate::parse::{Method, path::ParsedPath};

use super::ParameterStyle;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Operation<'a, Ty> {
    pub id: &'a str,
    pub method: Method,
    pub path: &'a ParsedPath<'a>,
    pub resource: Option<&'a str>,
    pub description: Option<&'a str>,
    pub params: &'a [Parameter<'a, Ty>],
    pub request: Option<Request<Ty>>,
    pub response: Option<Response<Ty>>,
}

impl<'a, Ty> Operation<'a, Ty> {
    /// Returns an iterator over all the types that this operation
    /// references directly.
    pub fn types(&self) -> impl Iterator<Item = &Ty> {
        itertools::chain!(
            self.params.iter().map(|param| match param {
                Parameter::Path(info) => &info.ty,
                Parameter::Query(info) => &info.ty,
            }),
            self.request.as_ref().and_then(|request| match request {
                Request::Json(ty) => Some(ty),
                Request::Multipart => None,
            }),
            self.response.as_ref().map(|response| match response {
                Response::Json(ty) => ty,
            })
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Response<Ty> {
    Json(Ty),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Request<Ty> {
    Json(Ty),
    Multipart,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Parameter<'a, Ty> {
    Path(ParameterInfo<'a, Ty>),
    Query(ParameterInfo<'a, Ty>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ParameterInfo<'a, Ty> {
    pub name: &'a str,
    pub ty: Ty,
    pub required: bool,
    pub description: Option<&'a str>,
    pub style: Option<ParameterStyle>,
}
