/// Asserts that `$expr` matches `$pattern`, panicking with the actual
/// value if it doesn't.
#[allow(unused_macros)]
macro_rules! assert_matches {
    ($expr:expr, $pattern:pat) => {
        let value = $expr;
        assert!(
            matches!(&value, $pattern),
            "expected {}, got `{:?}`",
            stringify!($pattern),
            value
        );
    };
}

#[allow(unused_imports)]
pub(crate) use assert_matches;
