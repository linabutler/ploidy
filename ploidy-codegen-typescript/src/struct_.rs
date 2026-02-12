use ploidy_core::ir::{
    ContainerView, InlineIrTypeView, IrStructFieldName, IrStructView, IrTypeView, SchemaIrTypeView,
};
use swc_ecma_ast::{Decl, TsTypeElement};

use super::{
    emit::{interface_decl, intersection, nullable, property_sig, type_alias_decl, type_lit},
    naming::{ts_field_name, ts_struct_field_hint_name},
    ref_::ts_type_ref,
};

/// Generates a TypeScript interface or intersection type from a struct.
///
/// Decision tree:
/// 1. No named schema parents, no flattened fields ->
///    `export interface Foo { ... }` (includes linearized inline parents)
/// 2. All parents are named schema structs, no flattened ->
///    `export interface Foo extends Bar { ... }` (own fields only)
/// 3. Flattened fields or mixed parents ->
///    `export type Foo = Bar & Baz & { ... }`
pub fn ts_struct(name: &str, ty: &IrStructView<'_>) -> Decl {
    let parents: Vec<_> = ty.parents().collect();
    let has_flattened = ty.own_fields().any(|f| f.flattened());

    // Partition parents into named schema types and inline types.
    // Inline parents are already linearized into `fields()`, so
    // they don't need special handling.
    let schema_parents: Vec<_> = parents
        .iter()
        .filter(|p| matches!(p, IrTypeView::Schema(_)))
        .collect();
    let all_parents_are_inline = schema_parents.is_empty();
    let all_parents_are_schema_structs = !parents.is_empty()
        && parents
            .iter()
            .all(|p| matches!(p, IrTypeView::Schema(SchemaIrTypeView::Struct(_, _))));

    if all_parents_are_inline && !has_flattened {
        // Case 1: No named schema parents. Inline parents are
        // linearized into `fields()`, so emit a simple interface.
        ts_struct_interface(name, ty, &[], true)
    } else if all_parents_are_schema_structs && !has_flattened {
        // Case 2: Interface with extends.
        let extends: Vec<String> = parents
            .iter()
            .filter_map(|p| {
                if let IrTypeView::Schema(view) = p {
                    Some(super::naming::CodegenTypeName::Schema(view).type_name())
                } else {
                    None
                }
            })
            .collect();
        ts_struct_interface(name, ty, &extends, false)
    } else {
        // Case 3: Intersection type.
        ts_struct_intersection(name, ty, &parents)
    }
}

/// Emits `interface Name [extends Parents] { fields }`.
fn ts_struct_interface(
    name: &str,
    ty: &IrStructView<'_>,
    extends: &[String],
    include_inherited: bool,
) -> Decl {
    let members: Vec<TsTypeElement> = if include_inherited {
        ty.fields()
            .map(|field| ts_property_for_field(&field))
            .collect()
    } else {
        ty.own_fields()
            .map(|field| ts_property_for_field(&field))
            .collect()
    };

    interface_decl(name, extends, members)
}

/// Emits `type Name = Parent1 & Parent2 & { own fields }`.
fn ts_struct_intersection(name: &str, ty: &IrStructView<'_>, parents: &[IrTypeView<'_>]) -> Decl {
    let mut parts = Vec::new();

    // Add parent types.
    for parent in parents {
        parts.push(ts_type_ref(parent));
    }

    // Add own non-flattened fields as an anonymous object type.
    let own_members: Vec<TsTypeElement> = ty
        .own_fields()
        .filter(|f| !f.flattened())
        .map(|field| ts_property_for_field(&field))
        .collect();

    // Add flattened fields as individual intersection members.
    for field in ty.own_fields().filter(|f| f.flattened()) {
        let ty = field.ty();
        parts.push(ts_type_ref(&ty));
    }

    if !own_members.is_empty() {
        parts.push(type_lit(own_members));
    }

    let result_ty = if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        intersection(parts)
    };

    type_alias_decl(name, result_ty)
}

/// Converts a struct field to a TypeScript property signature.
fn ts_property_for_field(field: &ploidy_core::ir::IrStructFieldView<'_, '_>) -> TsTypeElement {
    let name = match field.name() {
        IrStructFieldName::Name(n) => ts_field_name(n),
        IrStructFieldName::Hint(hint) => ts_struct_field_hint_name(hint),
    };

    // Peel away optional layers to get the inner type and nullability.
    //
    // The IR wraps non-required fields in `Optional(T)`. For required
    // nullable fields, it uses `Optional(T)` too. For non-required
    // nullable fields, it double-wraps: `Optional(Optional(T))`.
    //
    // For TypeScript, `?:` handles the "not present" semantics from
    // the outer Optional. Only additional Optional layers indicate
    // the value can be `null`.
    let field_ty = field.ty();
    let (inner_ty, depth) = peel_optional(field_ty);

    let ts_ty = ts_type_ref(&inner_ty);

    // Determine optionality and nullability:
    // - Required field with depth >= 1: `prop: T | null`
    // - Optional field with depth >= 2: `prop?: T | null`
    //   (depth 1 = the "not present" Optional only)
    // - Optional field with depth <= 1: `prop?: T`
    // - Required field with depth == 0: `prop: T`
    let (optional, final_ty) = if field.required() {
        if depth >= 1 {
            (false, nullable(ts_ty))
        } else {
            (false, ts_ty)
        }
    } else {
        // Non-required fields always get `?:`. The outer Optional
        // (depth=1) is consumed by `?:`. Any remaining layers
        // (depth >= 2) indicate nullability.
        if depth >= 2 {
            (true, nullable(ts_ty))
        } else {
            (true, ts_ty)
        }
    };

    property_sig(&name, optional, final_ty)
}

/// Peels away `Optional` container layers, returning the inner type
/// and the number of layers peeled.
fn peel_optional(ty: IrTypeView<'_>) -> (IrTypeView<'_>, usize) {
    let mut current = ty;
    let mut depth = 0;
    loop {
        match current {
            IrTypeView::Schema(SchemaIrTypeView::Container(_, ContainerView::Optional(inner)))
            | IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
                current = inner.ty();
                depth += 1;
            }
            _ => return (current, depth),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use swc_common::DUMMY_SP;

    use crate::{
        CodegenGraph,
        emit::{TsComments, emit_module, export_decl},
        naming::CodegenTypeName,
    };

    #[test]
    fn test_struct_simple_interface() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
                    age:
                      type: integer
                      format: int32
                  required:
                    - name
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_struct(&name, struct_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            indoc::indoc! {"
                export interface Pet {
                  name: string;
                  age?: number;
                }
            "}
        );
    }

    #[test]
    fn test_struct_required_nullable_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Record:
                  type: object
                  properties:
                    id:
                      type: string
                    deleted_at:
                      type: string
                      format: date-time
                      nullable: true
                  required:
                    - id
                    - deleted_at
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_struct(&name, struct_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            indoc::indoc! {"
                export interface Record {
                  id: string;
                  deleted_at: string | null;
                }
            "}
        );
    }

    #[test]
    fn test_struct_optional_nullable_field() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Record:
                  type: object
                  properties:
                    id:
                      type: string
                    deleted_at:
                      type: string
                      format: date-time
                      nullable: true
                  required:
                    - id
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_struct(&name, struct_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            indoc::indoc! {"
                export interface Record {
                  id: string;
                  deleted_at?: string;
                }
            "}
        );
    }

    #[test]
    fn test_struct_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Pet:
                  description: A pet in the store.
                  type: object
                  properties:
                    name:
                      type: string
                  required:
                    - name
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_struct(&name, struct_view);

        // Description is handled by the caller (schema.rs) via TsComments.
        // Here we just verify the decl itself produces correct output.
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            indoc::indoc! {"
                export interface Pet {
                  name: string;
                }
            "}
        );
    }

    #[test]
    fn test_struct_linearizes_inline_all_of_parent_fields() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Person:
                  allOf:
                    - type: object
                      properties:
                        name:
                          type: string
                      required:
                        - name
                    - type: object
                      properties:
                        age:
                          type: integer
                          format: int32
                      required:
                        - age
                  properties:
                    email:
                      type: string
                  required:
                    - email
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Person");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Person`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).type_name();
        let decl = ts_struct(&name, struct_view);
        let comments = TsComments::new();
        let items = vec![export_decl(decl, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
            indoc::indoc! {"
                export interface Person {
                  name: string;
                  age: number;
                  email: string;
                }
            "}
        );
    }
}
