//! Utilities for grouping and sorting imports, inspired by
//! the Python `isort` utility, and Ruff's `Isort` linter.
//!
//! Each codegen type includes its own imports independently,
//! which may produce duplicate imports in the combined module.
//! This module provides the [`isort`] function, which rewrites
//! the module's syntax tree to consolidate, deduplicate, and sort
//! all its import statements.

use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use ploidy_core::ir::{
    ExtendableView, InlineIrTypeView, IrTypeView, PrimitiveIrType, SccId, SchemaIrTypeView, View,
    ViewNode,
};
use quasiquodo_py::{
    py_quote,
    ruff::{
        python_ast::{Alias, AtomicNodeIndex, Identifier, Stmt, StmtImportFrom, Suite},
        text_size::TextRange,
    },
};

use super::naming::{CodegenIdent, CodegenIdentUsage};

/// Bundles the SCC identity and module-name mapping needed to resolve
/// cross-SCC imports during codegen.
#[derive(Clone, Copy, Debug)]
pub struct ImportContext<'a> {
    pub this_scc: SccId,
    pub scc_module_names: &'a BTreeMap<SccId, String>,
}

impl<'a> ImportContext<'a> {
    pub fn new(this_scc: SccId, scc_module_names: &'a BTreeMap<SccId, String>) -> Self {
        Self {
            this_scc,
            scc_module_names,
        }
    }
}

/// Returns the import statements needed by a type's transitive
/// dependencies.
///
/// Returns a [`Suite`] of all the import statements
/// for a type's transitive dependencies.
///
/// The suite may contain duplicates; [`isort`] filters them out.
pub fn collect_imports<'a>(ty: &impl View<'a>, context: ImportContext<'_>) -> Suite {
    ty.dependencies()
        .filter_map(|ty| match ty {
            IrTypeView::Inline(InlineIrTypeView::Primitive(_, prim))
            | IrTypeView::Schema(SchemaIrTypeView::Primitive(_, prim)) => match prim.ty() {
                PrimitiveIrType::DateTime | PrimitiveIrType::UnixTime | PrimitiveIrType::Date => {
                    Some(py_quote!("import datetime" as Stmt))
                }
                PrimitiveIrType::Uuid => Some(py_quote!("from uuid import UUID" as Stmt)),
                _ => None,
            },

            IrTypeView::Inline(InlineIrTypeView::Any(..))
            | IrTypeView::Schema(SchemaIrTypeView::Any(..)) => {
                Some(py_quote!("from typing import Any" as Stmt))
            }

            IrTypeView::Schema(sv) if sv.scc_id() != context.this_scc => {
                let ident = sv.extensions().get::<CodegenIdent>()?;
                let class = CodegenIdentUsage::Class(&ident).display().to_string();
                let module = &context.scc_module_names[&sv.scc_id()];
                Some(py_quote!(
                    "from .#{m} import #{n}" as Stmt,
                    m: Identifier = Identifier::new(module, TextRange::default()),
                    n: Alias = py_quote!(
                        "#{n}" as Alias,
                        n: Identifier = Identifier::new(&class, TextRange::default())
                    )
                ))
            }

            _ => None,
        })
        .collect()
}

/// A key for grouping and sorting imports in a [`Suite`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ImportKey<'a> {
    /// A future statement: `from __future__ import ...`.
    Future,
    /// Imports like `import datetime`, `import pydantic`, `import typing`,
    /// and so on; sorted lexicographically by module name.
    Absolute(&'a str),
    /// Imports like `from enum import ...`, `from uuid import ...`, and so on;
    /// grouped by module name, and sorted lexicographically.
    AbsoluteFrom(&'a str),
    /// Relative imports like `from . import ...`, `from .. import ...`,
    /// `from .module import ...`, `from ..module import ...`, and so on;
    /// grouped by `.` level and name, and sorted according to level and name.
    RelativeFrom(u32, RelativeImportKey<'a>),
}

impl<'a> From<&'a StmtImportFrom> for ImportKey<'a> {
    fn from(stmt: &'a StmtImportFrom) -> Self {
        let Some(module) = stmt.module.as_ref().map(|m| m.as_str()) else {
            return ImportKey::RelativeFrom(stmt.level, RelativeImportKey::Root);
        };

        if stmt.level > 0 {
            return ImportKey::RelativeFrom(stmt.level, RelativeImportKey::Named(module));
        }

        match module {
            "__future__" => ImportKey::Future,
            other => ImportKey::AbsoluteFrom(other),
        }
    }
}

/// The module name suffix of a relative import, following the `.`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RelativeImportKey<'a> {
    Root,
    Named(&'a str),
}

/// Groups and sorts import statements in a [`Suite`].
///
/// Partitions the suite into import and non-import statements,
/// merges `from...import` statements that share the same module,
/// sorts the imports in [canonical order][`ImportKey`], and
/// moves them before the non-import statements.
pub fn isort(suite: &mut Suite) {
    let (imports, non_imports) = {
        // Partition the statements in-place.
        let mut rest = std::mem::take(suite);
        let imports = rest
            .extract_if(.., |stmt| {
                matches!(stmt, Stmt::Import(_) | Stmt::ImportFrom(_))
            })
            .collect_vec();
        (imports, rest)
    };

    let imports = {
        let mut grouping: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for stmt in &imports {
            match stmt {
                Stmt::Import(import) => {
                    for alias in &import.names {
                        let key = ImportKey::Absolute(alias.name.as_str());
                        grouping.entry(key).or_default();
                    }
                }
                Stmt::ImportFrom(import_from) => {
                    let key = ImportKey::from(import_from);
                    grouping
                        .entry(key)
                        .or_default()
                        .extend(import_from.names.iter().map(|alias| alias.name.as_str()));
                }
                // Impossible; we only extracted `import` and `from...import`
                // statements above.
                _ => continue,
            }
        }
        grouping
    };

    // Emit consolidated imports in sorted order.
    suite.extend(imports.into_iter().map(|(key, names)| {
        let names: Vec<Alias> = names
            .into_iter()
            .map(|n| {
                py_quote!(
                    "#{name}" as Alias,
                    name: Identifier = Identifier::new(n, TextRange::default()),
                )
            })
            .collect();
        match key {
            ImportKey::Absolute(module) => py_quote!(
                "import #{m}" as Stmt,
                m: Identifier = Identifier::new(module, TextRange::default())
            ),
            ImportKey::RelativeFrom(level, name) => {
                let module = match name {
                    RelativeImportKey::Root => None,
                    RelativeImportKey::Named(name) => {
                        Some(Identifier::new(name, TextRange::default()))
                    }
                };
                // We construct an `Stmt::ImportFrom` directly here,
                // because there's no `py_quote!` variable type to express
                // "repeat `.` `level` times".
                Stmt::ImportFrom(StmtImportFrom {
                    node_index: AtomicNodeIndex::NONE,
                    range: TextRange::default(),
                    module,
                    names,
                    level,
                })
            }
            ImportKey::Future => py_quote!(
                "from __future__ import #{names}" as Stmt,
                names: Vec<Alias> = names,
            ),
            ImportKey::AbsoluteFrom(name) => py_quote!(
                "from #{m} import #{names}" as Stmt,
                m: Identifier = Identifier::new(name, TextRange::default()),
                names: Vec<Alias> = names,
            ),
        }
    }));

    // ...Then append the non-import statements in their original order.
    suite.extend(non_imports);
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    use super::*;

    use crate::generate_source;

    #[test]
    fn test_consolidate_deduplicates_and_sorts() {
        let mut suite: Suite = vec![
            py_quote!("from pydantic import BaseModel" as Stmt),
            py_quote!("import datetime" as Stmt),
            py_quote!("x = 1" as Stmt),
            py_quote!("from pydantic import Field" as Stmt),
            py_quote!("from typing import Annotated" as Stmt),
            py_quote!("y = 2" as Stmt),
            py_quote!("from pydantic import BaseModel" as Stmt),
            py_quote!("z = 3" as Stmt),
        ];

        isort(&mut suite);
        let source = generate_source(&suite);

        assert_eq!(
            source,
            indoc! {"
                import datetime
                from pydantic import BaseModel, Field
                from typing import Annotated
                x = 1
                y = 2
                z = 3"
            },
        );
    }

    #[test]
    fn test_consolidate_future_first() {
        let mut suite: Suite = vec![
            py_quote!("from pydantic import BaseModel" as Stmt),
            py_quote!("from __future__ import annotations" as Stmt),
        ];

        isort(&mut suite);
        let source = generate_source(&suite);

        assert_eq!(
            source,
            indoc! {"
                from __future__ import annotations
                from pydantic import BaseModel"
            },
        );
    }

    #[test]
    fn test_consolidate_relative_imports_sorted() {
        let mut suite: Suite = vec![
            py_quote!("from .zebra import Z" as Stmt),
            py_quote!("from .alpha import A" as Stmt),
        ];

        isort(&mut suite);
        let source = generate_source(&suite);

        assert_eq!(
            source,
            indoc! {"
                from .alpha import A
                from .zebra import Z"
            },
        );
    }

    #[test]
    fn test_consolidate_merges_relative_imports() {
        let mut suite: Suite = vec![
            py_quote!("from .pet import Pet" as Stmt),
            py_quote!("from .pet import Owner" as Stmt),
        ];

        isort(&mut suite);
        let source = generate_source(&suite);

        assert_eq!(
            source,
            indoc! {"
                from .pet import Owner, Pet"
            },
        );
    }
}
