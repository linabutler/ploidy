/// Generates a `match` expression that wraps each arm in nested
/// [`Either`][::either::Either] variants. All arms except the last are wrapped
/// in `depth` [`Either::Right`][::either::Either::Right]s around an
/// [`Either::Left`][::either::Either::Left]. The last arm is wrapped in `depth`
/// [`Either::Right`][::either::Either::Right]s around the last expression.
macro_rules! either {
    (match $val:tt { $($body:tt)+ }) => {
        either!(@collect $val; []; []; $($body)+)
    };
    // All arms except the last.
    (@collect $val:expr; [$($arms:tt)*]; [$($depth:tt)*]; $pat:pat => $expr:expr, $($rest:tt)+) => {
        either!(@collect $val;
            [$($arms)* $pat => either!(@left [$($depth)*] $expr),];
            [$($depth)* R];
            $($rest)+)
    };
    // Last arm.
    (@collect $val:expr; [$($arms:tt)*]; [$($depth:tt)*]; $pat:pat => $expr:expr $(,)?) => {
        match $val {
            $($arms)*
            $pat => either!(@right [$($depth)*] $expr),
        }
    };
    // Wrap with `depth` `Right`s, then a `Left`.
    (@left [] $expr:expr) => { ::either::Either::Left($expr) };
    (@left [R $($rest:tt)*] $expr:expr) => {
        ::either::Either::Right(either!(@left [$($rest)*] $expr))
    };
    // Wrap with `depth` `Right`s only, for the last arm.
    (@right [] $expr:expr) => { $expr };
    (@right [R $($rest:tt)*] $expr:expr) => {
        ::either::Either::Right(either!(@right [$($rest)*] $expr))
    };
}
