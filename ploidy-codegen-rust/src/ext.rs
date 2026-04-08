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
        self.traverse(Reach::Dependencies, |kind, view| match kind {
            EdgeKind::Inherits => match view {
                // A tagged or untagged variant "is-a" member of a union,
                // it doesn't "have-a" union. Yield the union itself
                // to check its common fields, but don't explore its
                // other variants.
                TypeView::Schema(SchemaTypeView::Tagged(..) | SchemaTypeView::Untagged(..))
                | TypeView::Inline(InlineTypeView::Tagged(..) | InlineTypeView::Untagged(..)) => {
                    Traversal::Stop
                }

                // For other parent types (e.g., structs), skip the type
                // itself, but explore its neighbors.
                _ => Traversal::Skip,
            },
            EdgeKind::Reference => Traversal::Visit,
        })
        .all(|(kind, view)| match (kind, view) {
            // We're a variant, and this is our union.
            // Check the common fields that we inherit.
            (
                EdgeKind::Inherits,
                TypeView::Schema(SchemaTypeView::Tagged(_, tagged))
                | TypeView::Inline(InlineTypeView::Tagged(_, tagged)),
            ) => tagged
                .fields()
                .map(|f| UnionFieldTypeExt(f.ty()))
                .all(|v| v.hashable()),
            (
                EdgeKind::Inherits,
                TypeView::Schema(SchemaTypeView::Untagged(_, untagged))
                | TypeView::Inline(InlineTypeView::Untagged(_, untagged)),
            ) => untagged
                .fields()
                .map(|f| UnionFieldTypeExt(f.ty()))
                .all(|v| v.hashable()),

            // Floating-point numbers aren't `Eq` or `Hash`. If we
            // transitively contain one, we aren't hashable.
            (
                EdgeKind::Reference,
                TypeView::Inline(InlineTypeView::Primitive(_, p))
                | TypeView::Schema(SchemaTypeView::Primitive(_, p)),
            ) if matches!(p.ty(), PrimitiveType::F32 | PrimitiveType::F64) => false,

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
                (
                    EdgeKind::Inherits,
                    TypeView::Schema(SchemaTypeView::Tagged(..) | SchemaTypeView::Untagged(..))
                    | TypeView::Inline(InlineTypeView::Tagged(..) | InlineTypeView::Untagged(..)),
                ) => {
                    // A tagged or untagged variant "is-a" member of a union,
                    // it doesn't "have-a" union. Yield the union itself
                    // to check its common fields, but don't explore its
                    // other variants.
                    Traversal::Stop
                }
                (EdgeKind::Inherits, _) => {
                    // Struct inheritance: explore inherited fields.
                    Traversal::Skip
                }
                (EdgeKind::Reference, _) => {
                    // Any other type that this struct references must be
                    // defaultable for this struct to derive `Default`.
                    Traversal::Visit
                }
            }
        })
        .all(|(kind, view)| {
            match (kind, view) {
                // We're a variant, and this is our union.
                // Check the common fields that we inherit.
                (
                    EdgeKind::Inherits,
                    TypeView::Schema(SchemaTypeView::Tagged(_, tagged))
                    | TypeView::Inline(InlineTypeView::Tagged(_, tagged)),
                ) => tagged
                    .fields()
                    .map(|f| UnionFieldTypeExt(f.ty()))
                    .all(|f| f.defaultable()),
                (
                    EdgeKind::Inherits,
                    TypeView::Schema(SchemaTypeView::Untagged(_, untagged))
                    | TypeView::Inline(InlineTypeView::Untagged(_, untagged)),
                ) => untagged
                    .fields()
                    .map(|f| UnionFieldTypeExt(f.ty()))
                    .all(|f| f.defaultable()),

                // `Url` doesn't implement `Default`. If we transitively
                // contain one, we aren't defaultable.
                (
                    EdgeKind::Reference,
                    TypeView::Inline(InlineTypeView::Primitive(_, p))
                    | TypeView::Schema(SchemaTypeView::Primitive(_, p)),
                ) => !matches!(p.ty(), PrimitiveType::Url),

                // Tagged and untagged unions aren't defaultable. If we
                // transitively contain one, we aren't defaultable.
                (
                    EdgeKind::Reference,
                    TypeView::Schema(SchemaTypeView::Tagged(..) | SchemaTypeView::Untagged(..))
                    | TypeView::Inline(InlineTypeView::Tagged(..) | InlineTypeView::Untagged(..)),
                ) => false,

                _ => true,
            }
        })
    }
}

/// Implements [`ViewExt`] for a tagged or untagged union's
/// common field type.
#[derive(Debug)]
struct UnionFieldTypeExt<'a>(TypeView<'a>);

impl<'a> ViewExt for UnionFieldTypeExt<'a> {
    fn hashable(&self) -> bool {
        !self
            .0
            .dependencies()
            .chain(std::iter::once(self.0.reborrow()))
            .any(|view| match view {
                TypeView::Inline(InlineTypeView::Primitive(_, p))
                | TypeView::Schema(SchemaTypeView::Primitive(_, p)) => {
                    matches!(p.ty(), PrimitiveType::F32 | PrimitiveType::F64)
                }
                _ => false,
            })
    }

    fn defaultable(&self) -> bool {
        !self
            .0
            .dependencies()
            .chain(std::iter::once(self.0.reborrow()))
            .any(|view| match view {
                // `Url` doesn't implement `Default`; all other primitives do.
                TypeView::Inline(InlineTypeView::Primitive(_, p))
                | TypeView::Schema(SchemaTypeView::Primitive(_, p)) => {
                    matches!(p.ty(), PrimitiveType::Url)
                }
                // Tagged and untagged unions aren't defaultable;
                // there's no meaningful default variant.
                TypeView::Schema(SchemaTypeView::Tagged(..) | SchemaTypeView::Untagged(..))
                | TypeView::Inline(InlineTypeView::Tagged(..) | InlineTypeView::Untagged(..)) => {
                    true
                }
                // All other types are transparent; their transitive
                // dependencies determine whether they're defaultable.
                _ => false,
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
