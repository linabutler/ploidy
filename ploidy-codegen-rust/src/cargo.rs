use std::collections::{BTreeMap, BTreeSet};

use cargo_toml::{Dependency, DependencyDetail, Edition, Manifest};
use itertools::Itertools;
use ploidy_core::{codegen::IntoCode, ir::View};
use serde::{Deserialize, Serialize};

use super::{config::CodegenConfig, graph::CodegenGraph, naming::CargoFeature};

const PLOIDY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct CodegenCargoManifest<'a> {
    graph: &'a CodegenGraph<'a>,
    manifest: &'a Manifest<CargoMetadata>,
}

impl<'a> CodegenCargoManifest<'a> {
    #[inline]
    pub fn new(graph: &'a CodegenGraph<'a>, manifest: &'a Manifest<CargoMetadata>) -> Self {
        Self { graph, manifest }
    }

    pub fn to_manifest(self) -> Manifest<CargoMetadata> {
        let mut manifest = self.manifest.clone();

        // Ploidy generates Rust 2024-compatible code.
        manifest
            .package
            .as_mut()
            .unwrap()
            .edition
            .set(Edition::E2024);

        // `ploidy-util` is our only runtime dependency.
        manifest.dependencies.insert(
            "ploidy-util".to_owned(),
            Dependency::Detailed(
                DependencyDetail {
                    version: Some(PLOIDY_VERSION.to_owned()),
                    ..Default::default()
                }
                .into(),
            ),
        );

        // Translate resource names from operations and schemas into
        // Cargo feature names with dependencies.
        let features = {
            let mut deps_by_feature = BTreeMap::new();

            // For each schema type with an explicitly declared resource name,
            // use the resource name as the feature name, and enable features
            // for all its transitive dependencies.
            for schema in self.graph.schemas() {
                let feature = match schema.resource().map(CargoFeature::from_name) {
                    Some(CargoFeature::Named(name)) => CargoFeature::Named(name),
                    _ => continue,
                };
                let entry: &mut BTreeSet<_> = deps_by_feature.entry(feature).or_default();
                for dep in schema.dependencies().filter_map(|ty| {
                    match CargoFeature::from_name(ty.into_schema().ok()?.resource()?) {
                        CargoFeature::Named(name) => Some(CargoFeature::Named(name)),
                        CargoFeature::Default => None,
                    }
                }) {
                    entry.insert(dep);
                }
            }

            // For each operation with an explicitly declared resource name,
            // use the resource name as the feature name, and enable features for
            // all the types that are reachable from the operation.
            for op in self.graph.operations() {
                let feature = match op.resource().map(CargoFeature::from_name) {
                    Some(CargoFeature::Named(name)) => CargoFeature::Named(name),
                    _ => continue,
                };
                let entry = deps_by_feature.entry(feature).or_default();
                for dep in op.dependencies().filter_map(|ty| {
                    match CargoFeature::from_name(ty.into_schema().ok()?.resource()?) {
                        CargoFeature::Named(name) => Some(CargoFeature::Named(name)),
                        CargoFeature::Default => None,
                    }
                }) {
                    entry.insert(dep);
                }
            }

            // Build the `features` section of the manifest.
            let mut features: BTreeMap<_, _> = deps_by_feature
                .iter()
                .map(|(feature, deps)| {
                    (
                        feature.display().to_string(),
                        deps.iter()
                            .map(|dep| dep.display().to_string())
                            .collect_vec(),
                    )
                })
                .collect();
            if features.is_empty() {
                BTreeMap::new()
            } else {
                // `default` enables all other features.
                features.insert(
                    "default".to_owned(),
                    deps_by_feature
                        .keys()
                        .map(|feature| feature.display().to_string())
                        .collect_vec(),
                );
                features
            }
        };

        Manifest {
            features,
            ..manifest
        }
    }
}

impl IntoCode for CodegenCargoManifest<'_> {
    type Code = (&'static str, Manifest<CargoMetadata>);

    fn into_code(self) -> Self::Code {
        ("Cargo.toml", self.to_manifest())
    }
}

/// Cargo metadata for the generated crate.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CargoMetadata {
    #[serde(default)]
    pub ploidy: Option<CodegenConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use cargo_toml::Package;
    use ploidy_core::{
        ir::{IrGraph, IrSpec},
        parse::Document,
    };

    use crate::tests::assert_matches;

    fn default_manifest() -> Manifest<CargoMetadata> {
        Manifest {
            package: Some(Package::new("test-client", "0.1.0")),
            ..Default::default()
        }
    }

    // MARK: Feature collection

    #[test]
    fn test_schema_with_x_resource_id_creates_feature() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let keys = manifest
            .features
            .keys()
            .map(|feature| feature.as_str())
            .collect_vec();
        assert_matches!(&*keys, ["customer", "default"]);
    }

    #[test]
    fn test_operation_with_x_resource_name_creates_feature() {
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
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let keys = manifest
            .features
            .keys()
            .map(|feature| feature.as_str())
            .collect_vec();
        assert_matches!(&*keys, ["default", "pets"]);
    }

    #[test]
    fn test_unnamed_schema_creates_no_features() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Simple:
                  type: object
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        let keys = manifest
            .features
            .keys()
            .map(|feature| feature.as_str())
            .collect_vec();
        assert_matches!(&*keys, []);
    }

    // MARK: Schema feature dependencies

    #[test]
    fn test_schema_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    billing:
                      $ref: '#/components/schemas/BillingInfo'
                BillingInfo:
                  type: object
                  x-resourceId: billing
                  properties:
                    card:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Customer` depends on `BillingInfo`, so the `customer` feature
        // should depend on `billing`.
        let customer_deps = manifest.features["customer"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*customer_deps, ["billing"]);
    }

    #[test]
    fn test_transitive_schema_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Order:
                  type: object
                  x-resourceId: orders
                  properties:
                    customer:
                      $ref: '#/components/schemas/Customer'
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    billing:
                      $ref: '#/components/schemas/BillingInfo'
                BillingInfo:
                  type: object
                  x-resourceId: billing
                  properties:
                    card:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Order` → `Customer` → `BillingInfo`, so `order` should
        // depend on both `customer` and `billing`.
        let order_deps = manifest.features["orders"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*order_deps, ["billing", "customer"]);
    }

    #[test]
    fn test_unnamed_dependency_does_not_create_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `Customer` depends on `Address`, which doesn't have a resource.
        // The `customer` feature should _not_ depend on `default`;
        // that's handled via `cfg` attributes instead.
        let customer_deps = manifest.features["customer"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*customer_deps, []);
    }

    #[test]
    fn test_feature_does_not_depend_on_itself() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Node:
                  type: object
                  x-resourceId: nodes
                  properties:
                    children:
                      type: array
                      items:
                        $ref: '#/components/schemas/Node'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // Self-referential schemas should not create self-dependencies.
        let node_deps = manifest.features["nodes"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*node_deps, []);
    }

    // MARK: Operation feature dependencies

    #[test]
    fn test_operation_type_dependency_creates_feature_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /orders:
                get:
                  operationId: listOrders
                  x-resource-name: orders
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Order'
            components:
              schemas:
                Order:
                  type: object
                  properties:
                    customer:
                      $ref: '#/components/schemas/Customer'
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `listOrders` returns `Order`, which references `Customer`, so
        // `orders` should depend on `customer`.
        let orders_deps = manifest.features["orders"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*orders_deps, ["customer"]);
    }

    #[test]
    fn test_operation_with_unnamed_type_dependency_does_not_create_full_dependency() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths:
              /customers:
                get:
                  operationId: listCustomers
                  x-resource-name: customer
                  responses:
                    '200':
                      description: OK
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Customer'
            components:
              schemas:
                Customer:
                  type: object
                  properties:
                    address:
                      $ref: '#/components/schemas/Address'
                Address:
                  type: object
                  properties:
                    street:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `listOrders` returns `Customer`, which references `Address`, but
        // `customer` should _not_ depend on `default`.
        let customer_deps = manifest.features["customer"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*customer_deps, []);
    }

    // MARK: Diamond dependencies

    #[test]
    fn test_diamond_dependency_deduplicates_feature() {
        // A -> B, A -> C, B -> D, C -> D. All have resources.
        // A's feature should depend on B, C, and D; D should appear once.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                    c:
                      $ref: '#/components/schemas/C'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    d:
                      $ref: '#/components/schemas/D'
                D:
                  type: object
                  x-resourceId: d
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // `a` depends directly on `b`, `c`; transitively on `d` though `b` and `c`.
        let a_deps = manifest.features["a"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*a_deps, ["b", "c", "d"]);

        // `b` and `c` each depend on `d`.
        let b_deps = manifest.features["b"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*b_deps, ["d"]);

        let c_deps = manifest.features["c"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*c_deps, ["d"]);

        // `d` has no dependencies.
        let d_deps = manifest.features["d"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*d_deps, []);
    }

    // MARK: Cycles with mixed resources

    #[test]
    fn test_cycle_with_mixed_resources_does_not_create_feature_dependency() {
        // Type A (resource `a`) -> Type B (no resource) -> Type C (resource `c`) -> Type A.
        // Since B doesn't have a resource, we don't create a dependency on it;
        // that's handled via `#[cfg(...)]` attributes.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // A depends on B (unnamed) and C. Since B is unnamed, A only depends on C.
        let a_deps = manifest.features["a"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*a_deps, ["c"]);

        // C depends on A (which depends on B, unnamed). C only depends on A.
        let c_deps = manifest.features["c"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*c_deps, ["a"]);

        // `default` should include both named features.
        let default_deps = manifest.features["default"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*default_deps, ["a", "c"]);
    }

    #[test]
    fn test_cycle_with_all_named_resources_creates_mutual_dependencies() {
        // Type A (resource `a`) -> Type B (resource `b`) -> Type C (resource `c`) -> Type A.
        // Each feature should depend on the others in the cycle.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                A:
                  type: object
                  x-resourceId: a
                  properties:
                    b:
                      $ref: '#/components/schemas/B'
                B:
                  type: object
                  x-resourceId: b
                  properties:
                    c:
                      $ref: '#/components/schemas/C'
                C:
                  type: object
                  x-resourceId: c
                  properties:
                    a:
                      $ref: '#/components/schemas/A'
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // A transitively depends on B and C.
        let a_deps = manifest.features["a"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*a_deps, ["b", "c"]);

        // B transitively depends on A and C.
        let b_deps = manifest.features["b"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*b_deps, ["a", "c"]);

        // C transitively depends on A and B.
        let c_deps = manifest.features["c"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*c_deps, ["a", "b"]);

        // `default` should include all three.
        let default_deps = manifest.features["default"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*default_deps, ["a", "b", "c"]);
    }

    // MARK: Default feature

    #[test]
    fn test_default_feature_includes_all_other_features() {
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
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
                Order:
                  type: object
                  x-resourceId: orders
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // The `default` feature should include all other features, but not itself.
        let default_deps = manifest.features["default"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*default_deps, ["customer", "orders", "pets"]);
    }

    #[test]
    fn test_default_feature_includes_all_named_features() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Customer:
                  type: object
                  x-resourceId: customer
                  properties:
                    id:
                      type: string
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &default_manifest()).to_manifest();

        // The `default` feature should include all named features.
        let default_deps = manifest.features["default"]
            .iter()
            .map(|dep| dep.as_str())
            .collect_vec();
        assert_matches!(&*default_deps, ["customer"]);
    }

    // MARK: Dependencies

    #[test]
    fn test_preserves_existing_dependencies() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            paths: {}
        "})
        .unwrap();

        let mut manifest = default_manifest();
        manifest
            .dependencies
            .insert("serde".to_owned(), Dependency::Simple("1.0".to_owned()));

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir_graph = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir_graph);
        let manifest = CodegenCargoManifest::new(&graph, &manifest).to_manifest();

        let dep_names = manifest
            .dependencies
            .keys()
            .map(|k| k.as_str())
            .collect_vec();
        assert_matches!(&*dep_names, ["ploidy-util", "serde"]);
    }
}
