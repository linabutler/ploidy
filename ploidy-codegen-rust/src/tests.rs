//! Shared test-only helpers.

use std::fmt::{Arguments, Debug, Display};

use pretty_assertions::Comparison;

/// Asserts that an expression matches the given pattern.
///
/// The pattern can be optionally followed by a match guard. This works
/// exactly like the unstable `assert_matches!()` macro (rust-lang/rust#82775).
/// Once it's stabilized, we can remove this version.
macro_rules! assert_matches {
    ($left:expr, $($pattern:pat_param)|+ $(if $guard:expr)? $(,)?) => {
        match $left {
            $($pattern)|+ $(if $guard)? => {}
            ref left => {
                crate::tests::assert_matches_failed(
                    left,
                    stringify!($($pattern)|+ $(if $guard)?),
                    None,
                );
            }
        }
    };
    ($left:expr, $($pattern:pat_param)|+ $(if $guard:expr)?, $($arg:tt)+) => {
        match $left {
            $($pattern)|+ $(if $guard)? => {}
            ref left => {
                crate::tests::assert_matches_failed(
                    left,
                    stringify!($($pattern)|+ $(if $guard)?),
                    Some(format_args!($($arg)+)),
                );
            }
        }
    };
}

pub(crate) use assert_matches;

#[track_caller]
pub(crate) fn assert_matches_failed(left: impl Debug, right: &str, message: Option<Arguments<'_>>) {
    struct Pattern<'a>(&'a str);
    impl Debug for Pattern<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Display::fmt(self.0, f)
        }
    }

    let right = Pattern(right);
    let comparison = Comparison::new(&left, &right);
    match message {
        Some(message) => panic!(
            "{}",
            indoc::formatdoc! {"
                assertion `left matches right` failed: {message}

                {comparison}
            "},
        ),
        None => panic!(
            "{}",
            indoc::formatdoc! {"
                assertion `left matches right` failed

                {comparison}
            "},
        ),
    }
}
