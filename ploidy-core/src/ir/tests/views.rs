//! Tests for the IR view layer, indirection, and extension system.

use itertools::Itertools;

use crate::{
    ir::{
        InlineIrTypePathRoot, InlineIrTypePathSegment, InlineIrTypeView, IrEnumVariant, IrGraph,
        IrParameterStyle, IrRequestView, IrResponseView, IrSpec, IrStructFieldName, IrTypeView,
        PrimitiveIrType, SchemaIrTypeView, SomeIrUntaggedVariant, View,
    },
    parse::{Document, Method, path::PathFragment},
    tests::assert_matches,
};

// MARK: View construction

#[test]
fn test_struct_view_fields_iterator() {
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
                age:
                  type: integer
                email:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let person_schema = graph.schemas().find(|s| s.name() == "Person").unwrap();
    let person_struct = match person_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Person`; got {other:?}"),
    };

    // `fields()` should iterate over all struct fields.
    let mut field_names = person_struct
        .fields()
        .map(|f| match f.name() {
            IrStructFieldName::Name(n) => n,
            other => panic!("expected explicit struct field name; got {other:?}"),
        })
        .collect_vec();
    field_names.sort();
    assert_matches!(&*field_names, ["age", "email", "name"]);
}

#[test]
fn test_struct_field_view_accessors() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Record:
              type: object
              properties:
                id:
                  type: string
              required:
                - id
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let record_schema = graph.schemas().find(|s| s.name() == "Record").unwrap();
    let record_struct = match record_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Record`; got {other:?}"),
    };

    let id_field = record_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("id")))
        .unwrap();
    assert_matches!(id_field.name(), IrStructFieldName::Name("id"));
    assert!(id_field.required());
}

#[test]
fn test_schema_view_from_graph() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            MyStruct:
              type: object
              properties:
                field:
                  type: string
            MyEnum:
              type: string
              enum: [A, B, C]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    // Should be able to construct views for different schema types.
    let struct_view = graph.schemas().find(|s| s.name() == "MyStruct").unwrap();
    let enum_view = graph.schemas().find(|s| s.name() == "MyEnum").unwrap();

    // Verify types.
    assert_matches!(struct_view, SchemaIrTypeView::Struct(_, _));
    assert_matches!(enum_view, SchemaIrTypeView::Enum(_, _));
}

#[test]
fn test_operation_view_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: getUsers
              description: Get all users
              responses:
                '200':
                  description: OK
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

    // Should be able to access the operation and its types.
    let operation = graph.operations().next().unwrap();
    assert_eq!(operation.id(), "getUsers");
    assert_eq!(operation.description(), Some("Get all users"));

    // `getUsers` should reference the `User` schema.
    let user_schema = graph.schemas().find(|s| s.name() == "User").unwrap();

    // `User` should be used by `getUsers`.
    let used_by_ops = user_schema.used_by().map(|op| op.id()).collect_vec();
    assert_matches!(&*used_by_ops, ["getUsers"]);
}

// MARK: Extension system

#[test]
fn test_extension_insertion_retrieval() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            TestStruct:
              type: object
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let mut schema_view = graph.schemas().next().unwrap();

    // Insert an extension.
    schema_view.extensions_mut().insert("test_data");

    // Retrieve the extension.
    let retrieved = schema_view.extensions().get::<&str>();
    assert!(retrieved.is_some());
    assert_eq!(*retrieved.unwrap(), "test_data");
}

#[test]
fn test_extension_type_safety() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            TestStruct:
              type: object
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let mut schema_view = graph.schemas().next().unwrap();

    // Insert a string extension.
    schema_view.extensions_mut().insert("test_string");

    // Trying to retrieve as the wrong type should return `None`.
    let wrong_type = schema_view.extensions().get::<i32>();
    assert!(wrong_type.is_none());

    // Retrieving as the correct type should work.
    let correct_type = schema_view.extensions().get::<&str>();
    assert!(correct_type.is_some());
    assert_eq!(*correct_type.unwrap(), "test_string");
}

#[test]
fn test_extension_per_node_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Struct1:
              type: object
              properties:
                field:
                  type: string
            Struct2:
              type: object
              properties:
                field:
                  type: string
            Struct3:
              type: object
              properties:
                ref1:
                  $ref: '#/components/schemas/Struct1'
                ref2:
                  $ref: '#/components/schemas/Struct2'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let mut schema1 = graph.schemas().find(|s| s.name() == "Struct1").unwrap();
    let mut schema2 = graph.schemas().find(|s| s.name() == "Struct2").unwrap();

    // Set different extensions on different node types.
    schema1.extensions_mut().insert("data_1");
    schema2.extensions_mut().insert("data_2");

    // Each node type should have its own extension.
    let ext1 = schema1.extensions().get::<&str>();
    let ext2 = schema2.extensions().get::<&str>();

    assert!(ext1.is_some());
    assert!(ext2.is_some());
    assert_eq!(*ext1.unwrap(), "data_1");
    assert_eq!(*ext2.unwrap(), "data_2");

    // Extensions are per-type, not per-node, so `Struct3`'s fields
    // should return the same extensions.
    let schema3 = graph.schemas().find(|s| s.name() == "Struct3").unwrap();
    let struct3_view = match schema3 {
        SchemaIrTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Struct3`; got {other:?}"),
    };

    let ref1_field = struct3_view
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("ref1")))
        .unwrap();
    let ref1_ty = ref1_field.ty();
    let ref1_schema = match ref1_ty {
        IrTypeView::Schema(schema) => schema,
        other => panic!("expected schema reference; got {other:?}"),
    };

    let ref2_field = struct3_view
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("ref2")))
        .unwrap();
    let ref2_ty = ref2_field.ty();
    let ref2_schema = match ref2_ty {
        IrTypeView::Schema(view) => view,
        other => panic!("expected schema reference; got {other:?}"),
    };

    // Accessing the same type through different paths
    // should return the same extensions.
    let ref1_ext = ref1_schema.extensions().get::<&str>();
    let ref2_ext = ref2_schema.extensions().get::<&str>();

    assert_eq!(*ref1_ext.unwrap(), "data_1");
    assert_eq!(*ref2_ext.unwrap(), "data_2");
}

// MARK: `reachable()`

#[test]
fn test_reachable_multiple_dependencies() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf1:
              type: object
              properties:
                value:
                  type: string
            Leaf2:
              type: object
              properties:
                data:
                  type: integer
            Branch:
              type: object
              properties:
                leaf1:
                  $ref: '#/components/schemas/Leaf1'
                leaf2:
                  $ref: '#/components/schemas/Leaf2'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let branch_schema = graph.schemas().find(|s| s.name() == "Branch").unwrap();

    // `reachable()` should include `Branch`, `Leaf1`, and `Leaf2`.
    let mut reachable_names = branch_schema
        .reachable()
        .filter_map(|view| match view {
            IrTypeView::Schema(view) => Some(view.name()),
            _ => None,
        })
        .collect_vec();
    reachable_names.sort();
    assert_matches!(&*reachable_names, ["Branch", "Leaf1", "Leaf2"]);
}

#[test]
fn test_reachable_no_dependencies() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Standalone:
              type: object
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let standalone_schema = graph.schemas().next().unwrap();

    // `reachable()` is a BFS from the starting node to all reachable nodes.
    // For a struct with a primitive field, the reachable set includes
    // both the struct and the primitive type.
    assert_eq!(standalone_schema.reachable().count(), 2);
}

#[test]
fn test_reachable_handles_cycles_without_infinite_loop() {
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

    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();

    // `reachable()` should not revisit already-visited schemas.
    assert_eq!(a_schema.reachable().count(), 2);
}

#[test]
fn test_reachable_from_array_includes_inner_types() {
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
            Container:
              type: object
              properties:
                items:
                  type: array
                  items:
                    $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let items_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
        .unwrap();

    let array_view = match items_field.ty() {
        IrTypeView::Array(view) => view,
        other => panic!("expected array; got {other:?}"),
    };

    // `reachable()` from the array should include the array, the schema
    // reference, and the primitive field in `Item`.
    let reachable_types = array_view.reachable().collect_vec();
    assert_eq!(reachable_types.len(), 3);

    // Verify the array itself is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Array(_)))
    );

    // Verify the `Item` schema is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Schema(SchemaIrTypeView::Struct("Item", _))))
    );

    // Verify the primitive field is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Primitive(PrimitiveIrType::String)))
    );
}

#[test]
fn test_reachable_from_map_includes_inner_types() {
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
                name:
                  type: string
            Container:
              type: object
              properties:
                map_field:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let map_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("map_field")))
        .unwrap();

    let map_view = match map_field.ty() {
        IrTypeView::Map(view) => view,
        other => panic!("expected map; got {other:?}"),
    };

    // `reachable()` from the map should include the map, the schema
    // reference, and the primitive field in `Item`.
    let reachable_types = map_view.reachable().collect_vec();
    assert_eq!(reachable_types.len(), 3);

    // Verify the map itself is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Map(_)))
    );

    // Verify the `Item` schema is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Schema(SchemaIrTypeView::Struct("Item", _))))
    );

    // Verify the primitive field is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Primitive(PrimitiveIrType::String)))
    );
}

#[test]
fn test_reachable_from_nullable_includes_inner_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Item:
              type: object
              nullable: true
              properties:
                value:
                  type: string
            Container:
              type: object
              properties:
                nullable_field:
                  $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("nullable_field")))
        .unwrap();

    let nullable_view = match nullable_field.ty() {
        IrTypeView::Nullable(view) => view,
        other => panic!("expected nullable; got {other:?}"),
    };

    // `reachable()` from the nullable should include the nullable, the schema
    // reference, and the primitive field in `Item`.
    let reachable_types = nullable_view.reachable().collect_vec();
    assert_eq!(reachable_types.len(), 3);

    // Verify the nullable itself is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Nullable(_)))
    );

    // Verify the `Item` schema is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Schema(SchemaIrTypeView::Struct("Item", _))))
    );

    // Verify the primitive field is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Primitive(PrimitiveIrType::String)))
    );
}

#[test]
fn test_reachable_from_inline_includes_inner_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            RefSchema:
              type: object
              properties:
                value:
                  type: string
            Container:
              type: object
              properties:
                inline_field:
                  type: object
                  properties:
                    ref_field:
                      $ref: '#/components/schemas/RefSchema'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let inline_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("inline_field")))
        .unwrap();

    let inline_view = match inline_field.ty() {
        IrTypeView::Inline(view) => view,
        other => panic!("expected inline; got {other:?}"),
    };

    // `reachable()` from an inline type should include the inline struct,
    // the schema reference, and the primitive field in `RefSchema`.
    let reachable_types = inline_view.reachable().collect_vec();
    assert_eq!(reachable_types.len(), 3);

    // Verify the inline itself is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Inline(_)))
    );

    // Verify the `RefSchema` schema is reachable.
    assert!(reachable_types.iter().any(|t| matches!(
        t,
        IrTypeView::Schema(SchemaIrTypeView::Struct("RefSchema", _))
    )));

    // Verify the primitive field is reachable.
    assert!(
        reachable_types
            .iter()
            .any(|t| matches!(t, IrTypeView::Primitive(PrimitiveIrType::String)))
    );
}

#[test]
fn test_reachable_from_primitive_returns_itself() {
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
                name:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let simple_schema = graph.schemas().next().unwrap();
    let simple_struct = match simple_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Simple`; got {other:?}"),
    };

    let name_field = simple_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("name")))
        .unwrap();

    let primitive_view = match name_field.ty() {
        IrTypeView::Primitive(_) => name_field.ty(),
        other => panic!("expected primitive; got {other:?}"),
    };

    // A primitive has no graph edges, so `reachable()` should
    // only include itself.
    let reachable_types = primitive_view.reachable().collect_vec();
    assert_matches!(
        &*reachable_types,
        [IrTypeView::Primitive(PrimitiveIrType::String)]
    );
}

#[test]
fn test_reachable_from_any_returns_itself() {
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
                untyped:
                  additionalProperties: true
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let untyped_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("untyped")))
        .unwrap();

    let any_view = match untyped_field.ty() {
        IrTypeView::Any => untyped_field.ty(),
        other => panic!("expected any; got {other:?}"),
    };

    // `Any` has no graph edges, so `reachable()` should only include itself.
    let reachable_types = any_view.reachable().collect_vec();
    assert_matches!(&*reachable_types, [IrTypeView::Any]);
}

// MARK: `inlines()`

#[test]
fn test_inlines_finds_inline_structs_in_struct_fields() {
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
                inline_obj:
                  type: object
                  properties:
                    nested_field:
                      type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let parent_schema = graph.schemas().next().unwrap();

    // Should find the inline struct.
    assert_eq!(parent_schema.inlines().count(), 1);
}

#[test]
fn test_inlines_finds_inline_types_in_nested_arrays() {
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
                items:
                  type: array
                  items:
                    type: object
                    properties:
                      item:
                        type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();

    // Should find inline types within array.
    assert_eq!(container_schema.inlines().count(), 1);
}

#[test]
fn test_inlines_empty_for_schemas_with_no_inlines() {
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
                field:
                  $ref: '#/components/schemas/Other'
            Other:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let simple_schema = graph.schemas().find(|s| s.name() == "Simple").unwrap();

    // `Simple` only references schemas, so shouldn't have any inlines.
    assert_eq!(simple_schema.inlines().count(), 0);
}

// MARK: Tagged union variant views

#[test]
fn test_tagged_variant_iteration() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
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
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  cat: '#/components/schemas/Cat'
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaIrTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let mut variant_names = tagged_view.variants().map(|v| v.name()).collect_vec();
    variant_names.sort();
    assert_matches!(&*variant_names, ["Cat", "Dog"]);
}

#[test]
fn test_tagged_variant_name_and_aliases_access() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
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
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  cat: '#/components/schemas/Cat'
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaIrTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let variant = tagged_view.variants().next().unwrap();
    assert_eq!(variant.name(), "Cat");
    assert_matches!(variant.aliases(), ["cat"]);
}

#[test]
fn test_tagged_variant_type_access() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        components:
          schemas:
            Cat:
              type: object
              properties:
                meow:
                  type: string
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
              discriminator:
                propertyName: kind
                mapping:
                  cat: '#/components/schemas/Cat'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaIrTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let variant = tagged_view.variants().next().unwrap();
    let ty = variant.ty();

    // Verify the type is accessible and is a schema reference to `Cat`.
    assert_matches!(ty, IrTypeView::Schema(view) if view.name() == "Cat");
}

// MARK: Untagged union variant views

#[test]
fn test_untagged_variant_iteration() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
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
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
                - $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let untagged_view = match animal_schema {
        SchemaIrTypeView::Untagged(_, view) => view,
        _ => panic!("`Animal` should be an untagged union"),
    };

    // Untagged variants contain `Cat` and `Dog` schema references.
    assert_eq!(untagged_view.variants().count(), 2);
}

// MARK: Wrapper views

#[test]
fn test_array_view_provides_access_to_item_type() {
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
                items:
                  type: array
                  items:
                    type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let items_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
        .unwrap();

    // Verify the array's inner type is accessible,
    // and is a string primitive.
    assert_matches!(
        items_field.ty(),
        IrTypeView::Array(view) if matches!(
            view.inner(),
            IrTypeView::Primitive(PrimitiveIrType::String),
        ),
    );
}

#[test]
fn test_map_view_provides_access_to_value_type() {
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
                map_field:
                  type: object
                  additionalProperties:
                    type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let map_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("map_field")))
        .unwrap();

    // Verify the map's inner type is accessible,
    // and is a string primitive.
    assert_matches!(
        map_field.ty(),
        IrTypeView::Map(view) if matches!(
            view.inner(),
            IrTypeView::Primitive(PrimitiveIrType::String),
        ),
    );
}

#[test]
fn test_nullable_view_provides_access_to_inner_type() {
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
                nullable_field:
                  type: object
                  nullable: true
                  properties:
                    data:
                      type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("nullable_field")))
        .unwrap();

    // Verify the nullable's inner type is accessible,
    // and is an inline struct.
    assert_matches!(
        nullable_field.ty(),
        IrTypeView::Nullable(view) if matches!(
            view.inner(),
            IrTypeView::Inline(InlineIrTypeView::Struct(_, _)),
        ),
    );
}

// MARK: Inline type views

#[test]
fn test_inline_struct_view_construction_and_path_access() {
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
                inline_obj:
                  type: object
                  properties:
                    nested_field:
                      type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let inline_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("inline_obj")))
        .unwrap();

    let field_ty = inline_field.ty();
    let inline_view = match field_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // Should be able to match on the `Struct` variant.
    assert_matches!(inline_view, InlineIrTypeView::Struct(_, _));

    // `path()` should return the path to the inline type.
    let path = inline_view.path();
    assert_eq!(path.segments.len(), 1);
}

#[test]
fn test_inline_enum_view_construction() {
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
                status:
                  type: string
                  enum: [active, inactive, pending]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let status_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("status")))
        .unwrap();

    let field_ty = status_field.ty();
    let inline_view = match field_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline enum; got {other:?}"),
    };

    // Should construct an `Enum` variant.
    let enum_view = match inline_view {
        InlineIrTypeView::Enum(_, view) => view,
        other => panic!("expected inline enum; got {other:?}"),
    };

    // Verify the enum has the expected variants.
    assert_eq!(enum_view.variants().len(), 3);
}

#[test]
fn test_inline_untagged_view_construction() {
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
                value:
                  oneOf:
                    - type: string
                    - type: integer
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let value_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("value")))
        .unwrap();

    let field_ty = value_field.ty();
    let inline_view = match field_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline untagged union; got {other:?}"),
    };

    // Should construct an `Untagged` variant.
    let untagged_view = match inline_view {
        InlineIrTypeView::Untagged(_, view) => view,
        other => panic!("expected inline untagged union; got {other:?}"),
    };

    // Verify the untagged union has the expected number of variants.
    assert_eq!(untagged_view.variants().count(), 2);
}

#[test]
fn test_inline_view_path_method() {
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
                nested:
                  type: object
                  properties:
                    deep:
                      type: object
                      properties:
                        field:
                          type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let parent_schema = graph.schemas().next().unwrap();
    let parent_struct = match parent_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Parent`; got {other:?}"),
    };

    let nested_field = parent_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("nested")))
        .unwrap();

    let nested_ty = nested_field.ty();
    let nested_inline = match nested_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // `path()` should return a path with one segment.
    let path = nested_inline.path();
    assert_matches!(
        &*path.segments,
        [InlineIrTypePathSegment::Field(IrStructFieldName::Name(
            "nested"
        ))]
    );
}

#[test]
fn test_inline_view_with_view_trait_methods() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /records:
            post:
              operationId: createRecord
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        status:
                          type: string
                          enum: [draft, published]
              responses:
                '201':
                  description: Created
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    let request = operation.request().unwrap();

    let request_ty = match request {
        IrRequestView::Json(ty) => ty,
        other => panic!("expected JSON request; got `{other:?}`"),
    };

    let request_struct = match request_ty {
        IrTypeView::Inline(InlineIrTypeView::Struct(_, view)) => view,
        other => panic!("expected inline struct; got {other:?}"),
    };

    let status_field = request_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("status")))
        .unwrap();

    let status_ty = status_field.ty();
    let inline_enum = match status_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // `used_by()` should find the operations that use this inline type.
    let operations = inline_enum.used_by().map(|op| op.id()).collect_vec();
    assert_matches!(&*operations, ["createRecord"]);

    // `inlines()` includes the starting node.
    assert_eq!(inline_enum.inlines().count(), 1);

    // `reachable()` should include the inline enum itself.
    assert_eq!(inline_enum.reachable().count(), 1);
}

#[test]
fn test_untagged_variant_with_null_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
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
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
                - $ref: '#/components/schemas/Dog'
                - type: 'null'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let untagged_view = match animal_schema {
        SchemaIrTypeView::Untagged(_, view) => view,
        other => panic!("expected untagged union `Animal`; got {other:?}"),
    };

    let variants = untagged_view.variants().collect_vec();
    assert_eq!(variants.len(), 3);

    // The first two variants should be schema references.
    let cat_variant = &variants[0];
    assert_matches!(
        cat_variant.ty(),
        Some(SomeIrUntaggedVariant {
            view: IrTypeView::Schema(view),
            ..
        }) if view.name() == "Cat",
    );

    let dog_variant = &variants[1];
    assert_matches!(
        dog_variant.ty(),
        Some(SomeIrUntaggedVariant {
            view: IrTypeView::Schema(view),
            ..
        }) if view.name() == "Dog",
    );

    // The third variant should be `null`, returning `None`.
    let null_variant = &variants[2];
    assert!(null_variant.ty().is_none());
}

// MARK: Enum views

#[test]
fn test_enum_view_variants() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Status:
              type: string
              enum: [active, inactive, pending]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match status_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Status`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    assert_matches!(
        variants,
        [
            IrEnumVariant::String("active"),
            IrEnumVariant::String("inactive"),
            IrEnumVariant::String("pending"),
        ]
    );
}

#[test]
fn test_enum_view_with_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Priority:
              type: string
              description: Task priority level
              enum: [low, medium, high]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let priority_schema = graph.schemas().find(|s| s.name() == "Priority").unwrap();
    let enum_view = match priority_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Priority`; got {other:?}"),
    };
    assert_matches!(enum_view.description(), Some("Task priority level"));
}

#[test]
fn test_enum_view_without_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Status:
              type: string
              enum: [active, inactive]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match status_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Status`; got {other:?}"),
    };
    assert_matches!(enum_view.description(), None);
}

#[test]
fn test_enum_view_variants_with_numbers() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Priority:
              type: integer
              enum: [1, 2, 3]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let priority_schema = graph.schemas().find(|s| s.name() == "Priority").unwrap();
    let enum_view = match priority_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Priority`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    let [
        IrEnumVariant::Number(n1),
        IrEnumVariant::Number(n2),
        IrEnumVariant::Number(n3),
    ] = variants
    else {
        panic!("expected 3 variants; got {variants:?}");
    };
    assert_eq!(n1.as_i64(), Some(1));
    assert_eq!(n2.as_i64(), Some(2));
    assert_eq!(n3.as_i64(), Some(3));
}

#[test]
fn test_enum_view_variants_with_booleans() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Toggle:
              type: boolean
              enum: [true, false]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let toggle_schema = graph.schemas().find(|s| s.name() == "Toggle").unwrap();
    let enum_view = match toggle_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Toggle`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    let &[IrEnumVariant::Bool(b1), IrEnumVariant::Bool(b2)] = variants else {
        panic!("expected 2 variants; got {variants:?}");
    };
    assert!(b1);
    assert!(!b2);
}

#[test]
fn test_enum_view_with_view_trait_methods() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /tasks:
            get:
              operationId: getTasks
              responses:
                '200':
                  description: OK
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          status:
                            $ref: '#/components/schemas/Status'
        components:
          schemas:
            Status:
              type: string
              enum: [pending, completed, cancelled]
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match status_schema {
        SchemaIrTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Status`; got {other:?}"),
    };

    // `used_by()` should find the operations that use this enum.
    let operations = enum_view.used_by().map(|op| op.id()).collect_vec();
    assert_matches!(&*operations, ["getTasks"]);

    // Enums can't contain inline types, so `inlines()` should be empty.
    assert_eq!(enum_view.inlines().count(), 0);

    // `reachable()` should include the enum itself.
    assert_eq!(enum_view.reachable().count(), 1);
}

// MARK: Operation views

#[test]
fn test_operation_view_resource() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: getUsers
              x-resource-name: UserResource
              responses:
                '200':
                  description: OK
                  content:
                    application/json:
                      schema:
                        type: object
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    assert_eq!(operation.resource(), "UserResource");
}

#[test]
fn test_operation_view_method() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: getUsers
              responses:
                '200':
                  description: OK
            post:
              operationId: createUser
              responses:
                '201':
                  description: Created
            put:
              operationId: updateUser
              responses:
                '200':
                  description: OK
            patch:
              operationId: patchUser
              responses:
                '200':
                  description: OK
            delete:
              operationId: deleteUser
              responses:
                '204':
                  description: No Content
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operations = graph.operations().collect_vec();
    assert_eq!(operations.len(), 5);

    let get_op = operations.iter().find(|op| op.id() == "getUsers").unwrap();
    assert_matches!(get_op.method(), Method::Get);

    let post_op = operations
        .iter()
        .find(|op| op.id() == "createUser")
        .unwrap();
    assert_matches!(post_op.method(), Method::Post);

    let put_op = operations
        .iter()
        .find(|op| op.id() == "updateUser")
        .unwrap();
    assert_matches!(put_op.method(), Method::Put);

    let patch_op = operations.iter().find(|op| op.id() == "patchUser").unwrap();
    assert_matches!(patch_op.method(), Method::Patch);

    let delete_op = operations
        .iter()
        .find(|op| op.id() == "deleteUser")
        .unwrap();
    assert_matches!(delete_op.method(), Method::Delete);
}

#[test]
fn test_operation_view_inlines_excludes_schema_references() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: getUsers
              responses:
                '200':
                  description: OK
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

    let operation = graph.operations().next().unwrap();

    // The operation references a named schema, not an inline type.
    assert_eq!(operation.inlines().count(), 0);
}

#[test]
fn test_operation_view_inlines_with_mixed_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        profile:
                          $ref: '#/components/schemas/Profile'
                        metadata:
                          type: object
                          properties:
                            tags:
                              type: array
                              items:
                                type: string
              responses:
                '201':
                  description: Created
        components:
          schemas:
            Profile:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    // Should find the inline request body and the inline metadata object.
    // `Profile` is a schema reference, and should be excluded.
    assert_eq!(operation.inlines().count(), 2);
}

#[test]
fn test_operation_parameter_ty() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users/{id}:
            get:
              operationId: getUser
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
                - name: tags
                  in: query
                  schema:
                    type: array
                    items:
                      type: string
              responses:
                '200':
                  description: OK
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    // String path parameter.
    let path_param = operation.path().params().next().unwrap();
    assert_matches!(
        path_param.ty(),
        IrTypeView::Primitive(PrimitiveIrType::String),
    );

    // Array-of-strings query parameter.
    let query_param = operation.query().next().unwrap();
    assert_matches!(
        query_param.ty(),
        IrTypeView::Array(view) if matches!(
            view.inner(),
            IrTypeView::Primitive(PrimitiveIrType::String),
        ),
    );
}

#[test]
fn test_operation_parameter_style() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: tags
                  in: query
                  schema:
                    type: array
                    items:
                      type: string
                  style: form
                  explode: false
                - name: filters
                  in: query
                  schema:
                    type: array
                    items:
                      type: string
                  style: pipeDelimited
                - name: space_separated
                  in: query
                  schema:
                    type: array
                    items:
                      type: string
                  style: spaceDelimited
                - name: form_exploded
                  in: query
                  schema:
                    type: array
                    items:
                      type: string
                  style: form
                  explode: true
                - name: deep_obj
                  in: query
                  schema:
                    type: object
                  style: deepObject
                - name: no_style
                  in: query
                  schema:
                    type: string
              responses:
                '200':
                  description: OK
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    let query_params = operation.query().collect_vec();

    // Non-exploded `form` style.
    let tags = query_params.iter().find(|p| p.name() == "tags").unwrap();
    assert_matches!(
        tags.style(),
        Some(IrParameterStyle::Form { exploded: false }),
    );

    // `pipeDelimited` style.
    let filters = query_params.iter().find(|p| p.name() == "filters").unwrap();
    assert_matches!(filters.style(), Some(IrParameterStyle::PipeDelimited));

    // `spaceDelimited` style.
    let space = query_params
        .iter()
        .find(|p| p.name() == "space_separated")
        .unwrap();
    assert_matches!(space.style(), Some(IrParameterStyle::SpaceDelimited));

    // Exploded `form` style, explicitly specified.
    let form_exploded = query_params
        .iter()
        .find(|p| p.name() == "form_exploded")
        .unwrap();
    assert_matches!(
        form_exploded.style(),
        Some(IrParameterStyle::Form { exploded: true }),
    );

    // `deepObject` style.
    let deep_obj = query_params
        .iter()
        .find(|p| p.name() == "deep_obj")
        .unwrap();
    assert_matches!(deep_obj.style(), Some(IrParameterStyle::DeepObject));

    // No explicit style; defaults to the exploded `form` style.
    let no_style = query_params
        .iter()
        .find(|p| p.name() == "no_style")
        .unwrap();
    assert_matches!(
        no_style.style(),
        Some(IrParameterStyle::Form { exploded: true }),
    );
}

#[test]
fn test_operation_request_json() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        name:
                          type: string
              responses:
                '201':
                  description: Created
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    assert_matches!(
        operation.request(),
        Some(IrRequestView::Json(IrTypeView::Inline(_))),
    );
}

#[test]
fn test_operation_request_multipart() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /upload:
            post:
              operationId: uploadFile
              requestBody:
                content:
                  multipart/form-data:
                    schema:
                      type: object
                      properties:
                        file:
                          type: string
                          format: binary
              responses:
                '200':
                  description: OK
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    assert_matches!(operation.request(), Some(IrRequestView::Multipart));
}

#[test]
fn test_operation_path() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
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
                  description: OK
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          id:
                            type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    let segments = operation.path().segments().as_slice();
    let [a, b] = segments else {
        panic!("expected two path segments; got {segments:?}");
    };
    assert_matches!(
        a.fragments(),
        [PathFragment::Literal(n)] if n == "users",
    );
    assert_matches!(b.fragments(), [PathFragment::Param("id")]);
}

#[test]
fn test_operation_response_without_schema() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /resource:
            get:
              operationId: getResource
              responses:
                '200':
                  description: OK
                  content:
                    application/json:
                      schema: {}
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    // Empty response schema becomes `IrResponse::Json(IrType::Any)`.
    assert_matches!(
        operation.response(),
        Some(IrResponseView::Json(IrTypeView::Any))
    );
}

#[test]
fn test_operation_query() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: limit
                  in: query
                  required: true
                  schema:
                    type: integer
                - name: offset
                  in: query
                  schema:
                    type: integer
              responses:
                '200':
                  description: OK
                  content:
                    application/json:
                      schema:
                        type: object
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    let query_params = operation.query().collect_vec();
    let [limit, offset] = &*query_params else {
        panic!("expected two query parameters; got {query_params:?}");
    };
    assert_eq!(limit.name(), "limit");
    assert!(limit.required());
    assert_eq!(offset.name(), "offset");
    assert!(!offset.required());
}

#[test]
fn test_operation_view_inlines_finds_inline_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        name:
                          type: string
                        address:
                          type: object
                          properties:
                            street:
                              type: string
              responses:
                '201':
                  description: Created
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          id:
                            type: string
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    // `createUser` references two inline types: the request body,
    // and the response body. The request body also contains a nested
    // inline type (`address`), so the total is 3.
    let inlines = operation.inlines().collect_vec();
    assert_eq!(inlines.len(), 3);

    let address = inlines
        .iter()
        .find(|inline| {
            let path = inline.path();
            matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Request,
                    InlineIrTypePathSegment::Field(IrStructFieldName::Name("address")),
                ],
            )
        })
        .unwrap();
    assert_matches!(address, InlineIrTypeView::Struct(_, _));

    let request = inlines
        .iter()
        .find(|inline| {
            let path = inline.path();
            matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Request,
                ],
            )
        })
        .unwrap();
    assert_matches!(request, InlineIrTypeView::Struct(_, _));

    let response = inlines
        .iter()
        .find(|inline| {
            let path = inline.path();
            matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Response,
                ],
            )
        })
        .unwrap();
    assert_matches!(response, InlineIrTypeView::Struct(_, _));
}

#[test]
fn test_operation_request_and_response() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        name:
                          type: string
              responses:
                '201':
                  description: Created
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          id:
                            type: string
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    let Some(IrRequestView::Json(IrTypeView::Inline(request))) = operation.request() else {
        panic!(
            "expected inline request schema; got {:?}",
            operation.request(),
        );
    };
    assert_matches!(request.path().root, InlineIrTypePathRoot::Resource("full"));
    assert_matches!(
        &*request.path().segments,
        [
            InlineIrTypePathSegment::Operation("createUser"),
            InlineIrTypePathSegment::Request,
        ],
    );

    let Some(IrResponseView::Json(IrTypeView::Inline(response))) = operation.response() else {
        panic!(
            "expected inline response schema; got {:?}",
            operation.response(),
        );
    };
    assert_matches!(response.path().root, InlineIrTypePathRoot::Resource("full"));
    assert_matches!(
        &*response.path().segments,
        [
            InlineIrTypePathSegment::Operation("createUser"),
            InlineIrTypePathSegment::Response,
        ],
    );
}

// MARK: Inline tagged union views

#[test]
fn test_inline_tagged_view_construction() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
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
            Container:
              type: object
              properties:
                animal:
                  oneOf:
                    - $ref: '#/components/schemas/Cat'
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      cat: '#/components/schemas/Cat'
                      dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let animal_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("animal")))
        .unwrap();

    let field_ty = animal_field.ty();
    let inline_view = match field_ty {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Should construct a `Tagged` variant.
    let tagged_view = match inline_view {
        InlineIrTypeView::Tagged(_, view) => view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Verify the tag property.
    assert_eq!(tagged_view.tag(), "kind");

    // Verify the variants.
    let mut variant_names = tagged_view.variants().map(|v| v.name()).collect_vec();
    variant_names.sort();
    assert_matches!(&*variant_names, ["Cat", "Dog"]);
}

#[test]
fn test_inline_tagged_view_variant_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
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
            Container:
              type: object
              properties:
                animal:
                  oneOf:
                    - $ref: '#/components/schemas/Cat'
                  discriminator:
                    propertyName: kind
                    mapping:
                      cat: '#/components/schemas/Cat'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let animal_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("animal")))
        .unwrap();

    let inline_view = match animal_field.ty() {
        IrTypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    let tagged_view = match inline_view {
        InlineIrTypeView::Tagged(_, view) => view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Verify the variant type is accessible.
    let variant = tagged_view.variants().next().unwrap();
    assert_eq!(variant.name(), "Cat");
    assert_matches!(variant.ty(), IrTypeView::Schema(view) if view.name() == "Cat");
}

#[test]
fn test_inlines_finds_inline_tagged_unions() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
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
            Container:
              type: object
              properties:
                animal:
                  oneOf:
                    - $ref: '#/components/schemas/Cat'
                    - $ref: '#/components/schemas/Dog'
                  discriminator:
                    propertyName: kind
                    mapping:
                      cat: '#/components/schemas/Cat'
                      dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();

    // Should find the inline tagged union.
    let inlines = container_schema.inlines().collect_vec();
    assert_matches!(&*inlines, [InlineIrTypeView::Tagged(_, _)]);
}
