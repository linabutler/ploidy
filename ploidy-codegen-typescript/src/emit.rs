use std::cell::{Cell, RefCell};

use oxc_allocator::Allocator;
use oxc_ast::AstBuilder;
use oxc_ast::NONE;
use oxc_ast::ast::{
    Comment, CommentContent, CommentKind, CommentNewlines, CommentPosition, Declaration,
    ExportSpecifier, Expression, ImportDeclarationSpecifier, ImportOrExportKind, NumberBase,
    Statement, TSModuleDeclarationBody, TSModuleDeclarationKind, TSSignature, TSType, TSTypeName,
};
use oxc_codegen::{Codegen, CodegenOptions, IndentChar};
use oxc_span::{SPAN, SourceType, Span};

// MARK: Comments

/// Bundles a JSDoc comment store with a position counter for
/// allocating unique spans to attach JSDoc comments.
///
/// Comments are stored as text entries keyed by their `attached_to`
/// position. When emitting, these are built into Oxc `Comment`
/// objects referencing a synthetic source text string.
pub struct TsComments {
    entries: RefCell<Vec<CommentEntry>>,
    source_text: RefCell<String>,
    next_pos: Cell<u32>,
}

struct CommentEntry {
    /// Span within the synthetic source text where the comment text
    /// (including `/*` and `*/` delimiters) lives.
    source_span: Span,
    /// The `span.start` of the AST node this comment is attached to.
    attached_to: u32,
}

impl TsComments {
    /// Creates a new empty comment store.
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            source_text: RefCell::new(String::new()),
            // Start at 1 to avoid position 0, which is `SPAN.start`.
            next_pos: Cell::new(1),
        }
    }

    /// Allocates a unique span, optionally attaching a leading JSDoc
    /// comment (`/** description */`).
    pub fn span_with_jsdoc(&self, desc: Option<&str>) -> Span {
        let pos = self.next_pos.get();
        self.next_pos.set(pos + 1);
        let span = Span::new(pos, pos);

        if let Some(desc) = desc {
            let mut source_text = self.source_text.borrow_mut();
            let start = source_text.len() as u32;
            source_text.push_str(&format!("/** {desc} */"));
            let end = source_text.len() as u32;

            self.entries.borrow_mut().push(CommentEntry {
                source_span: Span::new(start, end),
                attached_to: pos,
            });
        }

        span
    }

    /// Builds the Oxc `Comment` objects and synthetic source text for
    /// use in a `Program`.
    fn build<'a>(&self, ast: &AstBuilder<'a>) -> (oxc_allocator::Vec<'a, Comment>, String) {
        let entries = self.entries.borrow();
        let comments = ast.vec_from_iter(entries.iter().map(|e| Comment {
            span: e.source_span,
            attached_to: e.attached_to,
            kind: CommentKind::MultiLineBlock,
            position: CommentPosition::Leading,
            newlines: CommentNewlines::Trailing,
            content: CommentContent::Jsdoc,
        }));
        let source_text = self.source_text.borrow().clone();
        (comments, source_text)
    }
}

// MARK: Expression helpers

/// Creates a static member expression `obj.field`.
pub fn member_expr<'a>(ast: &AstBuilder<'a>, obj: Expression<'a>, field: &str) -> Expression<'a> {
    Expression::from(ast.member_expression_static(
        SPAN,
        obj,
        ast.identifier_name(SPAN, ast.atom(field)),
        false,
    ))
}

// MARK: Type constructors

/// Creates a string literal type like `"active"`.
pub fn lit_str<'a>(ast: &AstBuilder<'a>, s: &str) -> TSType<'a> {
    let atom = ast.atom(s);
    ast.ts_type_literal_type(SPAN, ast.ts_literal_string_literal(SPAN, atom, None))
}

/// Creates a number literal type like `42`.
pub fn lit_num<'a>(ast: &AstBuilder<'a>, s: &str) -> TSType<'a> {
    let value: f64 = s.parse().unwrap_or(0.0);
    ast.ts_type_literal_type(
        SPAN,
        ast.ts_literal_numeric_literal(SPAN, value, Some(ast.atom(s)), NumberBase::Decimal),
    )
}

/// Creates a boolean literal type like `true` or `false`.
pub fn lit_bool<'a>(ast: &AstBuilder<'a>, b: bool) -> TSType<'a> {
    ast.ts_type_literal_type(SPAN, ast.ts_literal_boolean_literal(SPAN, b))
}

/// Creates an array type `T[]`, wrapping union and intersection
/// element types in parentheses to preserve precedence.
pub fn array<'a>(ast: &AstBuilder<'a>, elem: TSType<'a>) -> TSType<'a> {
    // Oxc's codegen does not parenthesize union/intersection types
    // inside array types, so `(A | B)[]` would emit as `A | B[]`.
    // Wrap in parenthesized type to force correct output.
    let elem = match &elem {
        TSType::TSUnionType(_) | TSType::TSIntersectionType(_) => {
            ast.ts_type_parenthesized_type(SPAN, elem)
        }
        _ => elem,
    };
    ast.ts_type_array_type(SPAN, elem)
}

/// Creates a `Record<string, T>` type reference.
pub fn record<'a>(ast: &AstBuilder<'a>, value: TSType<'a>) -> TSType<'a> {
    let type_name = ast.ts_type_name_identifier_reference(SPAN, ast.atom("Record"));
    let params = ast.vec_from_array([ast.ts_type_string_keyword(SPAN), value]);
    let type_args = ast.ts_type_parameter_instantiation(SPAN, params);
    ast.ts_type_type_reference(SPAN, type_name, Some(type_args))
}

/// Creates a union type `A | B | C`.
pub fn union<'a>(ast: &AstBuilder<'a>, types: oxc_allocator::Vec<'a, TSType<'a>>) -> TSType<'a> {
    ast.ts_type_union_type(SPAN, types)
}

/// Creates an intersection type `A & B & C`.
pub fn intersection<'a>(
    ast: &AstBuilder<'a>,
    types: oxc_allocator::Vec<'a, TSType<'a>>,
) -> TSType<'a> {
    ast.ts_type_intersection_type(SPAN, types)
}

/// Creates a type reference, parsing dotted names like `Order.Status`
/// into a qualified name chain.
pub fn type_ref<'a>(ast: &AstBuilder<'a>, name: &str) -> TSType<'a> {
    let type_name = parse_type_name(ast, name);
    ast.ts_type_type_reference(SPAN, type_name, NONE)
}

/// Creates an anonymous object type `{ field: Type; ... }`.
pub fn type_lit<'a>(
    ast: &AstBuilder<'a>,
    members: oxc_allocator::Vec<'a, TSSignature<'a>>,
) -> TSType<'a> {
    ast.ts_type_type_literal(SPAN, members)
}

/// Wraps a type in a nullable union (`T | null`).
pub fn nullable<'a>(ast: &AstBuilder<'a>, ty: TSType<'a>) -> TSType<'a> {
    let types = ast.vec_from_array([ty, ast.ts_type_null_keyword(SPAN)]);
    union(ast, types)
}

/// Parses a possibly-dotted name into a [`TSTypeName`].
fn parse_type_name<'a>(ast: &AstBuilder<'a>, name: &str) -> TSTypeName<'a> {
    let mut parts = name.split('.');
    let first = parts.next().unwrap();
    let mut entity = ast.ts_type_name_identifier_reference(SPAN, ast.atom(first));
    for part in parts {
        entity = TSTypeName::QualifiedName(ast.alloc(ast.ts_qualified_name(
            SPAN,
            entity,
            ast.identifier_name(SPAN, ast.atom(part)),
        )));
    }
    entity
}

// MARK: Property helper

/// Creates a property signature for an interface or type literal.
pub fn property_sig<'a>(
    ast: &AstBuilder<'a>,
    name: &str,
    optional: bool,
    ty: TSType<'a>,
    span: Span,
) -> TSSignature<'a> {
    let key = ast.property_key_static_identifier(SPAN, ast.atom(name));
    let type_ann = ast.ts_type_annotation(SPAN, ty);
    TSSignature::TSPropertySignature(ast.alloc(ast.ts_property_signature(
        span,
        false,
        optional,
        false,
        key,
        Some(type_ann),
    )))
}

// MARK: Declaration helpers

/// Creates an `interface Name [extends Parents] { members }`
/// declaration.
pub fn interface_decl<'a>(
    ast: &AstBuilder<'a>,
    name: &str,
    extends: &[String],
    members: oxc_allocator::Vec<'a, TSSignature<'a>>,
) -> Declaration<'a> {
    let id = ast.binding_identifier(SPAN, ast.atom(name));
    let heritage = ast.vec_from_iter(extends.iter().map(|e| {
        let expr = ast.expression_identifier(SPAN, ast.atom(e.as_str()));
        ast.ts_interface_heritage(SPAN, expr, NONE)
    }));
    let body = ast.ts_interface_body(SPAN, members);
    Declaration::TSInterfaceDeclaration(
        ast.alloc(ast.ts_interface_declaration(SPAN, id, NONE, heritage, body, false)),
    )
}

/// Creates a `type Name = Type` declaration.
pub fn type_alias_decl<'a>(ast: &AstBuilder<'a>, name: &str, ty: TSType<'a>) -> Declaration<'a> {
    let id = ast.binding_identifier(SPAN, ast.atom(name));
    Declaration::TSTypeAliasDeclaration(
        ast.alloc(ast.ts_type_alias_declaration(SPAN, id, NONE, ty, false)),
    )
}

/// Creates a `namespace Name { body }` declaration.
pub fn namespace_decl<'a>(
    ast: &AstBuilder<'a>,
    name: &str,
    body: oxc_allocator::Vec<'a, Statement<'a>>,
) -> Declaration<'a> {
    let id = ast.ts_module_declaration_name_identifier(SPAN, ast.atom(name));
    let block = ast.ts_module_block(SPAN, ast.vec(), body);
    let module_body = Some(TSModuleDeclarationBody::TSModuleBlock(ast.alloc(block)));
    Declaration::TSModuleDeclaration(ast.alloc(ast.ts_module_declaration(
        SPAN,
        id,
        module_body,
        TSModuleDeclarationKind::Namespace,
        false,
    )))
}

// MARK: Statement helpers

/// Wraps a declaration in `export <decl>` with the given span (for
/// JSDoc comment attachment).
pub fn export_decl<'a>(ast: &AstBuilder<'a>, decl: Declaration<'a>, span: Span) -> Statement<'a> {
    Statement::ExportNamedDeclaration(ast.alloc(ast.export_named_declaration(
        span,
        Some(decl),
        ast.vec(),
        None::<oxc_ast::ast::StringLiteral<'a>>,
        ImportOrExportKind::Value,
        NONE,
    )))
}

/// Creates `import type { names } from 'module';`.
pub fn import_type_decl<'a>(ast: &AstBuilder<'a>, names: &[String], module: &str) -> Statement<'a> {
    let specifiers = ast.vec_from_iter(names.iter().map(|n| {
        let atom = ast.atom(n.as_str());
        ImportDeclarationSpecifier::ImportSpecifier(ast.alloc(ast.import_specifier(
            SPAN,
            ast.module_export_name_identifier_name(SPAN, atom),
            ast.binding_identifier(SPAN, atom),
            ImportOrExportKind::Value,
        )))
    }));
    let source = ast.string_literal(SPAN, ast.atom(module), None);
    Statement::ImportDeclaration(ast.alloc(ast.import_declaration(
        SPAN,
        Some(specifiers),
        source,
        None,
        NONE,
        ImportOrExportKind::Type,
    )))
}

/// Creates `export type { names } from 'module';`.
pub fn reexport_type<'a>(ast: &AstBuilder<'a>, names: &[String], module: &str) -> Statement<'a> {
    let specifiers = ast.vec_from_iter(names.iter().map(|n| {
        let atom = ast.atom(n.as_str());
        let local = ast.module_export_name_identifier_name(SPAN, atom);
        ExportSpecifier {
            span: SPAN,
            local,
            exported: ast.module_export_name_identifier_name(SPAN, atom),
            export_kind: ImportOrExportKind::Value,
        }
    }));
    let source = ast.string_literal(SPAN, ast.atom(module), None);
    Statement::ExportNamedDeclaration(ast.alloc(ast.export_named_declaration(
        SPAN,
        None,
        specifiers,
        Some(source),
        ImportOrExportKind::Type,
        NONE,
    )))
}

// MARK: Emitter

/// Emits a list of statements as a formatted TypeScript string.
pub fn emit_module(
    allocator: &Allocator,
    ast: &AstBuilder<'_>,
    body: oxc_allocator::Vec<'_, Statement<'_>>,
    comments: &TsComments,
) -> String {
    emit_module_impl(allocator, ast, body, comments)
}

/// Inner implementation of module emission.
fn emit_module_impl<'a>(
    allocator: &'a Allocator,
    ast: &AstBuilder<'a>,
    body: oxc_allocator::Vec<'a, Statement<'a>>,
    comments: &TsComments,
) -> String {
    let (oxc_comments, source_text) = comments.build(ast);

    // Allocate the synthetic source text into the arena so it
    // outlives the `Program`.
    let source_text: &'a str = allocator.alloc_str(&source_text);

    let program = ast.program(
        SPAN,
        SourceType::ts(),
        source_text,
        oxc_comments,
        None,
        ast.vec(),
        body,
    );

    let codegen_options = CodegenOptions {
        indent_char: IndentChar::Space,
        indent_width: 2,
        ..CodegenOptions::default()
    };
    let mut result = Codegen::new()
        .with_options(codegen_options)
        .build(&program)
        .code;

    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    /// Emits a single exported type alias and returns the output string.
    fn emit_export_type(
        name: &str,
        ty_fn: impl for<'a> FnOnce(&'a AstBuilder<'a>) -> TSType<'a>,
    ) -> String {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let ty = ty_fn(&ast);
        let items = ast.vec1(export_decl(&ast, type_alias_decl(&ast, name, ty), SPAN));
        emit_module(&allocator, &ast, items, &comments)
    }

    // MARK: Type constructors

    #[test]
    fn test_keyword_string() {
        assert_eq!(
            emit_export_type("T", |ast| ast.ts_type_string_keyword(SPAN)),
            "export type T = string;\n"
        );
    }

    #[test]
    fn test_keyword_number() {
        assert_eq!(
            emit_export_type("T", |ast| ast.ts_type_number_keyword(SPAN)),
            "export type T = number;\n"
        );
    }

    #[test]
    fn test_keyword_boolean() {
        assert_eq!(
            emit_export_type("T", |ast| ast.ts_type_boolean_keyword(SPAN)),
            "export type T = boolean;\n"
        );
    }

    #[test]
    fn test_keyword_unknown() {
        assert_eq!(
            emit_export_type("T", |ast| ast.ts_type_unknown_keyword(SPAN)),
            "export type T = unknown;\n"
        );
    }

    #[test]
    fn test_keyword_null() {
        assert_eq!(
            emit_export_type("T", |ast| ast.ts_type_null_keyword(SPAN)),
            "export type T = null;\n"
        );
    }

    #[test]
    fn test_literal_string() {
        assert_eq!(
            emit_export_type("T", |ast| lit_str(ast, "active")),
            "export type T = \"active\";\n"
        );
    }

    #[test]
    fn test_literal_number() {
        assert_eq!(
            emit_export_type("T", |ast| lit_num(ast, "42")),
            "export type T = 42;\n"
        );
    }

    #[test]
    fn test_literal_bool() {
        assert_eq!(
            emit_export_type("T", |ast| lit_bool(ast, true)),
            "export type T = true;\n"
        );
        assert_eq!(
            emit_export_type("T", |ast| lit_bool(ast, false)),
            "export type T = false;\n"
        );
    }

    #[test]
    fn test_array_simple() {
        assert_eq!(
            emit_export_type("T", |ast| array(ast, ast.ts_type_string_keyword(SPAN))),
            "export type T = string[];\n"
        );
    }

    #[test]
    fn test_array_of_union_adds_parens() {
        assert_eq!(
            emit_export_type("T", |ast| {
                let types = ast.vec_from_array([
                    ast.ts_type_string_keyword(SPAN),
                    ast.ts_type_number_keyword(SPAN),
                ]);
                array(ast, union(ast, types))
            }),
            "export type T = (string | number)[];\n"
        );
    }

    #[test]
    fn test_record() {
        assert_eq!(
            emit_export_type("T", |ast| record(ast, ast.ts_type_string_keyword(SPAN))),
            "export type T = Record<string, string>;\n"
        );
    }

    #[test]
    fn test_union() {
        assert_eq!(
            emit_export_type("T", |ast| {
                let types = ast.vec_from_array([
                    ast.ts_type_string_keyword(SPAN),
                    ast.ts_type_number_keyword(SPAN),
                ]);
                union(ast, types)
            }),
            "export type T = string | number;\n"
        );
    }

    #[test]
    fn test_intersection() {
        assert_eq!(
            emit_export_type("T", |ast| {
                let types = ast.vec_from_array([type_ref(ast, "Foo"), type_ref(ast, "Bar")]);
                intersection(ast, types)
            }),
            "export type T = Foo & Bar;\n"
        );
    }

    #[test]
    fn test_nullable() {
        assert_eq!(
            emit_export_type("T", |ast| nullable(ast, ast.ts_type_string_keyword(SPAN))),
            "export type T = string | null;\n"
        );
    }

    #[test]
    fn test_type_ref_simple() {
        assert_eq!(
            emit_export_type("T", |ast| type_ref(ast, "Pet")),
            "export type T = Pet;\n"
        );
    }

    #[test]
    fn test_type_ref_qualified() {
        assert_eq!(
            emit_export_type("T", |ast| type_ref(ast, "Order.Status")),
            "export type T = Order.Status;\n"
        );
    }

    // MARK: Module emission

    #[test]
    fn test_module_with_interface() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let members = ast.vec_from_array([
            property_sig(&ast, "name", false, ast.ts_type_string_keyword(SPAN), SPAN),
            property_sig(&ast, "age", true, ast.ts_type_number_keyword(SPAN), SPAN),
        ]);
        let decl = interface_decl(&ast, "Pet", &[], members);
        let items = ast.vec1(export_decl(&ast, decl, SPAN));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            indoc::indoc! {"
                export interface Pet {
                  name: string;
                  age?: number;
                }
            "}
        );
    }

    #[test]
    fn test_module_with_interface_extends() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let members = ast.vec1(property_sig(
            &ast,
            "name",
            false,
            ast.ts_type_string_keyword(SPAN),
            SPAN,
        ));
        let items = ast.vec_from_array([
            import_type_decl(&ast, &["Base".to_owned()], "./base"),
            export_decl(
                &ast,
                interface_decl(&ast, "Pet", &["Base".to_owned()], members),
                SPAN,
            ),
        ]);
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            indoc::indoc! {r#"
                import type { Base } from "./base";
                export interface Pet extends Base {
                  name: string;
                }
            "#}
        );
    }

    #[test]
    fn test_module_with_type_alias() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let types = ast.vec_from_array([lit_str(&ast, "active"), lit_str(&ast, "inactive")]);
        let items = ast.vec1(export_decl(
            &ast,
            type_alias_decl(&ast, "Status", union(&ast, types)),
            SPAN,
        ));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "export type Status = \"active\" | \"inactive\";\n"
        );
    }

    #[test]
    fn test_module_with_description() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let span = comments.span_with_jsdoc(Some("The status of a resource."));
        let items = ast.vec1(export_decl(
            &ast,
            type_alias_decl(&ast, "Status", ast.ts_type_string_keyword(SPAN)),
            span,
        ));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "/** The status of a resource. */\nexport type Status = string;\n"
        );
    }

    #[test]
    fn test_module_with_namespace() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let iface = interface_decl(
            &ast,
            "Order",
            &[],
            ast.vec1(property_sig(
                &ast,
                "status",
                true,
                type_ref(&ast, "Order.Status"),
                SPAN,
            )),
        );
        let ns_types = ast.vec_from_array([lit_str(&ast, "placed"), lit_str(&ast, "approved")]);
        let ns = namespace_decl(
            &ast,
            "Order",
            ast.vec1(export_decl(
                &ast,
                type_alias_decl(&ast, "Status", union(&ast, ns_types)),
                SPAN,
            )),
        );
        let items =
            ast.vec_from_array([export_decl(&ast, iface, SPAN), export_decl(&ast, ns, SPAN)]);
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            indoc::indoc! {r#"
                export interface Order {
                  status?: Order.Status;
                }
                export namespace Order {
                  export type Status = "placed" | "approved";
                }
            "#}
        );
    }

    #[test]
    fn test_import_type_decl_output() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let items = ast.vec1(import_type_decl(
            &ast,
            &["Pet".to_owned(), "Order".to_owned()],
            "./pet",
        ));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "import type { Pet, Order } from \"./pet\";\n"
        );
    }

    #[test]
    fn test_reexport_type_output() {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let items = ast.vec1(reexport_type(&ast, &["Pet".to_owned()], "./pet"));
        assert_eq!(
            emit_module(&allocator, &ast, items, &comments),
            "export type { Pet } from \"./pet\";\n"
        );
    }
}
