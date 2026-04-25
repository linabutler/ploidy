use std::{collections::BTreeSet, ops::Deref};

use ploidy_core::ir::{CookedGraph, OperationView, SchemaTypeView};
use rustc_hash::FxHashMap;

use crate::CodegenResourceIdent;

use super::{
    config::{CodegenConfig, DateTimeFormat},
    naming::{CodegenOperationIdent, CodegenTypeIdent, UniqueIdent, UniqueIdents},
};

/// A [`CookedGraph`] decorated with Rust-specific information.
#[derive(Debug)]
pub struct CodegenGraph<'a> {
    cooked: CookedGraph<'a>,
    idents: FxHashMap<IdentMapping<'a>, &'a UniqueIdent>,
    date_time_format: DateTimeFormat,
}

impl<'a> CodegenGraph<'a> {
    /// Wraps a type graph with the default configuration.
    pub fn new(cooked: CookedGraph<'a>) -> Self {
        Self::with_config(cooked, &CodegenConfig::default())
    }

    /// Wraps a type graph with the given configuration.
    pub fn with_config(cooked: CookedGraph<'a>, config: &CodegenConfig) -> Self {
        let mut idents = FxHashMap::default();
        idents.extend({
            let mut scope = UniqueIdents::new(cooked.arena());
            cooked
                .schemas()
                .map(move |ty| (IdentMapping::Schema(ty.name()), scope.name(ty.name())))
        });
        idents.extend({
            let mut scope = UniqueIdents::new(cooked.arena());
            cooked
                .operations()
                .map(move |op| (IdentMapping::Operation(op.id()), scope.name(op.id())))
        });
        idents.extend({
            let resources: BTreeSet<_> =
                cooked.operations().filter_map(|op| op.resource()).collect();
            let mut scope = UniqueIdents::with_reserved(cooked.arena(), &["default"]);
            resources
                .into_iter()
                .map(move |name| (IdentMapping::Resource(name), scope.name(name)))
        });

        Self {
            cooked,
            idents,
            date_time_format: config.date_time_format,
        }
    }

    /// Returns an iterator over all named schemas with their type names.
    #[inline]
    pub fn schemas(&self) -> impl Iterator<Item = CodegenSchemaView<'_, 'a>> + use<'_, 'a> {
        self.cooked.schemas().map(|view| {
            let key = IdentMapping::Schema(view.name());
            CodegenSchemaView {
                ident: CodegenTypeIdent::Schema(self.idents[&key]),
                view,
            }
        })
    }

    /// Looks up a schema by name and returns it with its type name.
    #[inline]
    pub fn schema(&self, name: &str) -> Option<CodegenSchemaView<'_, 'a>> {
        let view = self.cooked.schema(name)?;
        let key = IdentMapping::Schema(view.name());
        Some(CodegenSchemaView {
            ident: CodegenTypeIdent::Schema(self.idents[&key]),
            view,
        })
    }

    /// Returns an iterator over all operations with their names.
    #[inline]
    pub fn operations(&self) -> impl Iterator<Item = CodegenOperationView<'_, 'a>> + use<'_, 'a> {
        self.cooked.operations().map(|view| {
            let key = IdentMapping::Operation(view.id());
            CodegenOperationView {
                ident: CodegenOperationIdent(self.idents[&key]),
                view,
            }
        })
    }

    /// Looks up the Rust type name for a resource name.
    #[inline]
    pub fn resource(&self, name: &str) -> Option<CodegenResourceIdent<'a>> {
        let key = IdentMapping::Resource(name);
        let &ident = self.idents.get(&key)?;
        Some(CodegenResourceIdent(ident))
    }

    /// Returns the format to use for `date-time` types.
    #[inline]
    pub fn date_time_format(&self) -> DateTimeFormat {
        self.date_time_format
    }
}

impl<'a> Deref for CodegenGraph<'a> {
    type Target = CookedGraph<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.cooked
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum IdentMapping<'a> {
    Schema(&'a str),
    Operation(&'a str),
    Resource(&'a str),
}

/// A [`SchemaTypeView`] decorated with its Rust type name.
#[derive(Debug)]
pub struct CodegenSchemaView<'graph, 'a> {
    ident: CodegenTypeIdent<'a>,
    view: SchemaTypeView<'graph, 'a>,
}

impl<'graph, 'a> CodegenSchemaView<'graph, 'a> {
    /// Returns the Rust type name for this schema.
    #[inline]
    pub fn ident(&self) -> CodegenTypeIdent<'a> {
        self.ident
    }

    /// Unwraps the view, discarding the codegen name.
    #[inline]
    pub fn into_view(self) -> SchemaTypeView<'graph, 'a> {
        self.view
    }
}

impl<'graph, 'a> Deref for CodegenSchemaView<'graph, 'a> {
    type Target = SchemaTypeView<'graph, 'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

/// An [`OperationView`] decorated with its Rust type name.
#[derive(Debug)]
pub struct CodegenOperationView<'graph, 'a> {
    ident: CodegenOperationIdent<'a>,
    view: OperationView<'graph, 'a>,
}

impl<'graph, 'a> CodegenOperationView<'graph, 'a> {
    /// Returns the codegen name for this operation.
    #[inline]
    pub fn ident(&self) -> CodegenOperationIdent<'a> {
        self.ident
    }
}

impl<'graph, 'a> Deref for CodegenOperationView<'graph, 'a> {
    type Target = OperationView<'graph, 'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.view
    }
}
