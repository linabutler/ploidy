use std::ops::Deref;

use rustc_hash::FxHashMap;

use ploidy_core::{codegen::UniqueNames, ir::CookedGraph};

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{CodegenIdent, CodegenIdentScope, CodegenIdentUsage},
};

/// An opaque, uniquified identifier for a named schema type.
///
/// Only [`CodegenGraph`] can construct these, ensuring that schema
/// references always use the deduplicated name. There is no `Deref`
/// to [`CodegenIdentRef`][super::naming::CodegenIdentRef]; the only
/// way to use a `SchemaIdent` is through [`as_type`][Self::as_type]
/// and [`as_module`][Self::as_module].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaIdent(CodegenIdent);

impl SchemaIdent {
    /// Returns this identifier formatted as a PascalCase type name.
    #[inline]
    pub fn as_type(&self) -> CodegenIdentUsage<'_> {
        CodegenIdentUsage::Type(&self.0)
    }

    /// Returns this identifier formatted as a snake_case module name.
    #[inline]
    pub fn as_module(&self) -> CodegenIdentUsage<'_> {
        CodegenIdentUsage::Module(&self.0)
    }
}

/// Decorates a [`CookedGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a> {
    inner: CookedGraph<'a>,
    idents: FxHashMap<&'a str, SchemaIdent>,
    date_time_format: DateTimeFormat,
}

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    pub fn new(cooked: CookedGraph<'a>) -> Self {
        Self::with_config(cooked, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    pub fn with_config(cooked: CookedGraph<'a>, config: &CodegenConfig) -> Self {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        let schema_idents = cooked
            .names()
            .map(|name| (name, SchemaIdent(scope.uniquify(name))))
            .collect();

        Self {
            inner: cooked,
            idents: schema_idents,
            date_time_format: config.date_time_format,
        }
    }

    /// Returns the uniquified identifier for a named schema type.
    #[inline]
    pub fn schema_ident(&self, name: &str) -> &SchemaIdent {
        &self.idents[name]
    }

    /// Returns the configured date-time format.
    #[inline]
    pub fn date_time_format(&self) -> DateTimeFormat {
        self.date_time_format
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = CookedGraph<'a>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
