# Ploidy

[<img src="https://img.shields.io/crates/v/ploidy?style=for-the-badge&logo=rust" alt="crates.io" height="24">](https://crates.io/crates/ploidy)
[<img src="https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge&logo=github" alt="Build status" height="24">](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[<img src="https://img.shields.io/docsrs/ploidy-codegen-rust/latest?style=for-the-badge&label=codegen-rust&logo=docs.rs" alt="ploidy-codegen-rust Documentation" height="24">](https://docs.rs/ploidy-codegen-rust)
[<img src="https://img.shields.io/docsrs/ploidy-core/latest?style=for-the-badge&label=core&logo=docs.rs" alt="ploidy-core Documentation" height="24">](https://docs.rs/ploidy-core)
[<img src="https://img.shields.io/docsrs/ploidy-pointer/latest?style=for-the-badge&label=pointer&logo=docs.rs" alt="ploidy-pointer Documentation" height="24">](https://docs.rs/ploidy-pointer)
[<img src="https://img.shields.io/docsrs/ploidy-util/latest?style=for-the-badge&label=util&logo=docs.rs" alt="ploidy-util Documentation" height="24">](https://docs.rs/ploidy-util)

Ploidy is an OpenAPI compiler for Rust, built especially for large and complex specs that use inheritance, composition, polymorphism, and inline schemas.

Ploidy thinks of generated code as source code that you can read, review, and debug, so it generates what a Rust developer would write by hand: an async client, typed models for all schemas, built-in trait implementations and derives, Cargo features, and [more](#why-ploidy).

## Table of Contents

* [Getting started](#getting-started)
  - [Minimum supported Rust version](#minimum-supported-rust-version)
* [Generating Rust code](#generating-rust-code)
  - [Options](#options)
  - [Advanced options](#advanced-options)
  - [Minimum Rust version for generated code](#minimum-rust-version-for-generated-code)
* [How it works](#how-it-works)
* [Why Ploidy?](#why-ploidy)
  - [Speed](#speed)
  - [Polymorphism first](#polymorphism-first)
  - [Inline schemas](#inline-schemas)
  - [The client](#the-client)
  - [Smart boxing](#smart-boxing)
  - [Per-resource feature gates](#per-resource-feature-gates)
  - [Choosing the right tool](#choosing-the-right-tool)
* [Supported OpenAPI features](#supported-openapi-features)
  - [For schemas](#for-schemas)
  - [For operations](#for-operations)
* [Contributing](#contributing)
  - [New languages](#new-languages)
* [Acknowledgments](#acknowledgments)

## Getting started

[Download a pre-built binary of Ploidy for your platform](https://github.com/linabutler/ploidy/releases/latest), or install Ploidy via [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

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

Ploidy's minimum supported Rust version (MSRV) is **Rust 1.89.0**. This applies when installing from source, or when depending on one of the Ploidy packages as a library. We may increase the MSRV in minor releases.

> [!NOTE]
> Generated Rust code has [a different MSRV](#minimum-rust-version-for-generated-code).

## Generating Rust code

To generate a Rust crate from your OpenAPI spec, run:

```sh
ploidy generate rust /path/to/spec.yaml -o my-api-client
```

This creates a `my-api-client` library crate with:

* A `Cargo.toml` manifest that you can extend with additional metadata, dependencies, or examples.
* A `types` module with type definitions for each schema in your spec.
* A `client` module with async methods for every operation in your spec.

The crate's only required dependency is [ploidy-util](https://docs.rs/ploidy-util), which re-exports Serde, Reqwest, and other runtime dependencies.

### Options

| Flag | Description |
|------|-------------|
| `-o`, `--output` | Set the output directory for the generated crate |
| `-c`, `--check` | Verify the generated crate compiles |
| `--name <NAME>` | Set the crate name. Defaults to `package.name` in the output directory's `Cargo.toml`, if present, or the output directory name |
| `--version <bump-major \| bump-minor \| bump-patch>` | Increment the major, minor, or patch component of the existing `package.version`, or of `0.1.0` for a new crate |

### Advanced options

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

### Minimum Rust version for generated code

The MSRV for the generated crate is **Rust 1.86.0**.

## How it works

Ploidy processes an OpenAPI spec in three stages:

**Parsing a JSON or YAML OpenAPI spec.** Ploidy starts by reading schemas, operations, parameters, request bodies, responses, and resource groups into Rust data structures. Parsing is forgiving, and covers just the parts of the spec that affect generated code—Ploidy isn't a validator.

**Constructing an intermediate representation.** Next, Ploidy builds a type graph from the parsed spec, which lets it answer questions like "which types can derive `Eq`, `Hash`, and `Default`?" and "which fields need `Box<T>` to break cycles?"

**Generating code.** Finally, Ploidy turns the IR types into Rust syntax trees with [`syn`](https://docs.rs/syn) and [`quote`](https://docs.rs/quote), then formats them into the final output with [`prettyplease`](https://docs.rs/prettyplease).

## Why Ploidy?

Use Ploidy when:

* The [size](#speed) or [shape](#polymorphism-first) of your spec is challenging for other generators.
* You want to generate typed models from your [inline schemas](#inline-schemas).
* Some of your schemas are [recursive or cyclic](#smart-boxing).
* You want [feature gates](#per-resource-feature-gates) for your schemas and operations.
* Your spec uses [some OpenAPI 3.1+ features](#supported-openapi-features).
* Generated code quality is important to you.

### Speed

Ploidy is fast, even for large specs:

| Spec | Types (approx.) | Operations (approx.) | Generation time |
|------|-----------------|----------------------|-----------------|
| Internal spec | 4,000 | 1,450 | <2s |
| [Stripe](https://github.com/stripe/openapi) | 1,400 | 600 | <2s |
| [GitHub](https://github.com/github/rest-api-description) | 900 | 1,100 | <2s |
| [OpenAI](https://github.com/openai/openai-openapi) | 900 | 240 | <1s |

These measurements were taken in May 2026 with [Hyperfine](https://github.com/sharkdp/hyperfine) on a 2021 M1 MacBook Pro. The internal spec is from a large production service, and is included to show scale.

### Polymorphism first

Ploidy has first-class support for inheritance and polymorphism:

* **`allOf`**: Structs with fields linearized from all parent schemas.
* **`oneOf` with a `discriminator`**: [Internally tagged](https://serde.rs/enum-representations.html#internally-tagged) enums with named newtype variants for all mappings.
* **`oneOf` without a `discriminator`**: [Untagged](https://serde.rs/enum-representations.html#untagged) enums with automatically named variants for all subschemas.
* **`anyOf`**, with or without a `discriminator`: Structs with optional [flattened fields](https://serde.rs/attr-flatten.html) for all subschemas.

For example, given this `oneOf` schema:

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

Ploidy generates:

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

> [!NOTE]
> `JsonPointee` and `JsonPointerTarget` are [ploidy-pointer](https://crates.io/crates/ploidy-pointer) traits that make the generated types navigable with JSON Pointer.

For `allOf`:

```yaml
User:
  type: object
  required: [id, email]
  properties:
    id:
      type: string
    email:
      type: string
AdminUser:
  allOf:
    - $ref: "#/components/schemas/User"
  required: [role]
  properties:
    role:
      type: string
```

Ploidy generates:

```rust
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
    JsonPointee, JsonPointerTarget,
)]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub role: String,
}
```

For `anyOf`:

```yaml
Address:
  type: object
  properties:
    street:
      type: string
Email:
  type: object
  properties:
    email:
      type: string
Contact:
  anyOf:
    - $ref: "#/components/schemas/Address"
    - $ref: "#/components/schemas/Email"
```

Ploidy generates:

```rust
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
    JsonPointee, JsonPointerTarget,
)]
pub struct Contact {
    #[serde(flatten, default, skip_serializing_if = "AbsentOr::is_absent")]
    #[ploidy(pointer(flatten))]
    pub address: AbsentOr<Address>,
    #[serde(flatten, default, skip_serializing_if = "AbsentOr::is_absent")]
    #[ploidy(pointer(flatten))]
    pub email: AbsentOr<Email>,
}
```

> [!NOTE]
> `AbsentOr` is an `Option`-like type that distinguishes between "value not present" and "value present but `null`".

### Inline schemas

Every example we've seen so far has used named schemas from `/components/schemas`. OpenAPI also allows anonymous schemas anywhere a schema is expected: in operation parameters, in request and response bodies, and inside other schemas.

Ploidy generates the same typed models for these inline schemas, with descriptive names that reflect their usage in the spec.

For example, given an operation with this inline response schema:

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
      "200":
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

### The client

In addition to typed models for your schemas, Ploidy generates a client with methods for every operation in your spec. Parameters and request bodies become method arguments; response schemas become return types.

Given a spec with an operation like:

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
      "200":
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/User"
```

...you can use the generated client to call that operation like:

```rust
use my_api_client::{Client, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let client = Client::new("https://api.example.com/v1")?
        .with_user_agent("my-api-client/0.1")?
        .with_header("Accept-Language", "en-US")?
        .with_sensitive_header("Authorization", "Bearer decafbadcafed00d")?;

    let user = client.get_user("user_123").await?;
    println!("{} <{}>", user.id, user.email);

    Ok(())
}
```

> [!NOTE]
> `with_user_agent`, `with_header`, and `with_sensitive_header` all set default headers for each request. Sensitive headers are excluded from debug output.

The generated client uses [Reqwest](https://docs.rs/reqwest) under the hood. If you need to configure connection options, like proxies, timeouts, or TLS, build your own `reqwest::Client` and pass it to `Client::with_reqwest_client`.

For requests that the typed methods don't cover, `Client::request` returns a raw `RequestBuilder` with the client's base URL and default headers already applied.

### Smart boxing

Schemas that represent graph- and tree-like structures can have circular references: a `User` might have `friends: Vec<User>`, a `Comment` might have a `parent: Option<Comment>` and `children: Vec<Comment>`, and so on. Ploidy detects these recursive types and inserts indirection where necessary.

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

Because `Vec` already provides indirection, `children` doesn't change; only `parent` needs a `Box` to break its cycle.

### Per-resource feature gates

Ploidy uses the `x-resourceId` (on schemas) and `x-resource-name` (on operations) extensions to generate [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html) and `#[cfg(feature = "...")]` attributes.

For example, given this spec:

```yaml
paths:
  /orders/{id}:
    get:
      operationId: getOrder
      x-resource-name: orders
      # ...
      responses:
        "200":
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Order"
components:
  schemas:
    Order:
      type: object
      x-resourceId: order
      properties:
        customer:
          $ref: "#/components/schemas/Customer"
        billing:
          $ref: "#/components/schemas/BillingInfo"
    Customer:
      type: object
      x-resourceId: customer
      properties:
        billing:
          $ref: "#/components/schemas/BillingInfo"
    BillingInfo:
      type: object
      x-resourceId: billing_info
      properties:
        card_number:
          type: string
```

Ploidy generates a feature for each resource:

```toml
[features]
billing-info = []
customer = ["billing-info"]
default = ["billing-info", "customer", "order", "orders"]
order = ["billing-info", "customer"]
orders = ["billing-info", "customer", "order"]
```

...gates each client method behind its operation's resource:

```rust
impl Client {
    #[cfg(feature = "orders")]
    pub async fn get_order(&self, id: &str) -> Result<types::Order, Error> {
        // ...
    }
}
```

...and gates each schema behind its own resource and the resources of the operations that use it:

```rust
#[cfg(all(feature = "customer", feature = "orders"))]
pub struct Customer {
    // ...
}
```

All features are enabled by default, so the generated crate works out of the box. To enable just a subset of the generated features:

```toml
[dependencies]
my-api-client = { version = "1", default-features = false, features = ["orders"] }
```

### Choosing the right tool

Ploidy focuses on generating Rust clients from modern OpenAPI specs. The broader ecosystem has strong options for other needs:

| If you need... | Look for... |
|----------------|-------------|
| Custom templates or a different HTTP client | A template-based generator like [OpenAPI Generator](https://openapi-generator.tech) or [Schema Tools](https://github.com/kstasik/schema-tools) |
| Languages other than Rust | OpenAPI Generator, or [Swagger Codegen](https://github.com/swagger-api/swagger-codegen) for OpenAPI < 3.1 |
| OpenAPI 2.0 (Swagger) support | OpenAPI Generator or Swagger Codegen |
| Server stubs | OpenAPI Generator for Rust web frameworks, or [Dropshot](https://github.com/oxidecomputer/dropshot) for generating specs from Rust definitions |

Ploidy is opinionated by design. We'd rather get the defaults right than expose a page of configuration options. If you need a feature that isn't supported yet, please [open an issue](https://github.com/linabutler/ploidy/issues/new)—it helps shape our roadmap!

## Supported OpenAPI features

### For schemas

| Feature | Status | Generated output |
|---------|--------|------------------|
| `type: [...]` | Supported | Type-only unions become untagged enums |
| `type: string`, `integer`, `number`, `boolean` | Supported | - |
| `format: date-time`, `unix-time`, `date`, `uri`, `uuid`, `byte`, `binary`, `int*`, `uint*`, `float`, `double` | Supported | - |
| `type: array`, `items` | Supported | `Vec<T>` |
| `type: object`, `properties`, `required` | Supported | Structs with `T` or `AbsentOr<T>` fields |
| `additionalProperties` | Supported | `BTreeMap<String, T>` when standalone; a flattened map field when mixed with named `properties` |
| `$ref` | Partial | Document-relative `#/components/schemas/...` references only; no external or nested references. `$ref` schemas with adjacent keywords become `allOf` |
| `enum` | Supported | Enums with all string values become Rust unit enums that derive built-in traits and implement `FromStr` and `Display`. Other enums become `String` type aliases |
| `nullable`, `type: [T, "null"]`, `oneOf` with `null` | Supported | `nullable` schemas and `[T, "null"]` unions become `Option<T>` type aliases; required nullable fields become `Option<T>`; optional fields become `AbsentOr<T>` |
| `allOf`, `oneOf`, `anyOf` | Supported | Covered in [Polymorphism first](#polymorphism-first) |
| Empty or unconstrained schemas | Supported | `serde_json::Value` |

### For operations

| Feature | Status | Generated output |
|---------|--------|------------------|
| Operations | Partial | `GET`, `POST`, `PUT`, `PATCH`, and `DELETE` operations with `operationId` become async client methods |
| Path parameters | Supported | `&str` arguments interpolated into path templates |
| Query parameters | Supported | `{OperationId}Query` struct argument |
| Query `style` | Supported | `form`, `spaceDelimited`, `pipeDelimited`, `deepObject` |
| Header and cookie parameters | Unsupported | - |
| Request bodies | Partial | `application/json` and `*/*` schemas become typed arguments; `multipart/form-data` becomes `reqwest::multipart::Form` |
| Responses | Partial | The first `application/json` or `*/*` schema from either the lowest 2xx response or `default` becomes the return value; other response schemas are ignored |

## Contributing

We love contributions!

If you find a case where Ploidy fails or generates incorrect or awkward code, please [open an issue](https://github.com/linabutler/ploidy/issues/new) with your OpenAPI spec. For questions or larger contributions, please [start a discussion](https://github.com/linabutler/ploidy/discussions).

Some areas where we'd especially appreciate help:

* OpenAPI feature coverage, particularly features that specs in the wild commonly use.
* Test coverage for edge cases.
* Documentation improvements.
* Support for new vendor extensions that group operations and types into Cargo features.

We follow the [LLVM AI Tool Use Policy](https://llvm.org/docs/AIToolPolicy.html) for contributions. Please review all AI-generated code and text before opening PRs, issues, or discussions; disclose substantial AI assistance; and be ready to answer questions about your change or request.

### New languages

Ploidy currently targets only Rust, but its architecture is designed to support other languages. We'll add a language target when we can:

1. Generate code from valid syntax trees, not from string templates. We want hard-to-generate constructs to be as reliable as simple ones.
2. Leverage existing parsers, linters, and formatters written _in_ Rust, like [SWC](https://swc.rs), [Biome](https://biomejs.dev), and [Ruff](https://astral.sh/ruff).
3. Maintain the same generated code quality as our Rust pipeline.

This means Ploidy won't target every language. We'd rather support a few languages well than many languages with gaps.

## Acknowledgments

Ploidy is inspired by and builds on the wonderful work of:

* The OpenAPI ecosystem: OpenAPI Generator, [Progenitor](https://github.com/oxidecomputer/progenitor), and other code generators.
* The Rust ecosystem: Tokio, Reqwest, Serde, `quote`, `syn`, and `winnow`.
* [Petgraph](https://crates.io/crates/petgraph), the Rust graph data structure library behind Ploidy's type graph.

And yes, the name is [a biology pun](https://en.wikipedia.org/wiki/Ploidy)!
