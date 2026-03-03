//! Pydantic `BaseModel` generation from IR structs.

use ploidy_core::{
    codegen::UniqueNames,
    ir::{
        ContainerView, ExtendableView, InlineIrTypeView, IrStructFieldName, IrStructView,
        IrTypeView, SchemaIrTypeView,
    },
};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Identifier, Stmt, Suite},
        text_size::TextRange,
    },
};

use crate::{
    graph::DiscriminatorFields,
    imports::ImportContext,
    naming::{
        CodegenIdent, CodegenIdentScope, CodegenIdentUsage, CodegenStructFieldName, CodegenTypeName,
    },
    ref_::CodegenRef,
};

/// Returns the inner type if the given type view is an optional container.
fn unwrap_optional<'a>(ty: &IrTypeView<'a>) -> Option<IrTypeView<'a>> {
    match ty {
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner)))
        | IrTypeView::Schema(SchemaIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            Some(inner.ty())
        }
        _ => None,
    }
}

/// Generates a Pydantic `BaseModel` class from an IR struct.
#[derive(Clone, Debug)]
pub struct CodegenModel<'a> {
    name: CodegenTypeName<'a>,
    ty: &'a IrStructView<'a>,
}

impl<'a> CodegenModel<'a> {
    pub fn new(name: CodegenTypeName<'a>, ty: &'a IrStructView<'a>) -> Self {
        Self { name, ty }
    }

    /// Generates all statements for this model: imports + class def.
    pub fn to_suite(&self, context: ImportContext<'_>) -> Suite {
        let mut suite = Suite::new();

        // Dependency imports (datetime, uuid, Any, cross-SCC refs).
        match &self.name {
            CodegenTypeName::Schema(sv) => {
                suite.extend(crate::imports::collect_imports(*sv, context));
            }
            CodegenTypeName::Inline(iv) => {
                suite.extend(crate::imports::collect_imports(*iv, context));
            }
        }

        // Structural imports.
        suite.push(py_quote!("from pydantic import BaseModel" as Stmt));

        // Check if any field needs a `Field(alias=...)` call.
        let needs_field_alias = {
            let unique = UniqueNames::new();
            let mut scope = CodegenIdentScope::new(&unique);
            self.ty.fields().any(|field| {
                if field.discriminator() {
                    return false;
                }
                match field.name() {
                    IrStructFieldName::Name(n) => {
                        let python_name = CodegenIdentUsage::Field(&scope.uniquify(n))
                            .display()
                            .to_string();
                        n != python_name
                    }
                    IrStructFieldName::Hint(_) => false,
                }
            })
        };
        if needs_field_alias {
            suite.push(py_quote!("from pydantic import Field" as Stmt));
        }

        let has_discriminator_fields = matches!(&self.name, CodegenTypeName::Schema(schema)
            if schema.extensions().get::<DiscriminatorFields>().is_some());
        if has_discriminator_fields {
            suite.push(py_quote!("from typing import Literal" as Stmt));
        }

        // Class definition.
        let unique = UniqueNames::new();
        let mut scope = CodegenIdentScope::new(&unique);

        // Collect fields, sorting required fields before optional ones.
        // Python requires fields with defaults to come after required fields.
        let mut required_fields = Vec::new();
        let mut optional_fields = Vec::new();

        // If this struct is used as a variant in tagged unions, add
        // discriminator fields. These have defaults so they go with optional
        // fields, but should come first among optionals.
        let mut discriminator_stmts = Vec::new();
        if let CodegenTypeName::Schema(schema) = &self.name
            && let Some(discriminators) = schema.extensions().get::<DiscriminatorFields>()
        {
            for disc in &discriminators.0 {
                let field_name = CodegenIdentUsage::Field(&CodegenIdent::new(&disc.field_name))
                    .display()
                    .to_string();
                let value = &disc.values[0];
                discriminator_stmts.push(py_quote!(
                    r#"#{name}: Literal[#{value}] = #{value}"# as Stmt,
                    name: Identifier = Identifier::new(&field_name, TextRange::default()),
                    value: &str = value
                ));
            }
        }

        for field in self.ty.fields() {
            // Skip discriminator fields from the IR; we handle them above
            // based on tagged union membership with proper Literal types.
            if field.discriminator() {
                continue;
            }

            let stmt = generate_field_stmt(field.name(), &field.ty(), field.required(), &mut scope);

            if field.required() {
                required_fields.push(stmt);
            } else {
                optional_fields.push(stmt);
            }
        }

        // Combine required fields first, then discriminator fields (which
        // have defaults), then optional fields.
        let mut class_body: Suite = required_fields;
        class_body.extend(discriminator_stmts);
        class_body.extend(optional_fields);

        if class_body.is_empty() {
            class_body.push(py_quote!("pass" as Stmt));
        }

        if let Some(desc) = self.ty.description() {
            class_body.insert(0, py_quote!("#{desc}" as Stmt, desc: &str = desc));
        }

        let class_name = self.name.as_class_name();
        suite.push(py_quote!(
            "class #{name}(BaseModel):
                 #{body}
            " as Stmt,
            name: Identifier = Identifier::new(&class_name, TextRange::default()),
            body: Suite = class_body
        ));
        suite
    }
}

/// Generates a single field statement with proper type hints and aliasing.
fn generate_field_stmt(
    name: IrStructFieldName<'_>,
    field_ty: &IrTypeView<'_>,
    required: bool,
    scope: &mut CodegenIdentScope<'_>,
) -> Stmt {
    let field_name: String = match name {
        IrStructFieldName::Name(n) => CodegenIdentUsage::Field(&scope.uniquify(n))
            .display()
            .to_string(),
        IrStructFieldName::Hint(hint) => CodegenStructFieldName(hint).to_string(),
    };
    let json_name = match name {
        IrStructFieldName::Name(n) => Some(n),
        IrStructFieldName::Hint(_) => None,
    };
    let name_ident = Identifier::new(&field_name, TextRange::default());

    let (type_expr, is_optional) = if required {
        if let Some(inner_ty) = unwrap_optional(field_ty) {
            // Required but nullable: `T | None` with no default.
            let inner = CodegenRef::new(&inner_ty).to_expr();
            (
                py_quote!("#{inner} | None" as Expr, inner: Expr = inner),
                false,
            )
        } else {
            (CodegenRef::new(field_ty).to_expr(), false)
        }
    } else {
        // Optional field: always `T | None = None`.
        let unwrapped = unwrap_optional(field_ty);
        let inner_ty = unwrapped.as_ref().unwrap_or(field_ty);
        let inner = CodegenRef::new(inner_ty).to_expr();
        (
            py_quote!("#{inner} | None" as Expr, inner: Expr = inner),
            true,
        )
    };

    let needs_alias = json_name.is_some_and(|n| n != field_name);

    match (is_optional, needs_alias) {
        (false, false) => {
            // Required, no alias: `name: Type`
            py_quote!(
                "#{name}: #{ty}" as Stmt,
                name: Identifier = name_ident,
                ty: Expr = type_expr
            )
        }
        (true, false) => {
            // Optional, no alias: `name: Type = None`
            py_quote!(
                "#{name}: #{ty} = None" as Stmt,
                name: Identifier = name_ident,
                ty: Expr = type_expr
            )
        }
        (false, true) => {
            // Required, aliased: `name: Type = Field(alias='original')`
            let alias = json_name.unwrap();
            py_quote!(
                r#"#{name}: #{ty} = Field(alias=#{alias})"# as Stmt,
                name: Identifier = name_ident,
                ty: Expr = type_expr,
                alias: &str = alias
            )
        }
        (true, true) => {
            // Optional, aliased: `name: Type = Field(None, alias='original')`
            let alias = json_name.unwrap();
            py_quote!(
                r#"#{name}: #{ty} = Field(None, alias=#{alias})"# as Stmt,
                name: Identifier = name_ident,
                ty: Expr = type_expr,
                alias: &str = alias
            )
        }
    }
}

/// Generates field statements for use in inline contexts (like tagged union
/// variants).
pub fn generate_field_stmts(ty: &IrStructView<'_>) -> Suite {
    let unique = UniqueNames::new();
    let mut scope = CodegenIdentScope::new(&unique);

    let mut required_fields = Vec::new();
    let mut optional_fields = Vec::new();

    for field in ty.fields() {
        // Skip discriminator fields; they're handled separately in the calling
        // context (e.g., tagged union variants).
        if field.discriminator() {
            continue;
        }

        let stmt = generate_field_stmt(field.name(), &field.ty(), field.required(), &mut scope);

        if field.required() {
            required_fields.push(stmt);
        } else {
            optional_fields.push(stmt);
        }
    }

    let mut suite = required_fields;
    suite.extend(optional_fields);
    suite
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    use crate::generate_source;
    use indoc::indoc;
    use ploidy_core::ir::{IrGraph, IrSpec, SchemaIrTypeView, ViewNode};
    use ploidy_core::parse::Document;
    use pretty_assertions::assert_eq;

    use crate::CodegenGraph;

    fn to_source(suite: &Suite) -> String {
        generate_source(suite)
    }

    #[test]
    fn test_model_basic_struct() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenModel::new(name, struct_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from pydantic import BaseModel
                class Pet(BaseModel):
                    name: str
                    age: int | None = None"
            },
        );
    }

    #[test]
    fn test_model_with_description() {
        let doc = Document::from_yaml(indoc! {"
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

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenModel::new(name, struct_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from pydantic import BaseModel
                class Pet(BaseModel):
                    'A pet in the store.'
                    name: str"
            },
        );
    }

    #[test]
    fn test_model_field_alias() {
        let doc = Document::from_yaml(indoc! {"
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
                    petName:
                      type: string
                  required:
                    - petName
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Pet");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Pet`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenModel::new(name, struct_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from pydantic import BaseModel
                from pydantic import Field
                class Pet(BaseModel):
                    pet_name: str = Field(alias='petName')"
            },
        );
    }

    #[test]
    fn test_model_required_nullable_field() {
        let doc = Document::from_yaml(indoc! {"
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
                    deleted_at:
                      type: string
                      format: date-time
                      nullable: true
                  required:
                    - deleted_at
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Record");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Record`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenModel::new(name, struct_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                import datetime
                from pydantic import BaseModel
                class Record(BaseModel):
                    deleted_at: datetime.datetime | None"
            },
        );
    }

    #[test]
    fn test_model_struct_all_optional_fields() {
        // Note: A truly empty object (no properties) is treated as `Any` in the IR,
        // not as a struct. This test verifies structs with all optional fields.
        let doc = Document::from_yaml(indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths: {}
            components:
              schemas:
                Config:
                  type: object
                  properties:
                    debug:
                      type: boolean
        "})
        .unwrap();

        let spec = IrSpec::from_doc(&doc).unwrap();
        let ir = IrGraph::new(&spec);
        let graph = CodegenGraph::new(ir);

        let schema = graph.schemas().find(|s| s.name() == "Config");
        let Some(schema @ SchemaIrTypeView::Struct(_, struct_view)) = &schema else {
            panic!("expected struct `Config`; got `{schema:?}`");
        };

        let name = CodegenTypeName::Schema(schema);
        let suite = CodegenModel::new(name, struct_view)
            .to_suite(ImportContext::new(schema.scc_id(), &BTreeMap::new()));

        let source = to_source(&suite);
        assert_eq!(
            source,
            indoc! {"
                from pydantic import BaseModel
                class Config(BaseModel):
                    debug: bool | None = None"
            },
        );
    }
}
