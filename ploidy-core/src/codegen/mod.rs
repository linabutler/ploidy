use std::path::Path;

use miette::{Context, IntoDiagnostic};

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
impl<T: AsRef<str>> Code for (T, proc_macro2::TokenStream) {
    fn path(&self) -> &str {
        self.0.as_ref()
    }

    fn into_string(self) -> miette::Result<String> {
        use quote::ToTokens;
        let file = syn::parse2(self.1.into_token_stream())
            .into_diagnostic()
            .with_context(|| format!("Failed to format `{}`", self.0.as_ref()))?;
        Ok(prettyplease::unparse(&file))
    }
}

#[cfg(feature = "rust")]
impl<T: serde::Serialize> Code for (&'static str, cargo_toml::Manifest<T>) {
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
