# AGENTS.md

Guidance for AI coding agents. Follow exactly; overrides defaults. `CLAUDE.md` is a symlink.

---

## Verification Checklist

After making changes, **always** run in order:

```bash
cargo check --workspace
cargo test --workspace --no-fail-fast --all-features
cargo doc --workspace --no-deps
cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged --no-deps # Auto-fixes lint suggestions
cargo +nightly fmt --all
```

Proofread new documentation and comments for tone and style.

**Task is not complete until all commands pass and proofreader approves.** If any fails: fix, re-run from step 1, repeat.

**If failing 3+ times:** Stop, re-read errors carefully, check if failure is in your code or pre-existing, ask for guidance if stuck.

---

## Architecture

OpenAPI code generator for polymorphic specs (`allOf`/`oneOf`/`anyOf`): Parse → IR → Codegen.

### Workspace Crates

| Crate | Purpose |
|-------|---------|
| **ploidy** | CLI entrypoint (keep thin) |
| **ploidy-core** | Language-agnostic IR and type graph |
| **ploidy-codegen-rust** | AST-based Rust code generator (`syn`/`quote`) |
| **ploidy-pointer** | RFC 6901 JSON Pointer for `$ref` resolution |
| **ploidy-pointer-derive** | `#[derive(JsonPointee)]` proc-macro |
| **ploidy-util** | Runtime support for generated clients |

### Key Abstractions

- **`Arena`**: Bump allocator that owns long-lived data. Types hold `&Arena`-borrowed references, slices, and strings, making them cheaply copyable.
- **`Spec`**: Raw IR data (schemas, operations). Created by `Spec::from_doc(&arena, &doc)`.
- **`RawGraph`** / **`CookedGraph`**: Wrap `Spec` with type graph for traversal, transitive closure, and cycle detection. Created by `RawGraph::new(&arena, &spec)` then `raw.cook()`. Use `RawGraph` for transformations; `CookedGraph` for traversal and view types.
- **View types** (e.g., `StructView`, `TaggedView`, `EnumView`, `OperationView`): Graph-aware wrappers providing `inlines()`, `traverse()`, `used_by()`, `needs_indirection()`, and metadata access.
- **Inline type paths**: Anonymous schemas get semantic paths like `Type/Field/MapValue` for stable naming.

### Polymorphic Type Mapping

| OpenAPI | IR Type | Rust Output |
|---------|---------|-------------|
| `allOf` | `Struct` with inherited fields | Single struct with linearized ancestor fields |
| `oneOf` + discriminator | `Tagged` | `#[serde(tag = "...")]` enum |
| `oneOf` without discriminator | `Untagged` | `#[serde(untagged)]` enum |
| `anyOf` | `Struct` with flattened optionals | Struct with optional flattened fields |

---

## Coding Style

These are requirements, not suggestions. Violations will produce incorrect or unacceptable code. When rules conflict: consistency wins, more specific rules apply, ask if genuinely unclear.

### Type Design

| Pattern | Rule |
|---------|------|
| Context objects | Bundle related data in structs instead of free functions with many params |
| Newtypes | Use to enforce invariants (e.g., `SchemaIdent(String)` for uniquified names) |
| Enums with data | Carry data in variants directly (e.g., `SpecType`, `GraphType`) |
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

- Use semantic names (`'view` for views, `'graph` for graphs) when multiple lifetimes coexist; `'a` is fine for single-lifetime cases.
- Never elide lifetimes that distinguish borrowed sources.

### Data Structures

- `IndexMap` where insertion order matters.
- `FxHash{Map, Set}` instead of `std::collections::Hash{Map, Set}` (HashDoS not a concern).
- `Box<T>` only to break recursive types; `Vec`/`HashMap` provide their own indirection.
- `.collect_vec()` (from `itertools`) instead of `.collect::<Vec<_>>()` or `let v: Vec<_> = … .collect()`.

### Documentation (`///` and `//!`)

**Tone:**
- Match Rust stdlib's tone: precise, terse, declarative. No filler, no hedging, no marketing.
- Proofread with Strunk & White's *Elements of Style*: omit needless words, use active voice, prefer positive statements.

**Style rules:**
- Complete sentences, indicative mood ("Returns", not "Return"), backticks for code items.
- Describe args/returns in prose, never separate sections.
- Document interface (what/why), not implementation details (how).
- Wrap at 80 chars.

```rust
// ✅ Indicative mood, inline prose
/// Creates and returns a representation of a feature-gated `impl Client` block
/// for a resource, with all its operations.
pub fn new(resource: &'a str, operations: &'a [OperationView<'a>]) -> Self { ... }

// ❌ Imperative mood, separate sections
/// Create a representation of a feature-gated `impl Client` block.
///
/// # Arguments
/// - resource (string): The resource name
/// - operations (list): The operations
///
/// # Returns
/// The representation
pub fn new(resource: &'a str, operations: &'a [OperationView<'a>]) -> Self { ... }
```

### Comments (`//`)

- Only for non-obvious logic
- Always complete sentences with periods; backticks for code items
- `// MARK:` for sections (under 50 chars, no period)

```rust
// ✅ Explains why, complete sentence, backticks
// Skip `f.discriminator`; it's handled separately in tagged unions.
if f.discriminator() { continue; }

// ❌ Restates code, sentence fragment, no backticks around `f`
// Check if f is discriminator
if f.discriminator() { continue; }
```

### Strings

- Raw strings (`r#"..."#`) for strings with quotes
- `.to_owned()` for `&str` → `String`
- `.to_string()` only when formatting (numbers, `Display` types)

### Other

- **Imports:** Always add `use` imports; never use inline qualified paths. Rename conflicting imports: `use std::{fmt::Result as FmtResult, io::Result as IoResult}`. Ordered groups (blank lines between): `std::` → external crates (alphabetical) → `crate::` → `super::`. No globs except re-exports in `mod.rs`, `use super::*` in tests.
- `#[inline]` for small `pub` functions
- `pub(in crate::path)` for internal constructors
- `thiserror` for errors, `miette` for user-facing diagnostics
- Justify lint suppressions with comments

---

## Testing

- **Naming:** `test_<behavior>_<condition>`, grouped with `// MARK:` comments.
- **No new helper functions.** Inline all fixtures directly. Use existing helpers, don't add new ones without asking.
- **YAML fixtures:** Always use `Document::from_yaml(indoc::indoc! { ... })` for OpenAPI documents. Never construct `Document` directly.
- **Assertions:** Prefer one structural pattern match with `assert_matches!` over multi-step match/`let-else` chains. Only use `let-else` when the bound variable is needed for subsequent method calls. Include actual value in `let-else` panic messages: `panic!("expected X; got `{ty:?}`")`.
- **Throwaway tests:** When behavior is unclear, write a quick test to prove it rather than theorizing. Delete or convert once done.

---

## Crate-Specific Rules

- **ploidy-core:** Language-agnostic only (no Rust-specific knowledge). Use view types for graph queries. Tests in `src/**/tests/*.rs`.
- **ploidy-codegen-rust:** All types implement `ToTokens`. Use `quote!` for tokens, never string-format. Tests compare AST with `parse_quote!`, never strings.
- **ploidy-pointer:** Follows RFC 6901. Tests in `src/lib.rs` and `tests/`. Simpler assertions OK.
- **ploidy-pointer-derive:** Proc-macro constraints apply. Test via `ploidy-pointer/tests/`.
- **ploidy-util:** Keep minimal. All data types must impl `Serialize`/`Deserialize`. Key types: `AbsentOr<T>`, `QuerySerializer`, `UnixSeconds`.

---

## Process

- **Dependencies:** Prefer `[workspace.dependencies]`. Justify new deps.
- **Breaking changes:** Make breaking changes; don't prioritize backward-compatibility.
- **Design:** Push back or propose alternatives. Keep changes modular for partial reverts. Don't `git revert`; manually restore.
- **Ask for help when:** requirements ambiguous, multiple valid approaches, tests fail for unclear reasons, scope larger than expected, new workspace crate needed, or approach seems wrong.
