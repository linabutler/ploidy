use percent_encoding::percent_decode_str;

pub use ::url::*;

/// Extensions to [`Url`].
pub trait UrlExt: Sized {
    /// Returns this URL with path segments and query parameters from
    /// `path_and_query` appended.
    fn with_path_and_query(self, path_and_query: &str) -> Result<Self, PathAndQueryError>;
}

impl UrlExt for Url {
    fn with_path_and_query(mut self, path_and_query: &str) -> Result<Self, PathAndQueryError> {
        let path_and_query = path_and_query.strip_prefix('/').unwrap_or(path_and_query);
        let (path, query) = path_and_query
            .split_once('?')
            .unwrap_or((path_and_query, ""));
        if !path.is_empty() {
            let mut segments = self.path_segments_mut().map_err(|()| PathAndQueryError)?;
            segments.pop_if_empty();
            for segment in path.split('/') {
                if segment.is_empty() || !segment.chars().all(is_path_char) {
                    Err(PathAndQueryError)?;
                }
                segments.push(
                    &percent_decode_str(segment)
                        .decode_utf8()
                        .map_err(|_| PathAndQueryError)?,
                );
            }
        }
        if !query.is_empty() {
            if !query.chars().all(is_query_char) {
                Err(PathAndQueryError)?;
            }
            self.query_pairs_mut()
                .extend_pairs(::url::form_urlencoded::parse(query.as_bytes()));
        }
        Ok(self)
    }
}

/// An error returned when a path and query can't be parsed.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("invalid path and query")]
pub struct PathAndQueryError;

/// Returns whether `c` is allowed in a URL path segment per
/// the WHATWG URL Standard's [path percent-encode set][set].
///
/// Matches `ploidy_core::parse::path`; duplicated here to avoid
/// `ploidy-util` depending on `ploidy-core`.
///
/// [set]: https://url.spec.whatwg.org/#path-percent-encode-set
fn is_path_char(c: char) -> bool {
    is_query_char(c) && !matches!(c, '/' | '?' | '^' | '`' | '{' | '}')
}

/// Returns whether `c` is allowed in a URL query string per
/// the WHATWG URL Standard's [query percent-encode set][set].
/// Duplicated from `ploidy_core::parse::path`.
///
/// [set]: https://url.spec.whatwg.org/#query-percent-encode-set
fn is_query_char(c: char) -> bool {
    !matches!(
        c,
        '\x00'..='\x1f' | ('\x7f'..) | ' ' | '"' | '#' | '<' | '>'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_appends_relative_path_and_query() {
        let url = Url::parse("https://api.example.com/v1")
            .unwrap()
            .with_path_and_query("pets/list?limit=10")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/pets/list?limit=10"
        );
    }

    #[test]
    fn test_appends_absolute_path() {
        let url = Url::parse("https://api.example.com/v1/")
            .unwrap()
            .with_path_and_query("/pets/list")
            .unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1/pets/list");
    }

    #[test]
    fn test_appends_query_only() {
        let url = Url::parse("https://api.example.com/v1?beta=true")
            .unwrap()
            .with_path_and_query("?limit=10")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1?beta=true&limit=10"
        );
    }

    #[test]
    fn test_decodes_path_segments_before_appending() {
        let url = Url::parse("https://api.example.com/v1")
            .unwrap()
            .with_path_and_query("pets/%E6%9F%B4%20%E7%8A%AC")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/pets/%E6%9F%B4%20%E7%8A%AC"
        );
    }

    #[test]
    fn test_ignores_empty_query() {
        let url = Url::parse("https://api.example.com/v1")
            .unwrap()
            .with_path_and_query("?")
            .unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1");
    }

    #[test]
    fn test_rejects_invalid_path_char() {
        let url = Url::parse("https://api.example.com/v1").unwrap();

        let err = url.with_path_and_query("pets/{id}");
        assert!(err.is_err());
    }

    #[test]
    fn test_rejects_empty_path_segment() {
        let url = Url::parse("https://api.example.com/v1").unwrap();

        let err = url.with_path_and_query("pets//list");
        assert!(err.is_err());
    }

    #[test]
    fn test_rejects_invalid_query_char() {
        let url = Url::parse("https://api.example.com/v1").unwrap();

        let err = url.with_path_and_query("pets?tag=dog#cat");
        assert!(err.is_err());
    }
}
