use ploidy_core::{
    ir::{
        ContainerView, InlineIrTypeView, IrOperationView, IrParameterStyle, IrParameterView,
        IrQueryParameter, IrRequestView, IrResponseView, IrTypeView, PrimitiveIrType,
    },
    parse::{Method, path::PathFragment},
};
use quasiquodo_ts::{
    Comments, JsDoc,
    swc::ecma_ast::{ClassMember, Expr, Param, Stmt, TsType, TsTypeElement},
    ts_quote,
};

use super::{
    naming::{CodegenIdent, CodegenIdentUsage},
    ref_::ts_type_ref,
};

// MARK: CodegenOperation

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
        CodegenIdentUsage::Method(&CodegenIdent::new(self.op.id()))
            .display()
            .to_string()
    }

    /// Generates the method as a [`ClassMember::Method`].
    pub fn emit(&self, comments: &Comments) -> ClassMember {
        let method_name = self.method_name();

        // Build formal parameters.
        let params = self.build_params(comments);

        // Build return type annotation: `Promise<T>` or `Promise<void>`.
        let return_type = self.build_return_type(comments);

        // Build body statements.
        let mut stmts: Vec<Stmt> = Vec::new();
        self.emit_url(&mut stmts);
        self.emit_query_params(&mut stmts);
        self.emit_fetch(&mut stmts);

        ts_quote!(
            comments,
            "#{doc} async #{name}(#{params}): #{ret} { #{body}; }" as ClassMember,
            doc: Option<JsDoc> = self.op.description().map(JsDoc::new),
            name: Ident = &*method_name,
            params: Vec<Param> = params,
            ret: TsType = return_type,
            body: Vec<Stmt> = stmts
        )
    }

    /// Builds formal parameters for the method signature.
    fn build_params(&self, comments: &Comments) -> Vec<Param> {
        let mut items: Vec<Param> = Vec::new();

        // Path parameters, in order.
        for param in self.op.path().params() {
            let name = CodegenIdentUsage::Param(&CodegenIdent::new(param.name()))
                .display()
                .to_string();
            items.push(ts_quote!(
                "#{name}: string" as Param,
                name: Ident = &*name
            ));
        }

        // Collect query parameters and check if any are required.
        let mut query_params: Vec<_> = self.op.query().collect();
        query_params.sort_by_key(|p| !p.required());
        let query_required = query_params.iter().any(|p| p.required());
        let has_query = !query_params.is_empty();

        // Helper to build query param.
        let build_query_param = |comments: &Comments| {
            let members: Vec<TsTypeElement> = query_params
                .iter()
                .map(|p| {
                    let name = p.name();
                    let ty = ts_type_ref(&p.ty(), comments);
                    if p.required() {
                        ts_quote!("#{name}: #{ty}" as TsTypeElement, name: &str = name, ty: TsType = ty)
                    } else {
                        ts_quote!("#{name}?: #{ty}" as TsTypeElement, name: &str = name, ty: TsType = ty)
                    }
                })
                .collect();

            let obj_type = ts_quote!(
                "{ #{members}; }" as TsType,
                members: Vec<TsTypeElement> = members
            );

            if query_required {
                ts_quote!(
                    "query: #{ty}" as Param,
                    ty: TsType = obj_type
                )
            } else {
                ts_quote!(
                    "query?: #{ty}" as Param,
                    ty: TsType = obj_type
                )
            }
        };

        // If query is required, add it now (before request body).
        if has_query && query_required {
            items.push(build_query_param(comments));
        }

        // Request body.
        match self.op.request() {
            Some(IrRequestView::Json(ty)) => {
                let ts_ty = ts_type_ref(&ty, comments);
                items.push(ts_quote!(
                    "request: #{ty}" as Param,
                    ty: TsType = ts_ty
                ));
            }
            Some(IrRequestView::Multipart) => {
                items.push(ts_quote!("request: FormData" as Param));
            }
            None => {}
        }

        // If query is optional, add it last (after request body).
        if has_query && !query_required {
            items.push(build_query_param(comments));
        }

        items
    }

    /// Builds the return type (`Promise<T>` or `Promise<void>`).
    fn build_return_type(&self, comments: &Comments) -> TsType {
        let inner = match self.op.response() {
            Some(IrResponseView::Json(ty)) => ts_type_ref(&ty, comments),
            None => ts_quote!("void" as TsType),
        };
        ts_quote!("Promise<#{t}>" as TsType, t: TsType = inner)
    }

    /// Emits URL construction that correctly appends the operation path
    /// to the base URL's existing pathname:
    ///
    /// ```js
    /// let url = new URL(this.baseUrl);
    /// url.pathname = prefix + "/pets/" + encodeURIComponent(petId);
    /// ```
    fn emit_url(&self, stmts: &mut Vec<Stmt>) {
        let segments: Vec<_> = self.op.path().segments().collect();

        // Statement 1: `let url = new URL(this.baseUrl);`
        stmts.push(ts_quote!("let url = new URL(this.baseUrl);" as Stmt));

        // Statement 2: `url.pathname = <prefix> + "/seg" + encodeURIComponent(p);`
        let mut parts: Vec<Expr> = vec![self.emit_pathname_prefix()];

        // Accumulator for adjacent literal fragments (merged into one
        // string to avoid redundant `"/" + "pets"` chains).
        let mut lit_acc = String::new();

        for segment in &segments {
            lit_acc.push('/');
            for fragment in segment.fragments() {
                match fragment {
                    PathFragment::Literal(text) => lit_acc.push_str(text),
                    PathFragment::Param(name) => {
                        // Flush accumulated literal before the expression.
                        if !lit_acc.is_empty() {
                            let s = std::mem::take(&mut lit_acc);
                            parts.push(ts_quote!("#{s}" as Expr, s: &str = &*s));
                        }
                        let normalized_name = CodegenIdentUsage::Param(&CodegenIdent::new(name))
                            .display()
                            .to_string();
                        parts.push(ts_quote!(
                            "encodeURIComponent(#{n})" as Expr,
                            n: Ident = &*normalized_name
                        ));
                    }
                }
            }
        }

        // Flush any remaining literal.
        if !lit_acc.is_empty() {
            parts.push(ts_quote!("#{s}" as Expr, s: &str = &*lit_acc));
        }

        let path = parts
            .into_iter()
            .reduce(|a, b| ts_quote!("#{a} + #{b}" as Expr, a: Expr = a, b: Expr = b))
            .unwrap();

        stmts.push(ts_quote!(
            "url.pathname = #{path};" as Stmt,
            path: Expr = path
        ));
    }

    /// Builds `(url.pathname.endsWith('/') ? url.pathname.slice(0, -1) : url.pathname)`.
    ///
    /// Wrapped in parens because the ternary is used as the left
    /// operand of `+`, which binds tighter.
    fn emit_pathname_prefix(&self) -> Expr {
        ts_quote!(
            r#"(url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname)"# as Expr
        )
    }

    /// Emits query parameter serialization statements.
    ///
    /// Generates `searchParams.set` or `searchParams.append` calls
    /// depending on the parameter's resolved IR type and style.
    fn emit_query_params(&self, stmts: &mut Vec<Stmt>) {
        for param in self.op.query() {
            let body = emit_query_param_body(&param);

            if param.required() {
                stmts.push(body);
            } else {
                let name = param.name();
                let test = ts_quote!(
                    "query?.[#{name}] !== undefined" as Expr,
                    name: &str = name
                );

                stmts.push(ts_quote!(
                    "if (#{test}) #{body}" as Stmt,
                    test: Expr = test,
                    body: Stmt = body
                ));
            }
        }
    }

    /// Emits the `fetch` call, error check, and response parsing.
    fn emit_fetch(&self, stmts: &mut Vec<Stmt>) {
        let method = match self.op.method() {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
        };
        let has_response = self.op.response().is_some();

        // `const response = await fetch(url, { ... })` with appropriate
        // options depending on whether the request has a body.
        match self.op.request() {
            Some(IrRequestView::Json(_)) => {
                stmts.push(ts_quote!(
                    r#"
                        const response = await fetch(url, {
                            method: #{method},
                            headers: {
                                ...this.headers,
                                "Content-Type": "application/json",
                            },
                            body: JSON.stringify(request),
                        });
                    "# as Stmt,
                    method: &str = method
                ));
            }
            Some(IrRequestView::Multipart) => {
                stmts.push(ts_quote!(
                    r#"
                        const response = await fetch(url, {
                            method: #{method},
                            headers: this.headers,
                            body: request,
                        });
                    "# as Stmt,
                    method: &str = method
                ));
            }
            None => {
                stmts.push(ts_quote!(
                    r#"
                        const response = await fetch(url, {
                            method: #{method},
                            headers: this.headers,
                        });
                    "# as Stmt,
                    method: &str = method
                ));
            }
        }

        // `if (!response.ok) throw new Error(...)`
        stmts.push(ts_quote!(
            r#"if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);"#
                as Stmt
        ));

        // `return await response.json()`
        if has_response {
            stmts.push(ts_quote!("return await response.json();" as Stmt));
        }
    }
}

/// Returns `true` if the primitive maps to a TS `string` type.
fn is_string_primitive(ty: PrimitiveIrType) -> bool {
    matches!(
        ty,
        PrimitiveIrType::String
            | PrimitiveIrType::DateTime
            | PrimitiveIrType::UnixTime
            | PrimitiveIrType::Date
            | PrimitiveIrType::Url
            | PrimitiveIrType::Uuid
            | PrimitiveIrType::Bytes
            | PrimitiveIrType::Binary
    )
}

/// Wraps `expr` in `String(expr)` if the inner type is not
/// string-like.
fn maybe_stringify(expr: Expr, ty: &IrTypeView<'_>) -> Expr {
    if is_string_type(ty) {
        expr
    } else {
        ts_quote!("String(#{e})" as Expr, e: Expr = expr)
    }
}

/// Returns `true` if the type resolves to a TS `string`.
fn is_string_type(ty: &IrTypeView<'_>) -> bool {
    match ty {
        IrTypeView::Inline(InlineIrTypeView::Primitive(_, view)) => is_string_primitive(view.ty()),
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            is_string_type(&inner.ty())
        }
        _ => false,
    }
}

/// Returns the inner element type of an array, unwrapping optionals.
fn array_inner_type<'a>(ty: &IrTypeView<'a>) -> Option<IrTypeView<'a>> {
    match ty {
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner))) => {
            Some(inner.ty())
        }
        IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner))) => {
            array_inner_type(&inner.ty())
        }
        _ => None,
    }
}

/// Emits the body statement for a single query parameter.
fn emit_query_param_body(param: &IrParameterView<'_, IrQueryParameter>) -> Stmt {
    let name = param.name();
    let ty = param.ty();

    // Check if the parameter type is an array (possibly wrapped in Optional).
    if let Some(inner_ty) = array_inner_type(&ty) {
        return emit_array_query_param(name, param.style(), &inner_ty);
    }

    // Scalar: `url.searchParams.set("name", value)` with optional
    // `String()` wrapping for non-string types.
    let query_field = ts_quote!("query[#{name}]" as Expr, name: &str = name);
    let value = maybe_stringify(query_field, &ty);
    ts_quote!(
        "url.searchParams.set(#{name}, #{value});" as Stmt,
        name: &str = name,
        value: Expr = value
    )
}

/// Emits serialization for an array query parameter.
fn emit_array_query_param(
    name: &str,
    style: Option<IrParameterStyle>,
    inner_ty: &IrTypeView<'_>,
) -> Stmt {
    match style {
        // Non-exploded form: `url.searchParams.set("k", query.k.join(","))`
        Some(IrParameterStyle::Form { exploded: false }) => {
            let joined = ts_quote!(
                r#"query[#{name}].join(",")"# as Expr,
                name: &str = name
            );
            ts_quote!(
                "url.searchParams.set(#{name}, #{joined});" as Stmt,
                name: &str = name,
                joined: Expr = joined
            )
        }

        // Pipe-delimited: `url.searchParams.set("k", query.k.join("|"))`
        Some(IrParameterStyle::PipeDelimited) => {
            let joined = ts_quote!(
                r#"query[#{name}].join("|")"# as Expr,
                name: &str = name
            );
            ts_quote!(
                "url.searchParams.set(#{name}, #{joined});" as Stmt,
                name: &str = name,
                joined: Expr = joined
            )
        }

        // Space-delimited: `url.searchParams.set("k", query.k.join(" "))`
        Some(IrParameterStyle::SpaceDelimited) => {
            let joined = ts_quote!(
                r#"query[#{name}].join(" ")"# as Expr,
                name: &str = name
            );
            ts_quote!(
                "url.searchParams.set(#{name}, #{joined});" as Stmt,
                name: &str = name,
                joined: Expr = joined
            )
        }

        // Exploded form (default): `for (const v of query.k)
        // url.searchParams.append("k", v)` (or `String(v)` for
        // non-string inner types).
        Some(IrParameterStyle::Form { exploded: true })
        | Some(IrParameterStyle::DeepObject)
        | None => {
            let value = maybe_stringify(ts_quote!("v" as Expr), inner_ty);
            ts_quote!(
                "for (const v of query[#{name}]) url.searchParams.append(#{name}, #{value});" as Stmt,
                name: &str = name,
                value: Expr = value
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::{codegen::Code, ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use quasiquodo_ts::Comments;

    use crate::{CodegenGraph, TsSource};

    /// Helper to find an operation by ID, emit its method as a
    /// `ClassMember`, wrap it in a minimal class, render through
    /// `TsSource`, and extract the method text.
    fn emit_operation(doc: &Document, operation_id: &str) -> String {
        let ir = Ir::from_doc(doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());
        let op = graph
            .operations()
            .find(|op| op.id() == operation_id)
            .unwrap();

        let comments = Comments::new();
        let member = CodegenOperation::new(&op).emit(&comments);
        TsSource::new(String::new(), comments, member)
            .into_string()
            .unwrap()
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
            indoc::indoc! {r#"
                async listPets(): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
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
            indoc::indoc! {r#"
                async getPet(petId: string): Promise<Pet> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets/" + encodeURIComponent(petId);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
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
            indoc::indoc! {r#"
                async listPets(query?: {
                  limit?: string;
                  offset?: string;
                }): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                  if (query?.offset !== undefined) url.searchParams.set("offset", query.offset);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
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
            indoc::indoc! {r#"
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
                }"#
            },
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
            indoc::indoc! {r#"
                async deletePet(petId: string): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets/" + encodeURIComponent(petId);
                  const response = await fetch(url, {
                    method: "DELETE",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
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
            indoc::indoc! {r#"
                async listUserPosts(userId: string, query?: {
                  limit?: string;
                }): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/users/" + encodeURIComponent(userId) + "/posts";
                  if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
        );
    }

    // MARK: Typed query parameters

    #[test]
    fn test_query_param_number() {
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
                      required: true
                      schema:
                        type: integer
                    - name: offset
                      in: query
                      schema:
                        type: integer
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
            indoc::indoc! {r#"
                async listPets(query: {
                  limit: number;
                  offset?: number;
                }): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  url.searchParams.set("limit", String(query.limit));
                  if (query?.offset !== undefined) url.searchParams.set("offset", String(query.offset));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_boolean() {
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
                    - name: active
                      in: query
                      schema:
                        type: boolean
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
            indoc::indoc! {r#"
                async listPets(query?: {
                  active?: boolean;
                }): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.active !== undefined) url.searchParams.set("active", String(query.active));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_array_exploded() {
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
                    - name: tags
                      in: query
                      schema:
                        type: array
                        items:
                          type: string
                  responses:
                    '200':
                      description: ok
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {r#"
                async listPets(query?: {
                  tags?: string[];
                }): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.tags !== undefined) for (const v of query.tags)url.searchParams.append("tags", v);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_array_exploded_numbers() {
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
                    - name: ids
                      in: query
                      required: true
                      schema:
                        type: array
                        items:
                          type: integer
                  responses:
                    '200':
                      description: ok
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {r#"
                async listPets(query: {
                  ids: number[];
                }): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  for (const v of query.ids)url.searchParams.append("ids", String(v));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_array_form_non_exploded() {
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
                    - name: tags
                      in: query
                      style: form
                      explode: false
                      schema:
                        type: array
                        items:
                          type: string
                  responses:
                    '200':
                      description: ok
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {r#"
                async listPets(query?: {
                  tags?: string[];
                }): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.tags !== undefined) url.searchParams.set("tags", query.tags.join(","));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_array_pipe_delimited() {
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
                    - name: filters
                      in: query
                      style: pipeDelimited
                      schema:
                        type: array
                        items:
                          type: string
                  responses:
                    '200':
                      description: ok
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {r#"
                async listPets(query?: {
                  filters?: string[];
                }): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.filters !== undefined) url.searchParams.set("filters", query.filters.join("|"));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
        );
    }

    #[test]
    fn test_query_param_array_space_delimited() {
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
                    - name: keywords
                      in: query
                      style: spaceDelimited
                      schema:
                        type: array
                        items:
                          type: string
                  responses:
                    '200':
                      description: ok
            components:
              schemas: {}
        "})
        .unwrap();

        assert_eq!(
            emit_operation(&doc, "listPets"),
            indoc::indoc! {r#"
                async listPets(query?: {
                  keywords?: string[];
                }): Promise<void> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  if (query?.keywords !== undefined) url.searchParams.set("keywords", query.keywords.join(" "));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }"#
            },
        );
    }

    // MARK: Descriptions

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
            indoc::indoc! {r#"
                /** Lists all pets in the store. */ async listPets(): Promise<string[]> {
                  let url = new URL(this.baseUrl);
                  url.pathname = (url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname) + "/pets";
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }"#
            },
        );
    }
}
