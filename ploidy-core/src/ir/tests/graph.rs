//! Tests for IR graph construction and cycle detection.

use itertools::Itertools;

use crate::{
    arena::Arena,
    ir::{InlineTypeView, RawGraph, SchemaTypeView, Spec, StructFieldName, TypeView, View},
    parse::Document,
    tests::assert_matches,
};

// MARK: Graph construction

#[test]
fn test_graph_basic_construction() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Person:
              type: object
              properties:
                name:
                  type: string
            Company:
              type: object
              properties:
                title:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should be able to iterate over schemas.
    // The order of iteration isn't guaranteed.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Company", "Person"]);
}

#[test]
fn test_graph_deduplication() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Shared:
              type: object
              properties:
                id:
                  type: string
            Container1:
              type: object
              properties:
                value:
                  $ref: '#/components/schemas/Shared'
            Container2:
              type: object
              properties:
                value:
                  $ref: '#/components/schemas/Shared'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have all 3 schemas, with `Shared` appearing only once.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Container1", "Container2", "Shared"]);
}

#[test]
fn test_graph_struct_field_edges() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            FieldType:
              type: object
              properties:
                value:
                  type: string
            Container:
              type: object
              properties:
                field:
                  $ref: '#/components/schemas/FieldType'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Both schemas should be present.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Container", "FieldType"]);
}

#[test]
fn test_graph_tagged_variant_edges() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                bark:
                  type: string
            Cat:
              type: object
              properties:
                meow:
                  type: string
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Dog'
                - $ref: '#/components/schemas/Cat'
              discriminator:
                propertyName: type
                mapping:
                  dog: '#/components/schemas/Dog'
                  cat: '#/components/schemas/Cat'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have all schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Animal", "Cat", "Dog"]);
}

#[test]
fn test_graph_untagged_variant_edges() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            TypeA:
              type: object
              properties:
                a:
                  type: string
            TypeB:
              type: object
              properties:
                b:
                  type: integer
            AOrB:
              oneOf:
                - $ref: '#/components/schemas/TypeA'
                - $ref: '#/components/schemas/TypeB'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have all schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["AOrB", "TypeA", "TypeB"]);
}

#[test]
fn test_graph_array_edge() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Item:
              type: object
              properties:
                value:
                  type: string
            Items:
              type: object
              properties:
                list:
                  type: array
                  items:
                    $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have both schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Item", "Items"]);
}

#[test]
fn test_graph_map_edge() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Value:
              type: object
              properties:
                data:
                  type: string
            Dictionary:
              type: object
              properties:
                map:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/Value'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have both schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Dictionary", "Value"]);
}

#[test]
fn test_graph_nullable_edge() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            NullableType:
              type: object
              nullable: true
              properties:
                value:
                  type: string
            Container:
              type: object
              properties:
                field:
                  $ref: '#/components/schemas/NullableType'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have both schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Container", "NullableType"]);
}

#[test]
fn test_graph_ref_resolution() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Child:
              type: object
              properties:
                name:
                  type: string
            Parent:
              type: object
              properties:
                child1:
                  $ref: '#/components/schemas/Child'
                child2:
                  $ref: '#/components/schemas/Child'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should have both `Parent` and `Child` schemas.
    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Child", "Parent"]);
}

// MARK: Circular reference detection

#[test]
fn test_circular_refs_simple_cycle() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                b:
                  $ref: '#/components/schemas/B'
            B:
              type: object
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // The field pointing to B should need indirection due to the cycle.
    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_box());
}

#[test]
fn test_circular_refs_self_reference() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Node:
              type: object
              properties:
                next:
                  $ref: '#/components/schemas/Node'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Self-references should be detected as cycles.
    let node_schema = graph.schema("Node").unwrap();
    let node_struct = match node_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let next_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("next")))
        .unwrap();
    assert!(next_field.needs_box());
}

#[test]
fn test_circular_refs_complex_cycle() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
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
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // At least one edge in the cycle should need indirection.
    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_box());
}

#[test]
fn test_circular_refs_no_cycles() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: string
            Branch:
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
            Root:
              type: object
              properties:
                branch:
                  $ref: '#/components/schemas/Branch'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Tree structure without direct or indirect self-references
    // shouldn't have any circular dependencies.
    let root_schema = graph.schema("Root").unwrap();
    let root_struct = match root_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Root`; got {other:?}"),
    };
    let branch_field = root_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("branch")))
        .unwrap();
    assert!(!branch_field.needs_box());
}

#[test]
fn test_circular_refs_multiple_sccs() {
    // Two cycles: A <-> B and C <-> D.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                b:
                  $ref: '#/components/schemas/B'
            B:
              type: object
              properties:
                a:
                  $ref: '#/components/schemas/A'
            C:
              type: object
              properties:
                d:
                  $ref: '#/components/schemas/D'
            D:
              type: object
              properties:
                c:
                  $ref: '#/components/schemas/C'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Both cycles should be detected.
    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let a_b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b")))
        .unwrap();
    assert!(a_b_field.needs_box());

    let c_schema = graph.schema("C").unwrap();
    let c_struct = match c_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `C`; got {other:?}"),
    };
    let c_d_field = c_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("d")))
        .unwrap();
    assert!(c_d_field.needs_box());
}

#[test]
fn test_circular_refs_through_containers() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                b_array:
                  type: array
                  items:
                    $ref: '#/components/schemas/B'
            B:
              type: object
              properties:
                a_map:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Heap-allocating containers (arrays and maps) break boxing
    // cycles, so fields behind them don't need `Box`.
    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_array_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b_array")))
        .unwrap();
    assert!(!b_array_field.needs_box());
}

#[test]
fn test_circular_refs_diamond_no_false_positive() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Base:
              type: string
            Left:
              type: object
              properties:
                base:
                  $ref: '#/components/schemas/Base'
            Right:
              type: object
              properties:
                base:
                  $ref: '#/components/schemas/Base'
            Top:
              type: object
              properties:
                left:
                  $ref: '#/components/schemas/Left'
                right:
                  $ref: '#/components/schemas/Right'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Diamond inheritance shouldn't be marked as circular.
    let top_schema = graph.schema("Top").unwrap();
    let top_struct = match top_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Top`; got {other:?}"),
    };
    let left_field = top_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("left")))
        .unwrap();

    assert!(!left_field.needs_box());
}

#[test]
fn test_circular_refs_tarjan_correctness() {
    // A more complex graph with nested cycles.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                b:
                  $ref: '#/components/schemas/B'
            B:
              type: object
              properties:
                c:
                  $ref: '#/components/schemas/C'
                a:
                  $ref: '#/components/schemas/A'
            C:
              type: object
              properties:
                d:
                  $ref: '#/components/schemas/D'
            D:
              type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // A and B should be in a cycle.
    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let a_b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b")))
        .unwrap();
    assert!(a_b_field.needs_box());

    // C and D shouldn't be in a cycle.
    let c_schema = graph.schema("C").unwrap();
    let c_struct = match c_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `C`; got {other:?}"),
    };
    let c_d_field = c_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("d")))
        .unwrap();
    assert!(!c_d_field.needs_box());
}

#[test]
fn test_needs_box_through_nullable() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              nullable: true
              properties:
                b:
                  $ref: '#/components/schemas/B'
            B:
              type: object
              nullable: true
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a_schema = graph.schema("A").unwrap();
    let a_struct = match a_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_box());
}

#[test]
fn test_needs_box_through_array() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Node:
              type: object
              properties:
                children:
                  type: array
                  items:
                    $ref: '#/components/schemas/Node'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let node_schema = graph.schema("Node").unwrap();
    let node_struct = match node_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let children_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("children")))
        .unwrap();
    assert!(!children_field.needs_box());
}

#[test]
fn test_needs_box_through_map() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Node:
              type: object
              properties:
                children_map:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/Node'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let node_schema = graph.schema("Node").unwrap();
    let node_struct = match node_schema {
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let children_map_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("children_map")))
        .unwrap();
    assert!(!children_map_field.needs_box());
}

#[test]
fn test_indirect_and_direct_siblings() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Direct:
              type: object
              properties:
                value:
                  type: string
            Container:
              type: object
              properties:
                direct_field:
                  $ref: '#/components/schemas/Direct'
                indirect_field:
                  $ref: '#/components/schemas/Container'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schema("Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let direct_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("direct_field")))
        .unwrap();

    let indirect_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("indirect_field")))
        .unwrap();

    // Only the cyclic field needs indirection.
    assert!(!direct_field.needs_box());
    assert!(indirect_field.needs_box());
}

// MARK: Operation metadata

#[test]
fn test_operations_transitive() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths:
          /users:
            get:
              operationId: getUsers
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/UserList'
        components:
          schemas:
            UserList:
              type: object
              properties:
                users:
                  type: array
                  items:
                    $ref: '#/components/schemas/User'
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Both `UserList` and `User` have no `x-resourceId`, and
    // the operation has no `x-resource-name`. The operation should
    // contribute `None` to `used_by`.
    let user_list = graph.schema("UserList").unwrap();
    let user = graph.schema("User").unwrap();

    assert_eq!(user_list.resource(), None);
    let user_list_used_by = user_list.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*user_list_used_by, [None]);
    assert_eq!(user.resource(), None);
    let user_used_by = user.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*user_used_by, [None]);
}

#[test]
fn test_operations_multiple() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths:
          /users:
            get:
              operationId: getUsers
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      $ref: '#/components/schemas/User'
              responses:
                '201':
                  description: Created
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `User` has no `x-resourceId`, and neither operation has `x-resource-name`.
    // Each operation without a resource contributes `None` to `used_by`.
    let user = graph.schema("User").unwrap();
    assert_eq!(user.resource(), None);
    // Two operations, so two `None` entries.
    let user_used_by = user.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*user_used_by, [None, None]);
}

// MARK: Forward propagation

#[test]
fn test_dependencies_propagation() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /data:
            get:
              operationId: getData
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/Response'
          /users/{id}:
            get:
              operationId: getUser
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
        components:
          schemas:
            Response:
              type: object
              properties:
                user:
                  $ref: '#/components/schemas/User'
                items:
                  type: array
                  items:
                    $ref: '#/components/schemas/Item'
                metadata:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/Meta'
            User:
              type: object
              x-resourceId: users
              properties:
                name:
                  type: string
            Item:
              type: object
              properties:
                id:
                  type: string
            Meta:
              type: object
              properties:
                key:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Dependencies are computed transitively through different edge types:
    // direct references, array items, and map values.
    let get_data = graph.operations().find(|o| o.id() == "getData").unwrap();
    let mut get_data_deps = get_data
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    get_data_deps.sort();
    assert_matches!(&*get_data_deps, ["Item", "Meta", "Response", "User"]);

    let get_user = graph.operations().find(|o| o.id() == "getUser").unwrap();
    let get_user_deps = get_user
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert_matches!(&*get_user_deps, ["User"]);

    // `x-resourceId` is a per-schema property; it doesn't propagate to
    // containing schemas.
    let user = graph.schema("User").unwrap();
    assert_eq!(user.resource(), Some("users"));

    let response = graph.schema("Response").unwrap();
    assert_eq!(response.resource(), None);

    // Each schema knows which operations use it. `User` is used by both
    // operations; the others are only used by `getData`.
    let mut user_used_by = user.used_by().map(|op| op.id()).collect_vec();
    user_used_by.sort();
    assert_eq!(&*user_used_by, ["getData", "getUser"]);

    let mut other_used_by = graph
        .schemas()
        .filter(|schema| ["Response", "Item", "Meta"].contains(&schema.name()))
        .flat_map(|schema| schema.used_by())
        .map(|op| op.id())
        .collect_vec();
    other_used_by.dedup();
    assert_eq!(&*other_used_by, ["getData"]);
}

// MARK: Backward propagation

#[test]
fn test_used_by_propagation() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths:
          /cats:
            get:
              operationId: getCat
              x-resource-name: cats
              parameters:
                - name: options
                  in: query
                  schema:
                    $ref: '#/components/schemas/CreateOptions'
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/Cat'
          /items:
            post:
              operationId: createItem
              x-resource-name: items
              parameters:
                - name: options
                  in: query
                  schema:
                    $ref: '#/components/schemas/CreateOptions'
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/Parent'
        components:
          schemas:
            Cat:
              type: object
              properties:
                pet:
                  $ref: '#/components/schemas/Pet'
            Pet:
              type: object
              properties:
                cat:
                  $ref: '#/components/schemas/Cat'
            Parent:
              type: object
              properties:
                child:
                  $ref: '#/components/schemas/Child'
            Child:
              type: object
              x-resourceId: children
              properties:
                name:
                  type: string
            CreateOptions:
              type: object
              x-resourceId: options
              properties:
                verbose:
                  type: boolean
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Propagation through cycles: `Cat` and `Pet` are in a cycle.
    // The operation directly uses `Cat`, but `Pet` should also be
    // marked as used by the operation.
    let cat = graph.schema("Cat").unwrap();
    let pet = graph.schema("Pet").unwrap();

    let cat_used_by = cat.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*cat_used_by, [Some("cats")]);

    let pet_used_by = pet.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*pet_used_by, [Some("cats")]);

    // Propagation through nested references: `Child` is nested under `Parent`.
    // The operation uses `Parent`, so `Child` should also be used.
    let child = graph.schema("Child").unwrap();
    assert_eq!(child.resource(), Some("children"));

    let child_used_by = child.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*child_used_by, [Some("items")]);

    // A schema can be used by multiple operations with different resources:
    // `CreateOptions` is a parameter for both `getCat` and `createItem`.
    let options = graph.schema("CreateOptions").unwrap();
    assert_eq!(options.resource(), Some("options"));

    let mut options_used_by = options.used_by().map(|op| op.resource()).collect_vec();
    options_used_by.sort();
    assert_matches!(&*options_used_by, [Some("cats"), Some("items")]);

    // The operation's dependencies include both parameter and response schemas.
    let op = graph.operations().find(|o| o.id() == "createItem").unwrap();
    let mut op_resources = op
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.resource())
        .collect_vec();
    op_resources.sort();
    assert_matches!(&*op_resources, [None, Some("children"), Some("options")]);
}

// MARK: Dependencies

#[test]
fn test_depends_on_simple_chain() {
    // A -> B -> C. A depends on B and C; B depends on C; C depends on neither.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
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
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schema("A").unwrap();
    let b = graph.schema("B").unwrap();
    let c = graph.schema("C").unwrap();

    assert!(a.depends_on(&b));
    assert!(a.depends_on(&c));
    assert!(b.depends_on(&c));
    assert!(!b.depends_on(&a));
    assert!(!c.depends_on(&a));
    assert!(!c.depends_on(&b));
}

#[test]
fn test_depends_on_cycle() {
    // A -> B -> C -> A. All depend on each other.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
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
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schema("A").unwrap();
    let b = graph.schema("B").unwrap();
    let c = graph.schema("C").unwrap();

    // All nodes in a cycle depend on each other.
    assert!(a.depends_on(&b));
    assert!(a.depends_on(&c));
    assert!(b.depends_on(&a));
    assert!(b.depends_on(&c));
    assert!(c.depends_on(&a));
    assert!(c.depends_on(&b));
}

#[test]
fn test_depends_on_independent() {
    // A and B are unrelated.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                value:
                  type: string
            B:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schema("A").unwrap();
    let b = graph.schema("B").unwrap();

    assert!(!a.depends_on(&b));
    assert!(!b.depends_on(&a));
}

// MARK: Dependents

#[test]
fn test_dependents_simple_chain() {
    // A depends on B, B depends on C.
    // C's dependents should include B and A.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
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
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let c = graph.schema("C").unwrap();
    let mut c_dependents = c
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    c_dependents.sort();
    // C's dependents are A and B (and shouldn't include C itself).
    assert_matches!(&*c_dependents, ["A", "B"]);

    // B's dependents should include A only.
    let b = graph.schema("B").unwrap();
    let mut b_dependents = b
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    b_dependents.sort();
    assert_matches!(&*b_dependents, ["A"]);

    // A has no dependents.
    let a = graph.schema("A").unwrap();
    let a_dependents = a
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert!(a_dependents.is_empty());
}

#[test]
fn test_dependents_multiple_dependents() {
    // Both A and B depend on C.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                c:
                  $ref: '#/components/schemas/C'
            B:
              type: object
              properties:
                c:
                  $ref: '#/components/schemas/C'
            C:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let c = graph.schema("C").unwrap();
    let mut c_dependents = c
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    c_dependents.sort();
    // C's dependents are A and B (and shouldn't include C itself).
    assert_matches!(&*c_dependents, ["A", "B"]);
}

#[test]
fn test_dependents_cycle() {
    // A -> B -> C -> A. All nodes are dependents of each other.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
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
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // In a cycle, all nodes transitively depend on each other,
    // but a type's dependents shouldn't include itself.
    let a = graph.schema("A").unwrap();
    let mut a_dependents = a
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    a_dependents.sort();
    assert_matches!(&*a_dependents, ["B", "C"]);

    let b = graph.schema("B").unwrap();
    let mut b_dependents = b
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    b_dependents.sort();
    assert_matches!(&*b_dependents, ["A", "C"]);

    let c = graph.schema("C").unwrap();
    let mut c_dependents = c
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    c_dependents.sort();
    assert_matches!(&*c_dependents, ["A", "B"]);
}

#[test]
fn test_dependents_is_inverse_of_dependencies() {
    // If A depends on B, then B's dependents should include A.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Container:
              type: object
              properties:
                item:
                  $ref: '#/components/schemas/Item'
            Item:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container = graph.schema("Container").unwrap();
    let item = graph.schema("Item").unwrap();

    // `Container` depends on `Item`.
    let container_deps = container
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert_matches!(&*container_deps, ["Item"]);

    // `Item`'s dependents include `Container`.
    let mut item_dependents = item
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    item_dependents.sort();
    assert_matches!(&*item_dependents, ["Container"]);
}

#[test]
fn test_dependencies_diamond() {
    // A -> B, A -> C, B -> D, C -> D. D should appear only once in A's dependencies.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              properties:
                b:
                  $ref: '#/components/schemas/B'
                c:
                  $ref: '#/components/schemas/C'
            B:
              type: object
              properties:
                d:
                  $ref: '#/components/schemas/D'
            C:
              type: object
              properties:
                d:
                  $ref: '#/components/schemas/D'
            D:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schema("A").unwrap();
    let b = graph.schema("B").unwrap();
    let c = graph.schema("C").unwrap();
    let d = graph.schema("D").unwrap();

    // A depends directly on B, C; transitively on D through B and C.
    let mut a_deps = a
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    a_deps.sort();
    assert_matches!(&*a_deps, ["B", "C", "D"]);

    // D's dependents should include A, B, and C.
    let mut d_dependents = d
        .dependents()
        .filter_map(|v| v.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    d_dependents.sort();
    assert_matches!(&*d_dependents, ["A", "B", "C"]);

    // B and C each depend on D only.
    assert!(b.depends_on(&d));
    assert!(c.depends_on(&d));
    assert!(!b.depends_on(&c));
    assert!(!c.depends_on(&b));
}

// MARK: Operations with no types

#[test]
fn test_operation_with_no_types() {
    // An operation with no parameters, request body, or response body.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths:
          /health:
            get:
              operationId: healthCheck
              x-resource-name: health
              responses:
                '200':
                  description: OK
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let op = graph
        .operations()
        .find(|o| o.id() == "healthCheck")
        .unwrap();

    // The operation has no type dependencies.
    let deps = op
        .dependencies()
        .filter_map(|v| v.into_schema().right())
        .collect_vec();
    assert_matches!(&*deps, []);

    // No types should be marked as used by this operation.
    assert!(graph.schemas().all(|schema| {
        schema
            .used_by()
            .map(|op| op.id())
            .all(|id| id != "healthCheck")
    }));
}

// MARK: Inheritance

#[test]
fn test_parents_returns_immediate_parents() {
    // `Entity` -> `NamedEntity` -> `User`; `User` should only have
    // `NamedEntity` as its parent.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Entity:
              type: object
              properties:
                id:
                  type: string
            NamedEntity:
              allOf:
                - $ref: '#/components/schemas/Entity'
              properties:
                name:
                  type: string
            User:
              allOf:
                - $ref: '#/components/schemas/NamedEntity'
              properties:
                email:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let user = graph.schema("User").unwrap();
    let user_struct = match user {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `User`; got {other:?}"),
    };

    // `User` should only have `NamedEntity` as a parent, not `Entity`.
    let parent_names = user_struct
        .parents()
        .filter_map(|p| p.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert_matches!(&*parent_names, ["NamedEntity"]);

    // ...But `User` should inherit fields from both ancestors.
    let field_names = user_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();
    assert_matches!(&*field_names, ["id", "name", "email"]);
}

#[test]
fn test_all_of_inheritance_with_fields() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Parent:
              type: object
              properties:
                parent_field:
                  type: string
            Child:
              allOf:
                - $ref: '#/components/schemas/Parent'
              properties:
                child_field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let child = graph.schema("Child").unwrap();
    let child_struct = match child {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Child`; got {other:?}"),
    };

    // `Child` should have `Parent` as its parent.
    let parent_names = child_struct
        .parents()
        .filter_map(|p| p.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert_matches!(&*parent_names, ["Parent"]);

    // `own_fields()` should only return the child's own fields.
    let own_field_names = child_struct
        .own_fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();
    assert_matches!(&*own_field_names, ["child_field"]);

    // `fields()` should return both the inherited and own fields.
    let all_field_names = child_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            _ => panic!("expected named field"),
        })
        .collect_vec();
    assert_matches!(&*all_field_names, ["parent_field", "child_field"]);

    // The `inherited()` flag should be correct for each field.
    let parent_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("parent_field")))
        .unwrap();
    assert!(parent_field.inherited());

    let child_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("child_field")))
        .unwrap();
    assert!(!child_field.inherited());
}

#[test]
fn test_circular_refs_excludes_inherits_edges() {
    // This test constructs a graph like:
    //   Parent --[Reference]--> Child --[Inherits]--> Parent
    //
    // This is a cycle, but since the back edge is an inheritance edge,
    // not a reference edge, `Parent.child` doesn't need `Box`.
    // Inheritance edges are excluded from the boxing SCC.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Parent:
              type: object
              properties:
                child:
                  $ref: '#/components/schemas/Child'
            Child:
              allOf:
                - $ref: '#/components/schemas/Parent'
              properties:
                own_field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let parent = graph.schema("Parent").unwrap();
    let parent_struct = match parent {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Parent`; got {other:?}"),
    };

    // `Parent.child` references `Child`, but the only path back to `Parent`
    // is through inheritance, so no indirection is needed.
    let child_field = parent_struct
        .own_fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("child")))
        .unwrap();
    assert!(!child_field.needs_box());
}

#[test]
fn test_multiple_parents() {
    // A schema with multiple `allOf` keywords should have multiple parents.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Mixin1:
              type: object
              properties:
                alpha:
                  type: string
                beta:
                  type: string
            Mixin2:
              type: object
              properties:
                gamma:
                  type: string
                delta:
                  type: string
            Combined:
              allOf:
                - $ref: '#/components/schemas/Mixin1'
                - $ref: '#/components/schemas/Mixin2'
              properties:
                own_field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let combined = graph.schema("Combined").unwrap();
    let combined_struct = match combined {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Combined`; got {other:?}"),
    };

    // Should have both mixins as parents.
    let parent_names = combined_struct
        .parents()
        .filter_map(|p| p.into_schema().right())
        .map(|s| s.name())
        .collect_vec();
    assert_matches!(&*parent_names, ["Mixin1", "Mixin2"]);

    // `own_fields()` should only return `own_field`.
    let own_field_names = combined_struct
        .own_fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();
    assert_matches!(&*own_field_names, ["own_field"]);

    // `fields()` should return ancestor fields first, in the order of
    // their parents in `allOf`, then own fields.
    let all_field_names = combined_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();
    assert_matches!(
        &*all_field_names,
        ["alpha", "beta", "gamma", "delta", "own_field"]
    );
}

#[test]
fn test_circular_all_of_terminates() {
    // A circular `allOf` (A -> B -> A) is invalid, but can appear in the wild.
    // `fields()` and `tag()` should still terminate and yield all
    // fields.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              allOf:
                - $ref: '#/components/schemas/B'
              properties:
                a_field:
                  type: string
            B:
              allOf:
                - $ref: '#/components/schemas/A'
              properties:
                b_field:
                  type: string
                kind:
                  type: string
              discriminator:
                propertyName: kind
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schema("A").unwrap();
    let a_struct = match a {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `A`; got {other:?}"),
    };

    // `fields()` must terminate and yield both A's own field
    // and B's inherited fields.
    let field_names = a_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();
    assert_matches!(&*field_names, ["b_field", "kind", "a_field"]);

    // `tag()` must terminate. Both `A` and `B` have no incoming edges
    // from tagged unions, so no field is a tag.
    let kind_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.tag());
    assert!(kind_field.inherited());

    // A's own field should not be a tag.
    let a_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("a_field")))
        .unwrap();
    assert!(!a_field.tag());
}

#[test]
fn test_all_of_parent_with_one_of_and_properties() {
    // When a parent has `oneOf` (with a `discriminator`) and `properties`,
    // children inheriting from that parent via `allOf` should still receive
    // the `oneOf` parent's `properties` as inherited fields.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Column:
              type: object
              properties:
                name:
                  type: string
            Base:
              oneOf:
                - $ref: '#/components/schemas/VariantA'
                - $ref: '#/components/schemas/VariantB'
              discriminator:
                propertyName: kind
                mapping:
                  a: '#/components/schemas/VariantA'
                  b: '#/components/schemas/VariantB'
              properties:
                schema:
                  type: array
                  items:
                    $ref: '#/components/schemas/Column'
                kind:
                  type: string
            VariantA:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                a_field:
                  type: string
            VariantB:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                b_field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `VariantA` inherits from `Base`, which has properties
    // `schema` and `kind` alongside its discriminator field.
    let variant_a = graph.schema("VariantA").unwrap();
    let variant_a_struct = match variant_a {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `VariantA`; got {other:?}"),
    };

    let all_field_names = variant_a_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got {other:?}"),
        })
        .collect_vec();

    // Should include inherited `schema` and `kind` from `Base`,
    // plus `VariantA`'s own `a_field`.
    assert_matches!(&*all_field_names, ["schema", "kind", "a_field"]);
}

#[test]
fn test_untagged_union_with_properties() {
    // When a parent has `oneOf` (without a `discriminator`) and `properties`,
    // the `Untagged` union should retain those properties as `fields`, and
    // children inheriting from the union via `allOf` should receive its
    // properties as inherited fields.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Base:
              oneOf:
                - $ref: '#/components/schemas/VariantA'
                - $ref: '#/components/schemas/VariantB'
              properties:
                shared_field:
                  type: string
                count:
                  type: integer
            VariantA:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                a_field:
                  type: string
            VariantB:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                b_field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `Base` has no `discriminator`, so it must be `Untagged`.
    let base = graph.schema("Base").unwrap();
    let base_untagged = match base {
        SchemaTypeView::Untagged(_, view) => view,
        other => panic!("expected untagged `Base`; got `{other:?}`"),
    };

    // The untagged union should store its own `properties` directly.
    let base_field_names = base_untagged
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got `{other:?}`"),
        })
        .collect_vec();
    assert_matches!(&*base_field_names, ["shared_field", "count"]);

    // `VariantA` inherits from `Base` via `allOf`, so should receive
    // `Base`'s fields alongside its own `a_field`.
    let variant_a = graph.schema("VariantA").unwrap();
    let variant_a_struct = match variant_a {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `VariantA`; got `{other:?}`"),
    };

    let variant_a_field_names = variant_a_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
            other => panic!("expected named field; got `{other:?}`"),
        })
        .collect_vec();

    // Inherited fields come first, then the own field.
    assert_matches!(
        &*variant_a_field_names,
        ["shared_field", "count", "a_field"]
    );
}

#[test]
fn test_tagged_union_inlines_include_field_types() {
    // A tagged union with an own inline enum property should
    // include that enum in its `inlines()`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                kind:
                  type: string
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
              properties:
                kind:
                  type: string
                severity:
                  type: string
                  enum: [low, high]
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let pet = graph.schema("Pet").unwrap();
    let has_inline_enum = pet.inlines().any(|i| matches!(i, InlineTypeView::Enum(..)));
    assert!(has_inline_enum);
}

#[test]
fn test_needs_box_through_inlined_tagged_variant() {
    // When a tagged union has own fields, its variants are inlined.
    // The inlined variant must retain a reference edge to the original,
    // so that SCC membership (and thus `needs_box()`) is preserved.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
              properties:
                kind:
                  type: string
                name:
                  type: string
            Dog:
              type: object
              properties:
                parent:
                  $ref: '#/components/schemas/Pet'
            Kennel:
              type: object
              properties:
                resident:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // The inlined variant `Pet/Dog` should have `parent` marked as
    // needing indirection, since it points back to `Pet`.
    let pet = graph.schema("Pet").unwrap();
    let tagged = match pet {
        SchemaTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged `Pet`; got {other:?}"),
    };
    let variant = tagged.variants().next().unwrap();
    let variant_struct = match variant.ty() {
        TypeView::Inline(InlineTypeView::Struct(_, _, view)) => view,
        other => panic!("expected inline struct variant; got {other:?}"),
    };
    let parent_field = variant_struct
        .own_fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("parent")))
        .unwrap();
    assert!(parent_field.needs_box());
}

// MARK: Simplification

#[test]
fn test_simplify_inline_ref_with_description_collapses_to_ref() {
    // OpenAPI 3.1 allows `$ref` siblings; the parser desugars
    // `{ "$ref": "...", "description": "..." }` into
    // `{ "allOf": [{ "$ref": "..." }], "description": "..." }`.
    // For a metadata-only sibling, the inline wrapper carries no
    // structural information and should collapse to a plain reference.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Pet:
              type: object
              properties:
                name:
                  type: string
            Owner:
              type: object
              properties:
                pet:
                  $ref: '#/components/schemas/Pet'
                  description: The owner's pet.
              required:
                - pet
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.simplify();
    let graph = raw.cook();

    let owner = graph.schema("Owner").unwrap();
    let owner_struct = match owner {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Owner`; got `{other:?}`"),
    };
    let pet_field = owner_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("pet")))
        .unwrap();

    // The field should resolve directly to the `Pet` schema, not
    // an inline struct that inherits from it.
    let target = match pet_field.ty() {
        TypeView::Schema(SchemaTypeView::Struct(info, _)) => info.name,
        other => panic!("expected struct view of `Pet`; got `{other:?}`"),
    };
    assert_eq!(target, "Pet");
}

#[test]
fn test_simplify_inherits_from_any_drops_edge() {
    // A schema that inherits from another that's effectively `Any`
    // (e.g. an empty schema) shouldn't carry the trivial parent
    // through to the cooked graph.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Anything: {}
            Thing:
              allOf:
                - $ref: '#/components/schemas/Anything'
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.simplify();
    let graph = raw.cook();

    let thing = graph.schema("Thing").unwrap();
    let thing_struct = match thing {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Thing`; got `{other:?}`"),
    };

    // After simplification, `Thing` has no parents.
    let parents = thing_struct.parents().collect_vec();
    assert!(parents.is_empty(), "expected no parents; got `{parents:?}`");
}

#[test]
fn test_simplify_named_schema_with_ref_sibling_preserves_name() {
    // A named schema whose entire body is `$ref + description` becomes
    // a single-parent struct with no own fields. The inline-collapse rule
    // must not remove the named node, since user code refers to it by name.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Pet:
              type: object
              properties:
                name:
                  type: string
            Owner:
              $ref: '#/components/schemas/Pet'
              description: A pet's owner.
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.simplify();
    let graph = raw.cook();

    let mut schema_names = graph.schemas().map(|s| s.name()).collect_vec();
    schema_names.sort();
    assert_matches!(&*schema_names, ["Owner", "Pet"]);
}

#[test]
fn test_simplify_preserves_ref_with_one_of_sibling() {
    // A `$ref` with a structural sibling like `oneOf` carries real
    // information; simplification mustn't collapse it.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Cat:
              type: object
              properties:
                meow:
                  type: string
            Dog:
              type: object
              properties:
                bark:
                  type: string
            Pet:
              type: object
              properties:
                name:
                  type: string
            Owner:
              type: object
              properties:
                pet:
                  $ref: '#/components/schemas/Pet'
                  oneOf:
                    - $ref: '#/components/schemas/Cat'
                    - $ref: '#/components/schemas/Dog'
              required:
                - pet
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.simplify();
    let graph = raw.cook();

    let owner = graph.schema("Owner").unwrap();
    let owner_struct = match owner {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Owner`; got `{other:?}`"),
    };
    let pet_field = owner_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("pet")))
        .unwrap();

    // The field should resolve to an inline untagged union, not
    // collapse to the `Pet` ref. Simplification mustn't fire because
    // the inline node carries structural variants.
    assert_matches!(
        pet_field.ty(),
        TypeView::Inline(InlineTypeView::Untagged(..))
    );
}

#[test]
fn test_simplify_is_idempotent() {
    // Running simplify twice should not change the graph.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Pet:
              type: object
              properties:
                name:
                  type: string
            Owner:
              type: object
              properties:
                pet:
                  $ref: '#/components/schemas/Pet'
                  description: The pet.
              required:
                - pet
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();

    let cooked_once = {
        let mut raw = RawGraph::new(&arena, &spec);
        raw.simplify();
        raw.cook()
    };
    let cooked_twice = {
        let mut raw = RawGraph::new(&arena, &spec);
        raw.simplify();
        raw.simplify();
        raw.cook()
    };

    let names_once = cooked_once.schemas().map(|s| s.name()).collect_vec();
    let names_twice = cooked_twice.schemas().map(|s| s.name()).collect_vec();
    assert_eq!(names_once, names_twice);
}
