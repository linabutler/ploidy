use std::cell::Cell;

use swc_common::{
    BytePos, DUMMY_SP, SourceMap, Span,
    comments::{Comment, CommentKind, Comments, SingleThreadedComments},
    sync::Lrc,
};
use swc_ecma_ast::{
    Bool, Decl, ExportDecl, ExportNamedSpecifier, ExportSpecifier, Expr, Ident, IdentName,
    ImportDecl, ImportNamedSpecifier, ImportPhase, ImportSpecifier, Module, ModuleDecl,
    ModuleExportName, ModuleItem, NamedExport, Number, Str, TsArrayType, TsEntityName,
    TsExprWithTypeArgs, TsInterfaceBody, TsInterfaceDecl, TsIntersectionType, TsKeywordType,
    TsKeywordTypeKind, TsLit, TsLitType, TsModuleBlock, TsModuleDecl, TsModuleName,
    TsNamespaceBody, TsParenthesizedType, TsPropertySignature, TsQualifiedName, TsType,
    TsTypeAliasDecl, TsTypeAnn, TsTypeElement, TsTypeLit, TsTypeParamInstantiation, TsTypeRef,
    TsUnionOrIntersectionType, TsUnionType,
};
use swc_ecma_codegen::{Emitter, text_writer::JsWriter};

// MARK: Comments

/// Bundles a [`SingleThreadedComments`] store with a [`BytePos`] counter
/// for allocating unique spans to attach JSDoc comments.
pub struct TsComments {
    comments: SingleThreadedComments,
    next_pos: Cell<u32>,
}

impl TsComments {
    /// Creates a new empty comment store.
    pub fn new() -> Self {
        Self {
            comments: SingleThreadedComments::default(),
            // Start at 1 to avoid `BytePos(0)`, which is `DUMMY_SP.lo`.
            next_pos: Cell::new(1),
        }
    }

    /// Allocates a unique span, optionally attaching a leading JSDoc
    /// comment (`/** description */`).
    pub fn span_with_jsdoc(&self, desc: Option<&str>) -> Span {
        let pos = self.next_pos.get();
        self.next_pos.set(pos + 1);
        let lo = BytePos(pos);
        let span = Span::new(lo, lo);

        if let Some(desc) = desc {
            self.comments.add_leading(
                lo,
                Comment {
                    kind: CommentKind::Block,
                    span: DUMMY_SP,
                    text: format!("* {desc} ").into(),
                },
            );
        }

        span
    }
}

// MARK: Ident helpers

/// Creates an [`Ident`] with `DUMMY_SP` and no syntax context.
pub fn ident(name: &str) -> Ident {
    Ident::new_no_ctxt(name.into(), DUMMY_SP)
}

/// Creates an [`IdentName`] with `DUMMY_SP`.
pub fn ident_name(name: &str) -> IdentName {
    IdentName::new(name.into(), DUMMY_SP)
}

// MARK: Type constructors

/// Creates a keyword type like `string`, `number`, `boolean`,
/// `unknown`, or `null`.
pub fn kw(kind: TsKeywordTypeKind) -> Box<TsType> {
    Box::new(TsType::TsKeywordType(TsKeywordType {
        span: DUMMY_SP,
        kind,
    }))
}

/// Creates a string literal type like `"active"`.
pub fn lit_str(s: &str) -> Box<TsType> {
    Box::new(TsType::TsLitType(TsLitType {
        span: DUMMY_SP,
        lit: TsLit::Str(Str {
            span: DUMMY_SP,
            value: s.into(),
            raw: None,
        }),
    }))
}

/// Creates a number literal type like `42`.
pub fn lit_num(s: &str) -> Box<TsType> {
    let value: f64 = s.parse().unwrap_or(0.0);
    Box::new(TsType::TsLitType(TsLitType {
        span: DUMMY_SP,
        lit: TsLit::Number(Number {
            span: DUMMY_SP,
            value,
            raw: Some(s.into()),
        }),
    }))
}

/// Creates a boolean literal type like `true` or `false`.
pub fn lit_bool(b: bool) -> Box<TsType> {
    Box::new(TsType::TsLitType(TsLitType {
        span: DUMMY_SP,
        lit: TsLit::Bool(Bool {
            span: DUMMY_SP,
            value: b,
        }),
    }))
}

/// Creates an array type `T[]`, wrapping union and intersection
/// element types in parentheses to preserve precedence.
pub fn array(elem: Box<TsType>) -> Box<TsType> {
    // SWC's emitter does not parenthesize union/intersection types
    // inside array types, so `(A | B)[]` would emit as `A | B[]`.
    // Wrap in `TsParenthesizedType` to force correct output.
    let elem = match *elem {
        TsType::TsUnionOrIntersectionType(_) => {
            Box::new(TsType::TsParenthesizedType(TsParenthesizedType {
                span: DUMMY_SP,
                type_ann: elem,
            }))
        }
        _ => elem,
    };
    Box::new(TsType::TsArrayType(TsArrayType {
        span: DUMMY_SP,
        elem_type: elem,
    }))
}

/// Creates a `Record<string, T>` type reference.
pub fn record(value: Box<TsType>) -> Box<TsType> {
    Box::new(TsType::TsTypeRef(TsTypeRef {
        span: DUMMY_SP,
        type_name: TsEntityName::Ident(ident("Record")),
        type_params: Some(Box::new(TsTypeParamInstantiation {
            span: DUMMY_SP,
            params: vec![kw(TsKeywordTypeKind::TsStringKeyword), value],
        })),
    }))
}

/// Creates a union type `A | B | C`.
#[allow(clippy::vec_box)] // `TsUnionType` requires `Vec<Box<TsType>>`.
pub fn union(types: Vec<Box<TsType>>) -> Box<TsType> {
    Box::new(TsType::TsUnionOrIntersectionType(
        TsUnionOrIntersectionType::TsUnionType(TsUnionType {
            span: DUMMY_SP,
            types,
        }),
    ))
}

/// Creates an intersection type `A & B & C`.
#[allow(clippy::vec_box)] // `TsIntersectionType` requires `Vec<Box<TsType>>`.
pub fn intersection(types: Vec<Box<TsType>>) -> Box<TsType> {
    Box::new(TsType::TsUnionOrIntersectionType(
        TsUnionOrIntersectionType::TsIntersectionType(TsIntersectionType {
            span: DUMMY_SP,
            types,
        }),
    ))
}

/// Creates a type reference, parsing dotted names like `Order.Status`
/// into a qualified name chain.
pub fn type_ref(name: &str) -> Box<TsType> {
    Box::new(TsType::TsTypeRef(TsTypeRef {
        span: DUMMY_SP,
        type_name: parse_entity_name(name),
        type_params: None,
    }))
}

/// Creates an anonymous object type `{ field: Type; ... }`.
pub fn type_lit(members: Vec<TsTypeElement>) -> Box<TsType> {
    Box::new(TsType::TsTypeLit(TsTypeLit {
        span: DUMMY_SP,
        members,
    }))
}

/// Wraps a type in a nullable union (`T | null`).
pub fn nullable(ty: Box<TsType>) -> Box<TsType> {
    union(vec![ty, kw(TsKeywordTypeKind::TsNullKeyword)])
}

/// Parses a possibly-dotted name into a [`TsEntityName`].
fn parse_entity_name(name: &str) -> TsEntityName {
    let mut parts = name.split('.');
    let first = parts.next().unwrap();
    let mut entity = TsEntityName::Ident(ident(first));
    for part in parts {
        entity = TsEntityName::TsQualifiedName(Box::new(TsQualifiedName {
            span: DUMMY_SP,
            left: entity,
            right: ident_name(part),
        }));
    }
    entity
}

// MARK: Property helper

/// Creates a property signature for an interface or type literal.
pub fn property_sig(name: &str, optional: bool, ty: Box<TsType>, span: Span) -> TsTypeElement {
    TsTypeElement::TsPropertySignature(TsPropertySignature {
        span,
        readonly: false,
        key: Box::new(Expr::Ident(ident(name))),
        computed: false,
        optional,
        type_ann: Some(Box::new(TsTypeAnn {
            span: DUMMY_SP,
            type_ann: ty,
        })),
    })
}

// MARK: Declaration helpers

/// Creates an `interface Name [extends Parents] { members }`
/// declaration.
pub fn interface_decl(name: &str, extends: &[String], members: Vec<TsTypeElement>) -> Decl {
    Decl::TsInterface(Box::new(TsInterfaceDecl {
        span: DUMMY_SP,
        id: ident(name),
        declare: false,
        type_params: None,
        extends: extends
            .iter()
            .map(|e| TsExprWithTypeArgs {
                span: DUMMY_SP,
                expr: Box::new(Expr::Ident(ident(e))),
                type_args: None,
            })
            .collect(),
        body: TsInterfaceBody {
            span: DUMMY_SP,
            body: members,
        },
    }))
}

/// Creates a `type Name = Type` declaration.
pub fn type_alias_decl(name: &str, ty: Box<TsType>) -> Decl {
    Decl::TsTypeAlias(Box::new(TsTypeAliasDecl {
        span: DUMMY_SP,
        declare: false,
        id: ident(name),
        type_params: None,
        type_ann: ty,
    }))
}

/// Creates a `namespace Name { body }` declaration.
pub fn namespace_decl(name: &str, body: Vec<ModuleItem>) -> Decl {
    Decl::TsModule(Box::new(TsModuleDecl {
        span: DUMMY_SP,
        declare: false,
        global: false,
        namespace: true,
        id: TsModuleName::Ident(ident(name)),
        body: Some(TsNamespaceBody::TsModuleBlock(TsModuleBlock {
            span: DUMMY_SP,
            body,
        })),
    }))
}

// MARK: Module-item helpers

/// Wraps a declaration in `export <decl>` with the given span (for
/// JSDoc comment attachment).
pub fn export_decl(decl: Decl, span: Span) -> ModuleItem {
    ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { span, decl }))
}

/// Creates `import type { names } from 'module';`.
pub fn import_type_decl(names: &[String], module: &str) -> ModuleItem {
    ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        span: DUMMY_SP,
        specifiers: names
            .iter()
            .map(|n| {
                ImportSpecifier::Named(ImportNamedSpecifier {
                    span: DUMMY_SP,
                    local: ident(n),
                    imported: None,
                    is_type_only: false,
                })
            })
            .collect(),
        src: Box::new(Str {
            span: DUMMY_SP,
            value: module.into(),
            raw: None,
        }),
        type_only: true,
        with: None,
        phase: ImportPhase::Evaluation,
    }))
}

/// Creates `export type { names } from 'module';`.
pub fn reexport_type(names: &[String], module: &str) -> ModuleItem {
    ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(NamedExport {
        span: DUMMY_SP,
        specifiers: names
            .iter()
            .map(|n| {
                ExportSpecifier::Named(ExportNamedSpecifier {
                    span: DUMMY_SP,
                    orig: ModuleExportName::Ident(ident(n)),
                    exported: None,
                    is_type_only: false,
                })
            })
            .collect(),
        src: Some(Box::new(Str {
            span: DUMMY_SP,
            value: module.into(),
            raw: None,
        })),
        type_only: true,
        with: None,
    }))
}

// MARK: Emitter

/// Renders a [`TsType`] as a formatted TypeScript string.
///
/// Emits a `type __T = <ty>;` module via [`emit_module`], then strips
/// the wrapper to extract the bare type text.
pub fn emit_type_to_string(ty: Box<TsType>) -> String {
    let comments = TsComments::new();
    let items = vec![ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
        span: DUMMY_SP,
        decl: Decl::TsTypeAlias(Box::new(TsTypeAliasDecl {
            span: DUMMY_SP,
            declare: false,
            id: ident("__T"),
            type_params: None,
            type_ann: ty,
        })),
    }))];
    let output = emit_module(items, &comments);
    // Strip `export type __T = ` prefix and `;\n` suffix.
    output
        .strip_prefix("export type __T = ")
        .and_then(|s| s.strip_suffix(";\n"))
        .unwrap_or(&output)
        .to_owned()
}

/// Emits a list of module items as a formatted TypeScript string.
pub fn emit_module(body: Vec<ModuleItem>, comments: &TsComments) -> String {
    let cm: Lrc<SourceMap> = Lrc::new(SourceMap::default());
    let mut buf = Vec::new();

    let module = Module {
        span: DUMMY_SP,
        body,
        shebang: None,
    };

    {
        let mut wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
        wr.set_indent_str("  ");
        let mut emitter = Emitter {
            cfg: Default::default(),
            cm: cm.clone(),
            comments: Some(&comments.comments),
            wr,
        };
        emitter.emit_module(&module).unwrap();
    }

    let mut result = String::from_utf8(buf).unwrap();

    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use swc_ecma_ast::TsKeywordTypeKind::*;

    /// Emits a single exported type alias and returns the output string.
    fn emit_export_type(name: &str, ty: Box<TsType>) -> String {
        let comments = TsComments::new();
        let items = vec![export_decl(type_alias_decl(name, ty), DUMMY_SP)];
        emit_module(items, &comments)
    }

    // MARK: Type constructors

    #[test]
    fn test_keyword_string() {
        assert_eq!(
            emit_export_type("T", kw(TsStringKeyword)),
            "export type T = string;\n"
        );
    }

    #[test]
    fn test_keyword_number() {
        assert_eq!(
            emit_export_type("T", kw(TsNumberKeyword)),
            "export type T = number;\n"
        );
    }

    #[test]
    fn test_keyword_boolean() {
        assert_eq!(
            emit_export_type("T", kw(TsBooleanKeyword)),
            "export type T = boolean;\n"
        );
    }

    #[test]
    fn test_keyword_unknown() {
        assert_eq!(
            emit_export_type("T", kw(TsUnknownKeyword)),
            "export type T = unknown;\n"
        );
    }

    #[test]
    fn test_keyword_null() {
        assert_eq!(
            emit_export_type("T", kw(TsNullKeyword)),
            "export type T = null;\n"
        );
    }

    #[test]
    fn test_literal_string() {
        assert_eq!(
            emit_export_type("T", lit_str("active")),
            "export type T = \"active\";\n"
        );
    }

    #[test]
    fn test_literal_number() {
        assert_eq!(
            emit_export_type("T", lit_num("42")),
            "export type T = 42;\n"
        );
    }

    #[test]
    fn test_literal_bool() {
        assert_eq!(
            emit_export_type("T", lit_bool(true)),
            "export type T = true;\n"
        );
        assert_eq!(
            emit_export_type("T", lit_bool(false)),
            "export type T = false;\n"
        );
    }

    #[test]
    fn test_array_simple() {
        assert_eq!(
            emit_export_type("T", array(kw(TsStringKeyword))),
            "export type T = string[];\n"
        );
    }

    #[test]
    fn test_array_of_union_adds_parens() {
        let ty = array(union(vec![kw(TsStringKeyword), kw(TsNumberKeyword)]));
        assert_eq!(
            emit_export_type("T", ty),
            "export type T = (string | number)[];\n"
        );
    }

    #[test]
    fn test_record() {
        assert_eq!(
            emit_export_type("T", record(kw(TsStringKeyword))),
            "export type T = Record<string, string>;\n"
        );
    }

    #[test]
    fn test_union() {
        let ty = union(vec![kw(TsStringKeyword), kw(TsNumberKeyword)]);
        assert_eq!(
            emit_export_type("T", ty),
            "export type T = string | number;\n"
        );
    }

    #[test]
    fn test_intersection() {
        let ty = intersection(vec![type_ref("Foo"), type_ref("Bar")]);
        assert_eq!(emit_export_type("T", ty), "export type T = Foo & Bar;\n");
    }

    #[test]
    fn test_nullable() {
        assert_eq!(
            emit_export_type("T", nullable(kw(TsStringKeyword))),
            "export type T = string | null;\n"
        );
    }

    #[test]
    fn test_type_ref_simple() {
        assert_eq!(
            emit_export_type("T", type_ref("Pet")),
            "export type T = Pet;\n"
        );
    }

    #[test]
    fn test_type_ref_qualified() {
        assert_eq!(
            emit_export_type("T", type_ref("Order.Status")),
            "export type T = Order.Status;\n"
        );
    }

    // MARK: Module emission

    #[test]
    fn test_module_with_interface() {
        let comments = TsComments::new();
        let decl = interface_decl(
            "Pet",
            &[],
            vec![
                property_sig("name", false, kw(TsStringKeyword), DUMMY_SP),
                property_sig("age", true, kw(TsNumberKeyword), DUMMY_SP),
            ],
        );
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
    fn test_module_with_interface_extends() {
        let comments = TsComments::new();
        let items = vec![
            import_type_decl(&["Base".to_owned()], "./base"),
            export_decl(
                interface_decl(
                    "Pet",
                    &["Base".to_owned()],
                    vec![property_sig("name", false, kw(TsStringKeyword), DUMMY_SP)],
                ),
                DUMMY_SP,
            ),
        ];
        assert_eq!(
            emit_module(items, &comments),
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
        let comments = TsComments::new();
        let items = vec![export_decl(
            type_alias_decl(
                "Status",
                union(vec![lit_str("active"), lit_str("inactive")]),
            ),
            DUMMY_SP,
        )];
        assert_eq!(
            emit_module(items, &comments),
            "export type Status = \"active\" | \"inactive\";\n"
        );
    }

    #[test]
    fn test_module_with_description() {
        let comments = TsComments::new();
        let span = comments.span_with_jsdoc(Some("The status of a resource."));
        let items = vec![export_decl(
            type_alias_decl("Status", kw(TsStringKeyword)),
            span,
        )];
        assert_eq!(
            emit_module(items, &comments),
            "/** The status of a resource. */ export type Status = string;\n"
        );
    }

    #[test]
    fn test_module_with_namespace() {
        let comments = TsComments::new();
        let iface = interface_decl(
            "Order",
            &[],
            vec![property_sig(
                "status",
                true,
                type_ref("Order.Status"),
                DUMMY_SP,
            )],
        );
        let ns = namespace_decl(
            "Order",
            vec![export_decl(
                type_alias_decl(
                    "Status",
                    union(vec![lit_str("placed"), lit_str("approved")]),
                ),
                DUMMY_SP,
            )],
        );
        let items = vec![export_decl(iface, DUMMY_SP), export_decl(ns, DUMMY_SP)];
        assert_eq!(
            emit_module(items, &comments),
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
        let comments = TsComments::new();
        let items = vec![import_type_decl(
            &["Pet".to_owned(), "Order".to_owned()],
            "./pet",
        )];
        assert_eq!(
            emit_module(items, &comments),
            "import type { Pet, Order } from \"./pet\";\n"
        );
    }

    #[test]
    fn test_reexport_type_output() {
        let comments = TsComments::new();
        let items = vec![reexport_type(&["Pet".to_owned()], "./pet")];
        assert_eq!(
            emit_module(items, &comments),
            "export type { Pet } from \"./pet\";\n"
        );
    }
}
