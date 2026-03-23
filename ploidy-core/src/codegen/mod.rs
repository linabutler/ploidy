//! Code generation output and file writing.
//!
//! This module defines the [`Code`] trait, which represents a
//! single generated output file with a relative path and a
//! content string.
//!
//! [`IntoCode`] converts codegen types into [`Code`]. Any type
//! that implements [`Code`] automatically implements
//! [`IntoCode`], so codegen types can implement either trait.
//!
//! [`write_to_disk`] takes an output directory and any [`IntoCode`]
//! value, creates intermediate directories as needed, and writes the file.
//!
//! # Feature-gated blanket implementations
//!
//! - **`proc-macro2`**: `(T, TokenStream)` where `T: AsRef<str>`
//!   formats the token stream with [prettyplease] and writes it
//!   to the path given by `T`.
//!
//! [prettyplease]: https://docs.rs/prettyplease/latest/prettyplease/

use std::path::Path;

use miette::{Context, IntoDiagnostic};

pub mod unique;

pub use unique::{UniqueNames, WordSegments};

pub fn write_to_disk(output: &Path, code: impl IntoCode) -> miette::Result<()> {
    let code = code.into_code();
    let path = output.join(code.path());
    let string = code.into_string()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .into_diagnostic()
            .with_context(|| format!("Failed to create directory `{}`", parent.display()))?;
    }
    std::fs::write(&path, string)
        .into_diagnostic()
        .with_context(|| format!("Failed to write `{}`", path.display()))?;
    Ok(())
}

pub trait Code {
    fn path(&self) -> &str;
    fn into_string(self) -> miette::Result<String>;
}

#[cfg(feature = "proc-macro2")]
impl<T: AsRef<str>> Code for (T, proc_macro2::TokenStream) {
    fn path(&self) -> &str {
        self.0.as_ref()
    }

    fn into_string(self) -> miette::Result<String> {
        use quote::ToTokens;
        let file = syn::parse2(self.1.into_token_stream()).into_diagnostic();
        match file {
            Ok(file) => Ok(prettyplease::unparse(&file)),
            Err(err) => Err(err.context(format!("Failed to format `{}`", self.0.as_ref()))),
        }
    }
}

pub trait IntoCode {
    type Code: Code;

    fn into_code(self) -> Self::Code;
}

impl<T: Code> IntoCode for T {
    type Code = T;

    fn into_code(self) -> Self::Code {
        self
    }
}
