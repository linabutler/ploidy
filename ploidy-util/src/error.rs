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
    QueryParam(#[from] crate::QueryParamError),

    /// Invalid HTTP header name.
    #[error("Invalid header name")]
    BadHeaderName(#[source] http::Error),

    /// Invalid HTTP header value.
    #[error("Invalid value for header `{0}`")]
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
