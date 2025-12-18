use std::path::Path;

use miette::{Context, IntoDiagnostic};
use proc_macro2::TokenStream;
use quote::ToTokens;

pub mod rust;

mod unique;

pub use unique::{UniqueNameSpace, WordSegments};

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

impl<T: AsRef<str>> Code for (T, TokenStream) {
    fn path(&self) -> &str {
        self.0.as_ref()
    }

    fn into_string(self) -> miette::Result<String> {
        let file = syn::parse2(self.1.into_token_stream())
            .into_diagnostic()
            .with_context(|| format!("Failed to format `{}`", self.0.as_ref()))?;
        Ok(prettyplease::unparse(&file))
    }
}

impl Code for (&'static str, toml::map::Map<String, toml::Value>) {
    fn path(&self) -> &str {
        self.0
    }

    fn into_string(self) -> miette::Result<String> {
        toml::to_string_pretty(&self.1)
            .into_diagnostic()
            .with_context(|| format!("Failed to serialize `{}`", self.0))
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
