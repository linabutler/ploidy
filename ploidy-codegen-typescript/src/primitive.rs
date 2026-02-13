use oxc_ast::AstBuilder;
use oxc_ast::ast::TSType;
use oxc_span::SPAN;
use ploidy_core::ir::PrimitiveIrType;

/// Maps a primitive IR type to a TypeScript type.
pub fn ts_primitive<'a>(ast: &AstBuilder<'a>, ty: PrimitiveIrType) -> TSType<'a> {
    match ty {
        PrimitiveIrType::String
        | PrimitiveIrType::DateTime
        | PrimitiveIrType::UnixTime
        | PrimitiveIrType::Date
        | PrimitiveIrType::Url
        | PrimitiveIrType::Uuid
        | PrimitiveIrType::Bytes
        | PrimitiveIrType::Binary => ast.ts_type_string_keyword(SPAN),

        PrimitiveIrType::I8
        | PrimitiveIrType::U8
        | PrimitiveIrType::I16
        | PrimitiveIrType::U16
        | PrimitiveIrType::I32
        | PrimitiveIrType::U32
        | PrimitiveIrType::I64
        | PrimitiveIrType::U64
        | PrimitiveIrType::F32
        | PrimitiveIrType::F64 => ast.ts_type_number_keyword(SPAN),

        PrimitiveIrType::Bool => ast.ts_type_boolean_keyword(SPAN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxc_allocator::Allocator;
    use pretty_assertions::assert_eq;

    use crate::emit::{TsComments, emit_module, export_decl, type_alias_decl};

    /// Emits a primitive as `export type T = <prim>;` and returns the
    /// output string.
    fn emit_prim(ty: PrimitiveIrType) -> String {
        let allocator = Allocator::default();
        let ast = AstBuilder::new(&allocator);
        let comments = TsComments::new();
        let items = ast.vec1(export_decl(
            &ast,
            type_alias_decl(&ast, "T", ts_primitive(&ast, ty)),
            SPAN,
        ));
        emit_module(&allocator, &ast, items, &comments)
    }

    #[test]
    fn test_string_primitives() {
        assert_eq!(
            emit_prim(PrimitiveIrType::String),
            "export type T = string;\n"
        );
        assert_eq!(
            emit_prim(PrimitiveIrType::DateTime),
            "export type T = string;\n"
        );
        assert_eq!(
            emit_prim(PrimitiveIrType::Date),
            "export type T = string;\n"
        );
        assert_eq!(emit_prim(PrimitiveIrType::Url), "export type T = string;\n");
        assert_eq!(
            emit_prim(PrimitiveIrType::Uuid),
            "export type T = string;\n"
        );
        assert_eq!(
            emit_prim(PrimitiveIrType::Bytes),
            "export type T = string;\n"
        );
        assert_eq!(
            emit_prim(PrimitiveIrType::Binary),
            "export type T = string;\n"
        );
    }

    #[test]
    fn test_number_primitives() {
        assert_eq!(emit_prim(PrimitiveIrType::I32), "export type T = number;\n");
        assert_eq!(emit_prim(PrimitiveIrType::U32), "export type T = number;\n");
        assert_eq!(emit_prim(PrimitiveIrType::I64), "export type T = number;\n");
        assert_eq!(emit_prim(PrimitiveIrType::U64), "export type T = number;\n");
        assert_eq!(emit_prim(PrimitiveIrType::F32), "export type T = number;\n");
        assert_eq!(emit_prim(PrimitiveIrType::F64), "export type T = number;\n");
    }

    #[test]
    fn test_bool_primitive() {
        assert_eq!(
            emit_prim(PrimitiveIrType::Bool),
            "export type T = boolean;\n"
        );
    }
}
