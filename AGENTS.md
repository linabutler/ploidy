# AGENTS.md / CLAUDE.md

Guidance for AI coding agents. Follow these instructions exactly; they override defaults.

**Note**: The canonical name is `AGENTS.md`; `CLAUDE.md` is a symlink.

---

## Quick Reference

| Task | Command / Location |
|------|-------------------|
| **Verify changes** | Run all 4 commands in "Verification Checklist" below |
| **Run all tests** | `cargo test --workspace --no-fail-fast --all-features` |
| **Run single crate tests** | `cargo test -p ploidy-core --no-fail-fast --all-features` |
| **Run single test** | `cargo test -p ploidy-core test_name` |
| **Run CLI** | `cargo run -p ploidy --` |
| **Add workspace dependency** | Add to `[workspace.dependencies]` in root `Cargo.toml` |
| **Find tests** | `src/**/tests/*.rs`, `tests/*.rs`, or `mod tests` |

---

## Starting a New Task

1. **Understand scope.** Identify crates: type system → **ploidy-core**, Rust codegen → **ploidy-codegen-rust**, JSON Pointer → **ploidy-pointer**, runtime → **ploidy-util**.
2. **Identify affected tests.**
3. **Match existing patterns exactly.** Find similar code and copy its structure (types, tests, imports, docs).
4. **Make changes incrementally.** Run `cargo check` as you work.
5. **Run the verification checklist.** Task is not complete until all commands pass.
6. **Ask if uncertain.** See "When to Ask for Help".

---

## Verification Checklist

After making changes, **always** run in order:

```bash
cargo check --workspace
cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged --no-deps
cargo +nightly fmt --all
cargo test --workspace --no-fail-fast --all-features
```

**Task is not complete until all commands pass.** If any fails: fix, re-run from step 1, repeat.

**If failing 3+ times:** Stop, re-read errors carefully, check if failure is in your code or pre-existing, ask for guidance if stuck.

---

## Architecture

Ploidy is an OpenAPI code generator for polymorphic specs (`allOf`, `oneOf`, `anyOf`):

```
Parse → IR (Intermediate Representation) → Codegen
```

### Workspace Crates

| Crate | Purpose |
|-------|---------|
| **ploidy** | CLI entrypoint |
| **ploidy-core** | Language-agnostic IR and type graph |
| **ploidy-codegen-rust** | AST-based Rust code generator (`syn`/`quote`) |
| **ploidy-pointer** | RFC 6901 JSON Pointer for `$ref` resolution |
| **ploidy-pointer-derive** | `#[derive(JsonPointee)]` proc-macro |
| **ploidy-util** | Runtime support for generated clients |

### Key Abstractions

- **`IrSpec`**: Raw IR data (schemas, operations). Created by `IrSpec::from_doc(&doc)`.
- **`IrGraph`**: Wraps `IrSpec` with type graph for traversal and cycle detection. Created by `IrGraph::from_spec(&ir)`. Use this for traversal and view types.
- **View types** (e.g., `IrStructView`, `IrTaggedView`, `IrEnumView`, `IrOperationView`): Graph-aware wrappers providing `inlines()`, `reachable()`, `used_by()`, `needs_indirection()`, and metadata access.
- **Inline type paths**: Anonymous schemas get semantic paths like `Type/Field/MapValue` for stable naming.

### Polymorphic Type Mapping

| OpenAPI | IR Type | Rust Output |
|---------|---------|-------------|
| `allOf` | `IrStruct` with inherited fields | Single struct with linearized ancestor fields |
| `oneOf` + discriminator | `IrTagged` | `#[serde(tag = "...")]` enum |
| `oneOf` without discriminator | `IrUntagged` | `#[serde(untagged)]` enum |
| `anyOf` | `IrStruct` with flattened optionals | Struct with optional flattened fields |

---

## Coding Style

These are requirements, not suggestions. Violations will produce incorrect or unacceptable code. When rules conflict: consistency wins, more specific rules apply, ask if genuinely unclear.

### Type Design

| Pattern | Rule |
|---------|------|
| Context objects | Bundle related data in structs instead of free functions with many params |
| Newtypes | Use to enforce invariants (e.g., `SchemaIdent(String)` for uniquified names) |
| Enums with data | Carry data in variants directly (e.g., `IrType` root type) |
| Symmetry | Similar types follow similar patterns, even if slightly redundant |

### Ownership and Lifetimes

```rust
// ✅ Borrow from source
struct MyView<'a> {
    name: &'a str,
    items: &'a [Item],
}

// ❌ Unnecessary allocation
struct MyView {
    name: String,
    items: Vec<Item>,
}
```

- Use minimal lifetimes. Name semantically: `'a` for data, `'view` for views, `'graph` for graphs.
- Never elide lifetimes that distinguish borrowed sources.

### Data Structures

- `IndexMap` where insertion order matters.
- `FxHashMap` instead of `std::collections::HashMap` (faster for CLI).
- `Box<T>` only to break recursive types; `Vec`/`HashMap` provide their own indirection.

### Documentation (`///`)

- Complete sentences, indicative mood ("Returns", not "Return"), backticks for code items.
- Describe args/returns in prose, never separate sections.
- Wrap at 80 chars.

```rust
// ✅ Indicative mood, inline prose
/// Creates and returns a representation of a feature-gated `impl Client` block
/// for a resource, with all its operations.
pub fn new(resource: &'a str, operations: &'a [IrOperationView<'a>]) -> Self { ... }

// ❌ Imperative mood, separate sections
/// Create a representation of a feature-gated `impl Client` block.
///
/// # Arguments
/// - resource (string): The resource name
/// - operations (list): The operations
///
/// # Returns
/// The representation
pub fn new(resource: &'a str, operations: &'a [IrOperationView<'a>]) -> Self { ... }
```

### Comments (`//`)

- Only for non-obvious logic; never restate code
- Complete sentences with periods; backticks for code items
- `// MARK:` for sections (under 50 chars, no period)

```rust
// ✅ Explains why
// Skip the discriminator field; it's handled separately in tagged unions.
if field.discriminator() { continue; }

// ❌ Restates code
// Check if field is discriminator.
if field.discriminator() { continue; }
```

### Strings

- Raw strings (`r#"..."#`) for strings with quotes
- `.to_owned()` or `.into()` for `&str` → `String`
- `.to_string()` only when formatting (numbers, `Display` types)

### Imports

Order with blank lines between groups:
1. `std::`
2. External crates (alphabetical)
3. `crate::`
4. `super::`

Explicit imports only; no globs except re-exports in `mod.rs`.

### Other

- `#[inline]` on trivial accessors only
- `pub(in crate::path)` for internal constructors
- `thiserror` for errors, `miette` for user-facing diagnostics
- Justify lint suppressions with comments

---

## Testing

### Naming and Organization

- Pattern: `test_<behavior>_<condition>` (e.g., `test_parses_path_parameter_string_type`)
- Group with `// MARK:` comments (no period, under 50 chars)

### No Helper Functions

Inline all fixtures directly in tests. Helpers obscure intent, add noise, and break debugging. Exception: shared utilities in `src/tests.rs` with `#[track_caller]`.

### YAML Fixtures

Always use `indoc::indoc!` for OpenAPI documents:

```rust
let doc = Document::from_yaml(indoc::indoc! {"
    openapi: 3.0.0
    info:
      title: Test API
      version: 1.0.0
    paths: {}
"}).unwrap();
```

Never construct `Document` directly in code.

### Assertions

**Use `assert_matches!`** from `crate::tests` for pattern matching:

```rust
// ✅ Pattern with guard
assert_matches!(ty, IrTypeView::Schema(view) if view.name() == "Cat");

// ✅ Slice patterns
assert_matches!(&*struct_.fields, [field1, field2]);

// ❌ Manual unpacking
assert_eq!(fields[0].name, "name");
assert!(fields[0].required);
```

**Deref coercion** is required for `assert_matches!` and `let-else` with smart pointers:

| You have | You want | Use |
|----------|----------|-----|
| `Vec<T>` | `&[T]` | `&*vec` |
| `Box<T>` | `&T` | `&*boxed` |
| `&Box<T>` | `&T` | `&**ref_to_box` |

```rust
// `&*` for Vec<T> → &[T]
assert_matches!(&*struct_.fields, [field1, field2]);

// `&**` for &Box<T> → &T (e.g., `inner` from `let IrType::Nullable(inner) = ty`)
assert_matches!(&**inner, IrType::Ref(_));
```

**When extracting values**, include actual value in panic:

```rust
// ✅ Helpful error
let IrTypeView::Schema(view) = ty else {
    panic!("expected schema; got `{ty:?}`");
};

// ❌ No context
let IrTypeView::Schema(view) = ty else { panic!() };
```

### Standard Imports

```rust
use itertools::Itertools;  // for collect_vec()

use crate::{
    ir::{IrGraph, IrSpec},
    parse::Document,
    tests::assert_matches,
};
```

---

## Crate-Specific Guidelines

### ploidy

CLI entrypoint. Keep thin; business logic goes in **ploidy-core** or **ploidy-codegen-rust**.

### ploidy-core

- Tests in `src/**/tests/*.rs`
- Use view types for graph operations; raw IR types only during transformation or in view implementations
- Never access `IrGraph.g` directly except in view implementations
- Language-agnostic only: no Rust-specific code (`syn`, naming conventions, etc.)

| Path | Contents |
|------|----------|
| `parse/types.rs` | OpenAPI 3.x structures: `Document`, `Schema`, `Operation`, `ComponentRef` |
| `parse/path.rs` | Path string parsing (`/pets/{petId}` → segments) |
| `ir/types.rs` | Type system: `IrType`, `SchemaIrType`, `InlineIrType`, structs/enums/tagged/untagged |
| `ir/spec.rs` | `IrSpec::from_doc()` transformation |
| `ir/transform.rs` | Schema-to-IR conversion with polymorphic support |
| `ir/graph.rs` | `IrGraph` for dependency analysis and cycle detection |
| `ir/views/*.rs` | View types: `IrStructView`, `IrTaggedView`, `IrEnumView`, field/variant views |
| `ir/tests/*.rs` | Tests organized by module (`graph.rs`, `spec.rs`, `transform.rs`, `views.rs`) |

### ploidy-codegen-rust

- All types implement `ToTokens`
- Use `quote!` for token generation; never string-format Rust code
- Tests compare AST structures with `parse_quote!`, never strings

| Path | Contents |
|------|----------|
| `graph.rs` | `CodegenGraph` wraps `IrGraph` with Rust-specific metadata |
| `naming.rs` | `SchemaIdent`, `CodegenIdent`, case transforms, keyword conflicts |
| `struct_.rs`, `enum_.rs`, `tagged.rs`, `untagged.rs` | Type generators implementing `ToTokens` |
| `schema.rs` | Generates complete schema modules (named type + inlines) |
| `ref_.rs` | Type reference generation |
| `derives.rs` | Determines derive macros for generated types |
| `client.rs`, `resource.rs`, `operation.rs` | API client generation |

### ploidy-pointer

- Follows RFC 6901
- Tests in `src/lib.rs` (`#[cfg(test)]`) and `tests/` directory
- Simpler assertions OK for primitive values

### ploidy-pointer-derive

- Proc-macro constraints apply
- Test via `ploidy-pointer/tests/`

### ploidy-util

- Runtime support for generated clients; keep minimal
- All data types must implement `Serialize`/`Deserialize`
- Key types: `AbsentOr<T>`, `QuerySerializer`, `UnixSeconds`

---

## Dependencies

- Check workspace first; prefer `[workspace.dependencies]` in root `Cargo.toml`
- **ploidy-core** must remain language-agnostic
- Justify new dependencies

---

## Breaking Changes

Ploidy is pre-release. Prefer breaking changes over adding parallel methods. Note affected crates and public APIs.

---

## When to Ask for Help

**Ask when:**
- Requirements are ambiguous
- Multiple valid approaches with tradeoffs
- Tests failing for unclear reasons
- Scope larger than expected
- Merge conflicts arise
- Need to add new workspace crate

**Don't ask when:**
- Task is clear and matches patterns
- Found a bug during implementation (fix and mention)
- Formatting/Clippy issues (fix as part of verification)

---

## Feature Flags

**ploidy-core:** `proc-macro2`, `cargo_toml` (for `Code` trait implementations)

**ploidy-pointer:** `derive` (default), `did-you-mean`, `full`

Gate optional dependencies behind features. Test with `--all-features` and default features.
