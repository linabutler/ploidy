//! Naming support for generated code.
//!
//! OpenAPI specs use different naming conventions for their types, operations,
//! and resources. When codegen emits these names, it needs to transform them
//! into identifiers that conform to the grammar and idiomatic case style of
//! each target language.
//!
//! Codegen segments OpenAPI names into [`NamePart`] segments. A [`UniqueNames`]
//! scope turns these segment sequences into a representation that's unique
//! within that scope, and stable regardless of whether it's rendered
//! [`AsPascalCase`], [`AsSnakeCase`], or [`AsKebabCase`].

use std::{
    fmt::{Display, Formatter, Result as FmtResult, Write},
    iter::{self, Peekable},
    mem,
};

use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::arena::Arena;

/// A scope that claims target language names before final case rendering.
///
/// [`UniqueNames`] canonicalizes source name parts into word segments,
/// assigns collision suffixes for already-claimed names, and returns
/// opaque [`UniqueName`] handles for codegen to render in any case style.
#[derive(Debug)]
pub struct UniqueNames<'a> {
    arena: &'a Arena,
    space: FxHashMap<Box<[UniCase<&'a str>]>, FxHashSet<SuffixSlot>>,
}

impl<'a> UniqueNames<'a> {
    /// Creates an empty name scope.
    pub fn new(arena: &'a Arena) -> Self {
        Self {
            arena,
            space: FxHashMap::default(),
        }
    }

    /// Creates a name scope that reserves existing names.
    pub fn with_reserved<'part, R>(arena: &'a Arena, reserved: R) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = NamePart<'part>>,
    {
        let mut space = FxHashMap::<_, FxHashSet<_>>::default();
        for parts in reserved {
            let segments = segments(parts)
                .map(|WordSegment(text, boundary)| WordSegment(&*arena.alloc_str(&text), boundary))
                .collect_vec();
            let decomposed = DecomposedName::new(&segments);
            space
                .entry(decomposed.prefix().map(|s| UniCase::new(s.0)).collect())
                .or_default()
                .insert(decomposed.slot());
        }
        Self { arena, space }
    }

    /// Claims a segmented source name, and returns a name that's
    /// unique within this scope.
    ///
    /// If the name has already been claimed, the returned name receives
    /// the next free unique numeric suffix.
    pub fn claim<'part>(
        &mut self,
        parts: impl IntoIterator<Item = NamePart<'part>>,
    ) -> UniqueName<'a> {
        let segments = segments(parts)
            .map(|WordSegment(text, boundary)| WordSegment(&*self.arena.alloc_str(&text), boundary))
            .collect_vec();
        UniqueName(self.claim_from_segments(&segments))
    }

    /// Claims a name that's already unique in another scope, and returns
    /// a unique form of that name in this scope.
    pub fn adopt(&mut self, name: UniqueName<'a>) -> UniqueName<'a> {
        UniqueName(self.claim_from_segments(name.0))
    }

    fn claim_from_segments(
        &mut self,
        segments: &[WordSegment<&'a str>],
    ) -> &'a [WordSegment<&'a str>] {
        let decomposed = DecomposedName::new(segments);
        let occupied = self
            .space
            .entry(decomposed.prefix().map(|s| UniCase::new(s.0)).collect())
            .or_default();

        match decomposed {
            DecomposedName::Empty { mut slot } => {
                // An empty or digit-only name becomes a single word
                // that's just the unique suffix.
                while !occupied.insert(SuffixSlot::Number(slot)) {
                    slot = slot.checked_add(1).unwrap();
                }
                std::slice::from_ref(self.arena.alloc(WordSegment(
                    self.arena.alloc_fmt(format_args!("{slot}")),
                    WordBoundary::First,
                )))
            }
            DecomposedName::Text {
                suffix: DecomposedSuffix::Source { mut slot, boundary },
                ..
            } => {
                // A name with an existing numeric suffix reuses the
                // boundary between the last stem and original suffix,
                // then adds the unique suffix.
                while !occupied.insert(SuffixSlot::Number(slot)) {
                    slot = slot.checked_add(1).unwrap();
                }
                self.arena
                    .alloc_slice(decomposed.prefix().chain(iter::once(WordSegment(
                        self.arena.alloc_fmt(format_args!("{slot}")),
                        boundary,
                    ))))
            }
            DecomposedName::Text {
                suffix: DecomposedSuffix::Absent,
                ..
            } => {
                let mut slot = SuffixSlot::Absent;
                while !occupied.insert(slot) {
                    slot = match slot {
                        SuffixSlot::Absent => SuffixSlot::Number(2),
                        SuffixSlot::Number(slot) => {
                            SuffixSlot::Number(slot.checked_add(1).unwrap())
                        }
                    };
                }
                match slot {
                    // A unique name doesn't need a suffix.
                    SuffixSlot::Absent => self.arena.alloc_slice(decomposed.prefix()),
                    // An unsuffixed name adds a separator, then the unique suffix.
                    SuffixSlot::Number(slot) => {
                        self.arena
                            .alloc_slice(decomposed.prefix().chain(iter::once(WordSegment(
                                self.arena.alloc_fmt(format_args!("{slot}")),
                                WordBoundary::After(SegmentBoundary::Separator),
                            ))))
                    }
                }
            }
        }
    }
}

/// A segment of an OpenAPI source name.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NamePart<'a> {
    /// Text to normalize and split into [`UniqueName`] segments.
    Text(&'a str),
    /// An explicit word boundary.
    Boundary,
}

/// A name that's unique within a scope, and that can be rendered in any case.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UniqueName<'a>(&'a [WordSegment<&'a str>]);

impl<'a> UniqueName<'a> {
    /// Returns the first character of this name's segment text.
    #[inline]
    pub fn first_char(&self) -> Option<char> {
        self.0.first().and_then(|s| s.0.chars().next())
    }

    /// Returns the segments that make up this name.
    #[inline]
    fn segments(&self) -> impl Iterator<Item = NameSegment<'a>> {
        self.0.iter().flat_map(|&WordSegment(text, boundary)| {
            either!(match boundary {
                WordBoundary::First => [NameSegment::Text(text)],
                WordBoundary::After(boundary) =>
                    [NameSegment::Boundary(boundary), NameSegment::Text(text)],
            })
            .into_iter()
        })
    }
}

/// A canonical text or boundary segment in a [`UniqueName`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum NameSegment<'a> {
    /// The canonicalized segment text.
    Text(&'a str),
    /// A segment boundary.
    Boundary(SegmentBoundary),
}

/// Formats a [`UniqueName`] as `PascalCase`.
///
/// Each segment starts with an uppercase character and continues in lowercase.
pub struct AsPascalCase<'a>(pub UniqueName<'a>);

impl Display for AsPascalCase<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        for segment in self.0.segments() {
            if let NameSegment::Text(text) = segment {
                let mut chars = text.chars();
                if let Some(c) = chars.next() {
                    write!(f, "{}", c.to_uppercase())?;
                    chars.try_for_each(|c| write!(f, "{}", c.to_lowercase()))?;
                }
            }
        }
        Ok(())
    }
}

/// Formats a [`UniqueName`] as `snake_case`.
///
/// Case and separator boundaries become `_`.
/// Letter-to-digit and digit-to-letter boundaries collapse to preserve
/// common names like `sha256`, `http2`, `x509`, and `s3`.
pub struct AsSnakeCase<'a>(pub UniqueName<'a>);

impl Display for AsSnakeCase<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        for segment in self.0.segments() {
            match segment {
                NameSegment::Boundary(
                    SegmentBoundary::LetterDigit | SegmentBoundary::DigitLetter,
                ) => continue,
                NameSegment::Boundary(_) => f.write_char('_')?,
                NameSegment::Text(text) => text
                    .chars()
                    .try_for_each(|c| write!(f, "{}", c.to_lowercase()))?,
            }
        }
        Ok(())
    }
}

/// Formats a name as `kebab-case`.
///
/// Case and separator boundaries become `-`.
/// Letter-to-digit and digit-to-letter boundaries collapse, like
/// [`AsSnakeCase`].
pub struct AsKebabCase<'a>(pub UniqueName<'a>);

impl Display for AsKebabCase<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        for segment in self.0.segments() {
            match segment {
                NameSegment::Boundary(
                    SegmentBoundary::LetterDigit | SegmentBoundary::DigitLetter,
                ) => continue,
                NameSegment::Boundary(_) => f.write_char('-')?,
                NameSegment::Text(text) => text
                    .chars()
                    .try_for_each(|c| write!(f, "{}", c.to_lowercase()))?,
            }
        }
        Ok(())
    }
}

/// A boundary between word segments.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum SegmentBoundary {
    /// The segment follows one or more separator parts.
    Separator,
    /// The segment follows a case transition.
    Case,
    /// The segment follows a letter-to-digit transition.
    LetterDigit,
    /// The segment follows a digit-to-letter transition.
    DigitLetter,
}

enum DecomposedName<'segments, 'text> {
    Empty {
        slot: usize,
    },
    Text {
        init: &'segments [WordSegment<&'text str>],
        last: Option<WordSegment<&'text str>>,
        suffix: DecomposedSuffix,
    },
}

impl<'segments, 'text> DecomposedName<'segments, 'text> {
    fn new(segments: &'segments [WordSegment<&'text str>]) -> Self {
        if segments.is_empty() {
            return Self::Empty { slot: 1 };
        }
        if let Some((&WordSegment(last, boundary), head)) = segments.split_last() {
            let stem = last.trim_end_matches(|c: char| c.is_ascii_digit());
            if let Some(slot) = last.strip_prefix(stem)
                && let Ok(slot) = slot.parse::<usize>()
            {
                if stem.is_empty() {
                    if head.is_empty() {
                        return Self::Empty { slot: slot.max(1) };
                    }
                    return Self::Text {
                        init: head,
                        last: None,
                        suffix: DecomposedSuffix::Source { slot, boundary },
                    };
                }
                let last = match head {
                    [] => WordSegment(stem, WordBoundary::First),
                    [..] => WordSegment(stem, WordBoundary::After(SegmentBoundary::Separator)),
                };
                return Self::Text {
                    init: head,
                    last: Some(last),
                    suffix: DecomposedSuffix::Source {
                        slot,
                        boundary: WordBoundary::After(SegmentBoundary::LetterDigit),
                    },
                };
            }
        }
        Self::Text {
            init: segments,
            last: None,
            suffix: DecomposedSuffix::Absent,
        }
    }

    fn prefix(&self) -> impl Iterator<Item = WordSegment<&'text str>> {
        let (init, last): (&'segments [_], Option<_>) = match self {
            Self::Empty { .. } => (&[], None),
            &Self::Text { init, last, .. } => (init, last),
        };
        init.iter().copied().chain(last)
    }

    fn slot(&self) -> SuffixSlot {
        match *self {
            Self::Empty { slot } => SuffixSlot::Number(slot),
            Self::Text {
                suffix: DecomposedSuffix::Absent,
                ..
            } => SuffixSlot::Absent,
            Self::Text {
                suffix: DecomposedSuffix::Source { slot, .. },
                ..
            } => SuffixSlot::Number(slot),
        }
    }
}

#[derive(Clone, Copy)]
enum DecomposedSuffix {
    Absent,
    Source { slot: usize, boundary: WordBoundary },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SuffixSlot {
    Absent,
    Number(usize),
}

/// Segments name parts into words.
///
/// Text parts are normalized to NFC before segmentation.
///
/// Word boundaries occur on:
///
/// * Whitespace, `-`, `_`, and explicit [`NamePart::Boundary`] parts.
/// * Lowercase-to-uppercase transitions (`httpResponse`).
/// * Uppercase-to-lowercase after an uppercase run (`XMLHttp`).
/// * Letter-to-ASCII-digit transitions (`sha256`).
/// * ASCII digit-to-letter transitions (`250g`).
fn segments<'a>(
    input: impl IntoIterator<Item = NamePart<'a>>,
) -> impl Iterator<Item = WordSegment<String>> {
    WordSegments {
        input: input
            .into_iter()
            .flat_map(|part| {
                either!(match part {
                    NamePart::Text(text) => text.nfc().map(NameChar::from),
                    NamePart::Boundary => iter::once(NameChar::Separator),
                })
            })
            .peekable(),
        state: WordState::Start,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NameChar {
    Continue(char),
    Separator,
}

impl From<char> for NameChar {
    fn from(c: char) -> Self {
        match c {
            c if c.is_whitespace() => Self::Separator,
            // Explicitly treat snake_case and kebab-case separators
            // as word boundaries.
            '_' | '-' => Self::Separator,
            c => Self::Continue(c),
        }
    }
}

/// The active or pending word state in a [`WordSegments`].
#[derive(Clone)]
enum WordState {
    /// Before the first word.
    Start,
    /// Between words, with the boundary to apply to the next word.
    Between(SegmentBoundary),
    /// Inside a word that can be emitted by the next boundary.
    InWord(String, WordBoundary, WordMode),
}

/// The character class of the active [`WordState::InWord`] state.
#[derive(Clone, Copy)]
enum WordMode {
    /// Currently in an uncased alphanumeric segment.
    Uncased,
    /// Currently in a lowercase segment.
    Lowercase,
    /// Currently in an uppercase segment.
    Uppercase,
    /// Currently in a digit segment.
    Digit,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum WordBoundary {
    First,
    After(SegmentBoundary),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct WordSegment<T>(T, WordBoundary);

struct WordSegments<I: Iterator<Item = NameChar>> {
    input: Peekable<I>,
    state: WordState,
}

impl<I: Iterator<Item = NameChar>> Iterator for WordSegments<I> {
    type Item = WordSegment<String>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(c) = self.input.next() {
            match c {
                NameChar::Separator => {
                    // Start a new word at this separator character.
                    match mem::replace(
                        &mut self.state,
                        WordState::Between(SegmentBoundary::Separator),
                    ) {
                        WordState::InWord(text, boundary, _) => {
                            while let Some(NameChar::Separator) = self.input.peek() {
                                self.input.next();
                            }
                            self.state = WordState::Between(SegmentBoundary::Separator);
                            return Some(WordSegment(text, boundary));
                        }
                        state => {
                            self.state = state;
                        }
                    }
                }
                NameChar::Continue(c) if c.is_uppercase() => {
                    match mem::replace(
                        &mut self.state,
                        WordState::Between(SegmentBoundary::Separator),
                    ) {
                        WordState::Start => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::First,
                                WordMode::Uppercase,
                            );
                        }
                        WordState::Between(next_boundary) => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(next_boundary),
                                WordMode::Uppercase,
                            );
                        }
                        WordState::InWord(
                            mut text,
                            boundary,
                            WordMode::Uncased | WordMode::Uppercase,
                        ) => {
                            let next_is_lowercase = self.input.peek().is_some_and(|next| {
                                matches!(next, NameChar::Continue(next) if next.is_lowercase())
                            });
                            if next_is_lowercase {
                                // `XMLHttp` case; start a new word with this uppercase
                                // character (the "H" in "Http").
                                self.state = WordState::InWord(
                                    c.to_string(),
                                    WordBoundary::After(SegmentBoundary::Case),
                                    WordMode::Uppercase,
                                );
                                return Some(WordSegment(text, boundary));
                            }
                            text.push(c);
                            self.state = WordState::InWord(text, boundary, WordMode::Uppercase);
                        }
                        WordState::InWord(text, boundary, WordMode::Digit) => {
                            let next_is_lowercase = self.input.peek().is_some_and(|next| {
                                matches!(next, NameChar::Continue(next) if next.is_lowercase())
                            });
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(if next_is_lowercase {
                                    SegmentBoundary::Case
                                } else {
                                    SegmentBoundary::DigitLetter
                                }),
                                WordMode::Uppercase,
                            );
                            return Some(WordSegment(text, boundary));
                        }
                        WordState::InWord(text, boundary, WordMode::Lowercase) => {
                            // Start a new word at the uppercase side of a case boundary.
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(SegmentBoundary::Case),
                                WordMode::Uppercase,
                            );
                            return Some(WordSegment(text, boundary));
                        }
                    }
                }
                NameChar::Continue(c) if c.is_lowercase() => {
                    match mem::replace(
                        &mut self.state,
                        WordState::Between(SegmentBoundary::Separator),
                    ) {
                        WordState::Start => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::First,
                                WordMode::Lowercase,
                            );
                        }
                        WordState::Between(next_boundary) => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(next_boundary),
                                WordMode::Lowercase,
                            );
                        }
                        WordState::InWord(
                            mut text,
                            boundary,
                            WordMode::Uncased | WordMode::Lowercase | WordMode::Uppercase,
                        ) => {
                            text.push(c);
                            self.state = WordState::InWord(text, boundary, WordMode::Lowercase);
                        }
                        WordState::InWord(text, boundary, WordMode::Digit) => {
                            // Start a new word after a digit segment.
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(SegmentBoundary::DigitLetter),
                                WordMode::Lowercase,
                            );
                            return Some(WordSegment(text, boundary));
                        }
                    }
                }
                NameChar::Continue(c) if c.is_ascii_digit() => {
                    match mem::replace(
                        &mut self.state,
                        WordState::Between(SegmentBoundary::Separator),
                    ) {
                        WordState::Start => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::First,
                                WordMode::Digit,
                            );
                        }
                        WordState::Between(next_boundary) => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(next_boundary),
                                WordMode::Digit,
                            );
                        }
                        WordState::InWord(mut text, boundary, WordMode::Digit) => {
                            text.push(c);
                            self.state = WordState::InWord(text, boundary, WordMode::Digit);
                        }
                        WordState::InWord(
                            text,
                            boundary,
                            WordMode::Uncased | WordMode::Lowercase | WordMode::Uppercase,
                        ) => {
                            // Start a new word after a letter segment.
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(SegmentBoundary::LetterDigit),
                                WordMode::Digit,
                            );
                            return Some(WordSegment(text, boundary));
                        }
                    }
                }
                NameChar::Continue(c) => {
                    // All other characters continue the current word.
                    match mem::replace(
                        &mut self.state,
                        WordState::Between(SegmentBoundary::Separator),
                    ) {
                        WordState::Start => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::First,
                                WordMode::Uncased,
                            );
                        }
                        WordState::Between(next_boundary) => {
                            self.state = WordState::InWord(
                                c.to_string(),
                                WordBoundary::After(next_boundary),
                                WordMode::Uncased,
                            );
                        }
                        WordState::InWord(mut text, boundary, mode) => {
                            text.push(c);
                            self.state = WordState::InWord(text, boundary, mode);
                        }
                    }
                }
            }
        }
        if let WordState::InWord(text, boundary, _) = mem::replace(
            &mut self.state,
            WordState::Between(SegmentBoundary::Separator),
        ) {
            // Trailing word.
            self.state = WordState::Between(SegmentBoundary::Separator);
            return Some(WordSegment(text, boundary));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NamePart::{Boundary, Text};

    use itertools::Itertools;

    fn segments(parts: &[NamePart<'_>]) -> Vec<String> {
        super::segments(parts.iter().copied())
            .map(|WordSegment(text, _)| text)
            .collect_vec()
    }

    #[test]
    fn test_segment_camel_case() {
        assert_eq!(segments(&[Text("camelCase")]), vec!["camel", "Case"]);
        assert_eq!(segments(&[Text("httpResponse")]), vec!["http", "Response"]);
    }

    #[test]
    fn test_segment_pascal_case() {
        assert_eq!(segments(&[Text("PascalCase")]), vec!["Pascal", "Case"]);
        assert_eq!(segments(&[Text("HttpResponse")]), vec!["Http", "Response"]);
    }

    #[test]
    fn test_segment_snake_case() {
        assert_eq!(
            segments(&[Text("snake"), Boundary, Text("case")]),
            vec!["snake", "case"]
        );
        assert_eq!(
            segments(&[Text("http"), Boundary, Text("response")]),
            vec!["http", "response"]
        );
    }

    #[test]
    fn test_segment_screaming_snake() {
        assert_eq!(
            segments(&[Text("SCREAMING"), Boundary, Text("SNAKE")]),
            vec!["SCREAMING", "SNAKE"]
        );
        assert_eq!(
            segments(&[Text("HTTP"), Boundary, Text("RESPONSE")]),
            vec!["HTTP", "RESPONSE"]
        );
    }

    #[test]
    fn test_segment_consecutive_uppercase() {
        assert_eq!(
            segments(&[Text("XMLHttpRequest")]),
            vec!["XML", "Http", "Request"]
        );
        assert_eq!(segments(&[Text("HTTPResponse")]), vec!["HTTP", "Response"]);
        assert_eq!(
            segments(&[Text("HTTP"), Boundary, Text("Response")]),
            vec!["HTTP", "Response"]
        );
        assert_eq!(segments(&[Text("ALLCAPS")]), vec!["ALLCAPS"]);
    }

    #[test]
    fn test_segment_unicode_case_boundaries() {
        assert_eq!(segments(&[Text("\u{e9}clair")]), vec!["\u{e9}clair"]);
        assert_eq!(segments(&[Text("\u{c9}clair")]), vec!["\u{c9}clair"]);
        assert_eq!(
            segments(&[Text("XML\u{c9}clair")]),
            vec!["XML", "\u{c9}clair"]
        );
        assert_eq!(
            segments(&[Text("CAF\u{c9}Token")]),
            vec!["CAF\u{c9}", "Token"]
        );
        assert_eq!(segments(&[Text("\u{e9}Tag")]), vec!["\u{e9}", "Tag"]);
        assert_eq!(segments(&[Text("\u{c9}Token")]), vec!["\u{c9}", "Token"]);
        assert_eq!(segments(&[Text("\u{e9}HTTP")]), vec!["\u{e9}", "HTTP"]);
        assert_eq!(
            segments(&[Text("foo"), Boundary, Text("bar")]),
            vec!["foo", "bar"]
        );
        assert_eq!(
            segments(&[Text("foo"), Boundary, Boundary, Text("bar")]),
            vec!["foo", "bar"]
        );
        assert_eq!(segments(&[Boundary, Text("foo"), Boundary]), vec!["foo"]);
        assert_eq!(
            segments(&[Text("foo"), Boundary, Text("2")]),
            vec!["foo", "2"]
        );
    }

    #[test]
    fn test_segment_with_numbers() {
        assert_eq!(segments(&[Text("Response2")]), vec!["Response", "2"]);
        assert_eq!(
            segments(&[Text("response"), Boundary, Text("2")]),
            vec!["response", "2"]
        );
        assert_eq!(
            segments(&[Text("HTTP2Protocol")]),
            vec!["HTTP", "2", "Protocol"]
        );
        assert_eq!(
            segments(&[Text("OAuth2Token")]),
            vec!["O", "Auth", "2", "Token"]
        );
        assert_eq!(segments(&[Text("HTTP2XML")]), vec!["HTTP", "2", "XML"]);
        assert_eq!(
            segments(&[Text("1099KStatus")]),
            vec!["1099", "K", "Status"]
        );
        assert_eq!(segments(&[Text("123abc")]), vec!["123", "abc"]);
        assert_eq!(segments(&[Text("123ABC")]), vec!["123", "ABC"]);
        assert_eq!(
            segments(&[Text("Sha2"), Boundary, Text("56Digest")]),
            vec!["Sha", "2", "56", "Digest"]
        );
    }

    #[test]
    fn test_segment_empty_and_special() {
        assert!(segments(&[]).is_empty());
        assert!(segments(&[Boundary, Boundary, Boundary]).is_empty());
        assert_eq!(segments(&[Text("a")]), vec!["a"]);
        assert_eq!(segments(&[Text("A")]), vec!["A"]);
    }

    #[test]
    fn test_segment_mixed_separators() {
        assert_eq!(
            segments(&[Text("foo"), Boundary, Text("bar"), Boundary, Text("baz"),]),
            vec!["foo", "bar", "baz"]
        );
        assert_eq!(
            segments(&[Text("foo"), Boundary, Boundary, Text("bar")]),
            vec!["foo", "bar"]
        );
    }

    #[test]
    fn test_segment_boundaries() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        let name = names.claim([Text("fooBar2"), Boundary, Text("baz3Qux")]);
        assert_eq!(
            name.segments().collect_vec(),
            [
                NameSegment::Text("foo"),
                NameSegment::Boundary(SegmentBoundary::Case),
                NameSegment::Text("Bar"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("2"),
                NameSegment::Boundary(SegmentBoundary::Separator),
                NameSegment::Text("baz"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("3"),
                NameSegment::Boundary(SegmentBoundary::Case),
                NameSegment::Text("Qux"),
            ]
        );

        let name = names.claim([Text("foo"), Boundary, Text("2Bar")]);
        assert_eq!(
            name.segments().collect_vec(),
            [
                NameSegment::Text("foo"),
                NameSegment::Boundary(SegmentBoundary::Separator),
                NameSegment::Text("2"),
                NameSegment::Boundary(SegmentBoundary::Case),
                NameSegment::Text("Bar"),
            ]
        );

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);
        let name = names.claim([Text("foo2bar")]);
        assert_eq!(
            name.segments().collect_vec(),
            [
                NameSegment::Text("foo"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("2"),
                NameSegment::Boundary(SegmentBoundary::DigitLetter),
                NameSegment::Text("bar"),
            ]
        );

        let name = names.claim([Text("Vector3D")]);
        assert_eq!(
            name.segments().collect_vec(),
            [
                NameSegment::Text("Vector"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("3"),
                NameSegment::Boundary(SegmentBoundary::DigitLetter),
                NameSegment::Text("D"),
            ]
        );

        let name = names.claim([Text("50GBPerSecond")]);
        assert_eq!(
            name.segments().collect_vec(),
            [
                NameSegment::Text("50"),
                NameSegment::Boundary(SegmentBoundary::DigitLetter),
                NameSegment::Text("GB"),
                NameSegment::Boundary(SegmentBoundary::Case),
                NameSegment::Text("Per"),
                NameSegment::Boundary(SegmentBoundary::Case),
                NameSegment::Text("Second"),
            ]
        );
    }

    #[test]
    fn test_deduplication_http_response_collision() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsPascalCase(names.claim([Text("HTTPResponse")])).to_string(),
            "HttpResponse"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("HTTP"), Boundary, Text("Response"),])).to_string(),
            "HttpResponse2"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("httpResponse")])).to_string(),
            "HttpResponse3"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("http"), Boundary, Text("response"),])).to_string(),
            "HttpResponse4"
        );
        // `HTTPRESPONSE` isn't a collision; it's a single word.
        assert_eq!(
            AsPascalCase(names.claim([Text("HTTPRESPONSE")])).to_string(),
            "Httpresponse"
        );
    }

    #[test]
    fn test_deduplication_xml_http_request() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("XMLHttpRequest")])).to_string(),
            "xml_http_request"
        );
        assert_eq!(
            AsSnakeCase(names.claim([
                Text("xml"),
                Boundary,
                Text("http"),
                Boundary,
                Text("request"),
            ]))
            .to_string(),
            "xml_http_request_2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("XmlHttpRequest")])).to_string(),
            "xml_http_request_3"
        );
    }

    #[test]
    fn test_deduplication_separator_parts() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("foo"), Boundary, Text("bar")])).to_string(),
            "foo_bar",
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("foo"), Boundary, Text("bar")])).to_string(),
            "foo_bar_2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("foo"), Boundary, Boundary, Boundary, Text("bar"),]))
                .to_string(),
            "foo_bar_3"
        );
    }

    #[test]
    fn test_deduplication_preserves_first_slot() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsPascalCase(names.claim([Text("HTTP"), Boundary, Text("Response"),])).to_string(),
            "HttpResponse"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("httpResponse")])).to_string(),
            "HttpResponse2"
        );
    }

    #[test]
    fn test_deduplication_same_prefix() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsPascalCase(names.claim([Text("HttpRequest")])).to_string(),
            "HttpRequest"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("HttpResponse")])).to_string(),
            "HttpResponse"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("HttpError")])).to_string(),
            "HttpError"
        );
    }

    #[test]
    fn test_deduplication_with_numbers() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("Response2")])).to_string(),
            "response2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("response"), Boundary, Text("2"),])).to_string(),
            "response_3"
        );

        assert_eq!(
            AsSnakeCase(names.claim([Text("Response0")])).to_string(),
            "response0"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("response")])).to_string(),
            "response"
        );

        // Internal digit boundaries collapse in PascalCase.
        assert_eq!(
            AsPascalCase(names.claim([Text("Http2Protocol")])).to_string(),
            "Http2Protocol"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("Http"), Boundary, Text("2Protocol"),])).to_string(),
            "Http2Protocol2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Sha2"), Boundary, Text("56Digest"),])).to_string(),
            "sha2_56_digest"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Sha256Digest")])).to_string(),
            "sha256_digest"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Vector3D")])).to_string(),
            "vector3d"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("50GBPerSecond")])).to_string(),
            "50gb_per_second"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Caf\u{e9}2")])).to_string(),
            "caf\u{e9}2"
        );

        // Digit-to-uppercase collisions.
        assert_eq!(
            AsPascalCase(names.claim([Text("1099KStatus")])).to_string(),
            "1099KStatus"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("1099K"), Boundary, Text("Status"),])).to_string(),
            "1099KStatus2"
        );
        assert_eq!(
            AsPascalCase(names.claim([Text("1099KStatus")])).to_string(),
            "1099KStatus3"
        );
        assert_eq!(
            AsPascalCase(names.claim([
                Text("1099"),
                Boundary,
                Text("K"),
                Boundary,
                Text("Status"),
            ]))
            .to_string(),
            "1099KStatus4"
        );

        // Digit-to-lowercase collisions.
        assert_eq!(
            AsSnakeCase(names.claim([Text("123abc")])).to_string(),
            "123abc"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("123"), Boundary, Text("abc"),])).to_string(),
            "123_abc_2"
        );
    }

    #[test]
    fn test_deduplication_numeric_suffixes() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("OAuth2")])).to_string(),
            "o_auth2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("OAuth"), Boundary, Text("2")])).to_string(),
            "o_auth_3"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("OAuth")])).to_string(),
            "o_auth"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("OAuth0")])).to_string(),
            "o_auth0"
        );
    }

    #[test]
    fn test_deduplication_numeric_suffix_preserves_source_boundary() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);
        assert_eq!(
            names
                .claim([NamePart::Text("Response2")])
                .segments()
                .collect_vec(),
            &[
                NameSegment::Text("Response"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("2"),
            ]
        );

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);
        assert_eq!(
            names
                .claim([NamePart::Text("Response0")])
                .segments()
                .collect_vec(),
            &[
                NameSegment::Text("Response"),
                NameSegment::Boundary(SegmentBoundary::LetterDigit),
                NameSegment::Text("0"),
            ]
        );
    }

    #[test]
    fn test_deduplication_numeric_suffix_slots() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(AsSnakeCase(names.claim([Text("v2")])).to_string(), "v2");
        assert_eq!(
            AsSnakeCase(names.claim([Text("v"), Boundary, Text("2")])).to_string(),
            "v_3"
        );
        assert_eq!(AsSnakeCase(names.claim([Text("v")])).to_string(), "v");
        assert_eq!(AsSnakeCase(names.claim([Text("v")])).to_string(), "v_4");

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsKebabCase(names.claim([Text("response")])).to_string(),
            "response"
        );
        assert_eq!(
            AsKebabCase(names.claim([Text("response")])).to_string(),
            "response-2"
        );
        assert_eq!(
            AsKebabCase(names.claim([Text("response2")])).to_string(),
            "response3"
        );
        assert_eq!(
            AsKebabCase(names.claim([Text("response")])).to_string(),
            "response-4"
        );
    }

    #[test]
    fn test_deduplication_source_zero_suffix_uses_own_slot() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("Response")])).to_string(),
            "response"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Response0")])).to_string(),
            "response0"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Response")])).to_string(),
            "response_2"
        );

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("Response0")])).to_string(),
            "response0"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Response")])).to_string(),
            "response"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Response0")])).to_string(),
            "response1"
        );
    }

    #[test]
    fn test_deduplication_unicode_case_family() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(AsSnakeCase(names.claim([Text("ß")])).to_string(), "ß");
        assert_eq!(AsSnakeCase(names.claim([Text("SS")])).to_string(), "ss_2");
        assert_eq!(AsSnakeCase(names.claim([Text("ss")])).to_string(), "ss_3");
        assert_eq!(
            AsSnakeCase(names.claim([Text("İ")])).to_string(),
            "i\u{307}"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("i\u{307}")])).to_string(),
            "i\u{307}_2"
        );
    }

    #[test]
    fn test_deduplication_normalizes_unicode_to_nfc() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(
            AsSnakeCase(names.claim([Text("cafe\u{301}")])).to_string(),
            "caf\u{e9}"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("caf\u{e9}")])).to_string(),
            "caf\u{e9}_2"
        );
    }

    #[test]
    fn test_deduplication_empty_names_start_at_one() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(AsSnakeCase(names.claim([])).to_string(), "1");
        assert_eq!(AsSnakeCase(names.claim([Boundary])).to_string(), "2");
        assert_eq!(
            AsSnakeCase(names.claim([Boundary, Boundary, Boundary])).to_string(),
            "3"
        );
    }

    #[test]
    fn test_deduplication_numeric_names_share_empty_stem() {
        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(AsSnakeCase(names.claim([Text("2")])).to_string(), "2");
        assert_eq!(AsSnakeCase(names.claim([])).to_string(), "1");
        assert_eq!(AsSnakeCase(names.claim([Text("2")])).to_string(), "3");

        let arena = Arena::new();
        let mut names = UniqueNames::new(&arena);

        assert_eq!(AsSnakeCase(names.claim([Text("0")])).to_string(), "1");
        assert_eq!(AsSnakeCase(names.claim([])).to_string(), "2");
    }

    #[test]
    fn test_reserved_digit_only_names_share_empty_stem_sequence() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [[Text("0")]]);

        assert_eq!(AsSnakeCase(names.claim([Text("0")])).to_string(), "2");
        assert_eq!(AsSnakeCase(names.claim([])).to_string(), "3");
    }

    #[test]
    fn test_reserved_boundary_only_shares_empty_stem_sequence() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [[Boundary]]);

        assert_eq!(AsSnakeCase(names.claim([Boundary])).to_string(), "2");
        assert_eq!(AsSnakeCase(names.claim([Boundary])).to_string(), "3");
    }

    #[test]
    fn test_reserved_multiple() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [[Boundary], [Text("reserved")]]);

        assert_eq!(AsSnakeCase(names.claim([Boundary])).to_string(), "2");
        assert_eq!(
            AsSnakeCase(names.claim([Text("reserved")])).to_string(),
            "reserved_2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("other")])).to_string(),
            "other"
        );
    }

    #[test]
    fn test_reserved_numeric_suffixes() {
        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [[Text("crate")]]);

        assert_eq!(
            AsSnakeCase(names.claim([Text("crate")])).to_string(),
            "crate_2"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("crate2")])).to_string(),
            "crate3"
        );

        let arena = Arena::new();
        let mut names = UniqueNames::with_reserved(&arena, [[Text("Response0")]]);

        assert_eq!(
            AsSnakeCase(names.claim([Text("Response")])).to_string(),
            "response"
        );
        assert_eq!(
            AsSnakeCase(names.claim([Text("Response0")])).to_string(),
            "response1"
        );
    }
}
