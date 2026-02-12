use std::collections::BTreeMap;

use ploidy_core::ir::{ExtendableView, IrTypeView, View};
use quasiquodo_ts::{
    Comments,
    swc::ecma_ast::{ClassMember, Module},
    ts_quote,
};

use super::{
    TsSource,
    graph::CodegenGraph,
    naming::{CodegenIdent, CodegenIdentUsage},
    operation::CodegenOperation,
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
    /// [`TsSource`].
    pub fn into_code(self) -> TsSource<Module> {
        let comments = Comments::new();

        // `type_name -> file_name` -- `BTreeMap` for sorted, deduplicated output.
        let mut all_imports: BTreeMap<String, String> = BTreeMap::new();

        // Build class members.
        let mut class_members: Vec<ClassMember> = Vec::new();

        class_members.push(ts_quote!("private baseUrl: string" as ClassMember));
        class_members.push(ts_quote!(
            "private headers: Record<string, string>" as ClassMember
        ));
        class_members.push(ts_quote!(
            r#"constructor(baseUrl: string, headers?: Record<string, string>) {
                this.baseUrl = baseUrl;
                this.headers = headers ?? {};
            }"# as ClassMember
        ));

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
                let type_name = CodegenIdentUsage::Type(&ident).display().to_string();
                let file_name = CodegenIdentUsage::Module(&ident).display().to_string();
                all_imports.entry(type_name).or_insert(file_name);
            }

            let codegen = CodegenOperation::new(&op);
            class_members.push(codegen.emit(&comments));
        }

        // Build class + export via `Vec<ClassMember>` splice.
        let class_stmt = ts_quote!(
            "export class Client { #{members}; }" as ModuleItem,
            members: Vec<ClassMember> = class_members
        );

        // Build import statements.
        let mut module = Module::default();
        for (type_name, file_name) in &all_imports {
            let spec = format!("./types/{file_name}");
            module.body.push(ts_quote!(
                r#"import type { #{name} } from #{spec}"# as ModuleItem,
                name: Ident = type_name,
                spec: &str = &spec,
            ));
        }
        module.body.push(class_stmt);

        TsSource::new("client.ts".to_owned(), comments, module)
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
                import type { CreatePetRequest } from "./types/create-pet-request";
                import type { Pet } from "./types/pet";
                export class Client {
                  private baseUrl: string;
                  private headers: Record<string, string>;
                  constructor(baseUrl: string, headers?: Record<string, string>){
                    this.baseUrl = baseUrl;
                    this.headers = headers ?? {};
                  }
                  async listPets(query?: {
                    limit?: string;
                  }): Promise<Pet[]> {
                    let url = new URL(this.baseUrl);
                    url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                    if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                    const response = await fetch(url, {
                      method: "GET",
                      headers: this.headers
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }
                  async createPet(request: CreatePetRequest): Promise<Pet> {
                    let url = new URL(this.baseUrl);
                    url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
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
                    let url = new URL(this.baseUrl);
                    url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets/" + encodeURIComponent(petId);
                    const response = await fetch(url, {
                      method: "GET",
                      headers: this.headers
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }
                  async deletePet(petId: string): Promise<void> {
                    let url = new URL(this.baseUrl);
                    url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets/" + encodeURIComponent(petId);
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
