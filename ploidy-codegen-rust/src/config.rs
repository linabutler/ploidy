use serde::{Deserialize, Serialize};

/// Configuration for Rust code generation, read from `[package.metadata.ploidy]`
/// in the `Cargo.toml` of the generated crate.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CodegenConfig {
    #[serde(default)]
    pub date_time_format: DateTimeFormat,
}

/// The format to use for `date-time` types.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateTimeFormat {
    /// RFC 3339 (ISO 8601) format, using `chrono::DateTime<Utc>`.
    /// This is how OpenAPI 3.0 represents `date-time` types.
    #[default]
    Rfc3339,

    /// Unix timestamps in seconds, using `ploidy_util::UnixSeconds`.
    /// This is also the representation for the `unix-time` type.
    UnixSeconds,

    /// Unix timestamps in milliseconds, using `ploidy_util::UnixMilliseconds`.
    UnixMilliseconds,

    /// Unix timestamps in microseconds, using `ploidy_util::UnixMicroseconds`.
    UnixMicroseconds,

    /// Unix timestamps in nanoseconds, using `ploidy_util::UnixNanoseconds`.
    UnixNanoseconds,
}
