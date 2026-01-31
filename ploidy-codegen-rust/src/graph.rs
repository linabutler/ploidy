use std::ops::Deref;

use ploidy_core::{
    codegen::UniqueNames,
    ir::{ExtendableView, IrGraph, PrimitiveIrType},
};

use super::{config::CodegenConfig, naming::CodegenIdentScope};

/// Decorates an [`IrGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a>(IrGraph<'a>);

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    pub fn new(graph: IrGraph<'a>) -> Self {
        Self::with_config(graph, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    pub fn with_config(graph: IrGraph<'a>, config: &CodegenConfig) -> Self {
        // Decorate named schema types with their Rust identifier names.
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        for mut view in graph.schemas() {
            let ident = scope.uniquify(view.name());
            view.extensions_mut().insert(ident);
        }

        // Decorate `DateTime` primitives with the format.
        for mut view in graph
            .primitives()
            .filter(|view| matches!(view.ty(), PrimitiveIrType::DateTime))
        {
            view.extensions_mut().insert(config.date_time_format);
        }

        Self(graph)
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = IrGraph<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
