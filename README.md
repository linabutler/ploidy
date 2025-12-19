# Ploidy

Ploidy is a code generator that supports polymorphic OpenAPI schemas. Its goal is to generate idiomatic Rust code for schemas with complex `oneOf`, `anyOf`, and `allOf` hierarchies.

## Installation

You can [download a pre-built binary of Ploidy for your platform](https://github.com/linabutler/ploidy/releases/latest), or install from source with:

```sh
cargo install --locked ploidy
```

## Quick Start

```sh
ploidy codegen <INPUT-SPEC> <OUTPUT-DIR> rust
```

This generates a complete Rust crate in the output directory, with:

* A `types` module, with Rust definitions for each named schema type.
* A `client` module, with methods for each operation.

### Options

| Flag | Description |
|------|-------------|
| `-c`, `--check` | Run `cargo check` on the generated code |
| `--version <VERSION>` | Increment the generated package version |
| `--name <NAME>` | Override the generated package name |
| `--package-license <LICENSE>` | Override the generated package license |
| `--package-description <DESCRIPTION>` | Override the generated package description |

### Configuration File

In addition to the command-line options above, you can place a `.ploidy.toml` file in the output directory to configure generation:

```toml
[rust]
version = "bump-major"
```
