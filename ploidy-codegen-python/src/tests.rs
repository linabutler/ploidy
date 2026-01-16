//! Shared test utilities for the Python code generator.

/// A helper macro for pattern matching assertions.
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $pattern:pat $(if $guard:expr)? $(,)?) => {
        match $expression {
            $pattern $(if $guard)? => {}
            ref e => panic!(
                "assertion failed: `{:?}` does not match `{}`",
                e,
                stringify!($pattern $(if $guard)?)
            ),
        }
    };
}
