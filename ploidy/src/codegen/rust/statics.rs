use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, format_ident, quote};

use crate::codegen::IntoCode;

#[derive(Clone, Copy, Debug)]
pub struct CodegenLibrary;

impl ToTokens for CodegenLibrary {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            pub mod absent;
            pub mod date_time;
            pub mod types;
            pub mod client;
            pub mod error;

            pub use client::Client;
            pub use error::Error;
        });
    }
}

impl IntoCode for CodegenLibrary {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/lib.rs", self.into_token_stream())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenDateTimeModule;

impl ToTokens for CodegenDateTimeModule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        #[derive(Clone, Copy)]
        enum Conversion {
            TryFrom(&'static str),
            From(&'static str),
        }
        const TYPES: &[(&str, &str, Conversion)] = &[
            (
                "UnixMicroseconds",
                "chrono::serde::ts_microseconds",
                Conversion::TryFrom("from_timestamp_micros"),
            ),
            (
                "UnixMilliseconds",
                "chrono::serde::ts_milliseconds",
                Conversion::TryFrom("from_timestamp_millis"),
            ),
            (
                "UnixNanoseconds",
                "chrono::serde::ts_nanoseconds",
                Conversion::From("from_timestamp_nanos"),
            ),
            (
                "UnixSeconds",
                "chrono::serde::ts_seconds",
                Conversion::TryFrom("from_timestamp_secs"),
            ),
        ];
        let types = TYPES.iter().map(|&(ty, module, conv)| {
            let ty = format_ident!("{ty}");
            let conv = match conv {
                Conversion::TryFrom(init) => {
                    let init = format_ident!("{init}");
                    quote! {
                        impl TryFrom<i64> for #ty {
                            type Error = TryFromTimestampError;

                            fn try_from(value: i64) -> Result<Self, Self::Error> {
                                Ok(Self(DateTime::#init(value).ok_or(TryFromTimestampError)?))
                            }
                        }
                    }
                }
                Conversion::From(init) => {
                    let init = format_ident!("{init}");
                    quote! {
                        impl From<i64> for #ty {
                            fn from(value: i64) -> Self {
                                Self(DateTime::#init(value))
                            }
                        }
                    }
                }
            };
            quote! {
                #[derive(
                    Clone, Copy, Debug, Deserialize, Default, Eq, Hash, Ord,
                    PartialEq, PartialOrd, Serialize,
                )]
                #[serde(transparent)]
                pub struct #ty(#[serde(with = #module)] DateTime<Utc>);

                impl From<DateTime<Utc>> for #ty {
                    fn from(value: DateTime<Utc>) -> Self {
                        Self(value)
                    }
                }

                impl From<#ty> for DateTime<Utc> {
                    fn from(value: #ty) -> Self {
                        value.0
                    }
                }

                #conv
            }
        });
        tokens.append_all(quote! {
            use chrono::{DateTime, Utc};
            use serde::{Deserialize, Serialize};

            #(#types)*

            #[derive(Debug, thiserror::Error)]
            #[error("timestamp out of range for `DateTime<Utc>`")]
            pub struct TryFromTimestampError;
        });
    }
}

impl IntoCode for CodegenDateTimeModule {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/date_time.rs", self.into_token_stream())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenAbsentModule;

impl ToTokens for CodegenAbsentModule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            use std::marker::PhantomData;

            use serde::{Deserialize, Deserializer, Serialize, Serializer};

            /// An [`Option`]-like type that distinguishes between
            /// "value not present" and "value present but `null`".
            #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
            pub enum AbsentOr<T> {
                #[default]
                Absent,
                Null,
                Present(T),
            }

            impl<T> AbsentOr<T> {
                #[inline]
                pub fn is_absent(&self) -> bool {
                    matches!(self, Self::Absent)
                }

                #[inline]
                pub fn is_null(&self) -> bool {
                    matches!(self, Self::Null)
                }

                #[inline]
                pub fn is_present(&self) -> bool {
                    matches!(self, Self::Present(_))
                }

                #[inline]
                pub fn ok(self) -> Result<T, AbsentError> {
                    match self {
                        Self::Absent => Err(AbsentError::Absent),
                        Self::Null => Err(AbsentError::Null),
                        Self::Present(value) => Ok(value),
                    }
                }

                #[inline]
                pub fn as_ref(&self) -> AbsentOr<&T> {
                    match self {
                        Self::Absent => AbsentOr::Absent,
                        Self::Null => AbsentOr::Null,
                        Self::Present(value) => AbsentOr::Present(value),
                    }
                }

                #[inline]
                pub fn into_option(self) -> Option<T> {
                    match self {
                        Self::Absent | Self::Null => None,
                        Self::Present(value) => Some(value),
                    }
                }
            }

            impl<T> From<T> for AbsentOr<T> {
                fn from(value: T) -> Self {
                    Self::Present(value)
                }
            }

            impl<T: Serialize> Serialize for AbsentOr<T> {
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    match self {
                        Self::Absent | Self::Null => serializer.serialize_none(),
                        Self::Present(value) => serializer.serialize_some(value),
                    }
                }
            }

            impl<'de, T: Deserialize<'de>> Deserialize<'de> for AbsentOr<T> {
                fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    struct Visitor<T>(PhantomData<T>);
                    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
                        type Value = AbsentOr<T>;

                        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            f.write_str("`null` or value")
                        }

                        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                            Ok(AbsentOr::Null)
                        }

                        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                            Ok(AbsentOr::Null)
                        }

                        fn visit_some<D: Deserializer<'de>>(
                            self,
                            deserializer: D,
                        ) -> Result<Self::Value, D::Error> {
                            T::deserialize(deserializer).map(AbsentOr::Present)
                        }
                    }
                    deserializer.deserialize_option(Visitor(PhantomData))
                }
            }

            #[derive(Debug, thiserror::Error)]
            pub enum AbsentError {
                #[error("value not present")]
                Absent,
                #[error("value is `null`")]
                Null,
            }

            impl AbsentError {
                #[inline]
                pub fn field(self, name: &'static str) -> FieldAbsentError {
                    match self {
                        Self::Absent => FieldAbsentError::Absent(name),
                        Self::Null => FieldAbsentError::Null(name),
                    }
                }
            }

            #[derive(Debug, thiserror::Error)]
            pub enum FieldAbsentError {
                #[error("field `{0}` not present")]
                Absent(&'static str),
                #[error("field `{0}` is `null`")]
                Null(&'static str),
            }
        });
    }
}

impl IntoCode for CodegenAbsentModule {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/absent.rs", self.into_token_stream())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodegenErrorModule;

impl ToTokens for CodegenErrorModule {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(quote! {
            /// Transport-level error types.
            #[derive(Debug, thiserror::Error)]
            pub enum Error {
                /// Network or connection error.
                #[error("Network error")]
                Network(#[from] reqwest::Error),

                /// Invalid JSON in request or response.
                #[error("Malformed JSON")]
                Json(#[from] JsonError),

                /// Invalid URL.
                #[error("Malformed URL")]
                Url(#[from] url::ParseError),

                /// URL can't be used as a base.
                #[error("Can't use URL as base URL")]
                UrlCannotBeABase,

                /// Invalid HTTP header name.
                #[error("invalid header name")]
                BadHeaderName(#[source] http::Error),

                /// Invalid HTTP header value.
                #[error("invalid value for header `{0}`")]
                BadHeaderValue(http::HeaderName, #[source] http::Error),
            }

            /// Invalid or unexpected JSON, with or without a path
            /// to the unexpected section.
            #[derive(Debug, thiserror::Error)]
            pub enum JsonError {
                #[error(transparent)]
                Json(#[from] serde_json::Error),
                #[error(transparent)]
                JsonWithPath(#[from] serde_path_to_error::Error<serde_json::Error>),
            }
        });
    }
}

impl IntoCode for CodegenErrorModule {
    type Code = (&'static str, TokenStream);

    fn into_code(self) -> Self::Code {
        ("src/error.rs", self.into_token_stream())
    }
}
