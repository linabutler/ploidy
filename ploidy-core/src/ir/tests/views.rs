//! Tests for the IR view layer, indirection, and extension system.

use itertools::Itertools;

use crate::{
    ir::{
        ContainerView, EdgeKind, ExtendableView, InlineIrTypePathRoot, InlineIrTypePathSegment,
        InlineIrTypeView, Ir, IrEnumVariant, IrParameterStyle, IrRequestView, IrResponseView,
        IrStructFieldName, IrTypeView, PrimitiveIrType, Reach, SchemaIrTypeView, SchemaTypeInfo,
        SomeIrUntaggedVariant, Traversal, View,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    // Should be able to construct views for different schema types.
    let struct_view = graph.schemas().find(|s| s.name() == "MyStruct").unwrap();
    let enum_view = graph.schemas().find(|s| s.name() == "MyEnum").unwrap();

    // Verify types.
    assert_matches!(struct_view, SchemaIrTypeView::Struct(..));
    assert_matches!(enum_view, SchemaIrTypeView::Enum(..));
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let branch_schema = graph.schemas().find(|s| s.name() == "Branch").unwrap();

    // `dependencies()` should include `Leaf1` and `Leaf2`,
    // but not `Branch` itself.
    let mut dep_names = branch_schema
        .dependencies()
        .filter_map(|view| match view {
            IrTypeView::Schema(view) => Some(view.name()),
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let standalone_schema = graph.schemas().next().unwrap();

    // For a struct with a required primitive field, the dependency set
    // includes just that primitive type.
    assert_eq!(standalone_schema.dependencies().count(), 1);
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let items_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("items")))
        .unwrap();

    let container_view = match items_field.ty() {
        IrTypeView::Inline(InlineIrTypeView::Container(_, view @ ContainerView::Array(_))) => view,
        other => panic!("expected inline array; got {other:?}"),
    };

    // The `dependencies()` of the array should include the schema reference
    // and the primitive field in `Item`, but not the array itself.
    let dep_types = container_view.dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Schema(SchemaIrTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let map_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("map_field")))
        .unwrap();

    let container_view = match map_field.ty() {
        IrTypeView::Inline(InlineIrTypeView::Container(_, view @ ContainerView::Map(_))) => view,
        other => panic!("expected inline map; got {other:?}"),
    };

    // The `dependencies()` of the map should include the schema reference,
    // and the primitive field in `Item`, but not the map itself.
    let dep_types = container_view.dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Schema(SchemaIrTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("nullable_field")))
        .unwrap();

    let container_view = match nullable_field.ty() {
        IrTypeView::Inline(InlineIrTypeView::Container(_, view @ ContainerView::Optional(_))) => {
            view
        }
        other => panic!("expected optional; got {other:?}"),
    };

    // The `dependencies()` of the optional should include the schema reference,
    // and the primitive field in `Item`, but not the optional itself.
    let dep_types = container_view.dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `Item` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Schema(SchemaIrTypeView::Struct(
            SchemaTypeInfo { name: "Item", .. },
            ..
        ))
    )));

    // Verify the primitive field is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    // The `dependencies()` of the inline type should include the schema reference
    // and the primitive field in `RefSchema`, but not the inline itself.
    let dep_types = inline_view.dependencies().collect_vec();
    assert_eq!(dep_types.len(), 2);

    // Verify the `RefSchema` schema is a dependency.
    assert!(dep_types.iter().any(|t| matches!(
        t,
        IrTypeView::Schema(SchemaIrTypeView::Struct(
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
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, view)) => view,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let untyped_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("untyped")))
        .unwrap();

    let untyped_view = match untyped_field.ty() {
        IrTypeView::Inline(InlineIrTypeView::Any(_, view)) => view,
        other => panic!("expected any; got {other:?}"),
    };

    // `Any` has no graph edges, so `dependencies()` returns nothing.
    assert_eq!(untyped_view.dependencies().count(), 0);
}

#[test]
fn test_traverse_skip_excludes_node_but_continues_traversal() {
    // Graph: Root -> Middle -> Leaf. Skipping `Middle` should yield
    // `[Leaf]` only, excluding both `Root` and `Middle`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: object
              properties:
                value:
                  type: string
            Middle:
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
            Root:
              type: object
              properties:
                middle:
                  $ref: '#/components/schemas/Middle'
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let root = graph.schemas().find(|s| s.name() == "Root").unwrap();

    // Skip `Middle`, but continue into its neighbors.
    let dep_names = root
        .traverse(Reach::Dependencies, |kind, view| {
            assert_eq!(kind, EdgeKind::Reference);
            match view {
                IrTypeView::Schema(s) if s.name() == "Middle" => Traversal::Skip,
                _ => Traversal::Visit,
            }
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();

    // `Middle` is skipped, but `Leaf` is still reachable through it.
    assert_eq!(dep_names, vec!["Leaf"]);
}

#[test]
fn test_traverse_stop_includes_node_but_stops_traversal() {
    // Graph: Root -> Middle -> Leaf. Stopping at `Middle`
    // should yield just `[Middle]`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: object
              properties:
                value:
                  type: string
            Middle:
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
            Root:
              type: object
              properties:
                middle:
                  $ref: '#/components/schemas/Middle'
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let root = graph.schemas().find(|s| s.name() == "Root").unwrap();

    // Stop at `Middle`; don't descend into its children.
    let dep_names = root
        .traverse(Reach::Dependencies, |kind, view| {
            assert_eq!(kind, EdgeKind::Reference);
            match view {
                IrTypeView::Schema(s) if s.name() == "Middle" => Traversal::Stop,
                _ => Traversal::Visit,
            }
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();

    // `Middle` is included, but `Leaf` isn't because we stopped at `Middle`.
    assert_eq!(dep_names, vec!["Middle"]);
}

#[test]
fn test_traverse_ignore_excludes_node_and_stops_traversal() {
    // Graph: Root -> Middle -> Leaf. Ignoring `Middle` should yield nothing.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: object
              properties:
                value:
                  type: string
            Middle:
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
            Root:
              type: object
              properties:
                middle:
                  $ref: '#/components/schemas/Middle'
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let root = graph.schemas().find(|s| s.name() == "Root").unwrap();

    // Ignore `Middle`: don't yield it, and don't visit its neighbors.
    let dep_names = root
        .traverse(Reach::Dependencies, |kind, view| {
            assert_eq!(kind, EdgeKind::Reference);
            match view {
                IrTypeView::Schema(s) if s.name() == "Middle" => Traversal::Ignore,
                _ => Traversal::Visit,
            }
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();

    // `Middle` is ignored entirely, so `Leaf` is also unreachable.
    assert!(dep_names.is_empty());
}

#[test]
fn test_traverse_dependents_yields_types_that_depend_on_node() {
    // Graph: Root -> Middle -> Leaf. Traversing dependents from `Leaf` should
    // yield everything that transitively depends on `Leaf`: `[Middle, Root]`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: object
              properties:
                value:
                  type: string
            Middle:
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
            Root:
              type: object
              properties:
                middle:
                  $ref: '#/components/schemas/Middle'
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let leaf = graph.schemas().find(|s| s.name() == "Leaf").unwrap();

    let dependent_names = leaf
        .traverse(Reach::Dependents, |kind, _| {
            assert_eq!(kind, EdgeKind::Reference);
            Traversal::Visit
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();

    assert_eq!(dependent_names, vec!["Middle", "Root"]);
}

#[test]
fn test_traverse_filter_on_edge_kind() {
    // `Child` inherits from `Parent` via `allOf` (an inheritance edge)
    // and has its own field referencing `Leaf` (a reference edge).
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Leaf:
              type: object
              properties:
                value:
                  type: string
            Parent:
              type: object
              properties:
                id:
                  type: integer
            Child:
              allOf:
                - $ref: '#/components/schemas/Parent'
              type: object
              properties:
                leaf:
                  $ref: '#/components/schemas/Leaf'
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let child = graph.schemas().find(|s| s.name() == "Child").unwrap();

    // Including just inheritance edges should yield the `allOf` parent.
    let inherits_only = child
        .traverse(Reach::Dependencies, |kind, _| match kind {
            EdgeKind::Inherits => Traversal::Visit,
            EdgeKind::Reference => Traversal::Ignore,
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();
    assert_eq!(inherits_only, vec!["Parent"]);

    // Including just reference edges should yield the field target.
    let references_only = child
        .traverse(Reach::Dependencies, |kind, _| match kind {
            EdgeKind::Reference => Traversal::Visit,
            EdgeKind::Inherits => Traversal::Ignore,
        })
        .filter_map(|view| match view {
            IrTypeView::Schema(s) => Some(s.name()),
            _ => None,
        })
        .collect_vec();
    assert_eq!(references_only, vec!["Leaf"]);
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let simple_schema = graph.schemas().find(|s| s.name() == "Simple").unwrap();

    // `Simple` has one inline: an optional for the `field` property.
    assert_eq!(simple_schema.inlines().count(), 1);
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let tagged_view = match animal_schema {
        SchemaIrTypeView::Tagged(_, view) => view,
        other => panic!("expected tagged union `Animal`; got {other:?}"),
    };

    let mut variant_names = tagged_view.variants().map(|v| v.name()).collect_vec();
    variant_names.sort();
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let animal_schema = graph.schemas().find(|s| s.name() == "Animal").unwrap();
    let untagged_view = match animal_schema {
        SchemaIrTypeView::Untagged(_, view) => view,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner)))
            if matches!(
                inner.ty(),
                IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Map(inner)))
            if matches!(
                inner.ty(),
                IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().next().unwrap();
    let container_struct = match container_schema {
        SchemaIrTypeView::Struct(_, view) => view,
        other => panic!("expected struct `Container`; got {other:?}"),
    };

    let nullable_field = container_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("nullable_field")))
        .unwrap();

    // Verify the optional's inner type is accessible, and is an inline struct.
    assert_matches!(
        nullable_field.ty(),
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner)))
            if matches!(
                inner.ty(),
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
              required: [inline_obj]
              properties:
                inline_obj:
                  type: object
                  properties:
                    nested_field:
                      type: string
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
              required: [status]
              properties:
                status:
                  type: string
                  enum: [active, inactive, pending]
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
              required: [value]
              properties:
                value:
                  oneOf:
                    - type: string
                    - type: integer
    "})
    .unwrap();

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    // The inline enum has no `x-resourceId`, and the only operation that uses it
    // has no `x-resource-name`, so it contributes `None` to `used_by`.
    let used_by = inline_enum.used_by().map(|op| op.resource()).collect_vec();
    assert_matches!(&*used_by, [None]);

    // `inlines()` includes the starting node.
    assert_eq!(inline_enum.inlines().count(), 1);

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let status_schema = graph.schemas().find(|s| s.name() == "Status").unwrap();
    let enum_view = match &status_schema {
        SchemaIrTypeView::Enum(_, view) => view,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let operation = graph.operations().next().unwrap();

    // String path parameter.
    let path_param = operation.path().params().next().unwrap();
    assert_matches!(
        path_param.ty(),
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String,
    );

    // Array-of-strings query parameter.
    let query_param = operation.query().next().unwrap();
    assert_matches!(
        query_param.ty(),
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner)))
            if matches!(
                inner.ty(),
                IrTypeView::Inline(InlineIrTypeView::Primitive(_, p)) if p.ty() == PrimitiveIrType::String,
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let operation = graph.operations().next().unwrap();

    // Empty response schema becomes `IrResponse::Json(IrType::Any)`.
    assert_matches!(
        operation.response(),
        Some(IrResponseView::Json(IrTypeView::Inline(
            InlineIrTypeView::Any(..)
        )))
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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
            matches!(inline, InlineIrTypeView::Struct(path, _) if matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Request,
                    InlineIrTypePathSegment::Field(IrStructFieldName::Name("address")),
                ],
            ))
        })
        .unwrap();
    assert_matches!(address, InlineIrTypeView::Struct(_, _));

    let request = inlines
        .iter()
        .find(|inline| {
            matches!(inline, InlineIrTypeView::Struct(path, _) if matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Request,
                ],
            ))
        })
        .unwrap();
    assert_matches!(request, InlineIrTypeView::Struct(_, _));

    let response = inlines
        .iter()
        .find(|inline| {
            matches!(inline, InlineIrTypeView::Struct(path, _) if matches!(
                &*path.segments,
                [
                    InlineIrTypePathSegment::Operation("createUser"),
                    InlineIrTypePathSegment::Response,
                ],
            ))
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let operation = graph.operations().next().unwrap();

    let Some(IrRequestView::Json(IrTypeView::Inline(request))) = operation.request() else {
        panic!(
            "expected inline request schema; got {:?}",
            operation.request(),
        );
    };
    assert_matches!(request.path().root, InlineIrTypePathRoot::Resource(None));
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
    assert_matches!(response.path().root, InlineIrTypePathRoot::Resource(None));
    assert_matches!(
        &*response.path().segments,
        [
            InlineIrTypePathSegment::Operation("createUser"),
            InlineIrTypePathSegment::Response,
        ],
    );
}

// MARK: Discriminator fields

#[test]
fn test_variant_field_matching_tagged_union_discriminator_is_discriminator() {
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    // `Comment.kind` should be detected as a discriminator because
    // `Comment` is a direct variant of the `Post` tagged union.
    let comment = graph.schemas().find(|s| s.name() == "Comment").unwrap();
    let SchemaIrTypeView::Struct(_, comment_struct) = comment else {
        panic!("expected struct `Comment`; got `{comment:?}`");
    };
    let kind_field = comment_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.discriminator());

    // Other fields on `Comment` should not be discriminators.
    let id_field = comment_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("id")))
        .unwrap();
    assert!(!id_field.discriminator());

    // `Reaction.kind` should also be detected as a discriminator.
    let reaction = graph.schemas().find(|s| s.name() == "Reaction").unwrap();
    let SchemaIrTypeView::Struct(_, reaction_struct) = reaction else {
        panic!("expected struct `Reaction`; got `{reaction:?}`");
    };
    let kind_field = reaction_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.discriminator());
}

#[test]
fn test_transitive_dependency_field_matching_discriminator_is_not_discriminator() {
    // `Inner` has a `kind` field that matches the `Outer` tagged union's
    // discriminator, but `Inner` is _not_ a direct variant of `Outer`;
    // only `Wrapper` is. The `kind` field on `Inner` should _not_ be
    // treated as a discriminator.
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    // `Wrapper.kind` _is_ a discriminator, because `Wrapper` is a
    // direct variant of `Outer`.
    let wrapper = graph.schemas().find(|s| s.name() == "Wrapper").unwrap();
    let SchemaIrTypeView::Struct(_, wrapper_struct) = wrapper else {
        panic!("expected struct `Wrapper`; got `{wrapper:?}`");
    };
    let kind_field = wrapper_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.discriminator());

    // `Inner.kind` is _not_ a discriminator, because `Inner` is only
    // transitively reachable from `Outer`, not a direct variant.
    let inner = graph.schemas().find(|s| s.name() == "Inner").unwrap();
    let SchemaIrTypeView::Struct(_, inner_struct) = inner else {
        panic!("expected struct `Inner`; got `{inner:?}`");
    };
    let kind_field = inner_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.discriminator());
}

#[test]
fn test_own_struct_discriminator_field() {
    // A struct used only inside a tagged union whose tag matches a field
    // should mark that field as a discriminator.
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let base = graph.schemas().find(|s| s.name() == "Base").unwrap();
    let SchemaIrTypeView::Struct(_, base_struct) = base else {
        panic!("expected struct `Base`; got `{base:?}`");
    };

    // The `kind` field should be marked as a discriminator.
    let kind_field = base_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.discriminator());

    // The `name` field should not be a discriminator.
    let name_field = base_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("name")))
        .unwrap();
    assert!(!name_field.discriminator());
}

#[test]
fn test_inherited_discriminator_field() {
    // A child struct that inherits a field matching the tag of an incoming
    // tagged union should mark that inherited field as a discriminator.
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let child = graph.schemas().find(|s| s.name() == "Child").unwrap();
    let SchemaIrTypeView::Struct(_, child_struct) = child else {
        panic!("expected struct `Child`; got `{child:?}`");
    };

    // The child's inherited `kind` field should be marked as a discriminator.
    let kind_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(kind_field.discriminator());
    assert!(kind_field.inherited());

    // The child's own `name` field should not be a discriminator.
    let name_field = child_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("name")))
        .unwrap();
    assert!(!name_field.discriminator());
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let person = graph.schemas().find(|s| s.name() == "Person").unwrap();
    let SchemaIrTypeView::Struct(_, person_struct) = person else {
        panic!("expected struct `Person`; got `{person:?}`");
    };

    // `fields()` should return all fields in declaration order: inherited
    // fields from the first parent, then the second parent, then own fields.
    let field_names = person_struct
        .fields()
        .filter_map(|f| match f.name() {
            IrStructFieldName::Name(n) => Some(n),
            _ => None,
        })
        .collect_vec();
    assert_eq!(field_names, vec!["name", "age", "email"]);

    // Fields from inline parents should be marked as inherited.
    let name_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("name")))
        .unwrap();
    assert!(name_field.inherited());

    let age_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("age")))
        .unwrap();
    assert!(age_field.inherited());

    // Own field should not be inherited.
    let email_field = person_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("email")))
        .unwrap();
    assert!(!email_field.inherited());
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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

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

    let ir = Ir::from_doc(&doc).unwrap();
    let graph = ir.graph().finalize();

    let container_schema = graph.schemas().find(|s| s.name() == "Container").unwrap();

    // Should find the optional for `animal` and the inline tagged union.
    let inlines = container_schema.inlines().collect_vec();
    assert_matches!(
        &*inlines,
        [
            InlineIrTypeView::Container(_, _),
            InlineIrTypeView::Tagged(_, _)
        ]
    );
}

// MARK: Discriminator detection

#[test]
fn test_discriminator_false_for_standalone_struct() {
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

    let ir = Ir::from_doc(&doc).unwrap();
    let mut raw = ir.graph();
    raw.lower_tagged_variants();
    let graph = raw.finalize();

    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaIrTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };

    // `Dog` is standalone (referenced by `Owner.dog`). After lowering,
    // the tagged union no longer references `Dog` directly, so `kind`
    // is not treated as a discriminator.
    let kind_field = dog_struct
        .fields()
        .find(|f| matches!(f.name(), IrStructFieldName::Name("kind")))
        .unwrap();
    assert!(!kind_field.discriminator());
}

#[test]
fn test_standalone_when_tagged_unions_disagree_on_discriminator() {
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

    let ir = Ir::from_doc(&doc).unwrap();
    let mut raw = ir.graph();
    raw.lower_tagged_variants();
    let graph = raw.finalize();

    // `Dog` is standalone because the two tagged unions disagree on
    // their discriminator. The original struct keeps all fields.
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let SchemaIrTypeView::Struct(_, dog_struct) = dog else {
        panic!("expected struct `Dog`; got `{dog:?}`");
    };
    let field_names = dog_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*field_names,
        [
            IrStructFieldName::Name("kind"),
            IrStructFieldName::Name("category"),
            IrStructFieldName::Name("bark"),
        ]
    );

    // Each tagged union should have an inline variant with all
    // fields present, but `discriminator()` true for the tag field.
    let by_kind = graph.schemas().find(|s| s.name() == "ByKind").unwrap();
    let SchemaIrTypeView::Tagged(_, by_kind_tagged) = by_kind else {
        panic!("expected tagged `ByKind`; got `{by_kind:?}`");
    };
    let variant = by_kind_tagged.variants().next().unwrap();
    let IrTypeView::Inline(InlineIrTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            IrStructFieldName::Name("kind"),
            IrStructFieldName::Name("category"),
            IrStructFieldName::Name("bark"),
        ]
    );
    let discriminators = inline_struct
        .fields()
        .filter(|f| f.discriminator())
        .map(|f| f.name())
        .collect_vec();
    assert_matches!(&*discriminators, [IrStructFieldName::Name("kind")]);

    let by_category = graph.schemas().find(|s| s.name() == "ByCategory").unwrap();
    let SchemaIrTypeView::Tagged(_, by_category_tagged) = by_category else {
        panic!("expected tagged `ByCategory`; got `{by_category:?}`");
    };
    let variant = by_category_tagged.variants().next().unwrap();
    let IrTypeView::Inline(InlineIrTypeView::Struct(_, inline_struct)) = variant.ty() else {
        panic!("expected inline struct variant; got `{:?}`", variant.ty());
    };
    let inline_fields = inline_struct.fields().map(|f| f.name()).collect_vec();
    assert_matches!(
        &*inline_fields,
        [
            IrStructFieldName::Name("kind"),
            IrStructFieldName::Name("category"),
            IrStructFieldName::Name("bark"),
        ]
    );
    let discriminators = inline_struct
        .fields()
        .filter(|f| f.discriminator())
        .map(|f| f.name())
        .collect_vec();
    assert_matches!(&*discriminators, [IrStructFieldName::Name("category")]);
}

#[test]
fn test_standalone_variant_inline_field_types_not_leaked() {
    // `Dog` is standalone (referenced by `Owner.dog`) and has an
    // inline field type (`details`). After lowering, `Pet`'s
    // `inlines()` should contain the inline variant struct for
    // `Dog`, but NOT `Dog`'s inline `Details` type.
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

    let ir = Ir::from_doc(&doc).unwrap();
    let mut raw = ir.graph();
    raw.lower_tagged_variants();
    let graph = raw.finalize();

    let pet = graph.schemas().find(|s| s.name() == "Pet").unwrap();
    let pet_inlines = pet.inlines().collect_vec();

    // Only the inline variant struct for `Dog`, not `Dog`'s
    // inline `Details` field type.
    assert_matches!(&*pet_inlines, [InlineIrTypeView::Struct(..)]);
    let InlineIrTypeView::Struct(path, _) = &pet_inlines[0] else {
        unreachable!()
    };
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Pet"));
    assert_matches!(
        &*path.segments,
        [InlineIrTypePathSegment::TaggedVariant("Dog")]
    );

    // `Dog`'s own `inlines()` still contains its inline types
    // (containers for optional fields, the `Details` struct, etc.).
    let dog = graph.schemas().find(|s| s.name() == "Dog").unwrap();
    let dog_inlines = dog.inlines().collect_vec();
    assert!(
        dog_inlines
            .iter()
            .any(|i| matches!(i, InlineIrTypeView::Struct(..))),
        "expected `Dog` to have an inline struct (`Details`)"
    );
    assert!(
        dog_inlines
            .iter()
            .all(|i| i.path().root == InlineIrTypePathRoot::Type("Dog")),
        "all of `Dog`'s inlines should be rooted at `Dog`"
    );
}
