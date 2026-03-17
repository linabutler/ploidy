use ploidy_core::ir::{
    EdgeKind, EnumVariant, EnumView, InlineTypeView, PrimitiveType, Reach, SchemaTypeView,
    Traversal, TypeView, View,
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
                    EdgeKind::Reference | EdgeKind::Inherits,
                    TypeView::Schema(SchemaTypeView::Struct(_, view))
                    | TypeView::Inline(InlineTypeView::Struct(_, view)),
                ) => {
                    // Structs may or may not implement `Default`,
                    // depending on their fields. If this struct
                    // inherits fields from another struct,
                    // we need to consider that struct's fields, too.
                    if view.fields().filter(|f| !f.tag()).all(|f| !f.required()) {
                        // If all non-tag fields of all reachable structs are optional,
                        // then this struct can derive `Default`.
                        Traversal::Ignore
                    } else {
                        // Otherwise, skip the struct itself, but visit all its fields
                        // to determine which ones are defaultable.
                        Traversal::Skip
                    }
                }
                (EdgeKind::Inherits, _) => {
                    // Inheriting from a non-struct type isn't semantically meaningful
                    // because the parent doesn't contribute any fields, so we can
                    // ignore it for the purposes of deriving `Default`.
                    Traversal::Ignore
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
