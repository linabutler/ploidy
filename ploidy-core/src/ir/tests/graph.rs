//! Tests for IR graph construction and cycle detection.

use itertools::Itertools;

use crate::{
    ir::{IrGraph, IrSpec, IrStructFieldName, SchemaIrTypeView, View},
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // The field pointing to B should need indirection due to the cycle.
    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Self-references should be detected as cycles.
    let node_schema = graph.schemas().find(|s| s.name() == "Node").unwrap();
    let node_struct = match node_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let next_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("next")))
        .unwrap();
    assert!(next_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // At least one edge in the cycle should need indirection.
    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Tree structure without direct or indirect self-references
    // shouldn't have any circular dependencies.
    let root_schema = graph.schemas().find(|s| s.name() == "Root").unwrap();
    let root_struct = match root_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Root`; got {other:?}"),
    };
    let branch_field = root_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("branch")))
        .unwrap();
    assert!(!branch_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Both cycles should be detected.
    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let a_b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b")))
        .unwrap();
    assert!(a_b_field.needs_indirection());

    let c_schema = graph.schemas().find(|s| s.name() == "C").unwrap();
    let c_struct = match c_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `C`; got {other:?}"),
    };
    let c_d_field = c_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("d")))
        .unwrap();
    assert!(c_d_field.needs_indirection());
}

#[test]
fn test_circular_refs_through_wrappers() {
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Cycles through wrappers should be detected.
    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_array_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b_array")))
        .unwrap();
    assert!(b_array_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Diamond inheritance shouldn't be marked as circular.
    let top_schema = graph.schemas().find(|s| s.name() == "Top").unwrap();
    let top_struct = match top_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Top`; got {other:?}"),
    };
    let left_field = top_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("left")))
        .unwrap();

    assert!(!left_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // A and B should be in a cycle.
    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let a_b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b")))
        .unwrap();
    assert!(a_b_field.needs_indirection());

    // C and D shouldn't be in a cycle.
    let c_schema = graph.schemas().find(|s| s.name() == "C").unwrap();
    let c_struct = match c_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `C`; got {other:?}"),
    };
    let c_d_field = c_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("d")))
        .unwrap();
    assert!(!c_d_field.needs_indirection());
}

#[test]
fn test_needs_indirection_through_nullable() {
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();
    let a_struct = match a_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `A`; got {other:?}"),
    };
    let b_field = a_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("b")))
        .unwrap();
    assert!(b_field.needs_indirection());
}

#[test]
fn test_needs_indirection_through_array() {
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let node_schema = graph.schemas().find(|s| s.name() == "Node").unwrap();
    let node_struct = match node_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let children_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("children")))
        .unwrap();
    assert!(children_field.needs_indirection());
}

#[test]
fn test_needs_indirection_through_map() {
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let node_schema = graph.schemas().find(|s| s.name() == "Node").unwrap();
    let node_struct = match node_schema {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Node`; got {other:?}"),
    };
    let children_map_field = node_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("children_map")))
        .unwrap();
    assert!(children_map_field.needs_indirection());
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let direct_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("direct_field")))
        .unwrap();

    let indirect_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("indirect_field")))
        .unwrap();

    // Only the cyclic field needs indirection.
    assert!(!direct_field.needs_indirection());
    assert!(indirect_field.needs_indirection());
}

// MARK: Operation metadata

#[test]
fn test_operations_metadata_basic() {
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
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // The `User` schema should be marked as used by the `getUsers` operation.
    let user_schema = graph.schemas().find(|s| s.name() == "User").unwrap();
    let used_by_ops = user_schema.used_by().map(|op| op.id()).collect_vec();

    assert_matches!(&*used_by_ops, ["getUsers"]);
}

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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Both `UserList` and `User` should be marked as used by the operation,
    // even though only `UserList` is directly referenced.
    let user_list = graph.schemas().find(|s| s.name() == "UserList").unwrap();
    let user = graph.schemas().find(|s| s.name() == "User").unwrap();

    let user_list_ops = user_list.used_by().map(|op| op.id()).collect_vec();
    let user_ops = user.used_by().map(|op| op.id()).collect_vec();

    assert_matches!(&*user_list_ops, ["getUsers"]);
    assert_matches!(&*user_ops, ["getUsers"]);
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

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // `User` should be used by both operations.
    let user = graph.schemas().find(|s| s.name() == "User").unwrap();
    let mut used_by_ops = user.used_by().map(|op| op.id()).collect_vec();
    used_by_ops.sort();
    assert_matches!(&*used_by_ops, ["createUser", "getUsers"]);
}
