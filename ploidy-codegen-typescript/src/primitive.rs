use ploidy_core::ir::PrimitiveIrType;
use swc_ecma_ast::{TsKeywordTypeKind, TsType};

use super::emit::kw;

/// Maps a primitive IR type to a TypeScript type.
pub fn ts_primitive(ty: PrimitiveIrType) -> Box<TsType> {
    match ty {
        PrimitiveIrType::String
        | PrimitiveIrType::DateTime
        | PrimitiveIrType::UnixTime
        | PrimitiveIrType::Date
        | PrimitiveIrType::Url
        | PrimitiveIrType::Uuid
        | PrimitiveIrType::Bytes
        | PrimitiveIrType::Binary => kw(TsKeywordTypeKind::TsStringKeyword),

        PrimitiveIrType::I8
        | PrimitiveIrType::U8
        | PrimitiveIrType::I16
        | PrimitiveIrType::U16
        | PrimitiveIrType::I32
        | PrimitiveIrType::U32
        | PrimitiveIrType::I64
        | PrimitiveIrType::U64
        | PrimitiveIrType::F32
        | PrimitiveIrType::F64 => kw(TsKeywordTypeKind::TsNumberKeyword),

        PrimitiveIrType::Bool => kw(TsKeywordTypeKind::TsBooleanKeyword),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use swc_common::DUMMY_SP;

    use crate::emit::{TsComments, emit_module, export_decl, type_alias_decl};

    /// Emits a primitive as `export type T = <prim>;` and returns the
    /// output string.
    fn emit_prim(ty: PrimitiveIrType) -> String {
        let comments = TsComments::new();
        let items = vec![export_decl(
            type_alias_decl("T", ts_primitive(ty)),
            DUMMY_SP,
        )];
        emit_module(items, &comments)
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
