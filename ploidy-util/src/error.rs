use std::fmt::{Display, Formatter, Result as FmtResult};

use http::{HeaderName, StatusCode};
use url::ParseError as UrlParseError;

use crate::{query::QueryParamError, url::PathAndQueryError};

/// A client error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("error building request")]
    Build(#[from] BuildError),

    #[error("HTTP transport error")]
    Transport(#[source] reqwest::Error),

    #[error("HTTP status error ({0})")]
    Status(StatusCode),

    #[error("invalid or unexpected response body")]
    Body(#[from] BodyError),

    #[error("invalid or missing response header")]
    Header(#[from] HeaderError),
}

impl Error {
    /// Creates an error for an invalid HTTP header name.
    #[cold]
    pub fn bad_header_name(err: impl Into<http::Error>) -> Self {
        Self::Build(BuildError::HeaderName(err.into()))
    }

    /// Creates an error for an invalid HTTP header value.
    #[cold]
    pub fn bad_header_value(name: HeaderName, err: impl Into<http::Error>) -> Self {
        Self::Build(BuildError::HeaderValue(name, err.into()))
    }

    /// Creates an error for a required response header that's missing.
    #[cold]
    pub fn missing_header(name: &'static str) -> Self {
        Self::Header(HeaderError::Missing(name))
    }

    /// Creates an error for a response header whose value isn't valid UTF-8.
    #[cold]
    pub fn invalid_header(name: &'static str) -> Self {
        Self::Header(HeaderError::NotUtf8(name))
    }

    /// Returns the telemetry category for this error.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Build(_) => ErrorCategory::Build,
            Self::Transport(err) if err.is_timeout() => ErrorCategory::Timeout,
            Self::Transport(err) if err.is_connect() => ErrorCategory::Connect,
            Self::Transport(_) => ErrorCategory::Transport,
            &Self::Status(status) => ErrorCategory::Status(status),
            Self::Body(_) => ErrorCategory::Body,
            Self::Header(_) => ErrorCategory::Header,
        }
    }
}

impl From<QueryParamError> for Error {
    fn from(err: QueryParamError) -> Self {
        Self::Build(BuildError::QueryParam(err))
    }
}

impl From<PathAndQueryError> for Error {
    fn from(err: PathAndQueryError) -> Self {
        Self::Build(BuildError::Path(err))
    }
}

impl From<UrlParseError> for Error {
    fn from(err: UrlParseError) -> Self {
        Self::Build(BuildError::Url(err))
    }
}

impl From<reqwest::Error> for Error {
    #[cold]
    fn from(err: reqwest::Error) -> Self {
        if err.is_builder() {
            Self::Build(BuildError::Request(err))
        } else if let Some(status) = err.status() {
            Self::Status(status)
        } else {
            Self::Transport(err)
        }
    }
}

impl From<serde_json::Error> for Error {
    #[cold]
    fn from(err: serde_json::Error) -> Self {
        Self::Body(BodyError::Json(err))
    }
}

impl From<serde_path_to_error::Error<serde_json::Error>> for Error {
    #[cold]
    fn from(err: serde_path_to_error::Error<serde_json::Error>) -> Self {
        Self::Body(BodyError::JsonWithPath(err))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("invalid URL")]
    Url(#[source] UrlParseError),
    #[error("invalid query parameter")]
    QueryParam(#[source] QueryParamError),
    #[error(transparent)]
    Path(PathAndQueryError),
    #[error("invalid header name")]
    HeaderName(#[source] http::Error),
    #[error("invalid value for header `{0}`")]
    HeaderValue(HeaderName, #[source] http::Error),
    #[error(transparent)]
    Request(reqwest::Error),
}

/// Invalid or unexpected response body, with or without a path
/// to the unexpected section.
#[derive(Debug, thiserror::Error)]
pub enum BodyError {
    #[error(transparent)]
    Json(serde_json::Error),
    #[error(transparent)]
    JsonWithPath(serde_path_to_error::Error<serde_json::Error>),
}

/// A missing or invalid header on a documented response.
///
/// Unlike [`BuildError::HeaderName`] and [`BuildError::HeaderValue`],
/// which describe *request* headers rejected while building a request,
/// [`HeaderError`] describes *response* headers that don't match the
/// API's documented shape.
#[derive(Debug, thiserror::Error)]
pub enum HeaderError {
    #[error("missing required header `{0}`")]
    Missing(&'static str),
    #[error("header `{0}` isn't valid UTF-8")]
    NotUtf8(&'static str),
}

/// The telemetry category for an [`Error`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    Build,
    Connect,
    Timeout,
    Transport,
    Status(StatusCode),
    Body,
    Header,
}

impl Display for ErrorCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(match self {
            Self::Build => "build",
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::Status(status) => status.as_str(),
            Self::Body => "body",
            Self::Header => "header",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_header_error_category_is_header() {
        let err = Error::missing_header("location");
        assert!(matches!(
            err,
            Error::Header(HeaderError::Missing("location")),
        ));
        assert_eq!(err.category(), ErrorCategory::Header);
        assert_eq!(err.category().to_string(), "header");
    }

    #[test]
    fn test_invalid_header_error_display_names_header() {
        let err = Error::invalid_header("etag");
        let Error::Header(inner) = err else {
            panic!("expected header error; got `{err:?}`");
        };
        assert_eq!(inner.to_string(), "header `etag` isn't valid UTF-8");
    }
}
