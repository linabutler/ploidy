use std::{iter::Peekable, str::CharIndices};

use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use unicase::UniCase;

use crate::arena::Arena;

/// Deduplicates names across case conventions.
#[derive(Debug)]
pub struct UniqueNames<'a> {
    arena: &'a Arena,
    space: FxHashMap<&'a [UniCase<&'a str>], FxHashSet<usize>>,
}

impl<'a> UniqueNames<'a> {
    pub fn new(arena: &'a Arena) -> Self {
        Self {
            arena,
            space: FxHashMap::default(),
        }
    }

    pub fn with_reserved<S: AsRef<str>>(
        arena: &'a Arena,
        reserved: impl IntoIterator<Item = S>,
    ) -> Self {
        let mut names = Self::new(arena);
        for name in reserved {
            names.reserve(name.as_ref());
        }
        names
    }

    /// Adds a name to this scope and returns a unique form.
    ///
    /// Names without word content use numeric forms starting at `1`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ploidy_core::{arena::Arena, codegen::UniqueNames};
    /// let arena = Arena::new();
    /// let mut names = UniqueNames::new(&arena);
    /// assert_eq!(names.uniquify("HTTPResponse"), "HTTPResponse");
    /// assert_eq!(names.uniquify("HTTP_Response"), "HTTP_Response2");
    /// assert_eq!(names.uniquify("httpResponse"), "httpResponse3");
    /// assert_eq!(names.uniquify("http_response_2"), "http_response4");
    /// ```
    pub fn uniquify(&mut self, name: &str) -> &'a str {
        let parsed = ParsedName::parse(name);
        let segments = self.arena.alloc_slice_exact(
            parsed
                .segments()
                .iter()
                .map(|(_, segment)| UniCase::new(&*self.arena.alloc_str(segment))),
        );
        let occupied = self.space.entry(segments).or_default();

        let suffix = if occupied.insert(parsed.suffix()) {
            parsed.suffix()
        } else {
            let mut suffix = parsed.min_suffix();
            while !occupied.insert(suffix) {
                suffix = suffix.checked_add(1).unwrap();
            }
            suffix
        };

        match parsed {
            ParsedName::Empty | ParsedName::Numeric(_) => self.arena.alloc_str(&suffix.to_string()),
            ParsedName::Stemmed { stem, .. } if suffix > 0 => {
                self.arena.alloc_str(&format!("{stem}{suffix}"))
            }
            ParsedName::Stemmed { stem, .. } => self.arena.alloc_str(stem),
        }
    }

    fn reserve(&mut self, name: &str) {
        let parsed = ParsedName::parse(name);
        let segments = self.arena.alloc_slice_exact(
            parsed
                .segments()
                .iter()
                .map(|(_, segment)| UniCase::new(&*self.arena.alloc_str(segment))),
        );
        self.space
            .entry(segments)
            .or_default()
            .insert(parsed.reserved_suffix());
    }
}

/// Segments a string into words and their byte offsets,
/// detecting word boundaries for case transformations.
///
/// Word boundaries occur on:
///
/// * Non-alphanumeric characters: underscores, hyphens, etc.
/// * Lowercase-to-uppercase transitions (`httpResponse`).
/// * Uppercase-to-lowercase after an uppercase run (`XMLHttp`).
/// * Letter-to-digit and digit-to-letter transitions (`Response2`, `250g`).
///
/// The digit transition rules are stricter than Heck's segmentation,
/// to ensure that names like (`Response2`, `Response_2`) and
/// (`1099KStatus`, `1099_K_Status`) collide. Without this rule, these cases
/// would produce similar-but-distinct names differing only in their
/// internal capitalization.
///
/// # Examples
///
/// ```
/// # use itertools::Itertools;
/// # use ploidy_core::codegen::WordSegments;
/// assert_eq!(
///     WordSegments::new("HTTPResponse").collect_vec(),
///     vec![(0, "HTTP"), (4, "Response")]
/// );
/// assert_eq!(
///     WordSegments::new("HTTP_Response").collect_vec(),
///     vec![(0, "HTTP"), (5, "Response")]
/// );
/// assert_eq!(
///     WordSegments::new("httpResponse").collect_vec(),
///     vec![(0, "http"), (4, "Response")]
/// );
/// assert_eq!(
///     WordSegments::new("XMLHttpRequest").collect_vec(),
///     vec![(0, "XML"), (3, "Http"), (7, "Request")]
/// );
/// assert_eq!(
///     WordSegments::new("Response2").collect_vec(),
///     vec![(0, "Response"), (8, "2")]
/// );
/// assert_eq!(
///     WordSegments::new("250g").collect_vec(),
///     vec![(0, "250"), (3, "g")]
/// );
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
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, c)) = self.chars.next() {
            if c.is_uppercase() {
                match self.mode {
                    WordMode::Boundary | WordMode::Digit => {
                        // Start a new word with this uppercase character.
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Uppercase;
                        if let Some(start) = start {
                            return Some((start, &self.input[start..index]));
                        }
                    }
                    WordMode::Lowercase => {
                        // camelCased word (previous was lowercase;
                        // current is uppercase), start a new word.
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Uppercase;
                        if let Some(start) = start {
                            return Some((start, &self.input[start..index]));
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
                            return Some((start, &self.input[start..index]));
                        }
                        // (Stay in uppercase mode).
                    }
                }
            } else if c.is_lowercase() {
                match self.mode {
                    WordMode::Boundary | WordMode::Digit => {
                        // Start a new word with this lowercase character.
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Lowercase;
                        if let Some(start) = start {
                            return Some((start, &self.input[start..index]));
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
            } else if c.is_ascii_digit() {
                match self.mode {
                    WordMode::Boundary | WordMode::Digit => {
                        if self.current_word_starts_at.is_none() {
                            self.current_word_starts_at = Some(index);
                        }
                        self.mode = WordMode::Digit;
                    }
                    WordMode::Lowercase | WordMode::Uppercase => {
                        let start = self.current_word_starts_at.replace(index);
                        self.mode = WordMode::Digit;
                        if let Some(start) = start {
                            return Some((start, &self.input[start..index]));
                        }
                    }
                }
            } else if !c.is_alphanumeric() {
                // Start a new word at this non-alphanumeric character.
                let start = std::mem::take(&mut self.current_word_starts_at);
                self.mode = WordMode::Boundary;
                if let Some(start) = start {
                    return Some((start, &self.input[start..index]));
                }
            } else {
                // Other alphanumeric character: continue the current word.
                if self.current_word_starts_at.is_none() {
                    self.current_word_starts_at = Some(index);
                }
            }
        }
        if let Some(start) = std::mem::take(&mut self.current_word_starts_at) {
            // Trailing word.
            return Some((start, &self.input[start..]));
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
    /// Currently in a digit segment.
    Digit,
}

enum ParsedName<'a> {
    Empty,
    Numeric(usize),
    Stemmed {
        segments: Vec<(usize, &'a str)>,
        stem: &'a str,
        suffix: usize,
    },
}

impl<'a> ParsedName<'a> {
    fn parse(name: &'a str) -> Self {
        let mut segments = WordSegments::new(name).collect_vec();
        if segments.is_empty() {
            return Self::Empty;
        }

        let Some((suffix_start, suffix)) = segments
            .iter()
            .last()
            .and_then(|&(offset, segment)| Some((offset, segment.parse::<usize>().ok()?)))
        else {
            return Self::Stemmed {
                segments,
                stem: name,
                suffix: 0,
            };
        };

        segments.pop();
        if segments.is_empty() {
            return Self::Numeric(suffix);
        }

        let stem = name[..suffix_start].trim_end_matches(|c: char| !c.is_alphanumeric());
        Self::Stemmed {
            segments,
            stem,
            suffix,
        }
    }

    fn segments(&self) -> &[(usize, &'a str)] {
        match self {
            Self::Empty | Self::Numeric(_) => &[],
            Self::Stemmed { segments, .. } => segments,
        }
    }

    fn suffix(&self) -> usize {
        match self {
            Self::Empty | Self::Numeric(0) => 1,
            &(Self::Numeric(suffix) | Self::Stemmed { suffix, .. }) => suffix,
        }
    }

    fn reserved_suffix(&self) -> usize {
        match self {
            Self::Empty | Self::Numeric(0) => 0,
            &(Self::Numeric(suffix) | Self::Stemmed { suffix, .. }) => suffix,
        }
    }

    fn min_suffix(&self) -> usize {
        match self {
            Self::Empty => 1,
            &Self::Numeric(suffix) => suffix.max(1),
            &Self::Stemmed { suffix, .. } => suffix.max(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;

    #[test]
    fn test_segment_camel_case() {
        assert_eq!(
            WordSegments::new("camelCase").collect_vec(),
            vec![(0, "camel"), (5, "Case")]
        );
        assert_eq!(
            WordSegments::new("httpResponse").collect_vec(),
            vec![(0, "http"), (4, "Response")]
        );
    }

    #[test]
    fn test_segment_pascal_case() {
        assert_eq!(
            WordSegments::new("PascalCase").collect_vec(),
            vec![(0, "Pascal"), (6, "Case")]
        );
        assert_eq!(
            WordSegments::new("HttpResponse").collect_vec(),
            vec![(0, "Http"), (4, "Response")]
        );
    }

    #[test]
    fn test_segment_snake_case() {
        assert_eq!(
            WordSegments::new("snake_case").collect_vec(),
            vec![(0, "snake"), (6, "case")]
        );
        assert_eq!(
            WordSegments::new("http_response").collect_vec(),
            vec![(0, "http"), (5, "response")]
        );
    }

    #[test]
    fn test_segment_screaming_snake() {
        assert_eq!(
            WordSegments::new("SCREAMING_SNAKE").collect_vec(),
            vec![(0, "SCREAMING"), (10, "SNAKE")]
        );
        assert_eq!(
            WordSegments::new("HTTP_RESPONSE").collect_vec(),
            vec![(0, "HTTP"), (5, "RESPONSE")]
        );
    }

    #[test]
    fn test_segment_consecutive_uppercase() {
        assert_eq!(
            WordSegments::new("XMLHttpRequest").collect_vec(),
            vec![(0, "XML"), (3, "Http"), (7, "Request")]
        );
        assert_eq!(
            WordSegments::new("HTTPResponse").collect_vec(),
            vec![(0, "HTTP"), (4, "Response")]
        );
        assert_eq!(
            WordSegments::new("HTTP_Response").collect_vec(),
            vec![(0, "HTTP"), (5, "Response")]
        );
        assert_eq!(
            WordSegments::new("ALLCAPS").collect_vec(),
            vec![(0, "ALLCAPS")]
        );
    }

    #[test]
    fn test_segment_with_numbers() {
        assert_eq!(
            WordSegments::new("Response2").collect_vec(),
            vec![(0, "Response"), (8, "2")]
        );
        assert_eq!(
            WordSegments::new("response_2").collect_vec(),
            vec![(0, "response"), (9, "2")]
        );
        assert_eq!(
            WordSegments::new("HTTP2Protocol").collect_vec(),
            vec![(0, "HTTP"), (4, "2"), (5, "Protocol")]
        );
        assert_eq!(
            WordSegments::new("OAuth2Token").collect_vec(),
            vec![(0, "O"), (1, "Auth"), (5, "2"), (6, "Token")]
        );
        assert_eq!(
            WordSegments::new("HTTP2XML").collect_vec(),
            vec![(0, "HTTP"), (4, "2"), (5, "XML")]
        );
        assert_eq!(
            WordSegments::new("1099KStatus").collect_vec(),
            vec![(0, "1099"), (4, "K"), (5, "Status")]
        );
        assert_eq!(
            WordSegments::new("123abc").collect_vec(),
            vec![(0, "123"), (3, "abc")]
        );
        assert_eq!(
            WordSegments::new("123ABC").collect_vec(),
            vec![(0, "123"), (3, "ABC")]
        );
    }

    #[test]
    fn test_segment_empty_and_special() {
        assert!(WordSegments::new("").collect_vec().is_empty());
        assert!(WordSegments::new("___").collect_vec().is_empty());
        assert_eq!(WordSegments::new("a").collect_vec(), vec![(0, "a")]);
        assert_eq!(WordSegments::new("A").collect_vec(), vec![(0, "A")]);
    }

    #[test]
    fn test_segment_mixed_separators() {
        assert_eq!(
            WordSegments::new("foo-bar_baz").collect_vec(),
            vec![(0, "foo"), (4, "bar"), (8, "baz")]
        );
        assert_eq!(
            WordSegments::new("foo--bar").collect_vec(),
            vec![(0, "foo"), (5, "bar")]
        );
    }

    #[test]
    fn test_deduplication_http_response_collision() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("HTTPResponse"), "HTTPResponse");
        assert_eq!(names.uniquify("HTTP_Response"), "HTTP_Response2");
        assert_eq!(names.uniquify("httpResponse"), "httpResponse3");
        assert_eq!(names.uniquify("http_response"), "http_response4");
        // `HTTPRESPONSE` isn't a collision; it's a single word.
        assert_eq!(names.uniquify("HTTPRESPONSE"), "HTTPRESPONSE");
    }

    #[test]
    fn test_deduplication_xml_http_request() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("XMLHttpRequest"), "XMLHttpRequest");
        assert_eq!(names.uniquify("xml_http_request"), "xml_http_request2");
        assert_eq!(names.uniquify("XmlHttpRequest"), "XmlHttpRequest3");
    }

    #[test]
    fn test_deduplication_preserves_original_casing() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("HTTP_Response"), "HTTP_Response");
        assert_eq!(names.uniquify("httpResponse"), "httpResponse2");
    }

    #[test]
    fn test_deduplication_same_prefix() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("HttpRequest"), "HttpRequest");
        assert_eq!(names.uniquify("HttpResponse"), "HttpResponse");
        assert_eq!(names.uniquify("HttpError"), "HttpError");
    }

    #[test]
    fn test_deduplication_with_numbers() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("Response2"), "Response2");
        assert_eq!(names.uniquify("response_2"), "response3");

        // `0` becomes the bare stem.
        assert_eq!(names.uniquify("Response0"), "Response");
        assert_eq!(names.uniquify("response"), "response4");

        // Digit-to-uppercase collisions.
        assert_eq!(names.uniquify("1099KStatus"), "1099KStatus");
        assert_eq!(names.uniquify("1099K_Status"), "1099K_Status2");
        assert_eq!(names.uniquify("1099KStatus"), "1099KStatus3");
        assert_eq!(names.uniquify("1099_K_Status"), "1099_K_Status4");

        // Digit-to-lowercase collisions.
        assert_eq!(names.uniquify("123abc"), "123abc");
        assert_eq!(names.uniquify("123_abc"), "123_abc2");
    }

    #[test]
    fn test_deduplication_numeric_suffixes() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("OAuth2"), "OAuth2");
        assert_eq!(names.uniquify("OAuth_2"), "OAuth3");
        assert_eq!(names.uniquify("OAuth"), "OAuth");
        assert_eq!(names.uniquify("OAuth0"), "OAuth4");
    }

    #[test]
    fn test_deduplication_empty_names_start_at_one() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify(""), "1");
        assert_eq!(names.uniquify("_"), "2");
        assert_eq!(names.uniquify("---"), "3");
    }

    #[test]
    fn test_deduplication_numeric_names_share_empty_stem() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("2"), "2");
        assert_eq!(names.uniquify(""), "1");
        assert_eq!(names.uniquify("2"), "3");

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(names.uniquify("0"), "1");
        assert_eq!(names.uniquify(""), "2");
    }

    #[test]
    fn test_with_reserved_empty_stem_uses_zero_slot() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [""]);

        assert_eq!(names.uniquify(""), "1");
        assert_eq!(names.uniquify(""), "2");

        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [""]);

        assert_eq!(names.uniquify("0"), "1");
        assert_eq!(names.uniquify(""), "2");

        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, ["0"]);

        assert_eq!(names.uniquify("0"), "1");
        assert_eq!(names.uniquify(""), "2");

        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, ["_"]);

        assert_eq!(names.uniquify("_"), "1");
        assert_eq!(names.uniquify("_"), "2");
    }

    #[test]
    fn test_with_reserved_multiple() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, ["_", "reserved"]);

        assert_eq!(names.uniquify("_"), "1");
        assert_eq!(names.uniquify("reserved"), "reserved2");
        assert_eq!(names.uniquify("other"), "other");
    }

    #[test]
    fn test_with_reserved_numeric_suffixes() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, ["crate"]);

        assert_eq!(names.uniquify("crate"), "crate2");
        assert_eq!(names.uniquify("crate2"), "crate3");
    }
}
