use std::ops::Deref;

use ploidy_core::{
    codegen::UniqueNameSpace,
    ir::{IrGraph, View},
};

use super::naming::SchemaIdent;

/// Decorates an [`IrGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a>(IrGraph<'a>);

impl<'a> CodegenGraph<'a> {
    pub fn new(graph: IrGraph<'a>) -> Self {
        let mut space = UniqueNameSpace::new();
        for mut view in graph.schemas() {
            let name = view.name();
            view.extensions_mut()
                .insert(SchemaIdent(space.uniquify(name).into_owned()));
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
