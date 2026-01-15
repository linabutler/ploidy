use std::ops::Deref;

use ploidy_core::{
    codegen::UniqueNames,
    ir::{IrGraph, View},
};

use super::naming::CodegenIdentScope;

/// Decorates an [`IrGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a>(IrGraph<'a>);

impl<'a> CodegenGraph<'a> {
    pub fn new(graph: IrGraph<'a>) -> Self {
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);
        for mut view in graph.schemas() {
            let ident = scope.uniquify(view.name());
            view.extensions_mut().insert(ident);
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
