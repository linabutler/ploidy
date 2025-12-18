use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::parse_quote;

/// Extra derives that can be added to types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtraDerive {
    /// Derive [`Eq`].
    ///
    /// Excluded if the type is unhashable.
    Eq,

    /// Derive [`Hash`][std::hash::Hash].
    ///
    /// Excluded if the type is unhashable.
    Hash,

    /// Derive [`Default`].
    ///
    /// Included if all fields are optional.
    Default,
}

impl ToTokens for ExtraDerive {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let path: syn::Path = match self {
            Self::Eq => parse_quote!(Eq),
            Self::Hash => parse_quote!(Hash),
            Self::Default => parse_quote!(Default),
        };
        path.to_tokens(tokens);
    }
}
