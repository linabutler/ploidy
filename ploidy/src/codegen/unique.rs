use std::borrow::Cow;
use std::collections::btree_map::Entry;
use std::str::CharIndices;
use std::{collections::BTreeMap, iter::Peekable};

use unicase::UniCase;

/// Produces names that will never collide with other names in this space,
/// even when converted to a different case.
///
/// [`UniqueNameSpace`] exists to disambiguate type and field names
/// that are distinct in the source spec, but collide when transformed
/// to a different case. (For example, both `HTTP_Response` and `HTTPResponse`
/// become `http_response` in snake case).
#[derive(Debug, Default)]
pub struct UniqueNameSpace<'a>(BTreeMap<Box<[UniCase<&'a str>]>, usize>);

impl<'a> UniqueNameSpace<'a> {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a unique name, ignoring case and case transformations.
    /// The unique name preserves the case of the original name, but adds
    /// a numeric suffix on collisions.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ploidy::codegen::unique::UniqueNameSpace;
    /// # let mut space = UniqueNameSpace::new();
    /// assert_eq!(space.uniquify("HTTPResponse"), "HTTPResponse");
    /// assert_eq!(space.uniquify("HTTP_Response"), "HTTP_Response2");
    /// assert_eq!(space.uniquify("httpResponse"), "httpResponse3");
    /// ```
    #[inline]
    pub fn uniquify(&mut self, name: &'a str) -> Cow<'a, str> {
        match self
            .0
            .entry(WordSegments::new(name).map(UniCase::new).collect())
        {
            Entry::Occupied(mut entry) => {
                let count = entry.get_mut();
                *count += 1;
                format!("{name}{count}").into()
            }
            Entry::Vacant(entry) => {
                entry.insert(1);
                name.into()
            }
        }
    }
}

/// Segments a string into words, following Heck's notion of word boundaries.
///
/// # Examples
///
/// ```
/// # use itertools::Itertools;
/// # use ploidy::codegen::unique::WordSegments;
/// assert_eq!(WordSegments::new("HTTPResponse").collect_vec(), vec!["HTTP", "Response"]);
/// assert_eq!(WordSegments::new("HTTP_Response").collect_vec(), vec!["HTTP", "Response"]);
/// assert_eq!(WordSegments::new("httpResponse").collect_vec(), vec!["http", "Response"]);
/// assert_eq!(WordSegments::new("XMLHttpRequest").collect_vec(), vec!["XML", "Http", "Request"]);
pub struct WordSegments<'a> {
    input: &'a str,
    chars: Peekable<CharIndices<'a>>,
    current_word_starts_at: Option<usize>,
    mode: WordMode,
}

impl<'a> WordSegments<'a> {
    #[inline]
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.char_indices().peekable(),
            current_word_starts_at: None,
            mode: WordMode::Boundary,
        }
    }
}

impl<'a> Iterator for WordSegments<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, c)) = self.chars.next() {
            if c.is_uppercase() {
                match self.mode {
                    WordMode::Boundary => {
                        // Start a new word with this uppercase character.
                        self.current_word_starts_at = Some(index);
                        self.mode = WordMode::Uppercase;
                    }
                    WordMode::Lowercase => {
                        // camelCased word (previous was lowercase;
                        // current is uppercase), start a new word.
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Uppercase;
                        if let Some(start) = start {
                            return Some(&self.input[start..index]);
                        }
                    }
                    WordMode::Uppercase => {
                        let next_is_lowercase = self
                            .chars
                            .peek()
                            .map(|&(_, next)| next.is_lowercase())
                            .unwrap_or(false);
                        if next_is_lowercase && let Some(start) = self.current_word_starts_at {
                            // `XMLHttp` case; start a new word with this uppercase
                            // character (the "H" in "Http").
                            self.current_word_starts_at = Some(index);
                            return Some(&self.input[start..index]);
                        }
                        // (Stay in uppercase mode).
                    }
                }
            } else if c.is_lowercase() {
                if self.current_word_starts_at.is_none() {
                    // Start a new word with this lowercase character
                    // (the "c" in "camelCase").
                    self.current_word_starts_at = Some(index);
                }
                self.mode = WordMode::Lowercase;
            } else if !c.is_alphanumeric() {
                // Start a new word at this non-alphanumeric character.
                let start = std::mem::take(&mut self.current_word_starts_at);
                self.mode = WordMode::Boundary;
                if let Some(start) = start {
                    return Some(&self.input[start..index]);
                }
            } else {
                // Digit or other character: continue the current word.
                if self.current_word_starts_at.is_none() {
                    self.current_word_starts_at = Some(index);
                }
            }
        }
        if let Some(start) = std::mem::take(&mut self.current_word_starts_at) {
            // Trailing word.
            return Some(&self.input[start..]);
        }
        None
    }
}

/// The current state of a [`WordSegments`] iterator.
#[derive(Clone, Copy)]
enum WordMode {
    /// At a word boundary: either at the start of a new word, or
    /// after a non-alphanumeric character.
    Boundary,
    /// Currently in a lowercase segment.
    Lowercase,
    /// Currently in an uppercase segment.
    Uppercase,
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;

    #[test]
    fn test_segment_camel_case() {
        assert_eq!(
            WordSegments::new("camelCase").collect_vec(),
            vec!["camel", "Case"]
        );
        assert_eq!(
            WordSegments::new("httpResponse").collect_vec(),
            vec!["http", "Response"]
        );
    }

    #[test]
    fn test_segment_pascal_case() {
        assert_eq!(
            WordSegments::new("PascalCase").collect_vec(),
            vec!["Pascal", "Case"]
        );
        assert_eq!(
            WordSegments::new("HttpResponse").collect_vec(),
            vec!["Http", "Response"]
        );
    }

    #[test]
    fn test_segment_snake_case() {
        assert_eq!(
            WordSegments::new("snake_case").collect_vec(),
            vec!["snake", "case"]
        );
        assert_eq!(
            WordSegments::new("http_response").collect_vec(),
            vec!["http", "response"]
        );
    }

    #[test]
    fn test_segment_screaming_snake() {
        assert_eq!(
            WordSegments::new("SCREAMING_SNAKE").collect_vec(),
            vec!["SCREAMING", "SNAKE"]
        );
        assert_eq!(
            WordSegments::new("HTTP_RESPONSE").collect_vec(),
            vec!["HTTP", "RESPONSE"]
        );
    }

    #[test]
    fn test_segment_consecutive_uppercase() {
        assert_eq!(
            WordSegments::new("XMLHttpRequest").collect_vec(),
            vec!["XML", "Http", "Request"]
        );
        assert_eq!(
            WordSegments::new("HTTPResponse").collect_vec(),
            vec!["HTTP", "Response"]
        );
        assert_eq!(
            WordSegments::new("HTTP_Response").collect_vec(),
            vec!["HTTP", "Response"]
        );
        assert_eq!(WordSegments::new("ALLCAPS").collect_vec(), vec!["ALLCAPS"]);
    }

    #[test]
    fn test_segment_with_numbers() {
        assert_eq!(
            WordSegments::new("Response2").collect_vec(),
            vec!["Response2"]
        );
        assert_eq!(
            WordSegments::new("response_2").collect_vec(),
            vec!["response", "2"]
        );
        assert_eq!(
            WordSegments::new("HTTP2Protocol").collect_vec(),
            vec!["HTTP2", "Protocol"]
        );
        assert_eq!(
            WordSegments::new("OAuth2Token").collect_vec(),
            vec!["O", "Auth2", "Token"]
        );
        assert_eq!(
            WordSegments::new("HTTP2XML").collect_vec(),
            vec!["HTTP2XML"]
        );
    }

    #[test]
    fn test_segment_empty_and_special() {
        assert!(WordSegments::new("").collect_vec().is_empty());
        assert!(WordSegments::new("___").collect_vec().is_empty());
        assert_eq!(WordSegments::new("a").collect_vec(), vec!["a"]);
        assert_eq!(WordSegments::new("A").collect_vec(), vec!["A"]);
    }

    #[test]
    fn test_segment_mixed_separators() {
        assert_eq!(
            WordSegments::new("foo-bar_baz").collect_vec(),
            vec!["foo", "bar", "baz"]
        );
        assert_eq!(
            WordSegments::new("foo--bar").collect_vec(),
            vec!["foo", "bar"]
        );
    }

    #[test]
    fn test_deduplication_http_response_collision() {
        let mut space = UniqueNameSpace::new();

        assert_eq!(space.uniquify("HTTPResponse"), "HTTPResponse");
        assert_eq!(space.uniquify("HTTP_Response"), "HTTP_Response2");
        assert_eq!(space.uniquify("httpResponse"), "httpResponse3");
        assert_eq!(space.uniquify("http_response"), "http_response4");
        // `HTTPRESPONSE` isn't a collision; it's a single word.
        assert_eq!(space.uniquify("HTTPRESPONSE"), "HTTPRESPONSE");
    }

    #[test]
    fn test_deduplication_xml_http_request() {
        let mut space = UniqueNameSpace::new();

        assert_eq!(space.uniquify("XMLHttpRequest"), "XMLHttpRequest");
        assert_eq!(space.uniquify("xml_http_request"), "xml_http_request2");
        assert_eq!(space.uniquify("XmlHttpRequest"), "XmlHttpRequest3");
    }

    #[test]
    fn test_deduplication_preserves_original_casing() {
        let mut space = UniqueNameSpace::new();

        assert_eq!(space.uniquify("HTTP_Response"), "HTTP_Response");
        assert_eq!(space.uniquify("httpResponse"), "httpResponse2");
    }

    #[test]
    fn test_deduplication_same_prefix() {
        let mut dedup = UniqueNameSpace::new();

        assert_eq!(dedup.uniquify("HttpRequest"), "HttpRequest");
        assert_eq!(dedup.uniquify("HttpResponse"), "HttpResponse");
        assert_eq!(dedup.uniquify("HttpError"), "HttpError");
    }

    #[test]
    fn test_deduplication_with_numbers() {
        let mut space = UniqueNameSpace::new();

        assert_eq!(space.uniquify("Response2"), "Response2");
        assert_eq!(space.uniquify("response_2"), "response_2");
    }
}
