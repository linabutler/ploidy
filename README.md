# Ploidy

[<img src="https://img.shields.io/crates/v/ploidy?style=for-the-badge&logo=rust" alt="crates.io" height="24">](https://crates.io/crates/ploidy)
[<img src="https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge&logo=github" alt="Build status" height="24">](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[<img src="https://img.shields.io/docsrs/ploidy-codegen-rust/latest?style=for-the-badge&label=codegen-rust&logo=docs.rs" alt="ploidy-codegen-rust Documentation" height="24">](https://docs.rs/ploidy-codegen-rust)
[<img src="https://img.shields.io/docsrs/ploidy-core/latest?style=for-the-badge&label=core&logo=docs.rs" alt="ploidy-core Documentation" height="24">](https://docs.rs/ploidy-core)
[<img src="https://img.shields.io/docsrs/ploidy-pointer/latest?style=for-the-badge&label=pointer&logo=docs.rs" alt="ploidy-pointer Documentation" height="24">](https://docs.rs/ploidy-pointer)
[<img src="https://img.shields.io/docsrs/ploidy-util/latest?style=for-the-badge&label=util&logo=docs.rs" alt="ploidy-util Documentation" height="24">](https://docs.rs/ploidy-util)

Ploidy is a polymorphism-first OpenAPI compiler. It generates Rust that reads like it was written by hand, even for specs with inheritance, recursive types, and inline schemas.

## Table of Contents

* [Getting Started](#getting-started)
  - [Minimum supported Rust version](#minimum-supported-rust-version)
* [Generating Code](#generating-code)
  - [Rust](#rust)
    * [Options](#options)
    * [Advanced options](#advanced-options)
    * [Minimum Rust version for generated code](#minimum-rust-version-for-generated-code)
* [Why Ploidy?](#why-ploidy)
  - [Choosing the right tool](#choosing-the-right-tool)
  - [Polymorphism first](#polymorphism-first)
  - [Fast and correct](#fast-and-correct)
  - [Code like what you'd write by hand](#code-like-what-youd-write-by-hand)
  - [Per-resource feature gates](#per-resource-feature-gates)
* [Under the Hood](#under-the-hood)
  - [The generation pipeline](#the-generation-pipeline)
  - [AST-based codegen](#ast-based-codegen)
  - [Smart boxing](#smart-boxing)
  - [Inline schemas](#inline-schemas)
  - [Cargo features](#cargo-features)
* [Contributing](#contributing)
  - [New languages](#new-languages)
* [Acknowledgments](#acknowledgments)

## Getting Started

[Download a pre-built binary of Ploidy for your platform](https://github.com/linabutler/ploidy/releases/latest), or install Ploidy via [**cargo-binstall**](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall ploidy
```

Or, to install from source:

```sh
cargo install --locked ploidy
```

> [!TIP]
> The `-linux-musl` binaries are statically linked with [musl](https://www.musl-libc.org/intro.html), and are a good choice for running Ploidy on CI platforms like GitHub Actions.

### Minimum supported Rust version

Ploidy's minimum supported Rust version (MSRV) is **Rust 1.89.0**. This only applies if you're installing from source, or depending on one of the **ploidy-\*** packages as a library. We may increase the MSRV in minor releases.

> [!NOTE]
> Generated Rust code has [a different MSRV](#minimum-rust-version-for-generated-code).

## Generating Code

### Rust

To generate a complete Rust client crate from your OpenAPI spec, run:

```sh
ploidy generate rust /path/to/spec.yaml
```

This produces a crate that includes:

* A `Cargo.toml` file, which you can extend with additional metadata, dependencies, or examples. For specs with resource annotations, the generated `Cargo.toml` includes [per-resource feature gates](#per-resource-feature-gates).
* A `types` module, with type definitions for each schema in your spec.
* A `client` module, with async methods for every operation in your spec.

#### Options

| Flag | Description |
|------|-------------|
| `-o`, `--output` | Set the output directory for the generated crate |
| `-c`, `--check` | Verify the generated crate compiles |
| `--name <NAME>` | Set the crate name. Defaults to `package.name` from the output directory's `Cargo.toml` if present, or the output directory name |
| `--version <bump-major \| bump-minor \| bump-patch>` | Increment the major, minor, or patch component of the existing `package.version`. If not passed, use the existing version, or 0.1.0 for new crates |

#### Advanced options

Ploidy reads additional options from `[package.metadata.ploidy]` in the generated crate's `Cargo.toml`:

| Key | Values | Default | Description |
|-----|--------|---------|-------------|
| `date-time-format` | `rfc3339`, [`unix-seconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixSeconds.html), [`unix-milliseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixMilliseconds.html), [`unix-microseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixMicroseconds.html), [`unix-nanoseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixNanoseconds.html) | `rfc3339` | How `date-time` types are represented |

For example:

```toml
[package.metadata.ploidy]
# Use `ploidy_util::UnixSeconds`, which parses `date-time` types as
# strings containing Unix timestamps in seconds.
date-time-format = "unix-seconds"
# date-time-format = "unix-milliseconds"  # Use `ploidy_util::UnixMilliseconds`.
# date-time-format = "unix-microseconds"  # Use `ploidy_util::UnixMicroseconds`.
# date-time-format = "unix-nanoseconds"   # Use `ploidy_util::UnixNanoseconds`.
# date-time-format = "rfc3339"            # Use `chrono::DateTime<Utc>` (RFC 3339 / ISO 8601 strings).
```

#### Minimum Rust version for generated code

The MSRV for the generated crate is **Rust 1.86.0**.

## Why Ploidy?

Ploidy is a good fit if:

* Your OpenAPI spec uses `allOf`, `oneOf`, or `anyOf`.
* You have a large or complex spec that's challenging for other generators.
* Your spec has many inline schemas, and you want the same strongly-typed models for them as for named schemas.
* Your spec has recursive or cyclic types.
* Your spec has [resource annotations](#cargo-features), and you want consumers to compile just the types and operations they need.
* Your spec uses OpenAPI 3.1 features like `type` arrays and sibling keywords alongside `$ref`.
* You want to generate Rust that reads like you wrote it.

### Choosing the right tool

The OpenAPI ecosystem has many options for different needs. Here's how to pick:

| If you need... | Consider |
|----------------|----------|
| **Custom templates or a different HTTP client** | A template-based generator like **openapi-generator** or [**Schema Tools**](https://github.com/kstasik/schema-tools), which offer more control over output |
| **Languages other than Rust** | **openapi-generator**, or [**swagger-codegen**](https://github.com/swagger-api/swagger-codegen) for OpenAPI < 3.1 |
| **OpenAPI 2.0 (Swagger) support** | **openapi-generator** or **swagger-codegen** |
| **Server stubs** | **openapi-generator** for Rust web frameworks, or [**Dropshot**](https://github.com/oxidecomputer/dropshot) for generating specs from Rust definitions |

Ploidy is opinionated by design. We'd rather get the defaults right than expose a page of configuration options. If you need a feature that isn't supported yet, please [open an issue](https://github.com/linabutler/ploidy/issues/new)—it helps shape our roadmap!

### Polymorphism first

Ploidy has first-class support for inheritance and polymorphism:

* ✅ **`allOf`**: Structs with fields linearized from all parent schemas.
* ✅ **`oneOf` with discriminator**: Enums with named newtype variants for each mapping, represented as an [internally tagged](https://serde.rs/enum-representations.html#internally-tagged) Serde enum.
* ✅ **`oneOf` without discriminator**: Enums with automatically named (`V1`, `V2`, `V3`...) variants for each subschema, represented as an [untagged](https://serde.rs/enum-representations.html#untagged) Serde enum.
* ✅ **`anyOf`**, with or without discriminator: Structs with optional [flattened fields](https://serde.rs/attr-flatten.html) for each subschema.

### Fast and correct

Ploidy aims to generate crates that compile as-is, without a post-processing step, and without sacrificing speed:

| Spec | Types | Operations | Generation time |
|------|-------|------------|------|
| Machinify | ~3,800 | ~1,400 | <2s |
| [Stripe](https://github.com/stripe/openapi) | ~1,400 | ~600 | <2s |
| [GitHub](https://github.com/github/rest-api-description) | ~900 | ~1,100 | <2s |
| [OpenAI](https://github.com/openai/openai-openapi) | ~900 | ~240 | <1s |
| Anthropic | ~650 | ~90 | <1s |

(Measured with [Hyperfine](https://github.com/sharkdp/hyperfine) on a 2021 M1 MacBook Pro).

### Code like what you'd write by hand

Generated code looks like it was written by an experienced Rust developer:

* **[Serde](https://serde.rs)-compatible type definitions**: Structs for `object` types and `anyOf` schemas, enums with data for `oneOf` schemas, unit-only enums for string `enum` types.
* **Built-in trait implementations** for generated types: `From<T>` for polymorphic enum variants; `FromStr` and `Display` for string enums.
* **Standard derives** for all types, plus `Hash`, `Eq`, and `Default` for types that support them.
* **Boxing** for recursive types.
* **A RESTful client with async endpoints**, using [Reqwest](https://docs.rs/reqwest) with the [Tokio](https://tokio.rs) runtime.

For example, given this schema:

```yaml
Customer:
  type: object
  required: [id, email]
  properties:
    id:
      type: string
    email:
      type: string
    name:
      type: string
```

Ploidy generates:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonPointee, JsonPointerTarget)]
pub struct Customer {
    pub id: String,
    pub email: String,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub name: AbsentOr<String>,
}
```

### Per-resource feature gates

Large OpenAPI specs can define hundreds of API resources, but most consumers only use a handful. Ploidy generates [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html) for each resource, so your crates can compile just the types and client methods that they need.

For example, given a spec with `Customer`, `Order`, and `BillingInfo` schemas, where `Customer` references `BillingInfo`, and `Order` references both, Ploidy generates:

```toml
[features]
default = ["billing-info", "customer", "order"]
billing-info = []
customer = ["billing-info"]
order = ["customer"]
```

All features are enabled by default, so the generated crate works out of the box. Consumers that only need a subset of the API can pick the specific features they need:

```toml
[dependencies]
my-api = { version = "1", default-features = false, features = ["customer"] }
```

This compiles just the `Customer` type—and its dependency, `BillingInfo`—along with the client methods for customer operations. Types and methods for other resources are excluded entirely, reducing compile times and binary size for large specs.

## Under the Hood

Ploidy takes a different approach to code generation. If you're curious about how it works, this section is for you!

### The generation pipeline

Ploidy processes an OpenAPI spec in three stages:

📝 **Parsing** a JSON or YAML OpenAPI spec into Rust data structures. The parser is intentionally forgiving; Ploidy accepts malformed specs that stricter validators reject.

🏗️ **Constructing an IR** (intermediate representation). Ploidy builds a type graph from the spec, which lets it answer questions like "which types can derive `Eq`, `Hash`, and `Default`?" and "which fields need `Box<T>` to break cycles?"

✍️ **Generating code** from the IR. Ploidy creates Rust syntax trees from the type graph, prettifies the code, and writes it to disk.

### AST-based codegen

Ploidy builds Rust **syntax trees** directly with [`syn`](https://docs.rs/syn) and [`quote`](https://docs.rs/quote), rather than assembling code from string templates. This has two benefits:

* **Generated code is syntactically valid by construction.** Nodes are typed `syn` values, built with `parse_quote!` and friends. Ploidy can't produce a crate with mismatched delimiters or malformed attributes.
* **Complex types compose cleanly.** Trait bounds, attribute macros, and nested generics combine as tokens, not concatenated strings. The generator never juggles whitespace or escaping, so hard-to-generate constructs are as reliable as simple ones.

Once the tree is built, [`prettyplease`](https://docs.rs/prettyplease) formats it into the final output.

### Smart boxing

Schemas that represent graph- and tree-like structures typically contain circular references: a `User` might have `friends: Vec<User>`; a `Comment` might have a `parent: Option<Comment>` and `children: Vec<Comment>`. Ploidy detects these cycles, and inserts `Box<T>` only where necessary.

For example, given a schema like:

```yaml
Comment:
  type: object
  required: [text]
  properties:
    text:
      type: string
    parent:
      $ref: "#/components/schemas/Comment"
    children:
      type: array
      items:
        $ref: "#/components/schemas/Comment"
```

Ploidy generates:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonPointee, JsonPointerTarget)]
pub struct Comment {
    pub text: String,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub parent: AbsentOr<Box<Comment>>,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub children: AbsentOr<Vec<Comment>>,
}
```

Since `Vec<T>` is already heap-allocated, only the `parent` field needs boxing to break the cycle.

### Inline schemas

OpenAPI specs can define schemas directly at their point of use—in operation parameters, in request and response bodies, or nested within other schemas—rather than in the `/components/schemas` section. These are called **inline schemas**.

Ploidy treats inline schemas as first-class, and generates the same strongly-typed models for them as for named schemas, with names that reflect their position in the spec.

For example, given an operation with an inline response schema:

```yaml
/users/{id}:
  get:
    operationId: getUser
    parameters:
      - name: id
        in: path
        required: true
        schema:
          type: string
    responses:
      '200':
        description: Success
        content:
          application/json:
            schema:
              type: object
              required: [id, email]
              properties:
                id:
                  type: string
                email:
                  type: string
                name:
                  type: string
```

Ploidy generates:

```rust
impl Client {
    pub async fn get_user(&self, id: &str) -> Result<types::GetUserResponse, Error> {
        // ...
    }
}
pub mod types {
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonPointee, JsonPointerTarget)]
    pub struct GetUserResponse {
        pub id: String,
        pub email: String,
        #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
        pub name: AbsentOr<String>,
    }
}
```

The inline schema gets a descriptive name, and the same trait implementations and derives as any named schema.

### Cargo features

When a spec includes resource conventions, Ploidy analyzes the type graph to determine the minimal set of `#[cfg(feature = "...")]` attributes for each type and operation. Resource conventions come from [vendor extensions](https://swagger.io/docs/specification/v3_0/openapi-extensions/) in the spec (Stripe-style `x-resourceId` on schemas; Machinify-style `x-resource-name` on operations):

* **Types with `x-resourceId`** are gated behind their own resource feature.
* **Types without `x-resourceId`** that are directly or transitively used by **operations with `x-resource-name`** are gated behind those operations' features.
* **Types with `x-resourceId` that are used by operations with `x-resource-name`** are gated behind both.
* **Types without `x-resourceId` that aren't used by any operation** remain ungated, so they're always available regardless of which features are enabled.
* **Feature dependencies** are transitively reduced: if enabling feature `a` already implies `b`—because `a` depends on `b` in `Cargo.toml`—a type that depends on both is gated behind just `a`.

## Contributing

We love contributions!

If you find a case where Ploidy fails, or generates incorrect or unidiomatic code, please [open an issue](https://github.com/linabutler/ploidy/issues/new) with your OpenAPI spec. For questions, or for planning larger contributions, please [start a discussion](https://github.com/linabutler/ploidy/discussions).

Some areas where we'd especially love help:

* Additional examples with real-world specs.
* Test coverage, especially for edge cases.
* Documentation improvements.
* New resource conventions for generating Cargo features.

We welcome LLM-assisted contributions, but hold them to the same quality bar: new code should fit the existing architecture, approach, and style. See [AGENTS.md](./AGENTS.md) for coding agent guidelines.

### New languages

Ploidy currently targets only Rust, but its architecture is designed to support other languages. We'll add a new language when we can:

1. Generate code from valid syntax trees that are correct by construction, rather than from string templates.
2. Leverage existing tools for those languages, like parsers, linters, and formatters, that are written _in_ Rust.
3. Maintain the same correctness guarantees and generated code quality as our Rust pipeline.

This means that Ploidy won't target every language. We'd rather support three languages perfectly than a dozen languages with gaps.

## Acknowledgments

Ploidy is inspired by, learns from, and builds on the work of:

* The OpenAPI ecosystem: **openapi-generator**, [**Progenitor**](https://github.com/oxidecomputer/progenitor), and other code generators.
* The Rust ecosystem: Tokio, Reqwest, Serde, `quote`, `syn`, and `winnow`.
* [**Petgraph**](https://crates.io/crates/petgraph), a Rust graph data structure library that's the backbone of Ploidy's type graph.

And yes, the name is a biology pun! [Ploidy](https://en.wikipedia.org/wiki/Ploidy) is the number of complete chromosome sets an organism carries—and the types Ploidy generates carry multiple sets of their own.
