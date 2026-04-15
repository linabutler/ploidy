//! Tests for the IR view layer, indirection, and extension system.

use itertools::Itertools;

use crate::{
    arena::Arena,
    ir::{
        ContainerView, EnumVariant, ExtendableView, InlineTypePath, InlineTypePathRoot,
        InlineTypePathSegment, InlineTypeView, ParameterStyle, PrimitiveType, RawGraph,
        RequestView, Required, ResponseView, SchemaTypeInfo, SchemaTypeView, SomeUntaggedVariant,
        Spec, StructFieldName, TypeView, View,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let person_schema = graph.schemas().find(|s| s.name() == "Person").unwrap();
    let person_struct = match person_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Person`; got {other:?}"),
    };

    // `fields()` should iterate over all struct fields.
    let mut field_names = person_struct
        .fields()
        .map(|f| match f.name() {
            StructFieldName::Name(n) => n,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let record_schema = graph.schemas().find(|s| s.name() == "Record").unwrap();
    let record_struct = match record_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Record`; got {other:?}"),
    };

    let id_field = record_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("id")))
        .unwrap();
    assert_matches!(id_field.name(), StructFieldName::Name("id"));
    assert_eq!(id_field.required(), Required::Required { nullable: false });
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // Should be able to construct views for different schema types.
    let struct_view = graph.schemas().find(|s| s.name() == "MyStruct").unwrap();
    let enum_view = graph.schemas().find(|s| s.name() == "MyEnum").unwrap();

    // Verify types.
    assert_matches!(struct_view, SchemaTypeView::Struct(..));
    assert_matches!(enum_view, SchemaTypeView::Enum(..));
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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
              required: [field]
              properties:
                field:
                  type: string
            Struct2:
              type: object
              required: [field]
              properties:
                field:
                  type: string
            Struct3:
              type: object
              required: [ref1, ref2]
              properties:
                ref1:
                  $ref: '#/components/schemas/Struct1'
                ref2:
                  $ref: '#/components/schemas/Struct2'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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
        SchemaTypeView::Struct(_, struct_) => struct_,
        other => panic!("expected struct `Struct3`; got {other:?}"),
    };

    let ref1_field = struct3_view
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("ref1")))
        .unwrap();
    let ref1_ty = ref1_field.ty();
    let ref1_schema = match ref1_ty {
        TypeView::Schema(schema) => schema,
        other => panic!("expected schema reference; got {other:?}"),
    };

    let ref2_field = struct3_view
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("ref2")))
        .unwrap();
    let ref2_ty = ref2_field.ty();
    let ref2_schema = match ref2_ty {
        TypeView::Schema(view) => view,
        other => panic!("expected schema reference; got {other:?}"),
    };

    // Accessing the same type through different paths
    // should return the same extensions.
    let ref1_ext = ref1_schema.extensions().get::<&str>();
    let ref2_ext = ref2_schema.extensions().get::<&str>();

    assert_eq!(*ref1_ext.unwrap(), "data_1");
    assert_eq!(*ref2_ext.unwrap(), "data_2");
}

// MARK: `dependencies()`

#[test]
fn test_dependencies_multiple() {
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let branch_schema = graph.schemas().find(|s| s.name() == "Branch").unwrap();

    // `dependencies()` should include `Leaf1` and `Leaf2`,
    // but not `Branch` itself.
    let mut dep_names = branch_schema
        .dependencies()
        .filter_map(|view| match view {
            TypeView::Schema(view) => Some(view.name()),
            _ => None,
        })
        .collect_vec();
    dep_names.sort();
    assert_matches!(&*dep_names, ["Leaf1", "Leaf2"]);
}

#[test]
fn test_dependencies_none() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Standalone:
              type: object
              required: [field]
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let schema = graph.schemas().next().unwrap();

    // For a struct with a required primitive field, the dependency set
    // includes just that primitive type.
    assert_eq!(schema.dependencies().count(), 1);
}

#[test]
fn test_dependencies_handles_cycles_without_infinite_loop() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            A:
              type: object
              required: [b]
              properties:
                b:
                  $ref: '#/components/schemas/B'
            B:
              type: object
              required: [a]
              properties:
                a:
                  $ref: '#/components/schemas/A'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a_schema = graph.schemas().find(|s| s.name() == "A").unwrap();

    // `dependencies()` should not revisit already-visited schemas.
    assert_eq!(a_schema.dependencies().count(), 1);
}

#[test]
fn test_dependencies_from_array_includes_inner_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Item:
              type: object
              required: [value]
              properties:
                value:
                  type: string
            Container:
              type: object
              required: [items]
              properties:
                items:
                  type: array
                  items:
                    $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let items_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("items")))
        .unwrap();

    let container_view = match items_field.ty() {
        TypeView::Inline(InlineTypeView::Container(_, view @ ContainerView::Array(_))) => view,
        other => panic!("expected inline array; got {other:?}"),
    };

    // The `dependencies()` of the array should include the schema reference
    // and the primitive field in `Item`, but not the array itself.
    let dep_types = container_view.ty().dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Schema(SchemaTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String
    )));
}

#[test]
fn test_dependencies_from_map_includes_inner_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Item:
              type: object
              required: [name]
              properties:
                name:
                  type: string
            Container:
              type: object
              required: [map_field]
              properties:
                map_field:
                  type: object
                  additionalProperties:
                    $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let map_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("map_field")))
        .unwrap();

    let container_view = match map_field.ty() {
        TypeView::Inline(InlineTypeView::Container(_, view @ ContainerView::Map(_))) => view,
        other => panic!("expected inline map; got {other:?}"),
    };

    // The `dependencies()` of the map should include the schema reference,
    // and the primitive field in `Item`, but not the map itself.
    let dep_types = container_view.ty().dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Schema(SchemaTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String
    )));
}

#[test]
fn test_dependencies_from_nullable_includes_inner_types() {
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
              required: [value]
              properties:
                value:
                  type: string
            Container:
              type: object
              required: [nullable_field]
              properties:
                nullable_field:
                  $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("nullable_field")))
        .unwrap();

    let container_view = match nullable_field.ty() {
        TypeView::Inline(InlineTypeView::Container(_, view @ ContainerView::Optional(_))) => view,
        other => panic!("expected optional; got {other:?}"),
    };

    // The `dependencies()` of the optional should include the schema reference,
    // and the primitive field in `Item`, but not the optional itself.
    let dep_types = container_view.ty().dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Schema(SchemaTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String
    )));
}

#[test]
fn test_dependencies_from_inline_includes_inner_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            RefSchema:
              type: object
              required: [value]
              properties:
                value:
                  type: string
            Container:
              type: object
              required: [inline_field]
              properties:
                inline_field:
                  type: object
                  required: [ref_field]
                  properties:
                    ref_field:
                      $ref: '#/components/schemas/RefSchema'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let inline_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("inline_field")))
        .unwrap();

    let inline_view = match inline_field.ty() {
        TypeView::Inline(view) => view,
        other => panic!("expected inline; got {other:?}"),
    };

    // The `dependencies()` of the inline type should include the schema reference
    // and the primitive field in `RefSchema`, but not the inline itself.
    let dep_types = inline_view.dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `RefSchema` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Schema(SchemaTypeView::Struct(
            SchemaTypeInfo {
                name: "RefSchema",
                ..
            },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String
    )));
}

#[test]
fn test_dependencies_from_primitive_returns_empty() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Simple:
              type: object
              required: [name]
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let simple_schema = graph.schemas().next().unwrap();
    let simple_struct = match simple_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Simple`; got {other:?}"),
    };

    let name_field = simple_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("name")))
        .unwrap();

    let primitive_view = match name_field.ty() {
        TypeView::Inline(InlineTypeView::Primitive(_, view)) => view,
        other => panic!("expected primitive; got {other:?}"),
    };

    // A primitive has no graph edges, so `dependencies()` returns nothing.
    assert_eq!(primitive_view.dependencies().count(), 0);
}

#[test]
fn test_dependencies_from_any_returns_empty() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Container:
              type: object
              required: [untyped]
              properties:
                untyped:
                  additionalProperties: true
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let untyped_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("untyped")))
        .unwrap();

    let untyped_view = match untyped_field.ty() {
        TypeView::Inline(InlineTypeView::Any(_, view)) => view,
        other => panic!("expected any; got {other:?}"),
    };

    // `Any` has no graph edges, so `dependencies()` returns nothing.
    assert_eq!(untyped_view.dependencies().count(), 0);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let parent_schema = graph.schemas().next().unwrap();

    // Should find (1) the optional from the `inline_obj` field; (2) the inline struct;
    // (3) the optional for `nested_field` within the inline struct; and (4) the
    // `string` primitive for `nested_field`.
    assert_eq!(parent_schema.inlines().count(), 4);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();

    // Should find (1) the optional for the `items` field; (2) the array;
    // (3) the inline struct inside the array; (4) the optional for `item`
    // within the struct; and (5) the `string` primitive for `item`.
    assert_eq!(container_schema.inlines().count(), 5);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let simple_schema = graph.schemas().find(|s| s.name() == "Simple").unwrap();

    // `Simple` has one inline: an optional for the `field` property.
    assert_eq!(simple_schema.inlines().count(), 1);
}

#[test]
fn test_inlines_discovers_nested_inline_all_of_parents() {
    // An inline `allOf` parent that itself has an inline `allOf`
    // parent. Both inline parents and their field types must be
    // discovered — inline types never get their own codegen pass,
    // so the root schema is responsible for the full chain.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Child:
              allOf:
                - allOf:
                    - type: object
                      properties:
                        grandparent_field:
                          type: object
                          properties:
                            deep:
                              type: string
                  properties:
                    parent_field:
                      type: string
                - type: object
                  properties:
                    own_field:
                      type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let child = graph.schemas().find(|s| s.name() == "Child").unwrap();
    let inlines = child.inlines().collect_vec();

    // The grandparent inline struct (with `grandparent_field`) must
    // be discovered, along with its `grandparent_field` inline struct.
    let has_grandparent_field_struct = inlines.iter().any(|i| {
        matches!(i, InlineTypeView::Struct(path, _)
            if path.segments.iter().any(|s| matches!(s,
                InlineTypePathSegment::Field(StructFieldName::Name("grandparent_field")))))
    });
    assert!(
        has_grandparent_field_struct,
        "expected `Child.inlines()` to discover the grandparent's \
         inline field struct; got {inlines:?}"
    );
}

// MARK: Tagged union variant views

#[test]
fn test_tagged_variant_names_and_aliases() {
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let variant_names = tagged_view.variants().map(|v| v.name()).collect_vec();
    assert_matches!(&*variant_names, ["Cat", "Dog"]);

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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let variant = tagged_view.variants().next().unwrap();
    let ty = variant.ty();

    // Verify the type is accessible and is a schema reference to `Cat`.
    assert_matches!(ty, TypeView::Schema(view) if view.name() == "Cat");
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let untagged_view = match animal_schema {
        SchemaTypeView::Untagged(_, view) => view,
        _ => panic!("`Animal` should be an untagged union"),
    };

    // Untagged variants contain `Cat` and `Dog` schema references.
    assert_eq!(untagged_view.variants().count(), 2);
}

// MARK: Container views

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
              required: [items]
              properties:
                items:
                  type: array
                  items:
                    type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let items_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("items")))
        .unwrap();

    // Verify the array's inner type is accessible,
    // and is a string primitive.
    assert_matches!(
        items_field.ty(),
        TypeView::Inline(InlineTypeView::Container(_, ContainerView::Array(inner)))
            if matches!(
                inner.ty(),
                TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String,
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
              required: [map_field]
              properties:
                map_field:
                  type: object
                  additionalProperties:
                    type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let map_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("map_field")))
        .unwrap();

    // Verify the map's inner type is accessible,
    // and is a string primitive.
    assert_matches!(
        map_field.ty(),
        TypeView::Inline(InlineTypeView::Container(_, ContainerView::Map(inner)))
            if matches!(
                inner.ty(),
                TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("nullable_field")))
        .unwrap();

    // Verify the optional's inner type is accessible, and is an inline struct.
    assert_matches!(
        nullable_field.ty(),
        TypeView::Inline(InlineTypeView::Container(_, ContainerView::Optional(inner)))
            if matches!(
                inner.ty(),
                TypeView::Inline(InlineTypeView::Struct(_, _)),
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
              required: [inline_obj]
              properties:
                inline_obj:
                  type: object
                  properties:
                    nested_field:
                      type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let inline_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("inline_obj")))
        .unwrap();

    let field_ty = inline_field.ty();
    let inline_view = match field_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // Should be able to match on the `Struct` variant.
    assert_matches!(inline_view, InlineTypeView::Struct(_, _));

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
              required: [status]
              properties:
                status:
                  type: string
                  enum: [active, inactive, pending]
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let status_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("status")))
        .unwrap();

    let field_ty = status_field.ty();
    let inline_view = match field_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline enum; got {other:?}"),
    };

    // Should construct an `Enum` variant.
    let enum_view = match inline_view {
        InlineTypeView::Enum(_, view) => view,
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
              required: [value]
              properties:
                value:
                  oneOf:
                    - type: string
                    - type: integer
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let value_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("value")))
        .unwrap();

    let field_ty = value_field.ty();
    let inline_view = match field_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline untagged union; got {other:?}"),
    };

    // Should construct an `Untagged` variant.
    let untagged_view = match inline_view {
        InlineTypeView::Untagged(_, view) => view,
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
              required: [nested]
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let parent_schema = graph.schemas().next().unwrap();
    let parent_struct = match parent_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Parent`; got {other:?}"),
    };

    let nested_field = parent_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("nested")))
        .unwrap();

    let nested_ty = nested_field.ty();
    let nested_inline = match nested_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // `path()` should return a path with one segment.
    let path = nested_inline.path();
    assert_matches!(
        path.segments,
        [InlineTypePathSegment::Field(StructFieldName::Name(
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
                      required: [status]
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    let request = operation.request().unwrap();

    let request_ty = match request {
        RequestView::Json(ty) => ty,
        other => panic!("expected JSON request; got `{other:?}`"),
    };

    let request_struct = match request_ty {
        TypeView::Inline(InlineTypeView::Struct(_, view)) => view,
        other => panic!("expected inline struct; got {other:?}"),
    };

    let status_field = request_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("status")))
        .unwrap();

    let status_ty = status_field.ty();
    let inline_enum = match status_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    // The inline enum has no `x-resourceId`, and the only operation that uses it
    // has no `x-resource-name`, so it contributes `None` to `used_by`.
    let used_by = inline_enum.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*used_by, [None]);

    // `inlines()` excludes the starting node.
    assert_eq!(inline_enum.inlines().count(), 0);

    // `dependencies()` should be empty for an inline enum.
    assert_eq!(inline_enum.dependencies().count(), 0);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let untagged_view = match animal_schema {
        SchemaTypeView::Untagged(_, view) => view,
        other => panic!("expected untagged union `Animal`; got {other:?}"),
    };

    let variants = untagged_view.variants().collect_vec();
    assert_eq!(variants.len(), 3);

    // The first two variants should be schema references.
    let cat_variant = &variants[0];
    assert_matches!(
        cat_variant.ty(),
        Some(SomeUntaggedVariant {
            view: TypeView::Schema(view),
            ..
        }) if view.name() == "Cat",
    );

    let dog_variant = &variants[1];
    assert_matches!(
        dog_variant.ty(),
        Some(SomeUntaggedVariant {
            view: TypeView::Schema(view),
            ..
        }) if view.name() == "Dog",
    );

    // The third variant should be `null`, returning `None`.
    let null_variant = &variants[2];
    assert!(null_variant.ty().is_none());
}

#[test]
fn test_null_variant_demotes_tagged_to_untagged() {
    // A `oneOf` with a discriminator that includes `{type: "null"}`
    // falls through `try_tagged` (which rejects inline schemas) and
    // becomes an untagged union. The null variant is `Unit`.
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
                kind:
                  type: string
                meow:
                  type: string
            Dog:
              type: object
              properties:
                kind:
                  type: string
                bark:
                  type: string
            Animal:
              oneOf:
                - $ref: '#/components/schemas/Cat'
                - $ref: '#/components/schemas/Dog'
                - type: 'null'
              discriminator:
                propertyName: kind
                mapping:
                  cat: '#/components/schemas/Cat'
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // The discriminator is ignored because `{type: "null"}` is an
    // inline schema, causing `try_tagged` to bail.
    let animal = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let SchemaTypeView::Untagged(_, untagged) = animal else {
        panic!("expected untagged `Animal`; got `{animal:?}`");
    };

    let variants = untagged.variants().collect_vec();
    assert_eq!(variants.len(), 3);

    assert_matches!(
        variants[0].ty(),
        Some(SomeUntaggedVariant {
            view: TypeView::Schema(view),
            ..
        }) if view.name() == "Cat",
    );
    assert_matches!(
        variants[1].ty(),
        Some(SomeUntaggedVariant {
            view: TypeView::Schema(view),
            ..
        }) if view.name() == "Dog",
    );
    assert!(variants[2].ty().is_none());
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match status_schema {
        SchemaTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Status`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    assert_matches!(
        variants,
        [
            EnumVariant::String("active"),
            EnumVariant::String("inactive"),
            EnumVariant::String("pending"),
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let priority_schema = graph.schemas().find(|s| s.name() == "Priority").unwrap();
    let enum_view = match priority_schema {
        SchemaTypeView::Enum(_, view) => view,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match status_schema {
        SchemaTypeView::Enum(_, view) => view,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let priority_schema = graph.schemas().find(|s| s.name() == "Priority").unwrap();
    let enum_view = match priority_schema {
        SchemaTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Priority`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    let [
        EnumVariant::I64(n1),
        EnumVariant::I64(n2),
        EnumVariant::I64(n3),
    ] = variants
    else {
        panic!("expected 3 variants; got {variants:?}");
    };
    assert_eq!(*n1, 1);
    assert_eq!(*n2, 2);
    assert_eq!(*n3, 3);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let toggle_schema = graph.schemas().find(|s| s.name() == "Toggle").unwrap();
    let enum_view = match toggle_schema {
        SchemaTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Toggle`; got {other:?}"),
    };
    let variants = enum_view.variants();

    // Verify the actual variant values.
    let &[EnumVariant::Bool(b1), EnumVariant::Bool(b2)] = variants else {
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match &status_schema {
        SchemaTypeView::Enum(_, view) => view,
        other => panic!("expected enum `Status`; got {other:?}"),
    };

    // `Status` has no `x-resourceId`, and the only operation that uses it
    // has no `x-resource-name`, so it contributes `None` to `used_by`.
    assert_eq!(status_schema.resource(), None);
    let used_by = enum_view.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*used_by, [None]);

    // Enums can't contain inline types, so `inlines()` should be empty.
    assert_eq!(enum_view.inlines().count(), 0);

    // `dependencies()` should be empty for an enum.
    assert_eq!(enum_view.dependencies().count(), 0);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    assert_eq!(operation.resource(), Some("UserResource"));
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // Should find (1) the inline request body struct; (2) the optional for the `profile` field;
    // (3) the optional for the `metadata` field; (4) the inline `metadata` struct;
    // (5) the optional for `tags`; (6) the array for `tags`; and (7) the `string`
    // primitive for `tags` items.
    //
    // `Profile` is a schema reference, and should be excluded.
    assert_eq!(operation.inlines().count(), 7);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // String path parameter.
    let path_param = operation.path().params().next().unwrap();
    assert_matches!(
        path_param.ty(),
        TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String,
    );

    // Array-of-strings query parameter.
    let query_param = operation.query().next().unwrap();
    assert_matches!(
        query_param.ty(),
        TypeView::Inline(InlineTypeView::Container(_, ContainerView::Array(inner)))
            if matches!(
                inner.ty(),
                TypeView::Inline(InlineTypeView::Primitive(_, p)) if p.ty() == PrimitiveType::String,
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    let query_params = operation.query().collect_vec();

    // Non-exploded `form` style.
    let tags = query_params.iter().find(|p| p.name() == "tags").unwrap();
    assert_matches!(tags.style(), Some(ParameterStyle::Form { exploded: false }),);

    // `pipeDelimited` style.
    let filters = query_params.iter().find(|p| p.name() == "filters").unwrap();
    assert_matches!(filters.style(), Some(ParameterStyle::PipeDelimited));

    // `spaceDelimited` style.
    let space = query_params
        .iter()
        .find(|p| p.name() == "space_separated")
        .unwrap();
    assert_matches!(space.style(), Some(ParameterStyle::SpaceDelimited));

    // Exploded `form` style, explicitly specified.
    let form_exploded = query_params
        .iter()
        .find(|p| p.name() == "form_exploded")
        .unwrap();
    assert_matches!(
        form_exploded.style(),
        Some(ParameterStyle::Form { exploded: true }),
    );

    // `deepObject` style.
    let deep_obj = query_params
        .iter()
        .find(|p| p.name() == "deep_obj")
        .unwrap();
    assert_matches!(deep_obj.style(), Some(ParameterStyle::DeepObject));

    // No explicit style; `None` defers to the serializer's default.
    let no_style = query_params
        .iter()
        .find(|p| p.name() == "no_style")
        .unwrap();
    assert_matches!(no_style.style(), None);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    assert_matches!(
        operation.request(),
        Some(RequestView::Json(TypeView::Inline(_))),
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    assert_matches!(operation.request(), Some(RequestView::Multipart));
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();
    let segments = operation.path().segments().as_slice();
    let [a, b] = segments else {
        panic!("expected two path segments; got {segments:?}");
    };
    assert_matches!(a.fragments(), [PathFragment::Literal("users")],);
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // Empty response schema becomes `Any`.
    assert_matches!(
        operation.response(),
        Some(ResponseView::Json(TypeView::Inline(InlineTypeView::Any(
            ..
        ))))
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // `createUser` references 10 inline types: (1) the request body struct,
    // (2) optional `name`, (3) `name` string primitive, (4) optional `address`,
    // (5) inline address struct, (6) optional `street`, (7) `street` string
    // primitive, (8) response body struct, (9) optional `id`, (10) `id` string
    // primitive.
    let inlines = operation.inlines().collect_vec();
    assert_eq!(inlines.len(), 10);

    let address = inlines
        .iter()
        .find(|inline| {
            matches!(inline, InlineTypeView::Struct(path, _) if matches!(
                path.segments,
                [
                    InlineTypePathSegment::Operation("createUser"),
                    InlineTypePathSegment::Request,
                    InlineTypePathSegment::Field(StructFieldName::Name("address")),
                ],
            ))
        })
        .unwrap();
    assert_matches!(address, InlineTypeView::Struct(_, _));

    let request = inlines
        .iter()
        .find(|inline| {
            matches!(inline, InlineTypeView::Struct(path, _) if matches!(
                path.segments,
                [
                    InlineTypePathSegment::Operation("createUser"),
                    InlineTypePathSegment::Request,
                ],
            ))
        })
        .unwrap();
    assert_matches!(request, InlineTypeView::Struct(_, _));

    let response = inlines
        .iter()
        .find(|inline| {
            matches!(inline, InlineTypeView::Struct(path, _) if matches!(
                path.segments,
                [
                    InlineTypePathSegment::Operation("createUser"),
                    InlineTypePathSegment::Response,
                ],
            ))
        })
        .unwrap();
    assert_matches!(response, InlineTypeView::Struct(_, _));
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    let Some(RequestView::Json(TypeView::Inline(request))) = operation.request() else {
        panic!(
            "expected inline request schema; got {:?}",
            operation.request(),
        );
    };
    assert_matches!(request.path().root, InlineTypePathRoot::Resource(None));
    assert_matches!(
        request.path().segments,
        [
            InlineTypePathSegment::Operation("createUser"),
            InlineTypePathSegment::Request,
        ],
    );

    let Some(ResponseView::Json(TypeView::Inline(response))) = operation.response() else {
        panic!(
            "expected inline response schema; got {:?}",
            operation.response(),
        );
    };
    assert_matches!(response.path().root, InlineTypePathRoot::Resource(None));
    assert_matches!(
        response.path().segments,
        [
            InlineTypePathSegment::Operation("createUser"),
            InlineTypePathSegment::Response,
        ],
    );
}

// MARK: Parameter views

#[test]
fn test_parameter_inlines_finds_inline_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0
        paths:
          /users/{id}:
            get:
              operationId: getUser
              x-resource-name: user
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
                - name: filter
                  in: query
                  schema:
                    type: object
                    properties:
                      status:
                        type: string
                      nested:
                        type: object
                        properties:
                          depth:
                            type: integer
              responses:
                '200':
                  description: OK
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // A primitive path parameter has a single inline: the primitive itself.
    let id_param = operation.path().params().next().unwrap();
    let id_inlines = id_param.inlines().collect_vec();
    assert_eq!(id_inlines.len(), 1);
    assert_matches!(id_inlines[0], InlineTypeView::Primitive(_, _));

    // The `filter` query parameter has an inline object with nested inlines:
    // (1) `filter` struct, (2) optional `status`, (3) `status` string primitive,
    // (4) optional `nested`, (5) inline `nested` struct, (6) optional `depth`,
    // (7) `depth` integer primitive.
    let filter_param = operation.query().find(|p| p.name() == "filter").unwrap();
    let filter_inlines = filter_param.inlines().collect_vec();
    assert_eq!(filter_inlines.len(), 7);

    // The root inline is the `filter` struct itself.
    let root = filter_inlines
        .iter()
        .filter_map(|inline| match inline {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .next()
        .unwrap();
    assert_matches!(
        root,
        InlineTypePath {
            root: InlineTypePathRoot::Resource(Some("user")),
            segments: [
                InlineTypePathSegment::Operation("getUser"),
                InlineTypePathSegment::Parameter("filter"),
            ],
        },
    );

    // The nested struct carries the full path.
    let nested = filter_inlines
        .iter()
        .filter_map(|inline| match inline {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .skip(1)
        .exactly_one()
        .unwrap();
    assert_matches!(
        nested,
        InlineTypePath {
            root: InlineTypePathRoot::Resource(Some("user")),
            segments: [
                InlineTypePathSegment::Operation("getUser"),
                InlineTypePathSegment::Parameter("filter"),
                InlineTypePathSegment::Field(StructFieldName::Name("nested")),
            ],
        },
    );
}

#[test]
fn test_parameter_inlines_empty_for_ref() {
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
                - name: sort
                  in: query
                  schema:
                    $ref: '#/components/schemas/SortOrder'
              responses:
                '200':
                  description: OK
        components:
          schemas:
            SortOrder:
              type: string
              enum:
                - asc
                - desc
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // A parameter referencing a named schema has no inlines.
    let sort_param = operation.query().next().unwrap();
    assert_eq!(sort_param.inlines().count(), 0);
}

#[test]
fn test_parameter_inlines_empty_for_ref_with_nested_inlines() {
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
                - name: filter
                  in: query
                  schema:
                    $ref: '#/components/schemas/Filter'
              responses:
                '200':
                  description: OK
        components:
          schemas:
            Filter:
              type: object
              properties:
                status:
                  type: string
                nested:
                  type: object
                  properties:
                    depth:
                      type: integer
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let operation = graph.operations().next().unwrap();

    // A parameter referencing a named schema has no inlines, even
    // when the schema itself contains inline types. Those inlines
    // belong to the schema, not the parameter.
    let filter_param = operation.query().next().unwrap();
    assert_eq!(filter_param.inlines().count(), 0);
}

// MARK: Discriminator fields

#[test]
fn test_variant_field_matching_tagged_union_tag_is_tag() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Post:
              oneOf:
                - $ref: '#/components/schemas/Comment'
                - $ref: '#/components/schemas/Reaction'
              discriminator:
                propertyName: kind
                mapping:
                  comment: '#/components/schemas/Comment'
                  reaction: '#/components/schemas/Reaction'
            Comment:
              type: object
              required: [kind, id, text]
              properties:
                kind:
                  type: string
                id:
                  type: string
                text:
                  type: string
            Reaction:
              type: object
              required: [kind, id, emoji]
              properties:
                kind:
                  type: string
                id:
                  type: string
                emoji:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `Comment.kind` should be detected as a tag field because
    // `Comment` is a direct variant of the `Post` tagged union.
    let comment = graph.schemas().find(|s| s.name() == "Comment").unwrap();
    let SchemaTypeView::Struct(_, comment_struct) = comment else {
        panic!("expected struct `Comment`; got `{comment:?}`");
    };
    let kind_field = comment_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.tag());

    // Other fields on `Comment` should not be tags.
    let id_field = comment_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("id")))
        .unwrap();
    assert!(!id_field.tag());

    // `Reaction.kind` should also be detected as a tag field.
    let reaction = graph.schemas().find(|s| s.name() == "Reaction").unwrap();
    let SchemaTypeView::Struct(_, reaction_struct) = reaction else {
        panic!("expected struct `Reaction`; got `{reaction:?}`");
    };
    let kind_field = reaction_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.tag());
}

#[test]
fn test_transitive_dependency_field_matching_tag_is_not_tag() {
    // `Inner` has a `kind` field that matches the `Outer` tagged union's
    // tag, but `Inner` is _not_ a direct variant of `Outer`; only
    // `Wrapper` is. The `kind` field on `Inner` should _not_ be treated
    // as a tag field.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Outer:
              oneOf:
                - $ref: '#/components/schemas/Wrapper'
              discriminator:
                propertyName: kind
                mapping:
                  wrapper: '#/components/schemas/Wrapper'
            Wrapper:
              type: object
              required: [kind, data]
              properties:
                kind:
                  type: string
                data:
                  $ref: '#/components/schemas/Inner'
            Inner:
              type: object
              properties:
                kind:
                  type: string
                value:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `Wrapper.kind` _is_ a tag field, because `Wrapper` is a
    // direct variant of `Outer`.
    let wrapper = graph.schemas().find(|s| s.name() == "Wrapper").unwrap();
    let SchemaTypeView::Struct(_, wrapper_struct) = wrapper else {
        panic!("expected struct `Wrapper`; got `{wrapper:?}`");
    };
    let kind_field = wrapper_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.tag());

    // `Inner.kind` is _not_ a tag field, because `Inner` is only
    // transitively reachable from `Outer`, not a direct variant.
    let inner = graph.schemas().find(|s| s.name() == "Inner").unwrap();
    let SchemaTypeView::Struct(_, inner_struct) = inner else {
        panic!("expected struct `Inner`; got `{inner:?}`");
    };
    let kind_field = inner_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.tag());
}

#[test]
fn test_own_struct_tag_field() {
    // A struct used only inside a tagged union whose tag matches a field
    // should mark that field as a tag.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Base:
              type: object
              properties:
                kind:
                  type: string
                name:
                  type: string
              discriminator:
                propertyName: kind
            Container:
              oneOf:
                - $ref: '#/components/schemas/Base'
              discriminator:
                propertyName: kind
                mapping:
                  base: '#/components/schemas/Base'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let base = graph.schemas().find(|s| s.name() == "Base").unwrap();
    let SchemaTypeView::Struct(_, base_struct) = base else {
        panic!("expected struct `Base`; got `{base:?}`");
    };

    // The `kind` field should be marked as a tag field.
    let kind_field = base_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.tag());

    // The `name` field should not be a tag field.
    let name_field = base_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("name")))
        .unwrap();
    assert!(!name_field.tag());
}

#[test]
fn test_inherited_tag_field() {
    // A child struct that inherits a field matching the tag of an incoming
    // tagged union should mark that inherited field as a tag.
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
                kind:
                  type: string
              discriminator:
                propertyName: kind
            Child:
              allOf:
                - $ref: '#/components/schemas/Parent'
              properties:
                name:
                  type: string
            Container:
              oneOf:
                - $ref: '#/components/schemas/Child'
              discriminator:
                propertyName: kind
                mapping:
                  child: '#/components/schemas/Child'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let child = graph.schemas().find(|s| s.name() == "Child").unwrap();
    let SchemaTypeView::Struct(_, child_struct) = child else {
        panic!("expected struct `Child`; got `{child:?}`");
    };

    // The child's inherited `kind` field should be marked as a tag.
    let kind_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.tag());
    assert!(kind_field.inherited());

    // The child's own `name` field should not be a tag.
    let name_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("name")))
        .unwrap();
    assert!(!name_field.tag());
    assert!(!name_field.inherited());
}

#[test]
fn test_fields_linearizes_inline_all_of_parents() {
    // Inline `allOf` schemas become parent types, and their fields should be
    // linearized into the child struct via `fields()`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Person:
              allOf:
                - type: object
                  properties:
                    name:
                      type: string
                - type: object
                  properties:
                    age:
                      type: integer
              properties:
                email:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let person = graph.schemas().find(|s| s.name() == "Person").unwrap();
    let SchemaTypeView::Struct(_, person_struct) = person else {
        panic!("expected struct `Person`; got `{person:?}`");
    };

    // `fields()` should return all fields in declaration order: inherited
    // fields from the first parent, then the second parent, then own fields.
    let field_names = person_struct
        .fields()
        .filter_map(|f| match f.name() {
            StructFieldName::Name(n) => Some(n),
            _ => None,
        })
        .collect_vec();
    assert_eq!(field_names, vec!["name", "age", "email"]);

    // Fields from inline parents should be marked as inherited.
    let name_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("name")))
        .unwrap();
    assert!(name_field.inherited());

    let age_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("age")))
        .unwrap();
    assert!(age_field.inherited());

    // Own field should not be inherited.
    let email_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("email")))
        .unwrap();
    assert!(!email_field.inherited());
}

#[test]
fn test_fields_linearizes_diamond_inheritance() {
    // Two parents sharing a common ancestor creates a diamond:
    //
    //       Base
    //       /  \
    //     P1    P2
    //       \  /
    //      Child
    //
    // `Base`'s fields must appear exactly once, then `P1`'s and `P2`'s
    // own fields, then `Child`'s own fields; all in declaration order.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Base:
              type: object
              properties:
                id:
                  type: string
                created_at:
                  type: string
            Parent1:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                p1_alpha:
                  type: string
                p1_beta:
                  type: integer
            Parent2:
              allOf:
                - $ref: '#/components/schemas/Base'
              properties:
                p2_alpha:
                  type: string
                p2_beta:
                  type: integer
            Child:
              allOf:
                - $ref: '#/components/schemas/Parent1'
                - $ref: '#/components/schemas/Parent2'
              properties:
                own_first:
                  type: string
                own_second:
                  type: boolean
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let child = graph.schemas().find(|s| s.name() == "Child").unwrap();
    let SchemaTypeView::Struct(_, child_struct) = child else {
        panic!("expected struct `Child`; got `{child:?}`");
    };

    let field_names = child_struct
        .fields()
        .filter_map(|f| match f.name() {
            StructFieldName::Name(n) => Some(n),
            _ => None,
        })
        .collect_vec();
    assert_matches!(
        &*field_names,
        [
            "id",
            "created_at",
            "p1_alpha",
            "p1_beta",
            "p2_alpha",
            "p2_beta",
            "own_first",
            "own_second",
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
              required: [animal]
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let animal_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("animal")))
        .unwrap();

    let field_ty = animal_field.ty();
    let inline_view = match field_ty {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Should construct a `Tagged` variant.
    let tagged_view = match inline_view {
        InlineTypeView::Tagged(_, view) => view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Verify the tag property.
    assert_eq!(tagged_view.tag(), "kind");

    // Verify the variants.
    let variant_names = tagged_view.variants().map(|v| v.name()).collect_vec();
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
              required: [animal]
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let animal_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("animal")))
        .unwrap();

    let inline_view = match animal_field.ty() {
        TypeView::Inline(inline_view) => inline_view,
        other => panic!("expected inline type; got {other:?}"),
    };

    let tagged_view = match inline_view {
        InlineTypeView::Tagged(_, view) => view,
        other => panic!("expected inline tagged union; got {other:?}"),
    };

    // Verify the variant type is accessible.
    let variant = tagged_view.variants().next().unwrap();
    assert_eq!(variant.name(), "Cat");
    assert_matches!(variant.ty(), TypeView::Schema(view) if view.name() == "Cat");
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

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();

    // Should find the optional for `animal` and the inline tagged union.
    let inlines = container_schema.inlines().collect_vec();
    assert_matches!(
        &*inlines,
        [
            InlineTypeView::Container(_, _),
            InlineTypeView::Tagged(_, _)
        ]
    );
}

// MARK: `hashable()` and `defaultable()`

#[test]
fn test_struct_not_hashable_when_inherited_field_type_inherits_float() {
    // `S` inherits from tagged union `U`, which has common field `data: T`.
    // `T` has no own fields, but inherits from `Parent`, which has an
    // `f64` field. `S` can't be hashable because it inherits an `f64`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Parent:
              type: object
              properties:
                val:
                  type: number
                  format: double
              required:
                - val
            T:
              type: object
              allOf:
                - $ref: '#/components/schemas/Parent'
            S:
              type: object
              allOf:
                - $ref: '#/components/schemas/U'
              properties:
                name:
                  type: string
              required:
                - name
            OtherVariant:
              type: object
              properties:
                label:
                  type: string
              required:
                - label
            U:
              oneOf:
                - $ref: '#/components/schemas/S'
                - $ref: '#/components/schemas/OtherVariant'
              discriminator:
                propertyName: type
              properties:
                data:
                  $ref: '#/components/schemas/T'
              required:
                - type
                - data
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `s.data` reaches an `f64` through `Parent`, so `s` is unhashable.
    let s = graph.schemas().find(|s| s.name() == "S").unwrap();
    assert!(!s.hashable());
}

#[test]
fn test_struct_not_defaultable_when_inherited_field_type_inherits_non_defaultable() {
    // Same inheritance chain as the hashable test above, but for `Default`:
    // `S` inherits from tagged union `U`, which has required common field
    // `data: T`. `T` has no own fields, but inherits from `Parent`, which has
    // a required tagged union field. `S` can't be defaultable.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Kind:
              oneOf:
                - $ref: '#/components/schemas/KindA'
                - $ref: '#/components/schemas/KindB'
              discriminator:
                propertyName: type
            KindA:
              type: object
              allOf:
                - $ref: '#/components/schemas/Kind'
              properties:
                a:
                  type: string
            KindB:
              type: object
              allOf:
                - $ref: '#/components/schemas/Kind'
              properties:
                b:
                  type: string
            Parent:
              type: object
              properties:
                kind:
                  $ref: '#/components/schemas/Kind'
              required:
                - kind
            T:
              type: object
              allOf:
                - $ref: '#/components/schemas/Parent'
            S:
              type: object
              allOf:
                - $ref: '#/components/schemas/U'
              properties:
                name:
                  type: string
              required:
                - name
            OtherVariant:
              type: object
              properties:
                label:
                  type: string
              required:
                - label
            U:
              oneOf:
                - $ref: '#/components/schemas/S'
                - $ref: '#/components/schemas/OtherVariant'
              discriminator:
                propertyName: type
              properties:
                data:
                  $ref: '#/components/schemas/T'
              required:
                - type
                - data
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    // `s.data` reaches tagged union `Kind` through `Parent`,
    // so `s` is undefaultable.
    let s = graph.schemas().find(|s| s.name() == "S").unwrap();
    assert!(!s.defaultable());
}

#[test]
fn test_struct_not_hashable_when_own_field_type_inherits_float() {
    // `X` has own field `t: T`. `T` inherits from `Parent`, which has
    // a required `f64` field. `X` should not be hashable because its
    // own-field chain crosses an `Inherits` edge to reach `f64`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Parent:
              type: object
              properties:
                val:
                  type: number
                  format: double
              required:
                - val
            T:
              type: object
              allOf:
                - $ref: '#/components/schemas/Parent'
            X:
              type: object
              properties:
                t:
                  $ref: '#/components/schemas/T'
              required:
                - t
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(!x.hashable());
    // `T` itself is also not hashable.
    let t = graph.schemas().find(|s| s.name() == "T").unwrap();
    assert!(!t.hashable());
}

#[test]
fn test_struct_not_defaultable_when_own_field_type_inherits_non_defaultable() {
    // `X` has required field `t: T`. `T` inherits from `Parent`, which
    // has a required tagged union field. `X` should not be defaultable.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Kind:
              oneOf:
                - $ref: '#/components/schemas/KindA'
                - $ref: '#/components/schemas/KindB'
              discriminator:
                propertyName: type
            KindA:
              type: object
              allOf:
                - $ref: '#/components/schemas/Kind'
              properties:
                a:
                  type: string
            KindB:
              type: object
              allOf:
                - $ref: '#/components/schemas/Kind'
              properties:
                b:
                  type: string
            Parent:
              type: object
              properties:
                kind:
                  $ref: '#/components/schemas/Kind'
              required:
                - kind
            T:
              type: object
              allOf:
                - $ref: '#/components/schemas/Parent'
            X:
              type: object
              properties:
                t:
                  $ref: '#/components/schemas/T'
              required:
                - t
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(!x.defaultable());
}

#[test]
fn test_struct_not_hashable_when_own_field_is_float() {
    // Direct own-field case: `X { f: f64 }` → not hashable.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            X:
              type: object
              properties:
                f:
                  type: number
                  format: double
              required:
                - f
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(!x.hashable());
}

#[test]
fn test_struct_not_hashable_when_container_field_holds_float() {
    // `X { f: Array<f64> }` — the `Contains` edge path to `f64`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            X:
              type: object
              properties:
                f:
                  type: array
                  items:
                    type: number
                    format: double
              required:
                - f
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(!x.hashable());
}

#[test]
fn test_struct_not_hashable_when_union_field_has_unhashable_variant() {
    // `X { u: U }` where `U` is a tagged union with variant `V` that has
    // an `f64` field. `X` depends on the full union type, so it is not
    // hashable.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            V:
              type: object
              allOf:
                - $ref: '#/components/schemas/U'
              properties:
                score:
                  type: number
                  format: double
              required:
                - score
            W:
              type: object
              allOf:
                - $ref: '#/components/schemas/U'
              properties:
                label:
                  type: string
              required:
                - label
            U:
              oneOf:
                - $ref: '#/components/schemas/V'
                - $ref: '#/components/schemas/W'
              discriminator:
                propertyName: type
            X:
              type: object
              properties:
                u:
                  $ref: '#/components/schemas/U'
              required:
                - u
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(!x.hashable());
}

#[test]
fn test_struct_defaultable_when_all_fields_optional() {
    // All fields optional → `AbsentOr<T>` wrapping, which is always
    // `Default`. Validates the `Optional` → `AbsentOr` interaction.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            X:
              type: object
              properties:
                name:
                  type: string
                count:
                  type: integer
                  format: int32
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let x = graph.schemas().find(|s| s.name() == "X").unwrap();
    assert!(x.defaultable());
}

#[test]
fn test_recursive_struct_is_hashable() {
    // A self-referential type with no `f64` in its closure is hashable.
    // Validates that the same-SCC skip in Pass 3 does not incorrectly
    // taint recursive types.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  $ref: '#/components/schemas/Node'
              required:
                - value
                - next
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let node = graph.schemas().find(|s| s.name() == "Node").unwrap();
    assert!(node.hashable());
}

#[test]
fn test_recursive_struct_defaultable_when_self_reference_optional() {
    // A self-referential type where the recursive field is optional.
    // `AbsentOr<Box<Node>>` is always `Default` (defaults to absent),
    // so the recursion is broken and the type is defaultable.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  $ref: '#/components/schemas/Node'
              required:
                - value
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let node = graph.schemas().find(|s| s.name() == "Node").unwrap();
    assert!(node.defaultable());
}

#[test]
fn test_recursive_struct_defaultable_when_self_reference_required() {
    // A self-referential type where the recursive field is required.
    // `Box<T>` implements `Default` when `T: Default`, so the derive
    // is valid.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  $ref: '#/components/schemas/Node'
              required:
                - value
                - next
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let node = graph.schemas().find(|s| s.name() == "Node").unwrap();
    assert!(node.defaultable());
}

#[test]
fn test_struct_hashable_when_field_and_inheritance_form_cycle() {
    // `A.child: B`, where `B` inherits from `A`, creates a
    // `Field` + `Inherits` cycle in the graph. Neither type has
    // a floating-point field, so both should be hashable.
    // This verifies that the ancestors closure and same-SCC skip
    // handle mixed-edge SCCs.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            A:
              type: object
              properties:
                child:
                  $ref: '#/components/schemas/B'
              required:
                - child
            B:
              type: object
              allOf:
                - $ref: '#/components/schemas/A'
              properties:
                name:
                  type: string
              required:
                - name
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schemas().find(|s| s.name() == "A").unwrap();
    let b = graph.schemas().find(|s| s.name() == "B").unwrap();
    assert!(a.hashable());
    assert!(b.hashable());
    assert!(a.defaultable());
    assert!(b.defaultable());
}

#[test]
fn test_struct_not_hashable_when_field_and_inheritance_cycle_reaches_float() {
    // Same `Field` + `Inherits`, but `A` now has an `f64` field.
    // Both types should be unhashable: `A` directly, and `B` via
    // the inherited field `val`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            A:
              type: object
              properties:
                child:
                  $ref: '#/components/schemas/B'
                val:
                  type: number
                  format: double
              required:
                - child
                - val
            B:
              type: object
              allOf:
                - $ref: '#/components/schemas/A'
              properties:
                name:
                  type: string
              required:
                - name
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let a = graph.schemas().find(|s| s.name() == "A").unwrap();
    let b = graph.schemas().find(|s| s.name() == "B").unwrap();
    assert!(!a.hashable());
    assert!(!b.hashable());
}

// MARK: Shadow edge visibility

#[test]
fn test_shadow_edges_hide_inlines_but_preserve_dependencies() {
    // `Dog` has an anonymous object field, `metadata`.
    // `Pet` is a tagged union with `Dog` as a variant, and
    // `Owner` also references `Dog`. `inline_tagged_variants()`
    // creates an inline copy of `Dog` for `Pet`, with
    // shadow edges back to the original.
    //
    // Shadow edges must be invisible to `inlines()`, so that
    // `Pet/Dog` doesn't claim `Dog`'s inline types, but visible to
    // `dependencies()`, so that `Pet/Dog` still transitively depends on
    // everything that `Dog` depends on.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Dog:
              type: object
              properties:
                kind:
                  type: string
                metadata:
                  type: object
                  properties:
                    origin:
                      type: string
              required: [kind, metadata]
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // The original `Dog` schema should own the inline `metadata` struct.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let dog_inline_paths = dog
        .inlines()
        .filter_map(|i| match i {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .collect_vec();
    assert_matches!(
        &*dog_inline_paths,
        [InlineTypePath {
            root: InlineTypePathRoot::Type("Dog"),
            segments: [InlineTypePathSegment::Field(StructFieldName::Name(
                "metadata"
            ))],
        }],
    );

    // The inlined variant `Pet/Dog` should _not_ claim `Dog`'s inline types.
    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let SchemaTypeView::Tagged(_, pet_tagged) = pet else {
        panic!("expected tagged `Pet`; got `{pet:?}`");
    };
    let variant = pet_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inlined_dog)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let claimed_inline_structs = inlined_dog
        .inlines()
        .filter_map(|i| match i {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .collect_vec();
    assert_matches!(&*claimed_inline_structs, []);

    // But `dependencies()` should still reach `Dog` and its inlines.
    let mut dep_names = inlined_dog
        .dependencies()
        .filter_map(|view| match view {
            TypeView::Schema(view) => Some(view.name()),
            _ => None,
        })
        .collect_vec();
    dep_names.sort();
    assert_matches!(&*dep_names, ["Dog", "Pet"]);
}

#[test]
fn test_shadow_inherits_hides_ancestor_inlines() {
    // `Dog` inherits from `Animal` via `allOf`. `Animal` has
    // an anonymous object field, `tag`. `Pet` is a tagged union
    // with `Dog` as a variant, and `Owner` also references `Dog`.
    // `inline_tagged_variants()` creates an inline copy of `Dog`
    // for `Pet`, with shadow edges back to the original.
    //
    // The inlined `Pet/Dog` has a shadow inheritance edge back to
    // the original `Dog`, which in turn inherits from `Animal`.
    // `inlines()` must not follow those shadow edges, so `Pet/Dog`
    // doesn't claim `Animal`'s inline types.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Animal:
              type: object
              properties:
                kind:
                  type: string
                tag:
                  type: object
                  properties:
                    label:
                      type: string
              required: [kind, tag]
            Dog:
              allOf:
                - $ref: '#/components/schemas/Animal'
              properties:
                bark:
                  type: string
              required: [bark]
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // `Animal` should own the inline `tag` struct.
    let animal = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let animal_inline_paths = animal
        .inlines()
        .filter_map(|i| match i {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .collect_vec();
    assert_matches!(
        &*animal_inline_paths,
        [InlineTypePath {
            root: InlineTypePathRoot::Type("Animal"),
            segments: [InlineTypePathSegment::Field(StructFieldName::Name("tag"))],
        }],
    );

    // The inlined `Pet/Dog` should _not_ claim `Animal`'s inline types.
    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let SchemaTypeView::Tagged(_, pet_tagged) = pet else {
        panic!("expected tagged `Pet`; got `{pet:?}`");
    };
    let variant = pet_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inlined_dog)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let claimed_inline_structs = inlined_dog
        .inlines()
        .filter_map(|i| match i {
            InlineTypeView::Struct(path, _) => Some(path),
            _ => None,
        })
        .collect_vec();
    assert_matches!(&*claimed_inline_structs, []);

    // But `dependencies()` should still reach `Animal` and its inlines.
    let mut dep_names = inlined_dog
        .dependencies()
        .filter_map(|view| match view {
            TypeView::Schema(view) => Some(view.name()),
            _ => None,
        })
        .collect_vec();
    dep_names.sort();
    assert_matches!(&*dep_names, ["Animal", "Dog", "Pet"]);
}

// MARK: Tag field detection

#[test]
fn test_tag_false_for_inlined_struct() {
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
                bark:
                  type: string
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };

    // `Dog` is inlined (referenced by `Owner.dog`). After inlining,
    // the tagged union no longer references `Dog` directly, so `kind`
    // is not treated as a tag field.
    let kind_field = dog_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.tag());
}

#[test]
fn test_inlined_when_tagged_unions_disagree_on_tag() {
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
                category:
                  type: string
                bark:
                  type: string
            ByKind:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
            ByCategory:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: category
                mapping:
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // `Dog` is inlined because the two tagged unions disagree on
    // their tag. The original struct keeps all fields.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };
    let field_names = dog_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*field_names,
        [
            StructFieldName::Name("kind"),
            StructFieldName::Name("category"),
            StructFieldName::Name("bark"),
        ]
    );

    // Each tagged union should have an inline variant with all
    // fields present, and tag field should be `tag()`.
    let by_kind = graph.schemas().find(|s| s.name() == "ByKind").unwrap();
    let SchemaTypeView::Tagged(_, by_kind_tagged) = by_kind else {
        panic!("expected tagged `ByKind`; got `{by_kind:?}`");
    };
    let variant = by_kind_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            StructFieldName::Name("kind"),
            StructFieldName::Name("category"),
            StructFieldName::Name("bark"),
        ]
    );
    let tags = inline_struct
        .fields()
        .filter(|f| f.tag())
        .map(|f| f.name())
        .collect_vec();
    assert_matches!(&*tags, [StructFieldName::Name("kind")]);

    let by_category = graph.schemas().find(|s| s.name() == "ByCategory").unwrap();
    let SchemaTypeView::Tagged(_, by_category_tagged) = by_category else {
        panic!("expected tagged `ByCategory`; got `{by_category:?}`");
    };
    let variant = by_category_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            StructFieldName::Name("kind"),
            StructFieldName::Name("category"),
            StructFieldName::Name("bark"),
        ]
    );
    let tags = inline_struct
        .fields()
        .filter(|f| f.tag())
        .map(|f| f.name())
        .collect_vec();
    assert_matches!(&*tags, [StructFieldName::Name("category")]);
}

#[test]
fn test_inlined_when_tagged_unions_disagree_on_fields() {
    // When a variant struct appears in multiple tagged unions that
    // share the same tag but have different common fields, the variant
    // must be inlined so that each inline copy inherits just its
    // parent union's fields.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
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
                bark:
                  type: string
            UnionA:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
              properties:
                kind:
                  type: string
                name:
                  type: string
            UnionB:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
              properties:
                kind:
                  type: string
                habitat:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // The original `Dog` keeps all its own fields.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };
    let field_names = dog_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*field_names,
        [StructFieldName::Name("kind"), StructFieldName::Name("bark")]
    );

    // Each inlined `Dog` has an inheritance edge back to its
    // parent tagged union. `Dog`'s own fields come first, then
    // the union's own fields; minus duplicates like `kind`.
    let union_a = graph.schemas().find(|s| s.name() == "UnionA").unwrap();
    let SchemaTypeView::Tagged(_, union_a_tagged) = union_a else {
        panic!("expected tagged `UnionA`; got `{union_a:?}`");
    };
    let variant = union_a_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            // Inherited from `UnionA` (minus `kind` because it's shadowed by own).
            StructFieldName::Name("name"),
            // `Dog`'s own fields.
            StructFieldName::Name("kind"),
            StructFieldName::Name("bark"),
        ]
    );

    let union_b = graph.schemas().find(|s| s.name() == "UnionB").unwrap();
    let SchemaTypeView::Tagged(_, union_b_tagged) = union_b else {
        panic!("expected tagged `UnionB`; got `{union_b:?}`");
    };
    let variant = union_b_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            // Inherited from `UnionB` (minus `kind` because it's shadowed by own).
            StructFieldName::Name("habitat"),
            // `Dog`'s own fields.
            StructFieldName::Name("kind"),
            StructFieldName::Name("bark"),
        ]
    );
}

#[test]
fn test_not_inlined_when_variant_already_inherits_union_fields() {
    // When a variant struct already inherits from the tagged union
    // via `allOf`, the union's fields are already reachable through
    // the existing inheritance edge. Inlining would produce a
    // structurally identical copy, so it should be skipped.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Dog:
              allOf:
                - $ref: '#/components/schemas/Pet'
              properties:
                bark:
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
                name:
                  type: string
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // `Dog` is referenced by `Owner.dog` and inherits from `Pet` via `allOf`,
    // so the inline would be identical. The variant should remain
    // a direct reference to `Dog`, not an inline copy.
    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let tagged = match pet {
        SchemaTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged `Pet`; got {other:?}"),
    };
    let variant = tagged.variants().next().unwrap();
    assert_matches!(variant.ty(), TypeView::Schema(SchemaTypeView::Struct(..)));

    // `Dog` should still have the inherited fields from `Pet`.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };
    let field_names = dog_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*field_names,
        [
            StructFieldName::Name("kind"),
            StructFieldName::Name("name"),
            StructFieldName::Name("bark"),
        ]
    );
}

#[test]
fn test_inlining_preserves_field_type_edges() {
    // `Pet` is a tagged union with an inline enum property (`severity`).
    // `Dog` is both a variant of `Pet` and a property of `Owner`, so it's
    // inlined. After inlining, `Pet`'s inlines must still include the
    // `severity` inline enum; only its edge to `Dog` should have been updated.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Dog:
              type: object
              properties:
                kind:
                  type: string
                bark:
                  type: string
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
              properties:
                severity:
                  type: string
                  enum:
                    - low
                    - high
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // `Dog` is inlined because `Owner.dog` gives it a non-tagged incoming edge.
    // After inlining, `pet.inlines()` must still include the inline enum from
    // `Pet.severity`.
    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let has_inline_enum = pet.inlines().any(|i| matches!(i, InlineTypeView::Enum(..)));
    assert!(has_inline_enum);
}

#[test]
fn test_inlined_variant_inline_field_types_not_leaked() {
    // `Dog` is inlined (referenced by `Owner.dog`) and has an
    // inline field type (`details`). After inlining, `Pet`'s
    // `inlines()` should contain the inline struct variant for
    // `Dog`, but _not_ `Dog`'s inline `Details` type.
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
                details:
                  type: object
                  properties:
                    color:
                      type: string
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let pet_inlines = pet.inlines().collect_vec();

    // Inlines should only include the inline struct variant `Dog`,
    // not `Dog`'s inline field type `Details`.
    let [InlineTypeView::Struct(path, _)] = &*pet_inlines else {
        panic!("expected inline struct variant `Dog`; got `{pet_inlines:?}`");
    };
    assert_matches!(path.root, InlineTypePathRoot::Type("Pet"));
    assert_matches!(path.segments, [InlineTypePathSegment::TaggedVariant("Dog")]);

    // `Dog`'s own `inlines()` still contains its inline types:
    // containers for optional fields, the `Details` struct, etc.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let dog_inlines = dog.inlines().collect_vec();
    assert!(
        dog_inlines
            .iter()
            .any(|i| matches!(i, InlineTypeView::Struct(..))),
        "expected `Dog` to have inline struct `Details`"
    );
    assert!(
        dog_inlines
            .iter()
            .all(|i| i.path().root == InlineTypePathRoot::Type("Dog")),
        "all of `Dog`'s inlines should be rooted at `Dog`"
    );
}

#[test]
fn test_inlined_variant_parents_yields_tagged_union_and_original() {
    // When `Dog` is inlined into `Pet`, the inlined variant struct
    // should have two parents: the tagged union `Pet` (for its
    // common fields) and the original `Dog` schema (for its ancestors).
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
                bark:
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
                name:
                  type: string
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let SchemaTypeView::Tagged(_, pet_tagged) = pet else {
        panic!("expected tagged `Pet`; got `{pet:?}`");
    };
    let variant = pet_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };

    // The inlined variant struct should have both the tagged union
    // and the original variant struct as parents.
    let parents = inline_struct.parents().collect_vec();
    assert_matches!(
        &*parents,
        [
            TypeView::Schema(SchemaTypeView::Tagged(..)),
            TypeView::Schema(SchemaTypeView::Struct(..))
        ]
    );
    let parent_names = parents
        .iter()
        .filter_map(|p| match p {
            TypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();
    assert_matches!(&*parent_names, ["Pet", "Dog"]);
}

#[test]
fn test_tag_false_when_only_operation_prevents_inlining() {
    // `Dog` is referenced by the tagged union `Pet` (with tag `kind`) and by
    // an operation, but not by any other schema. The operation should cause
    // `Dog` to be inlined, because `kind` is only a tag field when `Dog` is
    // used in `Pet`; when it's used in the operation's response body,
    // `kind` is a regular field.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths:
          /dogs:
            get:
              operationId: getDog
              responses:
                '200':
                  description: OK
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/Dog'
        components:
          schemas:
            Dog:
              type: object
              properties:
                kind:
                  type: string
                bark:
                  type: string
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // `Dog` must be inlined because the operation needs the schema
    // struct with `kind` as a regular field.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };

    let kind_field = dog_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(
        !kind_field.tag(),
        "`kind` should not be a tag on the schema struct \
         when an operation references it"
    );
}

#[test]
fn test_inlined_when_struct_field_references_tagged_variant() {
    // `Dog` is both a variant of the `Pet` tagged union _and_ referenced
    // by `Owner.dog` as a regular struct field. Even though `Pet` is the
    // only tagged union using `Dog`, the non-tagged incoming edge from
    // `Owner` means `Dog` must be inlined, so that the schema struct
    // retains the `kind` field.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Dog:
              type: object
              properties:
                kind:
                  type: string
                bark:
                  type: string
              required:
                - kind
                - bark
            Owner:
              type: object
              properties:
                dog:
                  $ref: '#/components/schemas/Dog'
            Pet:
              oneOf:
                - $ref: '#/components/schemas/Dog'
              discriminator:
                propertyName: kind
                mapping:
                  dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    // The schema struct `Dog` should retain `kind` as a regular field,
    // not a tag, because it's referenced by `Owner.dog`.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };
    let kind_field = dog_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(
        !kind_field.tag(),
        "`kind` should not be a tag on the schema struct \
         when a non-tagged schema also references it"
    );

    // The `Pet` tagged union should wrap an inline variant,
    // not the schema struct directly.
    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let SchemaTypeView::Tagged(_, pet_tagged) = pet else {
        panic!("expected tagged `Pet`; got `{pet:?}`");
    };
    let variant = pet_tagged.variants().next().unwrap();
    let TypeView::Inline(InlineTypeView::Struct(path, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    assert_matches!(path.root, InlineTypePathRoot::Type("Pet"));
    assert_matches!(path.segments, [InlineTypePathSegment::TaggedVariant("Dog")]);

    // The inline struct should have the same fields, and `kind`
    // should be a tag there; it's the tag for `Pet`.
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [StructFieldName::Name("kind"), StructFieldName::Name("bark"),]
    );
    let kind_inline = inline_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(
        kind_inline.tag(),
        "`kind` should be a tag on the inlined struct variant"
    );
}

#[test]
fn test_tag_false_for_common_field_target() {
    // `Header` only has an incoming field edge from `Action`,
    // not a variant edge. `Header.kind` collides with `Action`'s
    // discriminator, but shouldn't be treated as a tag.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Action:
              oneOf:
                - $ref: '#/components/schemas/TextAction'
                - $ref: '#/components/schemas/MetricAction'
              discriminator:
                propertyName: kind
                mapping:
                  text: '#/components/schemas/TextAction'
                  metric: '#/components/schemas/MetricAction'
              properties:
                header:
                  $ref: '#/components/schemas/Header'
              required: [kind, header]
            TextAction:
              type: object
              properties:
                label:
                  type: string
              required: [label]
            MetricAction:
              type: object
              properties:
                value:
                  type: number
              required: [value]
            Header:
              type: object
              properties:
                kind:
                  type: string
                id:
                  type: string
              required: [kind, id]
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let mut raw = RawGraph::new(&arena, &spec);
    raw.inline_tagged_variants();
    let graph = raw.cook();

    let header = graph.schemas().find(|s| s.name() == "Header").unwrap();
    let SchemaTypeView::Struct(_, header_struct) = header else {
        panic!("expected struct `Header`; got `{header:?}`");
    };
    let kind_field = header_struct
        .fields()
        .find(|f| matches!(f.name(), StructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.tag());
}

#[test]
fn test_all_of_closer_ancestor_overrides_field() {
    // `C` inherits from both `A` and `B`, in order. Both `A` and `B`
    // declare `foo`; `C` doesn't. `B` is later in `allOf`,
    // so `B`'s `foo` should win.
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
                foo:
                  type: string
                bar:
                  type: string
            B:
              type: object
              properties:
                foo:
                  type: integer
                baz:
                  type: string
            C:
              allOf:
                - $ref: '#/components/schemas/A'
                - $ref: '#/components/schemas/B'
              properties:
                qux:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let graph = RawGraph::new(&arena, &spec).cook();

    let c = graph.schemas().find(|s| s.name() == "C").unwrap();
    let SchemaTypeView::Struct(_, c_struct) = c else {
        panic!("expected struct `C`; got `{c:?}`");
    };
    let field_names = c_struct.fields().map(|f| f.name()).collect_vec();
    // `A`'s fields first (minus `foo`, overridden by `B`),
    // then `B`'s fields, then `C`'s own.
    assert_matches!(
        &*field_names,
        [
            StructFieldName::Name("bar"),
            StructFieldName::Name("foo"),
            StructFieldName::Name("baz"),
            StructFieldName::Name("qux"),
        ]
    );
}
