use std::fmt::Write;

use heck::AsLowerCamelCase;
use ploidy_core::{
    ir::{IrOperationView, IrRequestView, IrResponseView},
    parse::path::PathFragment,
};

use super::{emit::emit_type_to_string, ref_::ts_type_ref};

/// Tracks type imports needed by generated operation code.
#[derive(Debug, Default)]
pub struct OperationImports {
    /// `(type_name, file_name)` pairs for `import type { T } from "./types/f"`.
    pub types: Vec<(String, String)>,
}

/// Generates a single `async` instance method for the `Client` class.
pub struct CodegenOperation<'a> {
    op: &'a IrOperationView<'a>,
}

impl<'a> CodegenOperation<'a> {
    pub fn new(op: &'a IrOperationView<'a>) -> Self {
        Self { op }
    }

    /// Returns the camelCase method name derived from `operationId`.
    fn method_name(&self) -> String {
        format!("{}", AsLowerCamelCase(self.op.id()))
    }

    /// Generates the method source code and collects needed imports.
    pub fn emit(&self) -> (String, OperationImports) {
        let mut imports = OperationImports::default();
        let mut out = String::new();

        // JSDoc from operation description.
        if let Some(desc) = self.op.description() {
            writeln!(out, "/** {desc} */").unwrap();
        }

        // Build parameter list.
        let params = self.build_params(&mut imports);
        let return_type = self.build_return_type(&mut imports);

        let method_name = self.method_name();
        writeln!(out, "async {method_name}({params}): {return_type} {{").unwrap();

        // URL construction.
        self.emit_url(&mut out);

        // Query parameters.
        self.emit_query_params(&mut out);

        // Fetch call.
        self.emit_fetch(&mut out, &mut imports);

        writeln!(out, "}}").unwrap();

        (out, imports)
    }

    /// Builds the parameter list string for the method signature.
    fn build_params(&self, imports: &mut OperationImports) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Path parameters, in order.
        for param in self.op.path().params() {
            parts.push(format!("{}: string", param.name()));
        }

        // Query parameters, bundled in an optional object.
        let query_params: Vec<_> = self.op.query().collect();
        if !query_params.is_empty() {
            let any_required = query_params.iter().any(|p| p.required());
            let mut members = String::new();
            for param in &query_params {
                let optional = if param.required() { "" } else { "?" };
                write!(members, " {}{optional}: string;", param.name()).unwrap();
            }
            let optional = if any_required { "" } else { "?" };
            parts.push(format!("query{optional}: {{{members} }}"));
        }

        // Request body (JSON only).
        if let Some(IrRequestView::Json(ty)) = self.op.request() {
            let ts = ts_type_ref(&ty);
            let type_str = emit_type_to_string(ts);
            self.collect_type_import(&type_str, imports);
            parts.push(format!("request: {type_str}"));
        }

        parts.join(", ")
    }

    /// Builds the return type string (`Promise<T>` or `Promise<void>`).
    fn build_return_type(&self, imports: &mut OperationImports) -> String {
        match self.op.response() {
            Some(IrResponseView::Json(ty)) => {
                let ts = ts_type_ref(&ty);
                let type_str = emit_type_to_string(ts);
                self.collect_type_import(&type_str, imports);
                format!("Promise<{type_str}>")
            }
            None => "Promise<void>".to_owned(),
        }
    }

    /// Emits URL construction lines.
    fn emit_url(&self, out: &mut String) {
        // Build the path template string.
        let mut path_template = String::new();
        let segments: Vec<_> = self.op.path().segments().collect();

        // Check if any segment has a parameter (need template literal).
        let has_params = segments.iter().any(|seg| {
            seg.fragments()
                .iter()
                .any(|f| matches!(f, PathFragment::Param(_)))
        });

        for segment in &segments {
            path_template.push('/');
            for fragment in segment.fragments() {
                match fragment {
                    PathFragment::Literal(text) => path_template.push_str(text),
                    PathFragment::Param(name) => {
                        write!(path_template, "${{encodeURIComponent({name})}}").unwrap();
                    }
                }
            }
        }

        if has_params {
            writeln!(
                out,
                "  const url = new URL(`{path_template}`, this.baseUrl);"
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "  const url = new URL(\"{path_template}\", this.baseUrl);"
            )
            .unwrap();
        }
    }

    /// Emits query parameter `searchParams.set` calls.
    fn emit_query_params(&self, out: &mut String) {
        for param in self.op.query() {
            let name = param.name();
            if param.required() {
                writeln!(out, "  url.searchParams.set(\"{name}\", query.{name});").unwrap();
            } else {
                writeln!(
                    out,
                    "  if (query?.{name} !== undefined) url.searchParams.set(\"{name}\", query.{name});"
                )
                .unwrap();
            }
        }
    }

    /// Emits the `fetch` call, error check, and response parsing.
    fn emit_fetch(&self, out: &mut String, imports: &mut OperationImports) {
        let method = format!("{:?}", self.op.method()).to_uppercase();
        let has_body = matches!(self.op.request(), Some(IrRequestView::Json(_)));
        let has_response = self.op.response().is_some();

        if has_body {
            writeln!(out, "  const response = await fetch(url, {{").unwrap();
            writeln!(out, "    method: \"{method}\",").unwrap();
            writeln!(
                out,
                "    headers: {{ \"Content-Type\": \"application/json\" }},"
            )
            .unwrap();
            writeln!(out, "    body: JSON.stringify(request),").unwrap();
            writeln!(out, "  }});").unwrap();
        } else {
            writeln!(
                out,
                "  const response = await fetch(url, {{ method: \"{method}\" }});"
            )
            .unwrap();
        }

        writeln!(
            out,
            "  if (!response.ok) throw new Error(`${{response.status}} ${{response.statusText}}`);"
        )
        .unwrap();

        if has_response {
            let _ = imports;
            writeln!(out, "  return await response.json();").unwrap();
        }
    }

    /// If `type_str` looks like a schema type name, records it for import.
    fn collect_type_import(&self, type_str: &str, imports: &mut OperationImports) {
        // Extract the base type name, stripping `[]` suffixes and
        // picking the first word before any `|`, `&`, or generic `<`.
        let base = type_str
            .trim_end_matches("[]")
            .split(['|', '&', '<', ' '])
            .next()
            .unwrap_or("")
            .trim();

        // Skip primitives and built-in types.
        if base.is_empty()
            || matches!(
                base,
                "string" | "number" | "boolean" | "unknown" | "null" | "void" | "Record" | "never"
            )
        {
            return;
        }

        // Only collect if it starts with an uppercase letter (type name).
        if base.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') {
            let file_name = format!("{}", heck::AsSnekCase(base));
            imports.types.push((base.to_owned(), file_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use crate::CodegenGraph;

    /// Helper to find an operation by ID and emit its method code.
    fn emit_operation(doc: &Document, operation_id: &str) -> String {
        let ir = Ir::from_doc(doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());
        let op = graph
            .operations()
            .find(|o| o.id() == operation_id)
            .unwrap_or_else(|| panic!("expected operation `{operation_id}`"));
        let (code, _) = CodegenOperation::new(&op).emit();
        code
    }

    // MARK: Basic operations

    #[test]
    fn test_get_no_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  responses:
                    '200':
                      description: ok
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              type: string
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {"
                async listPets(): Promise<string[]> {
                  const url = new URL(\"/pets\", this.baseUrl);
                  const response = await fetch(url, { method: \"GET\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }

    #[test]
    fn test_get_with_path_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
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
            components:
              schemas:
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "getPet"),
            indoc::indoc! {"
                async getPet(petId: string): Promise<Pet> {
                  const url = new URL(`/pets/${encodeURIComponent(petId)}`, this.baseUrl);
                  const response = await fetch(url, { method: \"GET\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }

    #[test]
    fn test_get_with_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
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
                    - name: offset
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
                              type: string
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {"
                async listPets(query?: { limit?: string; offset?: string; }): Promise<string[]> {
                  const url = new URL(\"/pets\", this.baseUrl);
                  if (query?.limit !== undefined) url.searchParams.set(\"limit\", query.limit);
                  if (query?.offset !== undefined) url.searchParams.set(\"offset\", query.offset);
                  const response = await fetch(url, { method: \"GET\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }

    #[test]
    fn test_post_with_json_body() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /pets:
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
            components:
              schemas:
                CreatePetRequest:
                  type: object
                  properties:
                    name:
                      type: string
                Pet:
                  type: object
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "createPet"),
            indoc::indoc! {"
                async createPet(request: CreatePetRequest): Promise<Pet> {
                  const url = new URL(\"/pets\", this.baseUrl);
                  const response = await fetch(url, {
                    method: \"POST\",
                    headers: { \"Content-Type\": \"application/json\" },
                    body: JSON.stringify(request),
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }

    #[test]
    fn test_delete_no_response() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /pets/{petId}:
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
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "deletePet"),
            indoc::indoc! {"
                async deletePet(petId: string): Promise<void> {
                  const url = new URL(`/pets/${encodeURIComponent(petId)}`, this.baseUrl);
                  const response = await fetch(url, { method: \"DELETE\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "}
        );
    }

    #[test]
    fn test_mixed_path_and_query_params() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /users/{userId}/posts:
                get:
                  operationId: listUserPosts
                  parameters:
                    - name: userId
                      in: path
                      required: true
                      schema:
                        type: string
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
                              type: string
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listUserPosts"),
            indoc::indoc! {"
                async listUserPosts(userId: string, query?: { limit?: string; }): Promise<string[]> {
                  const url = new URL(`/users/${encodeURIComponent(userId)}/posts`, this.baseUrl);
                  if (query?.limit !== undefined) url.searchParams.set(\"limit\", query.limit);
                  const response = await fetch(url, { method: \"GET\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }

    #[test]
    fn test_operation_with_description() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test API
              version: 1.0.0
            paths:
              /pets:
                get:
                  operationId: listPets
                  description: Lists all pets in the store.
                  responses:
                    '200':
                      description: ok
                      content:
                        application/json:
                          schema:
                            type: array
                            items:
                              type: string
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {"
                /** Lists all pets in the store. */
                async listPets(): Promise<string[]> {
                  const url = new URL(\"/pets\", this.baseUrl);
                  const response = await fetch(url, { method: \"GET\" });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "}
        );
    }
}
