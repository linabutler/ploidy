use std::collections::BTreeMap;

use oxc_allocator::Allocator;
use oxc_ast::AstBuilder;
use oxc_ast::NONE;
use oxc_ast::ast::{
    ClassElement, ClassType, Expression, FormalParameterKind, FunctionType, MethodDefinitionKind,
    MethodDefinitionType, PropertyDefinitionType, Statement, TSAccessibility,
};
use oxc_span::SPAN;
use ploidy_core::ir::View;
use ploidy_core::ir::{ExtendableView, IrTypeView};

use super::{
    emit::{TsComments, emit_module, import_type_decl},
    graph::CodegenGraph,
    naming::CodegenIdent,
    operation::CodegenOperation,
    schema::TsCode,
};

/// Generates a `client.ts` file with a `Client` class containing
/// async methods for each OpenAPI operation.
pub struct CodegenClient<'a> {
    graph: &'a CodegenGraph<'a>,
}

impl<'a> CodegenClient<'a> {
    pub fn new(graph: &'a CodegenGraph<'a>) -> Self {
        Self { graph }
    }

    /// Generates the full `client.ts` content and returns it as a
    /// [`TsCode`].
    pub fn into_code(self) -> TsCode {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();

        // `type_name → file_name` — `BTreeMap` for sorted, deduplicated output.
        let mut all_imports: BTreeMap<String, String> = BTreeMap::new();

        // Build class elements.
        let mut class_elements: Vec<ClassElement<'_>> = Vec::new();

        // `private baseUrl: string;`
        let base_url_prop = ClassElement::PropertyDefinition(ast.alloc(ast.property_definition(
            SPAN,
            PropertyDefinitionType::PropertyDefinition,
            ast.vec(),
            ast.property_key_static_identifier(SPAN, ast.atom("baseUrl")),
            Some(ast.ts_type_annotation(SPAN, ast.ts_type_string_keyword(SPAN))),
            None, // value
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Some(TSAccessibility::Private),
        )));
        class_elements.push(base_url_prop);

        // `private headers: Record<string, string>;`
        let headers_type = {
            let type_name = ast.ts_type_name_identifier_reference(SPAN, ast.atom("Record"));
            let params = ast.vec_from_array([
                ast.ts_type_string_keyword(SPAN),
                ast.ts_type_string_keyword(SPAN),
            ]);
            let type_args = ast.ts_type_parameter_instantiation(SPAN, params);
            ast.ts_type_type_reference(SPAN, type_name, Some(type_args))
        };
        let headers_prop = ClassElement::PropertyDefinition(ast.alloc(ast.property_definition(
            SPAN,
            PropertyDefinitionType::PropertyDefinition,
            ast.vec(),
            ast.property_key_static_identifier(SPAN, ast.atom("headers")),
            Some(ast.ts_type_annotation(SPAN, headers_type)),
            None, // value
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Some(TSAccessibility::Private),
        )));
        class_elements.push(headers_prop);

        // Constructor: `constructor(baseUrl: string, headers?: Record<string, string>)`
        let ctor_param_base_url = {
            let pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("baseUrl"));
            let type_ann = ast.ts_type_annotation(SPAN, ast.ts_type_string_keyword(SPAN));
            ast.formal_parameter(
                SPAN,
                ast.vec(),
                pattern,
                Some(type_ann),
                NONE,
                false,
                None,
                false,
                false,
            )
        };
        let ctor_param_headers = {
            let pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("headers"));
            let type_name = ast.ts_type_name_identifier_reference(SPAN, ast.atom("Record"));
            let params = ast.vec_from_array([
                ast.ts_type_string_keyword(SPAN),
                ast.ts_type_string_keyword(SPAN),
            ]);
            let type_args = ast.ts_type_parameter_instantiation(SPAN, params);
            let ty = ast.ts_type_type_reference(SPAN, type_name, Some(type_args));
            let type_ann = ast.ts_type_annotation(SPAN, ty);
            ast.formal_parameter(
                SPAN,
                ast.vec(),
                pattern,
                Some(type_ann),
                NONE,
                true, // optional
                None,
                false,
                false,
            )
        };
        let ctor_params = ast.formal_parameters(
            SPAN,
            FormalParameterKind::FormalParameter,
            ast.vec_from_array([ctor_param_base_url, ctor_param_headers]),
            NONE,
        );

        // `this.baseUrl = baseUrl`
        let this_base_url = oxc_ast::ast::AssignmentTarget::from(ast.member_expression_static(
            SPAN,
            ast.expression_this(SPAN),
            ast.identifier_name(SPAN, ast.atom("baseUrl")),
            false,
        ));
        let assign_base_url = ast.expression_assignment(
            SPAN,
            oxc_ast::ast::AssignmentOperator::Assign,
            this_base_url,
            ast.expression_identifier(SPAN, ast.atom("baseUrl")),
        );

        // `this.headers = headers ?? {}`
        let this_headers = oxc_ast::ast::AssignmentTarget::from(ast.member_expression_static(
            SPAN,
            ast.expression_this(SPAN),
            ast.identifier_name(SPAN, ast.atom("headers")),
            false,
        ));
        let headers_or_empty = ast.expression_logical(
            SPAN,
            ast.expression_identifier(SPAN, ast.atom("headers")),
            oxc_ast::ast::LogicalOperator::Coalesce,
            ast.expression_object(SPAN, ast.vec()),
        );
        let assign_headers = ast.expression_assignment(
            SPAN,
            oxc_ast::ast::AssignmentOperator::Assign,
            this_headers,
            headers_or_empty,
        );

        let ctor_func_body = ast.function_body(
            SPAN,
            ast.vec(),
            ast.vec_from_array([
                ast.statement_expression(SPAN, assign_base_url),
                ast.statement_expression(SPAN, assign_headers),
            ]),
        );
        let ctor_func = ast.function(
            SPAN,
            FunctionType::FunctionExpression,
            None,
            false,
            false,
            false,
            NONE,
            NONE,
            ctor_params,
            NONE,
            Some(ctor_func_body),
        );
        let ctor = ClassElement::MethodDefinition(ast.alloc(ast.method_definition(
            SPAN,
            MethodDefinitionType::MethodDefinition,
            ast.vec(),
            ast.property_key_static_identifier(SPAN, ast.atom("constructor")),
            ctor_func,
            MethodDefinitionKind::Constructor,
            false,
            false,
            false,
            false,
            None,
        )));
        class_elements.push(ctor);

        // Operation methods.
        for op in self.graph.operations() {
            // Collect imports by walking IR type dependencies.
            let views = op.dependencies().filter_map(|view| match view {
                IrTypeView::Schema(ty) => Some(ty),
                IrTypeView::Inline(_) => None,
            });
            for view in views {
                let ext = view.extensions();
                let ident = ext.get::<CodegenIdent>().unwrap();
                let type_name = ident.to_type_name();
                let file_name = heck::AsSnekCase(&type_name).to_string();
                all_imports.entry(type_name).or_insert(file_name);
            }

            let codegen = CodegenOperation::new(&op);
            class_elements.push(codegen.emit(&ast, &comments));
        }

        // Build class declaration.
        let class_body = ast.class_body(SPAN, ast.vec_from_iter(class_elements));
        let class = ast.class(
            SPAN,
            ClassType::ClassDeclaration,
            ast.vec(),
            Some(ast.binding_identifier(SPAN, ast.atom("Client"))),
            NONE,
            None::<Expression<'_>>,
            NONE,
            ast.vec(),
            class_body,
            false,
            false,
        );
        let class_decl = oxc_ast::ast::Declaration::ClassDeclaration(ast.alloc(class));
        let class_stmt = super::emit::export_decl(&ast, class_decl, SPAN);

        // Build import statements.
        let mut items: Vec<Statement<'_>> = Vec::new();
        for (type_name, file_name) in &all_imports {
            items.push(import_type_decl(
                &ast,
                std::slice::from_ref(type_name),
                &format!("./types/{file_name}"),
            ));
        }
        items.push(class_stmt);

        let body = ast.vec_from_iter(items);
        TsCode::new(
            "client.ts".to_owned(),
            emit_module(&allocator, &ast, body, &comments),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{codegen::Code, ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_full_client_class() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Pet Store
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  parameters:
                    - name: limit
                      in: query
                      schema:
                        type: string
                  responses:
                    '200':
                      description: ok
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              $ref: '#/components/schemas/Pet'
                post:
                  operationId: createPet
                  requestBody:
                    required: true
                    content:
                      application/json:
                        schema:
                          $ref: '#/components/schemas/CreatePetRequest'
                  responses:
                    '201':
                      description: created
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Pet'
              /pets/{petId}:
                get:
                  operationId: getPet
                  parameters:
                    - name: petId
                      in: path
                      required: true
                      schema:
                        type: string
                  responses:
                    '200':
                      description: ok
                      content:
                        application/json:
                          schema:
                            $ref: '#/components/schemas/Pet'
                delete:
                  operationId: deletePet
                  parameters:
                    - name: petId
                      in: path
                      required: true
                      schema:
                        type: string
                  responses:
                    '204':
                      description: deleted
            components:
              schemas:
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
                CreatePetRequest:
                  type: object
                  required:
                    - name
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        let ir = Ir::from_doc(&doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());
        let code = CodegenClient::new(&graph).into_code();

        assert_eq!(code.path(), "client.ts");
        assert_eq!(
            code.into_string().unwrap(),
            indoc::indoc! {r#"
                import type { CreatePetRequest } from "./types/create_pet_request";
                import type { Pet } from "./types/pet";
                export class Client {
                  private baseUrl: string;
                  private headers: Record<string, string>;
                  constructor(baseUrl: string, headers?: Record<string, string>) {
                    this.baseUrl = baseUrl;
                    this.headers = headers ?? {};
                  }
                  async listPets(query?: {
                    limit?: string;
                  }): Promise<Pet[]> {
                    const url = new URL("pets", this.baseUrl);
                    if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                    const response = await fetch(url, {
                      method: "GET",
                      headers: this.headers
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }
                  async createPet(request: CreatePetRequest): Promise<Pet> {
                    const url = new URL("pets", this.baseUrl);
                    const response = await fetch(url, {
                      method: "POST",
                      headers: {
                        ...this.headers,
                        "Content-Type": "application/json"
                      },
                      body: JSON.stringify(request)
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }
                  async getPet(petId: string): Promise<Pet> {
                    const url = new URL(`pets/${encodeURIComponent(petId)}`, this.baseUrl);
                    const response = await fetch(url, {
                      method: "GET",
                      headers: this.headers
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }
                  async deletePet(petId: string): Promise<void> {
                    const url = new URL(`pets/${encodeURIComponent(petId)}`, this.baseUrl);
                    const response = await fetch(url, {
                      method: "DELETE",
                      headers: this.headers
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  }
                }
            "#}
        );
    }
}
