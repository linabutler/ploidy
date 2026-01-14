# Ploidy

**An OpenAPI code generator for polymorphic specs.**

[![crates.io](https://img.shields.io/crates/v/ploidy?style=for-the-badge&logo=rust)](https://crates.io/crates/ploidy)
[![Build status](https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge&logo=github)](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[![Documentation](https://img.shields.io/docsrs/ploidy-core/latest?style=for-the-badge&label=ploidy-core&logo=docs.rs)](https://docs.rs/ploidy-core)

Many OpenAPI specs use `allOf` to model inheritance, and `oneOf`, `anyOf`, and discriminators to model [polymorphic or algebraic data types](https://swagger.io/docs/specification/v3_0/data-models/inheritance-and-polymorphism/). These patterns are powerful, but can be tricky to support correctly, and most code generators struggle with them. Ploidy was built specifically with inheritance and polymorphism in mind, and aims to generate clean, type-safe, and idiomatic Rust that looks like what you'd write by hand.

## Getting Started

To get started, [download a pre-built binary of Ploidy for your platform](https://github.com/linabutler/ploidy/releases/latest), or install Ploidy via [**cargo-binstall**](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall ploidy
```

...Or, if you'd prefer to install from source:

```sh
cargo install --locked ploidy
```

💡 **Tip**: The `-linux-musl` binaries are statically linked with [musl](https://www.musl-libc.org/intro.html), and are a good choice for running Ploidy on CI platforms like GitHub Actions.

### Minimum supported Rust version

Ploidy's minimum supported Rust version (MSRV) is **Rust 1.89.0**. This only applies if you're installing from source, or depending on one of the **ploidy-\*** packages as a library. The MSRV may increase in minor releases (e.g., Ploidy 1.1.x may require a newer MSRV than 1.0.x).

📝 **Note**: _Generated Rust code_ has [a different MSRV](#minimum-rust-version-for-generated-code).

## Generating Code

### Rust

To generate a complete Rust client crate from your OpenAPI spec, run:

```sh
ploidy codegen <INPUT-SPEC> <OUTPUT-DIR> rust
```

This produces a ready-to-use crate that includes:

* A `Cargo.toml` file, which you can extend with additional metadata, dependencies, or examples.
* A `types` module, which contains Rust types for every schema defined in your spec.
* A `client` module, with a RESTful HTTP client that provides async methods for every operation in your spec.

#### Minimum Rust version for generated code

The minimum supported Rust version (MSRV) for the generated crate is **Rust 1.85.0**, the first stable release to support the [2024 edition](https://doc.rust-lang.org/edition-guide/rust-2024/index.html).

#### Options

| Flag | Description |
|------|-------------|
| `-c`, `--check` | Run `cargo check` on the generated code |
| `--name <NAME>` | Set or override the generated package name. If not passed, and a `Cargo.toml` already exists in the output directory, preserves the existing `package.name`; otherwise, defaults to the name of the output directory |
| `--version <bump-major, bump-minor, bump-patch>` | If a `Cargo.toml` already exists in the output directory, increments the major, minor, or patch component of `package.version`. If not passed, preserves the existing `package.version`. Ignored if the package doesn't exist yet |

## Why Ploidy?

Ploidy is a good fit if:

* Your OpenAPI spec uses `allOf`, `oneOf`, or `anyOf`.
* You have a large or complex spec that's challenging for other generators.
* Your spec has inline schemas, and you'd like to generate the same strongly-typed models for them as for named schemas.
* Your spec has recursive or cyclic types.
* You want to generate Rust that reads like you wrote it.

### Choosing the right tool

The OpenAPI ecosystem has great options for different needs. Here's how to pick:

| If you need... | Consider |
|----------------|----------|
| **Broad OpenAPI feature coverage** | [**openapi-generator**](https://openapi-generator.tech), [**Progenitor**](https://github.com/oxidecomputer/progenitor), or [**Schema Tools**](https://github.com/kstasik/schema-tools), especially if your spec is simpler and doesn't rely heavily on polymorphism |
| **Custom templates or a different HTTP client** | A template-based generator like **openapi-generator**, which offers more control over output |
| **Languages other than Rust** | **openapi-generator** or [**swagger-codegen**](https://github.com/swagger-api/swagger-codegen) (OpenAPI <= 3.0) |
| **OpenAPI 2.0 (Swagger) support** | **openapi-generator** or **swagger-codegen** |
| **Server stubs** | **openapi-generator** for Rust web frameworks, or [**Dropshot**](https://github.com/oxidecomputer/dropshot) for generating specs from Rust definitions |

Ploidy is young and evolving. If you need a feature that isn't supported yet, please [open an issue](https://github.com/linabutler/ploidy/issues/new)—it helps shape our roadmap!

### Polymorphism first

Ploidy is designed from the ground up to handle inheritance and polymorphism correctly:

* ✅ **`allOf`**: Structs with fields from all parent schemas.
* ✅ **`oneOf` with discriminator**: Enums with named newtype variants for each mapping, represented as an [internally tagged](https://serde.rs/enum-representations.html#internally-tagged) Serde enum.
* ✅ **`oneOf` without discriminator**: Enums with automatically named (`V1`, `V2`, `V3`...) variants for each mapping, represented as an [untagged](https://serde.rs/enum-representations.html#untagged) Serde enum.
* ✅ **`anyOf`**: Structs with optional flattened fields for each mapping.

### Fast and correct

Ploidy is fast enough to run on every save, and correct enough that you won't need to hand-edit the output.

As a benchmark, Ploidy can generate a working crate from a large polymorphic spec (2.6 MB, ~3500 schemas, ~1300 operations) in under 2 seconds.

### Strongly opinionated, zero configuration

Ploidy keeps configuration to a minimum: few command-line options, no config files, and no design decisions to make.

This means it won't be the right tool for every job—but it should nail the ones it fits.

### Code like what you'd write by hand

Generated code looks like it was written by an experienced Rust developer:

* **[Serde](https://serde.rs)-compatible type definitions**: Structs for `object` types and `anyOf` schemas, enums with data for `oneOf` schemas, unit-only enums for string `enum` types.
* **Built-in trait implementations** for generated types: `From<T>` for polymorphic enum variants; `FromStr` and `Display` for string enums.
* **Standard derives** for all types, plus `Hash` and `Eq` for hashable types, and `Default` for types with all optional fields.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Customer {
    pub id: String,
    pub email: String,
    #[serde(default, skip_serializing_if = "AbsentOr::is_absent")]
    pub name: AbsentOr<String>,
}
```

The optional `name` field uses [`AbsentOr<T>`](https://docs.rs/ploidy-util/latest/ploidy_util/absent/enum.AbsentOr.html), a three-valued type that matches how OpenAPI represents optional fields: either "present with a value", "present and explicitly set to `null`", or "absent from the payload".

## Under the Hood

Ploidy takes a somewhat different approach to code generation. If you're curious about how it works, this section is for you!

### The generation pipeline

Ploidy processes an OpenAPI spec in three stages:

📝 **Parsing** a JSON or YAML OpenAPI spec into Rust data structures. The parser is intentionally forgiving; Ploidy doesn't rigorously enforce OpenAPI (or JSON Schema Draft 2020-12) semantics.

🏗️ **Constructing an IR** (intermediate representation). Ploidy constructs a type graph from the spec, which lets it answer questions like "which types can derive `Eq`, `Hash`, and `Default`?" and "which fields need `Box<T>` to break cycles?"

✍️ **Generating code** from the IR. During the final stage, Ploidy builds proper Rust syntax trees from the processed schema, prettifies the code, and writes the generated code to disk.

### AST-based generation

Most code generators use string templates, but Ploidy uses Rust's `syn` and `quote` crates to generate **syntax trees**. The generated code has the advantage of being syntactically valid by construction.

### Smart boxing

Schemas that represent graph and tree-like structures typically contain circular references: a `User` has `friends: Vec<User>`; a `Comment` has a `parent: Option<Comment>` and `children: Vec<Comment>`, and so on. Ploidy detects these cycles, and inserts `Box<T>` only where necessary.

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

Many code generators treat inline schemas as untyped values (`Any` or `serde_json::Value`), but Ploidy generates the same strongly-typed models for inline schemas as it does for named schemas. Inline schemas are named based on where they occur in the spec, and are namespaced in submodules within the parent schema module.

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
              required: [id, email, name]
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
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct GetUserResponse {
        pub id: String,
        pub email: String,
        pub name: String,
    }
}
```

The inline schema gets a descriptive name—in this case, `GetUserResponse`, derived from the `operationId` and its use as a response schema—and the same trait implementations and derives as any named schema. This "just works": inline schemas are first-class types in the generated code.

## Contributing

We love contributions: issues, feature requests, discussions, code, documentation, and examples are all welcome!

If you find a case where Ploidy fails, or generates incorrect or unidiomatic code, please [open an issue](https://github.com/linabutler/ploidy/issues/new) with your OpenAPI spec. For questions, or for planning larger contributions, please [start a discussion](https://github.com/linabutler/ploidy/discussions).

Some areas where we'd especially love help:

* Additional examples with real-world specs.
* Test coverage, especially for edge cases.
* Documentation improvements.

We welcome LLM-assisted contributions, but hold them to the same quality bar: new code should fit the existing architecture, approach, and style. See [AGENTS.md](./AGENTS.md) for coding agent guidelines.

### New languages

Ploidy only targets Rust now, but its architecture is designed to support other languages. Our philosophy is to only support languages where we can:

1. Generate code from valid syntax trees that are correct by construction, rather than from string templates.
2. Leverage existing tools for those languages, like parsers, linters, and formatters, that are written _in_ Rust.
3. Maintain the same correctness guarantees and generated code quality as our Rust pipeline.

This does mean that Ploidy won't target every language. We'd rather support three languages perfectly than a dozen languages with gaps.

## Acknowledgments

Ploidy is inspired by, learns from, and builds on the wonderful work of:

* The OpenAPI ecosystem: **openapi-generator**, **Progenitor**, and other code generators.
* The async Rust ecosystem: Tokio and Reqwest.
* The Rust parsing ecosystem: `quote`, `serde`, `syn`, and `winnow`.
