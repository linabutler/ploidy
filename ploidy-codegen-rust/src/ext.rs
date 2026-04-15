use ploidy_core::ir::{
    ContainerView, EnumVariant, EnumView, ParameterView, QueryParameter, StructFieldView,
    TaggedFieldView, TypeView, UntaggedFieldView,
};

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

/// Rust-specific extensions to struct, tagged, and untagged union field views.
pub(crate) trait FieldViewExt<'a> {
    /// Returns the inner type after peeling all `Optional` layers
    /// (e.g., `Optional(T)`, `Optional(Optional(T))` both return `T`).
    fn inner(&self) -> TypeView<'a>;
}

/// Peels all [`ContainerView::Optional`] wrapper layers from a type.
fn peel<'a>(mut ty: TypeView<'a>) -> TypeView<'a> {
    while let Some(ContainerView::Optional(inner)) = ty.as_container() {
        ty = inner.ty();
    }
    ty
}

impl<'view, 'a> FieldViewExt<'a> for StructFieldView<'view, 'a> {
    fn inner(&self) -> TypeView<'a> {
        peel(self.ty())
    }
}

impl<'view, 'a> FieldViewExt<'a> for TaggedFieldView<'view, 'a> {
    fn inner(&self) -> TypeView<'a> {
        peel(self.ty())
    }
}

impl<'view, 'a> FieldViewExt<'a> for UntaggedFieldView<'view, 'a> {
    fn inner(&self) -> TypeView<'a> {
        peel(self.ty())
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
