use heck::AsLowerCamelCase;
use oxc_ast::AstBuilder;
use oxc_ast::NONE;
use oxc_ast::ast::{
    Argument, ChainElement, ClassElement, Expression, ForStatementLeft, FormalParameterKind,
    FunctionType, MethodDefinitionKind, MethodDefinitionType, PropertyKey, PropertyKind, Statement,
    TSType, TSTypeName, TemplateElementValue, UnaryOperator, VariableDeclarationKind,
};
use oxc_span::SPAN;
use ploidy_core::ir::{
    ContainerView, InlineIrTypeView, IrOperationView, IrParameterStyle, IrParameterView,
    IrQueryParameter, IrRequestView, IrResponseView, IrTypeView, PrimitiveIrType,
};
use ploidy_core::parse::path::PathFragment;

use super::{
    emit::{TsComments, is_valid_js_identifier, member_expr, member_expr_auto},
    naming::ts_param_name,
    ref_::ts_type_ref,
};

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

    /// Generates the method as a [`ClassElement::MethodDefinition`].
    #[allow(clippy::too_many_lines)]
    pub fn emit<'b>(&self, ast: &AstBuilder<'b>, comments: &TsComments) -> ClassElement<'b> {
        let method_name = self.method_name();

        // Build formal parameters.
        let params = self.build_params(ast, comments);

        // Build return type annotation: `Promise<T>` or `Promise<void>`.
        let return_type = self.build_return_type(ast, comments);
        let return_annotation = ast.ts_type_annotation(SPAN, return_type);

        // Build body statements.
        let mut stmts: Vec<Statement<'b>> = Vec::new();
        self.emit_url(ast, &mut stmts);
        self.emit_query_params(ast, &mut stmts);
        self.emit_fetch(ast, &mut stmts);

        let body = ast.function_body(SPAN, ast.vec(), ast.vec_from_iter(stmts));

        let func = ast.function(
            SPAN,
            FunctionType::FunctionExpression,
            None,
            false, // generator
            true,  // async
            false, // declare
            NONE,  // type_parameters
            NONE,  // this_param
            params,
            Some(return_annotation),
            Some(body),
        );

        let key = ast.property_key_static_identifier(SPAN, ast.atom(&method_name));

        let span = comments.span_with_jsdoc(self.op.description());

        ClassElement::MethodDefinition(ast.alloc(ast.method_definition(
            span,
            MethodDefinitionType::MethodDefinition,
            ast.vec(), // decorators
            key,
            func,
            MethodDefinitionKind::Method,
            false, // computed
            false, // static
            false, // override
            false, // optional
            None,  // accessibility
        )))
    }

    /// Builds formal parameters for the method signature.
    fn build_params<'b>(
        &self,
        ast: &AstBuilder<'b>,
        comments: &TsComments,
    ) -> oxc_ast::ast::FormalParameters<'b> {
        let mut items: Vec<oxc_ast::ast::FormalParameter<'b>> = Vec::new();

        // Path parameters, in order.
        for param in self.op.path().params() {
            let name = ts_param_name(param.name());
            let pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom(&name));
            let type_ann = ast.ts_type_annotation(SPAN, ast.ts_type_string_keyword(SPAN));
            items.push(ast.formal_parameter(
                SPAN,
                ast.vec(), // decorators
                pattern,
                Some(type_ann),
                NONE,  // initializer
                false, // optional
                None,  // accessibility
                false, // readonly
                false, // override
            ));
        }

        // Collect query parameters and check if any are required.
        let mut query_params: Vec<_> = self.op.query().collect();
        query_params.sort_by_key(|p| !p.required());
        let query_required = query_params.iter().any(|p| p.required());
        let has_query = !query_params.is_empty();

        // Helper to build query param.
        let build_query_param = |ast: &AstBuilder<'b>| {
            let members = ast.vec_from_iter(query_params.iter().map(|p| {
                let name = p.name();
                let key = if is_valid_js_identifier(name) {
                    ast.property_key_static_identifier(SPAN, ast.atom(name))
                } else {
                    oxc_ast::ast::PropertyKey::StringLiteral(ast.alloc(ast.string_literal(
                        SPAN,
                        ast.atom(name),
                        None,
                    )))
                };
                let type_ann = ast.ts_type_annotation(SPAN, ts_type_ref(ast, &p.ty(), comments));
                oxc_ast::ast::TSSignature::TSPropertySignature(ast.alloc(
                    ast.ts_property_signature(
                        SPAN,
                        false,         // computed
                        !p.required(), // optional
                        false,         // readonly
                        key,
                        Some(type_ann),
                    ),
                ))
            }));

            let obj_type = ast.ts_type_type_literal(SPAN, members);
            let pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("query"));
            let type_ann = ast.ts_type_annotation(SPAN, obj_type);
            ast.formal_parameter(
                SPAN,
                ast.vec(),
                pattern,
                Some(type_ann),
                NONE,
                !query_required, // optional
                None,
                false,
                false,
            )
        };

        // If query is required, add it now (before request body).
        if has_query && query_required {
            items.push(build_query_param(ast));
        }

        // Request body (JSON only).
        if let Some(IrRequestView::Json(ty)) = self.op.request() {
            let ts_ty = ts_type_ref(ast, &ty, comments);
            let pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("request"));
            let type_ann = ast.ts_type_annotation(SPAN, ts_ty);
            items.push(ast.formal_parameter(
                SPAN,
                ast.vec(),
                pattern,
                Some(type_ann),
                NONE,
                false,
                None,
                false,
                false,
            ));
        }

        // If query is optional, add it last (after request body).
        if has_query && !query_required {
            items.push(build_query_param(ast));
        }

        ast.formal_parameters(
            SPAN,
            FormalParameterKind::FormalParameter,
            ast.vec_from_iter(items),
            NONE, // rest
        )
    }

    /// Builds the return type (`Promise<T>` or `Promise<void>`).
    fn build_return_type<'b>(&self, ast: &AstBuilder<'b>, comments: &TsComments) -> TSType<'b> {
        let inner = match self.op.response() {
            Some(IrResponseView::Json(ty)) => ts_type_ref(ast, &ty, comments),
            None => ast.ts_type_void_keyword(SPAN),
        };
        let type_name = TSTypeName::IdentifierReference(
            ast.alloc(ast.identifier_reference(SPAN, ast.atom("Promise"))),
        );
        let params = ast.vec1(inner);
        let type_args = ast.ts_type_parameter_instantiation(SPAN, params);
        ast.ts_type_type_reference(SPAN, type_name, Some(type_args))
    }

    /// Emits `const url = new URL(path, this.baseUrl);`.
    fn emit_url<'b>(&self, ast: &AstBuilder<'b>, stmts: &mut Vec<Statement<'b>>) {
        let segments: Vec<_> = self.op.path().segments().collect();
        let has_params = segments.iter().any(|seg| {
            seg.fragments()
                .iter()
                .any(|f| matches!(f, PathFragment::Param(_)))
        });

        // Build the path argument as either a string literal or template
        // literal. Paths are relative (no leading `/`) to append to baseUrl.
        let path_arg: Expression<'b> = if has_params {
            // Template literal: `pets/${encodeURIComponent(petId)}`
            let mut quasis: Vec<TemplateElementValue<'b>> = Vec::new();
            let mut expressions: Vec<Expression<'b>> = Vec::new();
            let mut current_str = String::new();

            for (i, segment) in segments.iter().enumerate() {
                if i > 0 {
                    current_str.push('/');
                }
                for fragment in segment.fragments() {
                    match fragment {
                        PathFragment::Literal(text) => current_str.push_str(text),
                        PathFragment::Param(name) => {
                            quasis.push(TemplateElementValue {
                                raw: ast.atom(&current_str),
                                cooked: Some(ast.atom(&current_str)),
                            });
                            current_str.clear();

                            // `encodeURIComponent(normalizedName)`
                            let normalized_name = ts_param_name(name);
                            let callee =
                                ast.expression_identifier(SPAN, ast.atom("encodeURIComponent"));
                            let arg = ast.expression_identifier(SPAN, ast.atom(&normalized_name));
                            let call = ast.expression_call(
                                SPAN,
                                callee,
                                NONE,
                                ast.vec1(Argument::from(arg)),
                                false,
                            );
                            expressions.push(call);
                        }
                    }
                }
            }

            // Final quasi (tail).
            quasis.push(TemplateElementValue {
                raw: ast.atom(&current_str),
                cooked: Some(ast.atom(&current_str)),
            });

            let expr_count = expressions.len();
            let template_elements = ast.vec_from_iter(
                quasis
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| ast.template_element(SPAN, value, i == expr_count, false)),
            );
            let template =
                ast.template_literal(SPAN, template_elements, ast.vec_from_iter(expressions));
            Expression::TemplateLiteral(ast.alloc(template))
        } else {
            // Plain string: build path from segments (relative, no leading `/`).
            let mut path = String::new();
            for (i, segment) in segments.iter().enumerate() {
                if i > 0 {
                    path.push('/');
                }
                for fragment in segment.fragments() {
                    if let PathFragment::Literal(text) = fragment {
                        path.push_str(text);
                    }
                }
            }
            ast.expression_string_literal(SPAN, ast.atom(&path), None)
        };

        // `this.baseUrl`
        let this_base_url = member_expr(ast, ast.expression_this(SPAN), "baseUrl");

        // `new URL(path, this.baseUrl)`
        let url_callee = ast.expression_identifier(SPAN, ast.atom("URL"));
        let new_url = ast.expression_new(
            SPAN,
            url_callee,
            NONE,
            ast.vec_from_array([Argument::from(path_arg), Argument::from(this_base_url)]),
        );

        // `const url = new URL(...)`
        let url_pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("url"));
        let decl = ast.variable_declaration(
            SPAN,
            VariableDeclarationKind::Const,
            ast.vec1(ast.variable_declarator(
                SPAN,
                VariableDeclarationKind::Const,
                url_pattern,
                NONE,
                Some(new_url),
                false,
            )),
            false,
        );
        stmts.push(Statement::VariableDeclaration(ast.alloc(decl)));
    }

    /// Emits query parameter serialization statements.
    ///
    /// Generates `searchParams.set` or `searchParams.append` calls
    /// depending on the parameter's resolved IR type and style.
    fn emit_query_params<'b>(&self, ast: &AstBuilder<'b>, stmts: &mut Vec<Statement<'b>>) {
        for param in self.op.query() {
            let body = emit_query_param_body(ast, &param);

            if param.required() {
                stmts.push(body);
            } else {
                // `if (query?.name !== undefined) <body>` or
                // `if (query?.["name"] !== undefined) <body>`
                let name = param.name();
                let chain_member = if is_valid_js_identifier(name) {
                    ChainElement::StaticMemberExpression(ast.alloc(ast.static_member_expression(
                        SPAN,
                        ast.expression_identifier(SPAN, ast.atom("query")),
                        ast.identifier_name(SPAN, ast.atom(name)),
                        true, // optional
                    )))
                } else {
                    let prop = ast.expression_string_literal(SPAN, ast.atom(name), None);
                    ChainElement::ComputedMemberExpression(ast.alloc(
                        ast.computed_member_expression(
                            SPAN,
                            ast.expression_identifier(SPAN, ast.atom("query")),
                            prop,
                            true, // optional
                        ),
                    ))
                };
                let chain_expr = ast.expression_chain(SPAN, chain_member);

                let test = ast.expression_binary(
                    SPAN,
                    chain_expr,
                    oxc_ast::ast::BinaryOperator::StrictInequality,
                    ast.expression_identifier(SPAN, ast.atom("undefined")),
                );

                stmts.push(ast.statement_if(SPAN, test, body, None));
            }
        }
    }

    /// Emits the `fetch` call, error check, and response parsing.
    fn emit_fetch<'b>(&self, ast: &AstBuilder<'b>, stmts: &mut Vec<Statement<'b>>) {
        let method = format!("{:?}", self.op.method()).to_uppercase();
        let has_body = matches!(self.op.request(), Some(IrRequestView::Json(_)));
        let has_response = self.op.response().is_some();

        // Build fetch options object.
        let method_prop = ast.object_property_kind_object_property(
            SPAN,
            PropertyKind::Init,
            ast.property_key_static_identifier(SPAN, ast.atom("method")),
            ast.expression_string_literal(SPAN, ast.atom(&method), None),
            false,
            false,
            false,
        );

        let fetch_opts: Expression<'b> = if has_body {
            // `{ ...this.headers, "Content-Type": "application/json" }`
            let spread = oxc_ast::ast::ObjectPropertyKind::SpreadProperty(ast.alloc(
                ast.spread_element(SPAN, member_expr(ast, ast.expression_this(SPAN), "headers")),
            ));
            let content_type_prop = ast.object_property_kind_object_property(
                SPAN,
                PropertyKind::Init,
                PropertyKey::StringLiteral(ast.alloc(ast.string_literal(
                    SPAN,
                    ast.atom("Content-Type"),
                    None,
                ))),
                ast.expression_string_literal(SPAN, ast.atom("application/json"), None),
                false,
                false,
                false,
            );
            let headers_obj =
                ast.expression_object(SPAN, ast.vec_from_array([spread, content_type_prop]));
            let headers_prop = ast.object_property_kind_object_property(
                SPAN,
                PropertyKind::Init,
                ast.property_key_static_identifier(SPAN, ast.atom("headers")),
                headers_obj,
                false,
                false,
                false,
            );

            // `JSON.stringify(request)`
            let json_stringify = member_expr(
                ast,
                ast.expression_identifier(SPAN, ast.atom("JSON")),
                "stringify",
            );
            let body_value = ast.expression_call(
                SPAN,
                json_stringify,
                NONE,
                ast.vec1(Argument::from(
                    ast.expression_identifier(SPAN, ast.atom("request")),
                )),
                false,
            );
            let body_prop = ast.object_property_kind_object_property(
                SPAN,
                PropertyKind::Init,
                ast.property_key_static_identifier(SPAN, ast.atom("body")),
                body_value,
                false,
                false,
                false,
            );

            ast.expression_object(
                SPAN,
                ast.vec_from_array([method_prop, headers_prop, body_prop]),
            )
        } else {
            // For requests without body, still include headers
            let headers_prop = ast.object_property_kind_object_property(
                SPAN,
                PropertyKind::Init,
                ast.property_key_static_identifier(SPAN, ast.atom("headers")),
                member_expr(ast, ast.expression_this(SPAN), "headers"),
                false,
                false,
                false,
            );
            ast.expression_object(SPAN, ast.vec_from_array([method_prop, headers_prop]))
        };

        // `await fetch(url, opts)`
        let fetch_callee = ast.expression_identifier(SPAN, ast.atom("fetch"));
        let fetch_call = ast.expression_call(
            SPAN,
            fetch_callee,
            NONE,
            ast.vec_from_array([
                Argument::from(ast.expression_identifier(SPAN, ast.atom("url"))),
                Argument::from(fetch_opts),
            ]),
            false,
        );
        let await_fetch = ast.expression_await(SPAN, fetch_call);

        // `const response = await fetch(...)`
        let response_pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("response"));
        let response_decl = ast.variable_declaration(
            SPAN,
            VariableDeclarationKind::Const,
            ast.vec1(ast.variable_declarator(
                SPAN,
                VariableDeclarationKind::Const,
                response_pattern,
                NONE,
                Some(await_fetch),
                false,
            )),
            false,
        );
        stmts.push(Statement::VariableDeclaration(ast.alloc(response_decl)));

        // `if (!response.ok) throw new Error(...)`
        let response = || ast.expression_identifier(SPAN, ast.atom("response"));
        let not_ok = ast.expression_unary(
            SPAN,
            UnaryOperator::LogicalNot,
            member_expr(ast, response(), "ok"),
        );

        // Template literal: `${response.status} ${response.statusText}`
        let response_status = member_expr(ast, response(), "status");
        let response_status_text = member_expr(ast, response(), "statusText");

        let error_template = ast.template_literal(
            SPAN,
            ast.vec_from_array([
                ast.template_element(
                    SPAN,
                    TemplateElementValue {
                        raw: ast.atom(""),
                        cooked: Some(ast.atom("")),
                    },
                    false,
                    false,
                ),
                ast.template_element(
                    SPAN,
                    TemplateElementValue {
                        raw: ast.atom(" "),
                        cooked: Some(ast.atom(" ")),
                    },
                    false,
                    false,
                ),
                ast.template_element(
                    SPAN,
                    TemplateElementValue {
                        raw: ast.atom(""),
                        cooked: Some(ast.atom("")),
                    },
                    true,
                    false,
                ),
            ]),
            ast.vec_from_array([response_status, response_status_text]),
        );

        let error_callee = ast.expression_identifier(SPAN, ast.atom("Error"));
        let new_error = ast.expression_new(
            SPAN,
            error_callee,
            NONE,
            ast.vec1(Argument::from(Expression::TemplateLiteral(
                ast.alloc(error_template),
            ))),
        );

        let throw_stmt = ast.statement_throw(SPAN, new_error);
        let if_stmt = ast.statement_if(SPAN, not_ok, throw_stmt, None);
        stmts.push(if_stmt);

        // `return await response.json()`
        if has_response {
            let response_json = member_expr(ast, response(), "json");
            let json_call = ast.expression_call(SPAN, response_json, NONE, ast.vec(), false);
            let await_json = ast.expression_await(SPAN, json_call);
            stmts.push(ast.statement_return(SPAN, Some(await_json)));
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
fn maybe_stringify<'b>(
    ast: &AstBuilder<'b>,
    expr: Expression<'b>,
    ty: &IrTypeView<'_>,
) -> Expression<'b> {
    if is_string_type(ty) {
        expr
    } else {
        wrap_string(ast, expr)
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

/// Wraps an expression in `String(expr)`.
fn wrap_string<'b>(ast: &AstBuilder<'b>, expr: Expression<'b>) -> Expression<'b> {
    let callee = ast.expression_identifier(SPAN, ast.atom("String"));
    ast.expression_call(SPAN, callee, NONE, ast.vec1(Argument::from(expr)), false)
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
fn emit_query_param_body<'b>(
    ast: &AstBuilder<'b>,
    param: &IrParameterView<'_, IrQueryParameter>,
) -> Statement<'b> {
    let name = param.name();
    let ty = param.ty();

    // Check if the parameter type is an array (possibly wrapped in Optional).
    if let Some(inner_ty) = array_inner_type(&ty) {
        return emit_array_query_param(ast, name, param.style(), &inner_ty);
    }

    // Scalar: `url.searchParams.set("name", value)` with optional
    // `String()` wrapping for non-string types.
    let query_field = member_expr_auto(
        ast,
        ast.expression_identifier(SPAN, ast.atom("query")),
        name,
    );
    let value = maybe_stringify(ast, query_field, &ty);
    emit_search_params_set(ast, name, value)
}

/// Emits serialization for an array query parameter.
fn emit_array_query_param<'b>(
    ast: &AstBuilder<'b>,
    name: &str,
    style: Option<IrParameterStyle>,
    inner_ty: &IrTypeView<'_>,
) -> Statement<'b> {
    let query_field = || {
        member_expr_auto(
            ast,
            ast.expression_identifier(SPAN, ast.atom("query")),
            name,
        )
    };

    match style {
        // Non-exploded form: `url.searchParams.set("k", query.k.join(","))`
        Some(IrParameterStyle::Form { exploded: false }) => {
            let join = member_expr(ast, query_field(), "join");
            let joined = ast.expression_call(
                SPAN,
                join,
                NONE,
                ast.vec1(Argument::from(ast.expression_string_literal(
                    SPAN,
                    ast.atom(","),
                    None,
                ))),
                false,
            );
            emit_search_params_set(ast, name, joined)
        }

        // Pipe-delimited: `url.searchParams.set("k", query.k.join("|"))`
        Some(IrParameterStyle::PipeDelimited) => {
            let join = member_expr(ast, query_field(), "join");
            let joined = ast.expression_call(
                SPAN,
                join,
                NONE,
                ast.vec1(Argument::from(ast.expression_string_literal(
                    SPAN,
                    ast.atom("|"),
                    None,
                ))),
                false,
            );
            emit_search_params_set(ast, name, joined)
        }

        // Space-delimited: `url.searchParams.set("k", query.k.join(" "))`
        Some(IrParameterStyle::SpaceDelimited) => {
            let join = member_expr(ast, query_field(), "join");
            let joined = ast.expression_call(
                SPAN,
                join,
                NONE,
                ast.vec1(Argument::from(ast.expression_string_literal(
                    SPAN,
                    ast.atom(" "),
                    None,
                ))),
                false,
            );
            emit_search_params_set(ast, name, joined)
        }

        // Exploded form (default): `for (const v of query.k)
        // url.searchParams.append("k", v)` (or `String(v)` for
        // non-string inner types).
        Some(IrParameterStyle::Form { exploded: true })
        | Some(IrParameterStyle::DeepObject)
        | None => {
            let v_ident = || ast.expression_identifier(SPAN, ast.atom("v"));
            let value = maybe_stringify(ast, v_ident(), inner_ty);

            // `url.searchParams.append("name", value)`
            let url_ident = ast.expression_identifier(SPAN, ast.atom("url"));
            let append = member_expr(ast, member_expr(ast, url_ident, "searchParams"), "append");
            let append_call = ast.expression_call(
                SPAN,
                append,
                NONE,
                ast.vec_from_array([
                    Argument::from(ast.expression_string_literal(SPAN, ast.atom(name), None)),
                    Argument::from(value),
                ]),
                false,
            );
            let body = ast.statement_expression(SPAN, append_call);

            // `for (const v of query.k) <body>`
            let v_pattern = ast.binding_pattern_binding_identifier(SPAN, ast.atom("v"));
            let v_decl = ast.variable_declaration(
                SPAN,
                VariableDeclarationKind::Const,
                ast.vec1(ast.variable_declarator(
                    SPAN,
                    VariableDeclarationKind::Const,
                    v_pattern,
                    NONE,
                    None,
                    false,
                )),
                false,
            );
            let left = ForStatementLeft::VariableDeclaration(ast.alloc(v_decl));
            ast.statement_for_of(SPAN, false, left, query_field(), body)
        }
    }
}

/// Emits `url.searchParams.set("name", value)` as an expression
/// statement.
fn emit_search_params_set<'b>(
    ast: &AstBuilder<'b>,
    name: &str,
    value: Expression<'b>,
) -> Statement<'b> {
    let url_ident = ast.expression_identifier(SPAN, ast.atom("url"));
    let set = member_expr(ast, member_expr(ast, url_ident, "searchParams"), "set");
    let call = ast.expression_call(
        SPAN,
        set,
        NONE,
        ast.vec_from_array([
            Argument::from(ast.expression_string_literal(SPAN, ast.atom(name), None)),
            Argument::from(value),
        ]),
        false,
    );
    ast.statement_expression(SPAN, call)
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxc_allocator::Allocator;
    use ploidy_core::{ir::Ir, parse::Document};
    use pretty_assertions::assert_eq;

    use crate::{
        CodegenGraph,
        emit::{TsComments, emit_module},
    };

    /// Helper to find an operation by ID, emit its method as a
    /// `ClassElement`, wrap it in a minimal class, render through
    /// `emit_module`, and extract the method text.
    fn emit_operation(doc: &Document, operation_id: &str) -> String {
        let ir = Ir::from_doc(doc).unwrap();
        let graph = CodegenGraph::new(ir.graph().finalize());
        let op = graph
            .operations()
            .find(|o| o.id() == operation_id)
            .unwrap_or_else(|| panic!("expected operation `{operation_id}`"));

        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();

        let class_elem = CodegenOperation::new(&op).emit(&ast, &comments);

        // Wrap in `class __C { <method> }`.
        let body = ast.class_body(SPAN, ast.vec1(class_elem));
        let class = ast.class(
            SPAN,
            oxc_ast::ast::ClassType::ClassDeclaration,
            ast.vec(),
            Some(ast.binding_identifier(SPAN, ast.atom("__C"))),
            NONE,
            None::<Expression<'_>>,
            NONE,
            ast.vec(),
            body,
            false,
            false,
        );
        let class_decl = oxc_ast::ast::Declaration::ClassDeclaration(ast.alloc(class));
        let items = ast.vec1(Statement::from(class_decl));
        let raw = emit_module(&allocator, &ast, items, &comments);

        // Strip the `class __C {\n` prefix and `}\n` suffix to get the
        // method body.
        let inner = raw
            .strip_prefix("class __C {\n")
            .and_then(|s| s.strip_suffix("}\n"))
            .unwrap_or(&raw);

        // Dedent by 2 spaces.
        let mut result = String::new();
        for line in inner.lines() {
            if let Some(stripped) = line.strip_prefix("  ") {
                result.push_str(stripped);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        result
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
                  const url = new URL("pets", this.baseUrl);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
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
                  const url = new URL(`pets/${encodeURIComponent(petId)}`, this.baseUrl);
                  const response = await fetch(url, {
                    method: \"GET\",
                    headers: this.headers
                  });
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
            indoc::indoc! {r#"
                async listPets(query?: {
                  limit?: string;
                  offset?: string;
                }): Promise<string[]> {
                  const url = new URL("pets", this.baseUrl);
                  if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                  if (query?.offset !== undefined) url.searchParams.set("offset", query.offset);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
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
            "#}
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
                  const url = new URL(`pets/${encodeURIComponent(petId)}`, this.baseUrl);
                  const response = await fetch(url, {
                    method: \"DELETE\",
                    headers: this.headers
                  });
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
            indoc::indoc! {r#"
                async listUserPosts(userId: string, query?: {
                  limit?: string;
                }): Promise<string[]> {
                  const url = new URL(`users/${encodeURIComponent(userId)}/posts`, this.baseUrl);
                  if (query?.limit !== undefined) url.searchParams.set("limit", query.limit);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  url.searchParams.set("limit", String(query.limit));
                  if (query?.offset !== undefined) url.searchParams.set("offset", String(query.offset));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  if (query?.active !== undefined) url.searchParams.set("active", String(query.active));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  if (query?.tags !== undefined) for (const v of query.tags) url.searchParams.append("tags", v);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  for (const v of query.ids) url.searchParams.append("ids", String(v));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  if (query?.tags !== undefined) url.searchParams.set("tags", query.tags.join(","));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  if (query?.filters !== undefined) url.searchParams.set("filters", query.filters.join("|"));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "#}
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
                  const url = new URL("pets", this.baseUrl);
                  if (query?.keywords !== undefined) url.searchParams.set("keywords", query.keywords.join(" "));
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                }
            "#}
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
                /** Lists all pets in the store. */
                async listPets(): Promise<string[]> {
                  const url = new URL("pets", this.baseUrl);
                  const response = await fetch(url, {
                    method: "GET",
                    headers: this.headers
                  });
                  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
                  return await response.json();
                }
            "#}
        );
    }
}
