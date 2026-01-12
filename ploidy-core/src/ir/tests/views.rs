//! Tests for the IR view layer, indirection, and extension system.

use itertools::Itertools;

use crate::{
    ir::{
        InlineIrTypeView, IrGraph, IrSpec, IrStructFieldName, IrTypeView, PrimitiveIrType,
        SchemaIrTypeView, View,
    },
    parse::Document,
    tests::assert_matches,
};

// MARK: View construction tests

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

    // `getUsers` should reference the `User` schema.
    let user_schema = graph.schemas().find(|s| s.name() == "User").unwrap();

    // `User` should be used by `getUsers`.
    let used_by_ops = user_schema.used_by().map(|op| op.id()).collect_vec();
    assert_matches!(&*used_by_ops, ["getUsers"]);
}

// MARK: Extension system tests

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

// MARK: `reachable()` tests

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

// MARK: `inlines()` tests

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

// MARK: Tagged union variant view tests

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

// MARK: Untagged union variant view tests

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

// MARK: Wrapper view tests

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

    // Access the array type through the field.
    let field_ty = items_field.ty();
    let array_view = match field_ty {
        IrTypeView::Array(array_view) => array_view,
        other => panic!("expected array; got {other:?}"),
    };
    // Verify inner type is accessible and is a string primitive.
    let inner = array_view.inner();
    assert_matches!(inner, IrTypeView::Primitive(PrimitiveIrType::String));
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

    // Access the map type through the field.
    let field_ty = map_field.ty();
    let map_view = match field_ty {
        IrTypeView::Map(map_view) => map_view,
        other => panic!("expected map; got {other:?}"),
    };
    // Verify inner type is accessible and is a string primitive.
    let inner = map_view.inner();
    assert_matches!(inner, IrTypeView::Primitive(PrimitiveIrType::String));
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

    // Access the nullable type through the field.
    let field_ty = nullable_field.ty();
    let nullable_view = match field_ty {
        IrTypeView::Nullable(nullable_view) => nullable_view,
        other => panic!("expected nullable; got {other:?}"),
    };
    // Verify inner type is accessible and is an inline struct.
    let inner = nullable_view.inner();
    assert_matches!(inner, IrTypeView::Inline(InlineIrTypeView::Struct(_, _)));
}

// MARK: Operation view detailed tests

#[test]
fn test_operation_view_path_method() {
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
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    let path_view = operation.path();

    assert_eq!(path_view.segments().len(), 2);
}

#[test]
fn test_operation_view_query_iterator() {
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
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();
    let query_param_names = operation.query().map(|p| p.name()).collect_vec();

    assert_matches!(&*query_param_names, ["limit", "offset"]);
}

#[test]
fn test_operation_view_request_and_response_methods() {
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
        components:
          schemas: {}
    "})
    .unwrap();

    let spec = IrSpec::from_doc(&doc).unwrap();
    let graph = IrGraph::new(&spec);

    let operation = graph.operations().next().unwrap();

    assert!(operation.request().is_some());
    assert!(operation.response().is_some());
}

#[test]
fn test_ir_parameter_view_accessors() {
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
                - name: include_details
                  in: query
                  required: false
                  schema:
                    type: boolean
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

    // Test path parameters.
    let path_params = operation.path().params().collect_vec();
    let [param] = &*path_params else {
        panic!("expected single path parameter; got {path_params:?}");
    };
    assert_eq!(param.name(), "id");
    assert!(param.required());

    // Test query parameters.
    let query_params = operation.query().collect_vec();
    let [param] = &*query_params else {
        panic!("expected single query parameter; got {query_params:?}");
    };
    assert_eq!(param.name(), "include_details");
    assert!(!param.required());
}

#[test]
fn test_operation_view_operation_info_accessors() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              description: Get all users
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

    assert_eq!(operation.id(), "listUsers");
    assert_matches!(operation.description(), Some("Get all users"));
}
