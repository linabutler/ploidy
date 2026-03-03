use itertools::Itertools;
use ploidy_core::ir::{
    ContainerView, InlineIrTypeView, IrStructFieldName, IrStructFieldView, IrStructView,
    IrTypeView, SchemaIrTypeView,
};
use quasiquodo_ts::{
    Comments, JsDoc,
    swc::ecma_ast::{ModuleItem, TsType, TsTypeElement},
    ts_quote,
};

use super::{
    naming::{CodegenIdent, CodegenIdentUsage, CodegenStructFieldName},
    ref_::ts_type_ref,
};

/// Resolves a struct to a TypeScript type expression (an object
/// literal type or an intersection of parent refs and own members).
pub fn ts_struct_type(ty: &IrStructView<'_>, comments: &Comments) -> TsType {
    let parents: Vec<_> = ty.parents().collect();
    let has_flattened = ty.own_fields().any(|f| f.flattened());

    let schema_parents: Vec<_> = parents
        .iter()
        .filter(|p| matches!(p, IrTypeView::Schema(_)))
        .collect();
    let all_parents_are_inline = schema_parents.is_empty();

    if all_parents_are_inline && !has_flattened {
        // No named schema parents; all fields (including inherited)
        // are emitted in a single object literal.
        let members: Vec<TsTypeElement> = ty
            .fields()
            .map(|field| ts_property_for_field(&field, comments))
            .collect();
        ts_quote!(
            "{ #{m}; }" as TsType,
            m: Vec<TsTypeElement> = members
        )
    } else {
        // Intersection: parent refs + own non-flattened fields +
        // flattened fields.
        ts_struct_intersection_type(ty, &parents, comments)
    }
}

/// Generates a TypeScript interface or intersection type from a struct,
/// returning a complete `export` module item.
///
/// Decision tree:
/// 1. No named schema parents, no flattened fields ->
///    `export interface Foo { ... }` (includes linearized inline parents)
/// 2. All parents are named schema structs, no flattened ->
///    `export interface Foo extends Bar { ... }` (own fields only)
/// 3. Flattened fields or mixed parents ->
///    `export type Foo = Bar & Baz & { ... }`
pub fn ts_struct(
    name: &str,
    ty: &IrStructView<'_>,
    comments: &Comments,
    desc: Option<&str>,
) -> ModuleItem {
    let parents: Vec<_> = ty.parents().collect();
    let has_flattened = ty.own_fields().any(|f| f.flattened());

    let schema_parents: Vec<_> = parents
        .iter()
        .filter(|p| matches!(p, IrTypeView::Schema(_)))
        .collect();
    let all_parents_are_inline = schema_parents.is_empty();
    let all_parents_are_schema_structs = !parents.is_empty()
        && parents
            .iter()
            .all(|p| matches!(p, IrTypeView::Schema(SchemaIrTypeView::Struct(_, _))));

    let doc = desc.map(JsDoc::new);

    if all_parents_are_inline && !has_flattened {
        // Case 1: No named schema parents. Inline parents are
        // linearized into `fields()`, so emit a simple interface.
        let members: Vec<TsTypeElement> = ty
            .fields()
            .map(|field| ts_property_for_field(&field, comments))
            .collect();
        ts_quote!(
            comments,
            "#{doc} export interface #{n} { #{m}; }" as ModuleItem,
            doc: Option<JsDoc> = doc,
            n: Ident = name,
            m: Vec<TsTypeElement> = members
        )
    } else if all_parents_are_schema_structs && !has_flattened {
        // Case 2: Interface with extends.
        let extends_idents: Vec<_> = parents
            .iter()
            .filter_map(|p| {
                if let IrTypeView::Schema(view) = p {
                    let type_name = super::naming::CodegenTypeName::Schema(view)
                        .display()
                        .to_string();
                    Some(ts_quote!("#{n}" as Ident, n: Ident = type_name))
                } else {
                    None
                }
            })
            .collect();
        let members: Vec<TsTypeElement> = ty
            .own_fields()
            .map(|field| ts_property_for_field(&field, comments))
            .collect();
        ts_quote!(
            comments,
            "#{doc} export interface #{n} extends #{parents} { #{m}; }" as ModuleItem,
            doc: Option<JsDoc> = doc,
            n: Ident = name,
            parents: Vec<Ident> = extends_idents,
            m: Vec<TsTypeElement> = members
        )
    } else {
        // Case 3: Intersection type.
        let intersection_ty = ts_struct_intersection_type(ty, &parents, comments);
        ts_quote!(
            comments,
            "#{doc} export type #{n} = #{t}" as ModuleItem,
            doc: Option<JsDoc> = doc,
            n: Ident = name,
            t: TsType = intersection_ty
        )
    }
}

/// Builds an intersection type from parents and own fields,
/// returning the raw `TsType`.
fn ts_struct_intersection_type(
    ty: &IrStructView<'_>,
    parents: &[IrTypeView<'_>],
    comments: &Comments,
) -> TsType {
    let mut parts: Vec<TsType> = Vec::new();

    for parent in parents {
        parts.push(ts_type_ref(parent, comments));
    }

    let own_members: Vec<TsTypeElement> = ty
        .own_fields()
        .filter(|f| !f.flattened())
        .map(|field| ts_property_for_field(&field, comments))
        .collect();

    for field in ty.own_fields().filter(|f| f.flattened()) {
        let ty = field.ty();
        parts.push(ts_type_ref(&ty, comments));
    }

    if !own_members.is_empty() {
        parts.push(ts_quote!(
            "{ #{m}; }" as TsType,
            m: Vec<TsTypeElement> = own_members
        ));
    }

    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        let mut iter = parts.into_iter();
        let first = iter.next().unwrap();
        ts_quote!(
            "#{first} & #{rest}" as TsType,
            first: TsType = first,
            rest: Vec<Box<TsType>> = iter.map(Box::new).collect_vec(),
        )
    }
}

/// Converts a struct field to a TypeScript property signature.
fn ts_property_for_field(field: &IrStructFieldView<'_, '_>, comments: &Comments) -> TsTypeElement {
    let name = match field.name() {
        IrStructFieldName::Name(n) => CodegenIdentUsage::Field(&CodegenIdent::new(n))
            .display()
            .to_string(),
        IrStructFieldName::Hint(hint) => CodegenStructFieldName(hint).display().to_string(),
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

    let ts_ty = ts_type_ref(&inner_ty, comments);

    // Determine optionality and nullability:
    // - Required field with depth >= 1: `prop: T | null`
    // - Optional field with depth >= 2: `prop?: T | null`
    //   (depth 1 = the "not present" Optional consumed by `?:`,
    //   depth 2 = an additional nullable layer from the schema)
    // - Optional field with depth <= 1: `prop?: T`
    // - Required field with depth == 0: `prop: T`
    //
    let nullable = |ty: TsType| ts_quote!("#{t} | null" as TsType, t: TsType = ty);
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

    let doc = field.description().map(JsDoc::new);
    if optional {
        ts_quote!(
            comments,
            "#{doc} #{name}?: #{ty}" as TsTypeElement,
            doc: Option<JsDoc> = doc,
            name: &str = &name,
            ty: TsType = final_ty,
        )
    } else {
        ts_quote!(
            comments,
            "#{doc} #{name}: #{ty}" as TsTypeElement,
            doc: Option<JsDoc> = doc,
            name: &str = &name,
            ty: TsType = final_ty,
        )
    }
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
        codegen::Code,
        ir::{Ir, SchemaIrTypeView},
        parse::Document,
    };
    use pretty_assertions::assert_eq;
    use quasiquodo_ts::{Comments, swc::ecma_ast::Module};

    use crate::{CodegenGraph, TsSource, naming::CodegenTypeName};

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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Record {
                  id: string;
                  deletedAt: string | null;
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Record {
                  id: string;
                  deletedAt?: string | null;
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();

        // Description is handled by the caller (schema.rs) via Comments.
        // Here we just verify the decl itself produces correct output.
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Pet {
                  name: string;
                }
            "}
        );
    }

    #[test]
    fn test_struct_field_descriptions() {
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
                      description: The pet's name.
                    age:
                      type: integer
                      format: int32
                      description: Age in years.
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Pet {
                  /** The pet's name. */ name: string;
                  /** Age in years. */ age?: number;
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

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Person {
                  name: string;
                  age: number;
                  email: string;
                }
            "}
        );
    }

    #[test]
    fn test_struct_extends_named_schema_parent() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Animal:
                  type: object
                  properties:
                    name:
                      type: string
                  required:
                    - name
                Employee:
                  allOf:
                    - $ref: '#/components/schemas/Animal'
                  properties:
                    role:
                      type: string
                  required:
                    - role
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Employee");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Employee`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        assert_eq!(
            TsSource::new(
                String::new(),
                comments,
                Module {
                    body: items,
                    ..Module::default()
                }
            )
            .into_string()
            .unwrap(),
            indoc::indoc! {"
                export interface Employee extends Animal {
                  role: string;
                }
            "}
        );
    }

    #[test]
    fn test_struct_intersection_type_with_flattened_any_of() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
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
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
                  required:
                    - name
                  anyOf:
                    - $ref: '#/components/schemas/Dog'
                    - $ref: '#/components/schemas/Cat'
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema).display().to_string();
        let comments = Comments::new();
        let items = vec![ts_struct(&name, struct_view, &comments, None)];
        let output = TsSource::new(
            String::new(),
            comments,
            Module {
                body: items,
                ..Module::default()
            },
        )
        .into_string()
        .unwrap();

        // The anyOf variants become flattened optional fields, producing
        // an intersection type: `Dog & Cat & { name: string; }`.
        assert!(output.contains("Dog"), "expected `Dog` in output: {output}");
        assert!(output.contains("Cat"), "expected `Cat` in output: {output}");
        assert!(output.contains("&"), "expected `&` in output: {output}");
        assert!(
            output.contains("name: string"),
            "expected own field `name: string` in output: {output}"
        );
    }
}
