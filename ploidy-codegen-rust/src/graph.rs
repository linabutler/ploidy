use std::ops::Deref;

use ploidy_core::{
    codegen::UniqueNames,
    ir::{ExtendableView, IrGraph, PrimitiveIrType},
};

use super::{config::CodegenConfig, naming::CodegenIdentScope};

/// Decorates an [`IrGraph`] with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a> {
    graph: IrGraph<'a>,
    has_resources: bool,
}

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

        // Check for named resources.
        let has_resources = graph.schemas().any(|s| s.resource().is_some())
            || graph.operations().any(|op| op.resource().is_some());

        Self {
            graph,
            has_resources,
        }
    }

    /// Returns whether the graph contains any types or operations that
    /// declare resource names.
    ///
    /// Ploidy understands `x-resourceId` extension fields on schema types,
    /// and `x-resource-name` on operations, as resource names.
    /// When present, these are used to generate Cargo features and
    /// `#[cfg(...)]` gates.
    #[inline]
    pub fn has_resources(&self) -> bool {
        self.has_resources
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = IrGraph<'a>;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{ir::IrSpec, parse::Document};

    #[test]
    fn test_has_resources_true_for_operation_with_resource() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  x-resource-name: pets
                  responses:
                    '200':
                      description: OK
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        assert!(graph.has_resources());
    }

    #[test]
    fn test_has_resources_true_for_schema_with_resource() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Pet:
                  type: object
                  x-resourceId: pet
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        assert!(graph.has_resources());
    }

    #[test]
    fn test_has_resources_false_when_neither_present() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /health:
                get:
                  operationId: healthCheck
                  responses:
                    '200':
                      description: OK
            components:
              schemas:
                Status:
                  type: object
                  properties:
                    healthy:
                      type: boolean
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);

        assert!(!graph.has_resources());
    }
}
