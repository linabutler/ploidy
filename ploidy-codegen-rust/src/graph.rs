use std::ops::Deref;

use ploidy_core::{
    codegen::UniqueNames,
    ir::{CookedGraph, ExtendableView, PrimitiveType},
};

use super::{config::CodegenConfig, naming::CodegenIdentScope};

/// Decorates a [`CookedGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a>(CookedGraph<'a>);

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    pub fn new(cooked: CookedGraph<'a>) -> Self {
        Self::with_config(cooked, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    pub fn with_config(cooked: CookedGraph<'a>, config: &CodegenConfig) -> Self {
        // Decorate named schema types with their Rust identifier names.
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        for mut view in cooked.schemas() {
            let ident = scope.uniquify(view.name());
            view.extensions_mut().insert(ident);
        }

        // Decorate `DateTime` primitives with the format.
        for mut view in cooked
            .primitives()
            .filter(|view| matches!(view.ty(), PrimitiveType::DateTime))
        {
            view.extensions_mut().insert(config.date_time_format);
        }

        Self(cooked)
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = CookedGraph<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
