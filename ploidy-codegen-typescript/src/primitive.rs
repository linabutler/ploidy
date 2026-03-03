use ploidy_core::ir::PrimitiveIrType;
use quasiquodo_ts::{swc::ecma_ast::TsType, ts_quote};

/// Maps a primitive IR type to a TypeScript type.
pub fn ts_primitive(ty: PrimitiveIrType) -> TsType {
    match ty {
        PrimitiveIrType::String
        | PrimitiveIrType::DateTime
        | PrimitiveIrType::UnixTime
        | PrimitiveIrType::Date
        | PrimitiveIrType::Url
        | PrimitiveIrType::Uuid
        | PrimitiveIrType::Bytes
        | PrimitiveIrType::Binary => ts_quote!("string" as TsType),

        PrimitiveIrType::I8
        | PrimitiveIrType::U8
        | PrimitiveIrType::I16
        | PrimitiveIrType::U16
        | PrimitiveIrType::I32
        | PrimitiveIrType::U32
        | PrimitiveIrType::I64
        | PrimitiveIrType::U64
        | PrimitiveIrType::F32
        | PrimitiveIrType::F64 => ts_quote!("number" as TsType),

        PrimitiveIrType::Bool => ts_quote!("boolean" as TsType),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ploidy_core::codegen::Code;
    use pretty_assertions::assert_eq;
    use quasiquodo_ts::{Comments, swc::ecma_ast::Module};

    use crate::TsSource;

    /// Emits a primitive as `export type T = <prim>;` and returns the
    /// output string.
    fn emit_prim(ty: PrimitiveIrType) -> String {
        let comments = Comments::new();
        let items = vec![ts_quote!(
            "export type T = #{t}" as ModuleItem,
            t: TsType = ts_primitive(ty)
        )];
        TsSource::new(
            String::new(),
            comments,
            Module {
                body: items,
                ..Module::default()
            },
        )
        .into_string()
        .unwrap()
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
