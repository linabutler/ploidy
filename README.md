# Ploidy

**An OpenAPI code generator for polymorphic specs.**

[![crates.io](https://img.shields.io/crates/v/ploidy?style=for-the-badge&logo=rust)](https://crates.io/crates/ploidy)
[![Build status](https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge&logo=github)](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[![Documentation](https://img.shields.io/docsrs/ploidy-core/latest?style=for-the-badge&label=ploidy-core&logo=docs.rs)](https://docs.rs/ploidy-core)

Many OpenAPI specs use `allOf` to model inheritance; and `oneOf`, `anyOf`, and discriminators to model [polymorphic or algebraic data types](https://swagger.io/docs/specification/v3_0/data-models/inheritance-and-polymorphism/). These patterns are powerful, but can be tricky to support correctly, and most code generators struggle with them. Ploidy was built specifically with inheritance and polymorphism in mind, and aims to generate clean, type-safe, and idiomatic Rust that looks like how you'd write it by hand.

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

## Generating Code

### Rust

To generate a complete Rust client crate from your OpenAPI spec, run:

```sh
ploidy codegen <INPUT-SPEC> <OUTPUT-DIR> rust
```

This produces a ready-to-use crate that includes:

* A `Cargo.toml` file, which you can extend with additional metadata, dependencies, or examples...
* A `types` module, which contains Rust types for every schema defined in your spec, and...
* A `client` module, with a RESTful HTTP client that provides async methods for every operation in your spec.

#### Options

| Flag | Description |
|------|-------------|
| `-c`, `--check` | Run `cargo check` on the generated code |
| `--name <NAME>` | Set or override the generated package name. If not passed, and a `Cargo.toml` already exists in the output directory, preserves the existing `package.name`; otherwise, defaults to the name of the output directory |
| `--version <bump-major, bump-minor, bump-patch>` | If a `Cargo.toml` already exists in the output directory, increments the major, minor, or patch component of `package.version`. If not passed, preserves the existing `package.version`. Ignored if the package doesn't exist yet |

## Why Ploidy?

🎉 Ploidy is a good fit if:

* Your OpenAPI spec uses `allOf`, `oneOf`, or `anyOf`.
* You have a large or complex spec that's challenging for other generators.
* Your spec has inline schemas, and you'd like to generate the same strongly-typed models for them as for named schemas.
* You prefer convention over configuration.

⚠️ Ploidy might **not** be the right fit if:

* Your spec uses OpenAPI features that Ploidy doesn't support yet. [**Progenitor**](https://github.com/oxidecomputer/progenitor), [**Schema Tools**](https://github.com/kstasik/schema-tools) or [**openapi-generator**](https://openapi-generator.tech) might be better choices, especially if your spec is simpler...but please [open an issue](https://github.com/linabutler/ploidy/issues/new) for Ploidy to support the features you need!
* You'd like to use a custom template for the generated code, or a different HTTP client; or to generate synchronous code. For these cases, consider a template-based generator like **openapi-generator**.
* You need to target a language other than Rust. **openapi-generator** supports many more languages; as does [**swagger-codegen**](https://github.com/swagger-api/swagger-codegen), if you don't need OpenAPI 3.1+ support.
* Your spec uses OpenAPI (Swagger) 2.0. Ploidy only supports OpenAPI 3.0+, but **openapi-generator** and **swagger-codegen** support older versions.
* You need to generate server stubs. Ploidy only generates clients, but **openapi-generator** can produce stubs for different Rust web frameworks. Alternatively, you can define your models and endpoints in Rust, and use [Dropshot](https://github.com/oxidecomputer/dropshot) to generate a Ploidy- or Progenitor-compatible OpenAPI spec from those definitions.
* You'd like a more mature, established tool.

Here are some of the things that make Ploidy different.

### Polymorphism first

Ploidy is designed from the ground up to handle inheritance and polymorphism correctly:

* ✅ **`allOf`**: Structs with fields from all parent schemas.
* ✅ **`oneOf` with discriminator**: Enums with named newtype variants for each mapping, represented as an [internally tagged](https://serde.rs/enum-representations.html#internally-tagged) Serde enum.
* ✅ **`oneOf` without discriminator**: Enums with automatically named (`V1`, `V2`, `Vn`...) variants for each mapping, represented as an [untagged](https://serde.rs/enum-representations.html#untagged) Serde enum.
* ✅ **`anyOf`**: Structs with optional flattened fields for each mapping.

### Fast and correct

Ploidy gives you speed and correctness, for quick iteration and zero manual fixes.

⏱️ Example: It takes Ploidy **5 seconds** to generate a working crate for a large (2.6 MB JSON) OpenAPI spec, with **~3500 schemas** and **~1300 operations**.

### Strongly opinionated, zero configuration

Ploidy intentionally keeps configuration to a minimum: just a handful of command-line options; no config files; and no design choices to make.

This philosophy means that Ploidy might not be the right tool for every job. Ploidy's goal is to produce ready-to-use code for ~90% of use cases with zero configuration, but if you'd like more control over the generated code, check out one of the other great tools mentioned above!

### Code like you'd write by hand

Generated code looks like it was written by an experienced Rust developer:

* **[Serde](https://serde.rs)-compatible type definitions**: Structs for `object` types and `anyOf` schemas, enums with data for `oneOf` schemas, unit-only enums for string `enum` types.
* **Built-in traits** for generated Rust types: `From<T>` for each polymorphic enum variant; `FromStr` and `Display` for string enums; standard derives for all types (plus `Hash` and `Eq` for all hashable types, and `Default` for types with all optional fields; all derived automatically).
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

📝 **Parsing** a JSON or YAML OpenAPI spec into Rust data structures. The parser is intentionally forgiving: short of syntax errors and type mismatches, Ploidy doesn't rigorously enforce OpenAPI (or JSON Schema Draft 2020-12) semantics.

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

Many code generators treat inline inline schemas as untyped values (`Any` or `serde_json::Value`), but Ploidy generates the same strongly-typed models for inline schemas as it does for named schemas. Inline schemas are named based on where they occur in the spec, and are namespaced in submodules within the parent schema module.

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

The inline schema gets a descriptive name (in this case, `GetUserResponse`; derived from the `operationId` and its use as a response schema), and the same derives as any named schema. This "just works": inline schemas are first-class types in the generated code.

## Contributing

We love contributions: issues, feature requests, discussions, code, documentation, and examples are all welcome!

If you find a case where Ploidy fails, or generates incorrect or unidiomatic code, please [open an issue](https://github.com/linabutler/ploidy/issues/new) with your OpenAPI spec. For questions, or for planning larger contributions, please [start a discussion](https://github.com/linabutler/ploidy/discussions).

Some areas where we'd especially love help are:

* Additional examples, with real-world specs.
* Test coverage, especially for edge cases.
* Documentation improvements.

🤖 We welcome LLM-assisted contributions, but hold them to the same quality bar: the code should fit in with the existing architecture, approach, and overall style of the project.

Thanks!

### New languages

Ploidy only targets Rust now, but its architecture is designed to support other languages. Our philosophy is to only support languages where we can:

1. Generate code from valid syntax trees that are correct by construction, rather than from string templates.
2. Leverage existing tools for those languages, like parsers, linters, and formatters, that are written _in_ Rust.
3. Maintain the same correctness guarantees and generated code quality as our Rust pipeline.

This does mean that Ploidy won't target every language. We'd rather support three languages perfectly, than a dozen languages with gaps.

## Acknowledgments

Ploidy is inspired by, learns from, and builds on the wonderful work of:

* The OpenAPI ecosystem: **openapi-generator**, **Progenitor**, and other code generators.
* The async Rust ecosystem: Tokio and Reqwest.
* The Rust parsing ecosystem: `quote`, `serde`, `syn`, and `winnow`.
