use std::borrow::Cow;

use miette::SourceSpan;
use winnow::{
    Parser,
    combinator::eof,
    error::{ContextError, ParseError},
};

/// Parses a path template, like `/v1/pets/{petId}/toy`.
///
/// The grammar for path templating is adapted directly from
/// https://spec.openapis.org/oas/v3.2.0.html#x4-8-2-path-templating.
pub fn parse<'a>(input: &'a str) -> Result<Vec<PathSegment<'a>>, BadPath> {
    (self::parser::template, eof)
        .map(|(segments, _)| segments)
        .parse(input)
        .map_err(BadPath::from_parse_error)
}

/// A slash-delimited path segment that contains zero or more
/// template fragments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathSegment<'input>(Vec<PathFragment<'input>>);

impl<'input> PathSegment<'input> {
    pub fn fragments(&self) -> &[PathFragment<'input>] {
        &self.0
    }
}

/// A fragment within a path segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathFragment<'input> {
    /// Literal text.
    Literal(Cow<'input, str>),
    /// Template parameter name.
    Param(&'input str),
}

mod parser {
    use super::*;

    use winnow::{
        Parser,
        combinator::{alt, delimited, repeat},
        token::take_while,
    };

    pub fn template<'a>(input: &mut &'a str) -> winnow::Result<Vec<PathSegment<'a>>> {
        alt((
            ('/', segment, template)
                .map(|(_, head, tail)| std::iter::once(head).chain(tail).collect()),
            ('/', segment).map(|(_, segment)| vec![segment]),
            '/'.map(|_| vec![PathSegment::default()]),
        ))
        .parse_next(input)
    }

    fn segment<'a>(input: &mut &'a str) -> winnow::Result<PathSegment<'a>> {
        repeat(1.., fragment).map(PathSegment).parse_next(input)
    }

    fn fragment<'a>(input: &mut &'a str) -> winnow::Result<PathFragment<'a>> {
        alt((param, literal)).parse_next(input)
    }

    pub fn param<'a>(input: &mut &'a str) -> winnow::Result<PathFragment<'a>> {
        delimited('{', take_while(1.., |c| c != '{' && c != '}'), '}')
            .map(PathFragment::Param)
            .parse_next(input)
    }

    pub fn literal<'a>(input: &mut &'a str) -> winnow::Result<PathFragment<'a>> {
        take_while(1.., |c| {
            matches!(c,
                'A'..='Z' | 'a'..='z' | '0'..='9' |
                '-' | '.' | '_' | '~' | ':' | '@' |
                '!' | '$' | '&' | '\'' | '(' | ')' |
                '*' | '+' | ',' | ';' | '=' | '%'
            )
        })
        .verify_map(|text| {
            percent_encoding::percent_decode_str(text)
                .decode_utf8()
                .ok()
                .map(PathFragment::Literal)
        })
        .parse_next(input)
    }
}

#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("invalid URL path template")]
pub struct BadPath {
    #[source_code]
    code: String,
    #[label]
    span: SourceSpan,
}

impl BadPath {
    fn from_parse_error(error: ParseError<&str, ContextError>) -> Self {
        let input = *error.input();
        Self {
            code: input.to_owned(),
            span: error.char_span().into(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_root_path() {
        let result = parse("/").unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].fragments(), &[]);
    }

    #[test]
    fn test_simple_literal() {
        let result = parse("/users").unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].fragments(),
            &[PathFragment::Literal("users".into())]
        );
    }

    #[test]
    fn test_trailing_slash() {
        let result = parse("/users/").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].fragments(),
            &[PathFragment::Literal("users".into())]
        );
        assert_eq!(result[1].fragments(), &[]);
    }

    #[test]
    fn test_simple_template() {
        let result = parse("/users/{userId}").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].fragments(),
            &[PathFragment::Literal("users".into())]
        );
        assert_eq!(
            result[1].fragments(),
            &[PathFragment::Param("userId".into())]
        );
    }

    #[test]
    fn test_nested_path() {
        let result = parse("/api/v1/resources/{resourceId}").unwrap();

        assert_eq!(result.len(), 4);
        assert_eq!(
            result[0].fragments(),
            &[PathFragment::Literal("api".into())]
        );
        assert_eq!(result[1].fragments(), &[PathFragment::Literal("v1".into())]);
        assert_eq!(
            result[2].fragments(),
            &[PathFragment::Literal("resources".into())]
        );
        assert_eq!(result[3].fragments(), &[PathFragment::Param("resourceId")]);
    }

    #[test]
    fn test_multiple_templates() {
        let result = parse("/users/{userId}/posts/{postId}").unwrap();

        assert_eq!(result.len(), 4);
        assert_eq!(
            result[0].fragments(),
            &[PathFragment::Literal("users".into())]
        );
        assert_eq!(result[1].fragments(), &[PathFragment::Param("userId")]);
        assert_eq!(
            result[2].fragments(),
            &[PathFragment::Literal("posts".into())]
        );
        assert_eq!(result[3].fragments(), &[PathFragment::Param("postId")]);
    }

    #[test]
    fn test_literal_with_extension() {
        let result =
            parse("/v1/storage/workspace/{workspace}/documents/download/{documentId}.pdf").unwrap();

        assert_eq!(result.len(), 7);
        assert_eq!(result[0].fragments(), &[PathFragment::Literal("v1".into())]);
        assert_eq!(
            result[1].fragments(),
            &[PathFragment::Literal("storage".into())]
        );
        assert_eq!(
            result[2].fragments(),
            &[PathFragment::Literal("workspace".into())]
        );
        assert_eq!(result[3].fragments(), &[PathFragment::Param("workspace")]);
        assert_eq!(
            result[4].fragments(),
            &[PathFragment::Literal("documents".into())]
        );
        assert_eq!(
            result[5].fragments(),
            &[PathFragment::Literal("download".into())]
        );
        assert_eq!(
            result[6].fragments(),
            &[
                PathFragment::Param("documentId"),
                PathFragment::Literal(".pdf".into())
            ]
        );
    }

    #[test]
    fn test_mixed_literal_and_param() {
        let result =
            parse("/v1/storage/workspace/{workspace}/documents/download/report-{documentId}.pdf")
                .unwrap();

        assert_eq!(result.len(), 7);
        assert_eq!(result[0].fragments(), &[PathFragment::Literal("v1".into())]);
        assert_eq!(
            result[1].fragments(),
            &[PathFragment::Literal("storage".into())]
        );
        assert_eq!(
            result[2].fragments(),
            &[PathFragment::Literal("workspace".into())]
        );
        assert_eq!(result[3].fragments(), &[PathFragment::Param("workspace")]);
        assert_eq!(
            result[4].fragments(),
            &[PathFragment::Literal("documents".into())]
        );
        assert_eq!(
            result[5].fragments(),
            &[PathFragment::Literal("download".into())]
        );
        assert_eq!(
            result[6].fragments(),
            &[
                PathFragment::Literal("report-".into()),
                PathFragment::Param("documentId"),
                PathFragment::Literal(".pdf".into())
            ]
        );
    }

    #[test]
    fn test_double_slash() {
        // Empty path segments aren't allowed.
        assert!(parse("/users//a").is_err());
    }

    #[test]
    fn test_invalid_chars_in_template() {
        // Parameter names can contain any character except for
        // `{` and `}`, per the `template-expression-param-name` terminal.
        assert!(parse("/users/{user/{id}}").is_err());
    }
}
