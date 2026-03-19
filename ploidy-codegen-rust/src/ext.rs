use ploidy_core::ir::{
    ContainerView, EdgeKind, EnumVariant, EnumView, InlineTypeView, ParameterView, PrimitiveType,
    QueryParameter, Reach, SchemaTypeView, Traversal, TypeView, View,
};

/// Rust-specific extensions to [`View`].
pub(crate) trait ViewExt {
    /// Returns `true` if this type implements `Eq` and `Hash`.
    fn hashable(&self) -> bool;

    /// Returns `true` if this type implements `Default`.
    fn defaultable(&self) -> bool;
}

impl<'a, T: View<'a>> ViewExt for T {
    fn hashable(&self) -> bool {
        self.dependencies().all(|view| match view {
            TypeView::Inline(InlineTypeView::Primitive(_, view))
            | TypeView::Schema(SchemaTypeView::Primitive(_, view)) => {
                !matches!(view.ty(), PrimitiveType::F32 | PrimitiveType::F64)
            }
            _ => true,
        })
    }

    fn defaultable(&self) -> bool {
        self.traverse(Reach::Dependencies, |kind, view| {
            match (kind, view) {
                (
                    EdgeKind::Reference,
                    TypeView::Schema(SchemaTypeView::Container(_, _))
                    | TypeView::Inline(InlineTypeView::Container(_, _)),
                ) => {
                    // All container types implement `Default`;
                    // no need to explore them.
                    Traversal::Ignore
                }
                (
                    EdgeKind::Reference,
                    TypeView::Schema(SchemaTypeView::Struct(_, _))
                    | TypeView::Inline(InlineTypeView::Struct(_, _)),
                ) => {
                    // Structs may or may not implement `Default`, depending on
                    // their fields. Skip the struct itself, but explore all
                    // its fields to determine which ones are defaultable.
                    Traversal::Skip
                }
                (EdgeKind::Inherits, _) => {
                    // Explore parents to check their defaultability.
                    Traversal::Skip
                }
                (EdgeKind::Reference, _) => {
                    // Any other type that this struct references must be defaultable
                    // for this struct to derive `Default`.
                    Traversal::Visit
                }
            }
        })
        .all(|view| {
            match view {
                // `serde_json::Value` implements `Default`.
                TypeView::Inline(InlineTypeView::Any(..))
                | TypeView::Schema(SchemaTypeView::Any(..)) => true,
                // `Url` doesn't implement `Default`, but other primitives do.
                TypeView::Inline(InlineTypeView::Primitive(_, prim))
                | TypeView::Schema(SchemaTypeView::Primitive(_, prim))
                    if matches!(prim.ty(), PrimitiveType::Url) =>
                {
                    false
                }
                TypeView::Inline(InlineTypeView::Primitive(..))
                | TypeView::Schema(SchemaTypeView::Primitive(..)) => true,
                // Representable enums derive `Default` via their
                // `Other` variants; unrepresentable enums become
                // `String` type aliases, which also implement `Default`.
                TypeView::Inline(InlineTypeView::Enum(..))
                | TypeView::Schema(SchemaTypeView::Enum(..)) => true,
                // Other types aren't defaultable.
                _ => false,
            }
        })
    }
}

/// Rust-specific extensions to [`EnumView`].
pub(crate) trait EnumViewExt {
    /// Returns `true` if all variants of this enum can be represented as
    /// unit variants in Rust. Enums with unrepresentable variants become
    /// Rust strings instead.
    fn representable(&self) -> bool;
}

impl EnumViewExt for EnumView<'_> {
    fn representable(&self) -> bool {
        self.variants().iter().all(|variant| match variant {
            // Only non-empty string variants with at least one identifier
            // character are representable as Rust enum variants.
            EnumVariant::String(s) => s.chars().any(unicode_ident::is_xid_continue),
            _ => false,
        })
    }
}

/// Rust-specific extensions to [`ParameterView`].
pub(crate) trait ParameterViewExt {
    /// Returns `true` if the struct field for this parameter
    /// should be wrapped in an [`Option`]. This is the case when
    /// the parameter isn't required and its schema type isn't
    /// already [`Optional`][ContainerView::Optional].
    fn optional(&self) -> bool;
}

impl<'view, 'a> ParameterViewExt for ParameterView<'view, 'a, QueryParameter> {
    fn optional(&self) -> bool {
        !self.required() && !matches!(self.ty().as_container(), Some(ContainerView::Optional(_)))
    }
}
