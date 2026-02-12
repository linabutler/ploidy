use std::{collections::BTreeMap, fmt::Write};

use super::{graph::CodegenGraph, operation::CodegenOperation, schema::TsCode};

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
        let mut methods = Vec::new();
        // `type_name → file_name` — `BTreeMap` for sorted, deduplicated output.
        let mut all_imports: BTreeMap<String, String> = BTreeMap::new();

        for op in self.graph.operations() {
            let codegen = CodegenOperation::new(&op);
            let (method_code, imports) = codegen.emit();
            methods.push(method_code);
            for (type_name, file_name) in imports.types {
                all_imports.entry(type_name).or_insert(file_name);
            }
        }

        let mut content = String::new();

        // Emit imports (already sorted by `BTreeMap`).
        for (type_name, file_name) in &all_imports {
            writeln!(
                content,
                "import type {{ {type_name} }} from \"./types/{file_name}\";"
            )
            .unwrap();
        }
        if !all_imports.is_empty() {
            content.push('\n');
        }

        // Class header.
        writeln!(content, "export class Client {{").unwrap();
        writeln!(content, "  private baseUrl: string;").unwrap();
        content.push('\n');
        writeln!(content, "  constructor(baseUrl: string) {{").unwrap();
        writeln!(content, "    this.baseUrl = baseUrl;").unwrap();
        writeln!(content, "  }}").unwrap();

        // Methods (indented by 2 spaces for the class body).
        for method in &methods {
            content.push('\n');
            for line in method.lines() {
                if line.is_empty() {
                    content.push('\n');
                } else {
                    content.push_str("  ");
                    content.push_str(line);
                    content.push('\n');
                }
            }
        }

        writeln!(content, "}}").unwrap();

        TsCode::new("client.ts".to_owned(), content)
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

                  constructor(baseUrl: string) {
                    this.baseUrl = baseUrl;
                  }

                  async listPets(query?: { limit?: string; }): Promise<Pet[]> {
                    const url = new URL("/pets", this.baseUrl);
                    if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                    const response = await fetch(url, { method: "GET" });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }

                  async createPet(request: CreatePetRequest): Promise<Pet> {
                    const url = new URL("/pets", this.baseUrl);
                    const response = await fetch(url, {
                      method: "POST",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify(request),
                    });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }

                  async getPet(petId: string): Promise<Pet> {
                    const url = new URL(`/pets/${encodeURIComponent(petId)}`, this.baseUrl);
                    const response = await fetch(url, { method: "GET" });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                    return await response.json();
                  }

                  async deletePet(petId: string): Promise<void> {
                    const url = new URL(`/pets/${encodeURIComponent(petId)}`, this.baseUrl);
                    const response = await fetch(url, { method: "DELETE" });
                    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  }
                }
            "#}
        );
    }
}
