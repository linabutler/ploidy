use std::{borrow::Cow, collections::hash_map::Entry, iter::Peekable, str::CharIndices};

use bumpalo::{
    Bump,
    collections::{CollectIn, Vec as BumpVec},
};
use rustc_hash::FxHashMap;
use unicase::UniCase;

/// Deduplicates names across case conventions.
#[derive(Debug, Default)]
pub struct UniqueNames(Bump);

impl UniqueNames {
    /// Creates a new arena for deduplicating names.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new, empty scope that's backed by this arena.
    ///
    /// A scope produces names that will never collide with other names
    /// within the same scope, even when converted to a different case.
    ///
    /// This is useful for disambiguating type and property names that are
    /// distinct in the source spec, but collide when transformed
    /// to a different case. For example, `HTTP_Response` and `HTTPResponse`
    /// are distinct, but both become `http_response` in snake case.
    #[inline]
    pub fn scope(&self) -> UniqueNamesScope<'_> {
        UniqueNamesScope::new(&self.0)
    }

    /// Creates a new scope that's backed by this arena, and that
    /// reserves the given names.
    ///
    /// This is useful for reserving variable names in generated code, or
    /// reserving placeholder names that would be invalid identifiers
    /// on their own.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ploidy_core::codegen::UniqueNames;
    /// let unique = UniqueNames::new();
    /// let mut scope = unique.scope_with_reserved(["_"]);
    /// assert_eq!(scope.uniquify("_"), "_2");
    /// assert_eq!(scope.uniquify("_"), "_3");
    /// ```
    #[inline]
    pub fn scope_with_reserved<S: AsRef<str>>(
        &self,
        reserved: impl IntoIterator<Item = S>,
    ) -> UniqueNamesScope<'_> {
        UniqueNamesScope::with_reserved(&self.0, reserved)
    }
}

/// A scope for unique names.
#[derive(Debug)]
pub struct UniqueNamesScope<'a> {
    arena: &'a Bump,
    space: FxHashMap<&'a [UniCase<&'a str>], usize>,
}

impl<'a> UniqueNamesScope<'a> {
    fn new(arena: &'a Bump) -> Self {
        Self {
            arena,
            space: FxHashMap::default(),
        }
    }

    fn with_reserved<S: AsRef<str>>(
        arena: &'a Bump,
        reserved: impl IntoIterator<Item = S>,
    ) -> Self {
        let space = reserved
            .into_iter()
            .map(|name| arena.alloc_str(name.as_ref()))
            .map(|name| {
                WordSegments::new(name)
                    .map(UniCase::new)
                    .collect_in::<BumpVec<_>>(arena)
            })
            .fold(FxHashMap::default(), |mut names, segments| {
                // Setting the count to 1 automatically filters out duplicates.
                names.insert(segments.into_bump_slice(), 1);
                names
            });
        Self { arena, space }
    }

    /// Adds a name to this scope. If the name doesn't exist within this scope
    /// yet, returns the name as-is; otherwise, returns the name with a
    /// unique numeric suffix.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ploidy_core::codegen::UniqueNames;
    /// let unique = UniqueNames::new();
    /// let mut scope = unique.scope();
    /// assert_eq!(scope.uniquify("HTTPResponse"), "HTTPResponse");
    /// assert_eq!(scope.uniquify("HTTP_Response"), "HTTP_Response2");
    /// assert_eq!(scope.uniquify("httpResponse"), "httpResponse3");
    /// ```
    pub fn uniquify<'b>(&mut self, name: &'b str) -> Cow<'b, str> {
        match self.space.entry(
            WordSegments::new(name)
                .map(|name| UniCase::new(&*self.arena.alloc_str(name)))
                .collect_in::<BumpVec<_>>(self.arena)
                .into_bump_slice(),
        ) {
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

/// Segments a string into words, detecting word boundaries for
/// case transformation.
///
/// Word boundaries occur on:
///
/// * Non-alphanumeric characters: underscores, hyphens, etc.
/// * Lowercase-to-uppercase transitions (`httpResponse`).
/// * Uppercase-to-lowercase after an uppercase run (`XMLHttp`).
/// * Digit-to-letter transitions (`1099KStatus`, `250g`).
///
/// The digit-to-letter rule is stricter than Heck's segmentation,
/// to ensure that names like `1099KStatus` and `1099_K_Status` collide.
/// Without this rule, these cases would produce similar-but-distinct names
/// differing only in their internal capitalization.
///
/// # Examples
///
/// ```
/// # use itertools::Itertools;
/// # use ploidy_core::codegen::WordSegments;
/// assert_eq!(WordSegments::new("HTTPResponse").collect_vec(), vec!["HTTP", "Response"]);
/// assert_eq!(WordSegments::new("HTTP_Response").collect_vec(), vec!["HTTP", "Response"]);
/// assert_eq!(WordSegments::new("httpResponse").collect_vec(), vec!["http", "Response"]);
/// assert_eq!(WordSegments::new("XMLHttpRequest").collect_vec(), vec!["XML", "Http", "Request"]);
/// assert_eq!(WordSegments::new("1099KStatus").collect_vec(), vec!["1099", "K", "Status"]);
/// assert_eq!(WordSegments::new("250g").collect_vec(), vec!["250", "g"]);
/// ```
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
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Uppercase;
                        if let Some(start) = start {
                            return Some(&self.input[start..index]);
                        }
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
                match self.mode {
                    WordMode::Boundary => {
                        // Start a new word with this lowercase character.
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Lowercase;
                        if let Some(start) = start {
                            return Some(&self.input[start..index]);
                        }
                    }
                    WordMode::Lowercase | WordMode::Uppercase => {
                        if self.current_word_starts_at.is_none() {
                            // Start or continue the current word.
                            self.current_word_starts_at = Some(index);
                        }
                        self.mode = WordMode::Lowercase;
                    }
                }
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
        assert_eq!(
            WordSegments::new("1099KStatus").collect_vec(),
            vec!["1099", "K", "Status"]
        );
        assert_eq!(
            WordSegments::new("123abc").collect_vec(),
            vec!["123", "abc"]
        );
        assert_eq!(
            WordSegments::new("123ABC").collect_vec(),
            vec!["123", "ABC"]
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
        let unique = UniqueNames::new();
        let mut scope = unique.scope();

        assert_eq!(scope.uniquify("HTTPResponse"), "HTTPResponse");
        assert_eq!(scope.uniquify("HTTP_Response"), "HTTP_Response2");
        assert_eq!(scope.uniquify("httpResponse"), "httpResponse3");
        assert_eq!(scope.uniquify("http_response"), "http_response4");
        // `HTTPRESPONSE` isn't a collision; it's a single word.
        assert_eq!(scope.uniquify("HTTPRESPONSE"), "HTTPRESPONSE");
    }

    #[test]
    fn test_deduplication_xml_http_request() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope();

        assert_eq!(scope.uniquify("XMLHttpRequest"), "XMLHttpRequest");
        assert_eq!(scope.uniquify("xml_http_request"), "xml_http_request2");
        assert_eq!(scope.uniquify("XmlHttpRequest"), "XmlHttpRequest3");
    }

    #[test]
    fn test_deduplication_preserves_original_casing() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope();

        assert_eq!(scope.uniquify("HTTP_Response"), "HTTP_Response");
        assert_eq!(scope.uniquify("httpResponse"), "httpResponse2");
    }

    #[test]
    fn test_deduplication_same_prefix() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope();

        assert_eq!(scope.uniquify("HttpRequest"), "HttpRequest");
        assert_eq!(scope.uniquify("HttpResponse"), "HttpResponse");
        assert_eq!(scope.uniquify("HttpError"), "HttpError");
    }

    #[test]
    fn test_deduplication_with_numbers() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope();

        assert_eq!(scope.uniquify("Response2"), "Response2");
        assert_eq!(scope.uniquify("response_2"), "response_2");

        // Digit-to-uppercase collisions.
        assert_eq!(scope.uniquify("1099KStatus"), "1099KStatus");
        assert_eq!(scope.uniquify("1099K_Status"), "1099K_Status2");
        assert_eq!(scope.uniquify("1099KStatus"), "1099KStatus3");
        assert_eq!(scope.uniquify("1099_K_Status"), "1099_K_Status4");

        // Digit-to-lowercase collisions.
        assert_eq!(scope.uniquify("123abc"), "123abc");
        assert_eq!(scope.uniquify("123_abc"), "123_abc2");
    }

    #[test]
    fn test_with_reserved_underscore() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope_with_reserved(["_"]);

        // `_` is reserved, so the first use gets a suffix.
        assert_eq!(scope.uniquify("_"), "_2");
        assert_eq!(scope.uniquify("_"), "_3");
    }

    #[test]
    fn test_with_reserved_multiple() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope_with_reserved(["_", "reserved"]);

        assert_eq!(scope.uniquify("_"), "_2");
        assert_eq!(scope.uniquify("reserved"), "reserved2");
        assert_eq!(scope.uniquify("other"), "other");
    }

    #[test]
    fn test_with_reserved_empty() {
        let unique = UniqueNames::new();
        let mut scope = unique.scope_with_reserved([""]);

        assert_eq!(scope.uniquify(""), "2");
        assert_eq!(scope.uniquify(""), "3");
    }
}
