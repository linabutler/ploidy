# Ploidy

[<img src="https://img.shields.io/crates/v/ploidy?style=for-the-badge&logo=rust" alt="crates.io" height="24">](https://crates.io/crates/ploidy)
[<img src="https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge&logo=github" alt="Build status" height="24">](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[<img src="https://img.shields.io/docsrs/ploidy-codegen-rust/latest?style=for-the-badge&label=codegen-rust&logo=docs.rs" alt="ploidy-codegen-rust Documentation" height="24">](https://docs.rs/ploidy-codegen-rust)
[<img src="https://img.shields.io/docsrs/ploidy-core/latest?style=for-the-badge&label=core&logo=docs.rs" alt="ploidy-core Documentation" height="24">](https://docs.rs/ploidy-core)
[<img src="https://img.shields.io/docsrs/ploidy-pointer/latest?style=for-the-badge&label=pointer&logo=docs.rs" alt="ploidy-pointer Documentation" height="24">](https://docs.rs/ploidy-pointer)
[<img src="https://img.shields.io/docsrs/ploidy-util/latest?style=for-the-badge&label=util&logo=docs.rs" alt="ploidy-util Documentation" height="24">](https://docs.rs/ploidy-util)

Ploidy is a polymorphism-first OpenAPI compiler for Rust. It generates code that reads like it was written by hand, even for specs with inheritance, recursive types, and inline schemas.

## Table of Contents

* [Getting started](#getting-started)
  - [Minimum supported Rust version](#minimum-supported-rust-version)
* [Generating code](#generating-code)
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
* [How it works](#how-it-works)
  - [The generation pipeline](#the-generation-pipeline)
  - [AST-based codegen](#ast-based-codegen)
  - [Inline schemas](#inline-schemas)
  - [Smart boxing](#smart-boxing)
  - [Cargo features](#cargo-features)
* [Supported OpenAPI features](#supported-openapi-features)
  - [For schemas](#for-schemas)
  - [For operations](#for-operations)
* [Contributing](#contributing)
  - [New languages](#new-languages)
* [Acknowledgments](#acknowledgments)

## Getting started

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

Ploidy's minimum supported Rust version (MSRV) is **Rust 1.89.0**. This applies when installing from source, or when depending on one of the **ploidy-\*** packages as a library. We may increase the MSRV in minor releases.

> [!NOTE]
> Generated Rust code has [a different MSRV](#minimum-rust-version-for-generated-code).

## Generating code

### Rust

To generate a complete Rust client crate from your OpenAPI spec, run:

```sh
ploidy generate rust /path/to/spec.yaml
```

This produces a crate that includes:

* A `Cargo.toml` file that you can extend with additional metadata, dependencies, or examples. For specs with resource annotations, the generated manifest includes [per-resource feature gates](#per-resource-feature-gates).
* A `types` module with type definitions for each schema in your spec.
* A `client` module with async methods for every operation in your spec.

#### Options

| Flag | Description |
|------|-------------|
| `-o`, `--output` | Set the output directory for the generated crate |
| `-c`, `--check` | Verify the generated crate compiles |
| `--name <NAME>` | Set the crate name. Defaults to `package.name` in the output directory's `Cargo.toml`, if present, or the output directory name |
| `--version <bump-major \| bump-minor \| bump-patch>` | Increment the major, minor, or patch component of the existing `package.version`. If omitted, Ploidy uses the existing version, or `0.1.0` for new crates |

#### Advanced options

Ploidy reads additional options from `[package.metadata.ploidy]` in the generated crate's `Cargo.toml`:

| Key | Values | Default | Description |
|-----|--------|---------|-------------|
| `date-time-format` | `rfc3339`, [`unix-seconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixSeconds.html), [`unix-milliseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixMilliseconds.html), [`unix-microseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixMicroseconds.html), [`unix-nanoseconds`](https://docs.rs/ploidy-util/latest/ploidy_util/date_time/struct.UnixNanoseconds.html) | `rfc3339` | How `date-time` types are represented |

For example:

```toml
[package.metadata.ploidy]
# Use `ploidy_util::UnixSeconds`, which represents `date-time` values as
# Unix timestamps in seconds.
date-time-format = "unix-seconds"
# date-time-format = "unix-milliseconds"  # Use `ploidy_util::UnixMilliseconds`.
# date-time-format = "unix-microseconds"  # Use `ploidy_util::UnixMicroseconds`.
# date-time-format = "unix-nanoseconds"   # Use `ploidy_util::UnixNanoseconds`.
# date-time-format = "rfc3339"            # Use `chrono::DateTime<Utc>` (RFC 3339 / ISO 8601 strings).
```

#### Minimum Rust version for generated code

The MSRV for the generated crate is **Rust 1.86.0**.

## Why Ploidy?

Use Ploidy when:

* Your OpenAPI spec uses `allOf`, `oneOf`, or `anyOf`.
* You have a large or complex spec that's challenging for other generators.
* Your spec has many inline schemas, and you want the same strongly-typed models for them as for named schemas.
* Your spec has recursive or cyclic types.
* Your spec has [resource annotations](#cargo-features), and you want consumers to compile just the types and operations they need.
* Your spec uses [some OpenAPI 3.1 features](#supported-openapi-features).
* You want to generate Rust that reads like you wrote it.

### Choosing the right tool

Ploidy focuses on generating Rust clients from modern OpenAPI specs. The broader ecosystem has strong options for other needs:

| If you need... | Look for... |
|----------------|-------------|
| **Custom templates or a different HTTP client** | A template-based generator like [**openapi-generator**](https://openapi-generator.tech) or [**Schema Tools**](https://github.com/kstasik/schema-tools), which offer more control over output |
| **Languages other than Rust** | **openapi-generator**, or [**swagger-codegen**](https://github.com/swagger-api/swagger-codegen) for OpenAPI < 3.1 |
| **OpenAPI 2.0 (Swagger) support** | **openapi-generator** or **swagger-codegen** |
| **Server stubs** | **openapi-generator** for Rust web frameworks, or [**Dropshot**](https://github.com/oxidecomputer/dropshot) for generating specs from Rust definitions |

Ploidy is opinionated by design. We'd rather get the defaults right than expose a page of configuration options. If you need a feature that isn't supported yet, please [open an issue](https://github.com/linabutler/ploidy/issues/new)—it helps shape our roadmap!

### Polymorphism first

Ploidy has first-class support for inheritance and polymorphism:

* **`allOf`**: Structs with fields linearized from all parent schemas.
* **`oneOf` with discriminator**: Enums with named newtype variants for each mapping, represented as an [internally tagged](https://serde.rs/enum-representations.html#internally-tagged) Serde enum.
* **`oneOf` without discriminator**: Enums with automatically named (`V1`, `V2`, `V3`...) variants for each subschema, represented as an [untagged](https://serde.rs/enum-representations.html#untagged) Serde enum.
* **`anyOf`**, with or without discriminator: Structs with optional [flattened fields](https://serde.rs/attr-flatten.html) for each subschema.

### Fast and correct

Ploidy is designed to generate crates that compile as-is, without a post-processing step, while staying fast:

| Spec | Types (approx.) | Operations (approx.) | Generation time |
|------|-----------------|----------------------|-----------------|
| Private production spec | 4,000 | 1,450 | <2s |
| [Stripe](https://github.com/stripe/openapi) | 1,400 | 600 | <2s |
| [GitHub](https://github.com/github/rest-api-description) | 900 | 1,100 | <2s |
| [OpenAI](https://github.com/openai/openai-openapi) | 900 | 240 | <1s |

Measurements were taken in May 2026 with [Hyperfine](https://github.com/sharkdp/hyperfine) on a 2021 M1 MacBook Pro. The private spec is from a large production service, included to show scale.

### Code like what you'd write by hand

Generated code looks like it was written by an experienced Rust developer:

* **[Serde](https://serde.rs)-compatible type definitions**: Structs for `object` types and `anyOf` schemas, enums with data for `oneOf` schemas, unit-only enums for string `enum` types.
* **Built-in trait implementations** for generated types: `From<T>` for polymorphic enum variants; `FromStr` and `Display` for string enums.
* **Standard derives** for all types, plus `Hash`, `Eq`, and `Default` for types that support them.
* **Typed JSON Pointer navigation** for generated types, via `JsonPointee` and `JsonPointerTarget` from [**ploidy-pointer**](https://crates.io/crates/ploidy-pointer).
* **Boxing** for recursive types.
* **A RESTful client with async endpoints**, using [Reqwest](https://docs.rs/reqwest) with the [Tokio](https://tokio.rs) runtime.

For example, given this schema:

```yaml
PaymentMethod:
  oneOf:
    - $ref: "#/components/schemas/Card"
    - $ref: "#/components/schemas/BankAccount"
  discriminator:
    propertyName: type
    mapping:
      card: "#/components/schemas/Card"
      bank_account: "#/components/schemas/BankAccount"
```

Ploidy generates code like:

```rust
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
    JsonPointee, JsonPointerTarget,
)]
#[serde(tag = "type")]
#[ploidy(pointer(tag = "type"))]
pub enum PaymentMethod {
    #[serde(rename = "card")]
    #[ploidy(pointer(rename = "card"))]
    Card(Card),

    #[serde(rename = "bank_account")]
    #[ploidy(pointer(rename = "bank_account"))]
    BankAccount(BankAccount),
}

impl From<Card> for PaymentMethod {
    fn from(value: Card) -> Self {
        Self::Card(value)
    }
}

impl From<BankAccount> for PaymentMethod {
    fn from(value: BankAccount) -> Self {
        Self::BankAccount(value)
    }
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

## How it works

### The generation pipeline

Ploidy processes an OpenAPI spec in three stages:

**Parsing** a JSON or YAML OpenAPI spec into Rust data structures. Ploidy reads the document shapes that affect generated code: schemas, operations, parameters, request bodies, responses, and resource annotations.

**Constructing an IR** (intermediate representation). Ploidy builds a type graph from the spec, which lets it answer questions like "which types can derive `Eq`, `Hash`, and `Default`?" and "which fields need `Box<T>` to break cycles?"

**Generating code** from the IR. Ploidy creates Rust syntax trees from the type graph, formats the code, and writes it to disk.

### AST-based codegen

Ploidy builds Rust **syntax trees** directly with [`syn`](https://docs.rs/syn) and [`quote`](https://docs.rs/quote), rather than assembling code from string templates. This has two benefits:

* **Generated code is syntactically valid by construction.** Nodes are typed `syn` values, built with `parse_quote!` and friends. Ploidy can't produce a crate with syntax errors.
* **Complex types compose cleanly.** Trait bounds, attribute macros, and nested generics combine as tokens, not concatenated strings. The generator never juggles whitespace or escaping, so hard-to-generate constructs are as reliable as simple ones.

Once the tree is built, [`prettyplease`](https://docs.rs/prettyplease) formats it into the final output.

### Inline schemas

OpenAPI specs can define schemas directly at their point of use—in operation parameters, in request and response bodies, or nested within other schemas—rather than in the `/components/schemas` section. These are called **inline schemas**.

Ploidy treats inline schemas as first-class types and generates the same strongly-typed models for them as for named schemas, with names that reflect their position in the spec.

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

Ploidy generates code like:

```rust
impl Client {
    pub async fn get_user(&self, id: &str) -> Result<types::GetUserResponse, Error> {
        // ...
    }
}
pub mod types {
    #[derive(
        Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
        JsonPointee, JsonPointerTarget,
    )]
    pub struct GetUserResponse {
        pub id: String,
        pub email: String,
        #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
        pub name: AbsentOr<String>,
    }
}
```

The inline schema gets a descriptive name and the same trait implementations and derives as any named schema. Here, `GetUserResponse` comes from the `operationId` and its use as a response schema.

### Smart boxing

Schemas that represent graph- and tree-like structures often contain circular references: a `User` might have `friends: Vec<User>`; a `Comment` might have a `parent: Option<Comment>` and `children: Vec<Comment>`. Ploidy detects these cycles and inserts `Box<T>` only where necessary.

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

Ploidy generates code like:

```rust
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
    JsonPointee, JsonPointerTarget,
)]
pub struct Comment {
    pub text: String,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub parent: AbsentOr<Box<Comment>>,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub children: AbsentOr<Vec<Comment>>,
}
```

Since `Vec<T>` is already heap-allocated, only the `parent` field needs boxing to break the cycle.

### Cargo features

When a spec includes resource annotations, Ploidy analyzes the type graph to determine the minimal set of `#[cfg(feature = "...")]` attributes for each type and operation. These annotations come from [vendor extensions](https://swagger.io/docs/specification/v3_0/openapi-extensions/) in the spec (`x-resourceId` on schemas and `x-resource-name` on operations):

* **Types with `x-resourceId`** are gated behind their own resource feature.
* **Types without `x-resourceId`** that are directly or transitively used by **operations with `x-resource-name`** are gated behind those operations' resource features.
* **Types with `x-resourceId` that are used by operations with `x-resource-name`** are gated behind both.
* **Types without `x-resourceId` that aren't used by any operation** remain ungated, so they're always available regardless of which features are enabled.
* **Feature dependencies** are transitively reduced: if enabling feature `a` already implies `b`—because `a` depends on `b` in `Cargo.toml`—a type that depends on both is gated behind just `a`.

## Supported OpenAPI features

### For schemas

| Feature | Status | Generated output |
|---------|--------|------------------|
| `type: [...]` | Partial | Type-only unions become untagged enums; `[T, "null"]` unions become `Option<T>` |
| `type: object`, `properties`, `required` | Supported | Structs with `T` or `AbsentOr<T>` fields |
| `additionalProperties` | Supported | `BTreeMap<String, T>` when standalone; a flattened map field when mixed with named `properties` |
| `type: array`, `items` | Supported | `Vec<T>` |
| Scalar types and formats | Supported | Strings, booleans, signed and unsigned integers, floats, dates and times, URLs, UUIDs, Base64-encoded bytes, and binary byte buffers |
| `$ref` | Partial | Document-relative `#/components/schemas/...` references only; no external or nested references. `$ref` schemas with adjacent keywords are treated as `allOf` |
| `enum` | Supported | Enums with all string values become Rust unit enums; others become `String` type aliases |
| `nullable`, `type: [T, "null"]`, `oneOf` with `null` | Supported | Nullable schemas become `Option<T>` type aliases; required nullable fields become `Option<T>`; optional fields become `AbsentOr<T>` |
| `allOf` | Supported | Structs with inherited fields linearized from parent schemas |
| `oneOf` with `discriminator` | Supported | Internally tagged enums with newtype variants |
| `oneOf` without `discriminator` | Supported | Untagged enums with generated variant names |
| `anyOf` | Supported | Structs with optional flattened fields for each subschema |
| Inline schemas | Supported | Named inline types based on their semantic path |
| Recursive schemas | Supported | `Box<T>` inserted where needed to break cycles |
| Empty or unconstrained schemas | Supported | `serde_json::Value` |

### For operations

| Feature | Status | Generated output |
|---------|--------|------------------|
| HTTP methods | Partial | `GET`, `POST`, `PUT`, `PATCH`, and `DELETE` operations generated |
| `operationId` | Required | Used as the Rust method name |
| Path templates and parameters | Supported | Path parameters become `&str` arguments |
| Query parameters | Supported | Generated as an inline type; `form`, `spaceDelimited`, `pipeDelimited`, and `deepObject` styles supported |
| Header and cookie parameters | Unsupported | - |
| Request bodies | Partial | `application/json` and `*/*` schemas become typed arguments; `multipart/form-data` becomes `reqwest::multipart::Form` |
| Responses | Partial | `application/json` and `*/*` from 2xx and `default` responses become typed return values; per-status responses are ignored |
| Authentication and security schemes | Ignored | - |
| Servers | Ignored | - |
| Tags | Ignored | - |
| Callbacks, links, examples, and component headers | Ignored | - |

## Contributing

We love contributions!

If you find a case where Ploidy fails or generates incorrect or unidiomatic code, please [open an issue](https://github.com/linabutler/ploidy/issues/new) with your OpenAPI spec. For questions or larger contributions, please [start a discussion](https://github.com/linabutler/ploidy/discussions).

Some areas where we'd especially love help:

* Additional examples with real-world specs.
* Test coverage, especially for edge cases.
* Documentation improvements.
* Support for new vendor extensions that group operations and types into Cargo features.

We welcome LLM-assisted contributions, but hold them to the same quality bar: new code should fit the existing architecture, approach, and style. See [AGENTS.md](./AGENTS.md) for coding agent guidelines.

### New languages

Ploidy currently targets only Rust, but its architecture is designed to support other languages. We'll add a language target when we can:

1. Generate code from valid syntax trees that are correct by construction, rather than from string templates.
2. Leverage existing tools for those languages, like parsers, linters, and formatters, that are written _in_ Rust.
3. Maintain the same correctness guarantees and generated code quality as our Rust pipeline.

This means Ploidy won't target every language. We'd rather support a few languages well than many languages with gaps.

## Acknowledgments

Ploidy is inspired by and builds on the wonderful work of:

* The OpenAPI ecosystem: **openapi-generator**, [**Progenitor**](https://github.com/oxidecomputer/progenitor), and other code generators.
* The Rust ecosystem: Tokio, Reqwest, Serde, `quote`, `syn`, and `winnow`.
* [**Petgraph**](https://crates.io/crates/petgraph), the Rust graph data structure library behind Ploidy's type graph.

And yes, the name is [a biology pun](https://en.wikipedia.org/wiki/Ploidy)!
