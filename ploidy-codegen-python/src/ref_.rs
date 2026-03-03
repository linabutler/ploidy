//! Type reference generation for Python type hints.

use ploidy_core::ir::{
    ContainerView, ExtendableView, InlineIrTypeView, IrTypeView, PrimitiveIrType, SchemaIrTypeView,
};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Expr, Identifier},
        text_size::TextRange,
    },
};

use crate::naming::{CodegenIdent, CodegenIdentUsage, CodegenTypeName};

/// Generates a Python type hint expression for an IR type.
pub struct CodegenRef<'a> {
    ty: &'a IrTypeView<'a>,
}

impl<'a> CodegenRef<'a> {
    pub fn new(ty: &'a IrTypeView<'a>) -> Self {
        Self { ty }
    }

    /// Converts the IR type to a Python type hint expression.
    pub fn to_expr(&self) -> Expr {
        match self.ty {
            // Inline container types (array, map, optional).
            IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Array(inner)))
            | IrTypeView::Schema(SchemaIrTypeView::Container(_, ContainerView::Array(inner))) => {
                let inner_ty = inner.ty();
                let inner = CodegenRef::new(&inner_ty).to_expr();
                py_quote!("list[#{inner}]" as Expr, inner: Expr = inner)
            }
            IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Map(inner)))
            | IrTypeView::Schema(SchemaIrTypeView::Container(_, ContainerView::Map(inner))) => {
                let inner_ty = inner.ty();
                let inner = CodegenRef::new(&inner_ty).to_expr();
                py_quote!("dict[str, #{inner}]" as Expr, inner: Expr = inner)
            }
            IrTypeView::Inline(InlineIrTypeView::Container(_, ContainerView::Optional(inner)))
            | IrTypeView::Schema(SchemaIrTypeView::Container(_, ContainerView::Optional(inner))) => {
                let inner_ty = inner.ty();
                let inner = CodegenRef::new(&inner_ty).to_expr();
                // Use Python 3.10+ union syntax: `T | None`.
                py_quote!("#{inner} | None" as Expr, inner: Expr = inner)
            }

            // Primitive types (inline or schema).
            IrTypeView::Inline(InlineIrTypeView::Primitive(_, view))
            | IrTypeView::Schema(SchemaIrTypeView::Primitive(_, view)) => {
                primitive_to_expr(view.ty())
            }

            // Any type (inline or schema).
            IrTypeView::Inline(InlineIrTypeView::Any(_, _))
            | IrTypeView::Schema(SchemaIrTypeView::Any(_, _)) => {
                py_quote!("Any" as Expr)
            }

            // Other inline types are defined in the same module, so no import needed.
            IrTypeView::Inline(ty) => {
                let type_name = CodegenTypeName::Inline(ty).as_class_name();
                py_quote!("#{name}" as Expr, name: Identifier = Identifier::new(&type_name, TextRange::default()))
            }

            // Named schema references.
            IrTypeView::Schema(view) => {
                let ext = view.extensions();
                let ident = ext.get::<CodegenIdent>().unwrap();
                let class_name = CodegenIdentUsage::Class(&ident).display().to_string();
                py_quote!("#{name}" as Expr, name: Identifier = Identifier::new(&class_name, TextRange::default()))
            }
        }
    }
}

/// Converts a primitive IR type to a Python type hint expression.
fn primitive_to_expr(ty: PrimitiveIrType) -> Expr {
    match ty {
        PrimitiveIrType::String => py_quote!("str" as Expr),
        PrimitiveIrType::I8
        | PrimitiveIrType::U8
        | PrimitiveIrType::I16
        | PrimitiveIrType::U16
        | PrimitiveIrType::I32
        | PrimitiveIrType::U32
        | PrimitiveIrType::I64
        | PrimitiveIrType::U64 => py_quote!("int" as Expr),
        PrimitiveIrType::F32 | PrimitiveIrType::F64 => py_quote!("float" as Expr),
        PrimitiveIrType::Bool => py_quote!("bool" as Expr),
        PrimitiveIrType::DateTime | PrimitiveIrType::UnixTime => {
            py_quote!("datetime.datetime" as Expr)
        }
        PrimitiveIrType::Date => py_quote!("datetime.date" as Expr),
        PrimitiveIrType::Url => {
            // URLs are strings in Python (no standard URL type).
            py_quote!("str" as Expr)
        }
        PrimitiveIrType::Uuid => py_quote!("UUID" as Expr),
        PrimitiveIrType::Bytes | PrimitiveIrType::Binary => py_quote!("bytes" as Expr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::generate_expr_source;
    use pretty_assertions::assert_eq;

    fn expr_to_source(expr: &Expr) -> String {
        generate_expr_source(expr)
    }

    // MARK: Primitives

    #[test]
    fn test_primitive_string() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::String)),
            "str"
        );
    }

    #[test]
    fn test_primitive_i32() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::I32)),
            "int"
        );
    }

    #[test]
    fn test_primitive_i64() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::I64)),
            "int"
        );
    }

    #[test]
    fn test_primitive_f32() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::F32)),
            "float"
        );
    }

    #[test]
    fn test_primitive_f64() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::F64)),
            "float"
        );
    }

    #[test]
    fn test_primitive_bool() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::Bool)),
            "bool"
        );
    }

    #[test]
    fn test_primitive_datetime() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::DateTime)),
            "datetime.datetime"
        );
    }

    #[test]
    fn test_primitive_date() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::Date)),
            "datetime.date"
        );
    }

    #[test]
    fn test_primitive_url() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::Url)),
            "str"
        );
    }

    #[test]
    fn test_primitive_uuid() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::Uuid)),
            "UUID"
        );
    }

    #[test]
    fn test_primitive_bytes() {
        assert_eq!(
            expr_to_source(&primitive_to_expr(PrimitiveIrType::Bytes)),
            "bytes"
        );
    }
}
