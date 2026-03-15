use miette::SourceSpan;
use winnow::{
    Parser, Stateful,
    combinator::eof,
    error::{ContextError, ParseError},
};

use crate::arena::Arena;

/// Parser input threaded with an allocation [`Arena`].
type Input<'a> = Stateful<&'a str, &'a Arena>;

/// Parses a path template, like `/v1/pets/{petId}/toy`.
///
/// The grammar for path templating is adapted directly from
/// [the OpenAPI spec][spec].
///
/// [spec]: https://spec.openapis.org/oas/v3.2.0.html#x4-8-2-path-templating
pub fn parse<'a>(arena: &'a Arena, input: &'a str) -> Result<Vec<PathSegment<'a>>, BadPath> {
    let stateful = Input {
        input,
        state: arena,
    };
    (self::parser::template, eof)
        .map(|(segments, _)| segments)
        .parse(stateful)
        .map_err(BadPath::from_parse_error)
}

/// A slash-delimited path segment that contains zero or more
/// template fragments.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PathSegment<'input>(&'input [PathFragment<'input>]);

impl<'input> PathSegment<'input> {
    /// Returns the template fragments within this segment.
    pub fn fragments(&self) -> &'input [PathFragment<'input>] {
        self.0
    }
}

/// A fragment within a path segment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PathFragment<'input> {
    /// Literal text.
    Literal(&'input str),
    /// Template parameter name.
    Param(&'input str),
}

mod parser {
    use super::*;

    use std::borrow::Cow;

    use winnow::{
        Parser,
        combinator::{alt, delimited, repeat},
        token::take_while,
    };

    pub fn template<'a>(input: &mut Input<'a>) -> winnow::Result<Vec<PathSegment<'a>>> {
        alt((
            ('/', segment, template)
                .map(|(_, head, tail)| std::iter::once(head).chain(tail).collect()),
            ('/', segment).map(|(_, segment)| vec![segment]),
            '/'.map(|_| vec![PathSegment::default()]),
        ))
        .parse_next(input)
    }

    fn segment<'a>(input: &mut Input<'a>) -> winnow::Result<PathSegment<'a>> {
        repeat(1.., fragment)
            .map(|fragments: Vec<_>| PathSegment(input.state.alloc_slice_copy(&fragments)))
            .parse_next(input)
    }

    fn fragment<'a>(input: &mut Input<'a>) -> winnow::Result<PathFragment<'a>> {
        alt((param, literal)).parse_next(input)
    }

    pub fn param<'a>(input: &mut Input<'a>) -> winnow::Result<PathFragment<'a>> {
        delimited('{', take_while(1.., |c| c != '{' && c != '}'), '}')
            .map(PathFragment::Param)
            .parse_next(input)
    }

    pub fn literal<'a>(input: &mut Input<'a>) -> winnow::Result<PathFragment<'a>> {
        take_while(1.., |c| {
            matches!(c,
                'A'..='Z' | 'a'..='z' | '0'..='9' |
                '-' | '.' | '_' | '~' | ':' | '@' |
                '!' | '$' | '&' | '\'' | '(' | ')' |
                '*' | '+' | ',' | ';' | '=' | '%'
            )
        })
        .verify_map(|text: &str| {
            let decoded = percent_encoding::percent_decode_str(text)
                .decode_utf8()
                .ok()?;
            Some(PathFragment::Literal(match decoded {
                Cow::Borrowed(s) => s,
                Cow::Owned(s) => input.state.alloc_str(&s),
            }))
        })
        .parse_next(input)
    }
}

/// An error returned when a path template can't be parsed.
#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("invalid URL path template")]
pub struct BadPath {
    #[source_code]
    code: String,
    #[label]
    span: SourceSpan,
}

impl BadPath {
    fn from_parse_error(error: ParseError<Input<'_>, ContextError>) -> Self {
        let stateful = error.input();
        Self {
            code: stateful.input.to_owned(),
            span: error.char_span().into(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::tests::assert_matches;

    #[test]
    fn test_root_path() {
        let arena = Arena::new();
        let result = parse(&arena, "/").unwrap();

        assert_matches!(&*result, [PathSegment([])]);
    }

    #[test]
    fn test_simple_literal() {
        let arena = Arena::new();
        let result = parse(&arena, "/users").unwrap();

        assert_matches!(&*result, [PathSegment([PathFragment::Literal("users")])]);
    }

    #[test]
    fn test_trailing_slash() {
        let arena = Arena::new();
        let result = parse(&arena, "/users/").unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("users")]),
                PathSegment([]),
            ],
        );
    }

    #[test]
    fn test_simple_template() {
        let arena = Arena::new();
        let result = parse(&arena, "/users/{userId}").unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("users")]),
                PathSegment([PathFragment::Param("userId")]),
            ],
        );
    }

    #[test]
    fn test_nested_path() {
        let arena = Arena::new();
        let result = parse(&arena, "/api/v1/resources/{resourceId}").unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("api")]),
                PathSegment([PathFragment::Literal("v1")]),
                PathSegment([PathFragment::Literal("resources")]),
                PathSegment([PathFragment::Param("resourceId")]),
            ],
        );
    }

    #[test]
    fn test_multiple_templates() {
        let arena = Arena::new();
        let result = parse(&arena, "/users/{userId}/posts/{postId}").unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("users")]),
                PathSegment([PathFragment::Param("userId")]),
                PathSegment([PathFragment::Literal("posts")]),
                PathSegment([PathFragment::Param("postId")]),
            ],
        );
    }

    #[test]
    fn test_literal_with_extension() {
        let arena = Arena::new();
        let result = parse(
            &arena,
            "/v1/storage/workspace/{workspace}/documents/download/{documentId}.pdf",
        )
        .unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("v1")]),
                PathSegment([PathFragment::Literal("storage")]),
                PathSegment([PathFragment::Literal("workspace")]),
                PathSegment([PathFragment::Param("workspace")]),
                PathSegment([PathFragment::Literal("documents")]),
                PathSegment([PathFragment::Literal("download")]),
                PathSegment([
                    PathFragment::Param("documentId"),
                    PathFragment::Literal(".pdf"),
                ]),
            ],
        );
    }

    #[test]
    fn test_mixed_literal_and_param() {
        let arena = Arena::new();
        let result = parse(
            &arena,
            "/v1/storage/workspace/{workspace}/documents/download/report-{documentId}.pdf",
        )
        .unwrap();

        assert_matches!(
            &*result,
            [
                PathSegment([PathFragment::Literal("v1")]),
                PathSegment([PathFragment::Literal("storage")]),
                PathSegment([PathFragment::Literal("workspace")]),
                PathSegment([PathFragment::Param("workspace")]),
                PathSegment([PathFragment::Literal("documents")]),
                PathSegment([PathFragment::Literal("download")]),
                PathSegment([
                    PathFragment::Literal("report-"),
                    PathFragment::Param("documentId"),
                    PathFragment::Literal(".pdf"),
                ]),
            ],
        );
    }

    #[test]
    fn test_double_slash() {
        let arena = Arena::new();
        // Empty path segments aren't allowed.
        assert!(parse(&arena, "/users//a").is_err());
    }

    #[test]
    fn test_invalid_chars_in_template() {
        let arena = Arena::new();
        // Parameter names can contain any character except for
        // `{` and `}`, per the `template-expression-param-name` terminal.
        assert!(parse(&arena, "/users/{user/{id}}").is_err());
    }
}
