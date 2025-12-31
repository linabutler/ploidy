# Ploidy

[![crates.io](https://img.shields.io/crates/v/ploidy?style=for-the-badge)](https://crates.io/crates/ploidy)
[![Build status](https://img.shields.io/github/actions/workflow/status/linabutler/ploidy/test.yml?style=for-the-badge)](https://github.com/linabutler/ploidy/actions?query=branch%3Amain)
[![Documentation](https://img.shields.io/docsrs/ploidy/latest?style=for-the-badge)](https://docs.rs/ploidy)

Ploidy is a code generator for OpenAPI schemas that use inheritance and polymorphism. Currently, Ploidy only generates Rust code, though support for Python and TypeScript is planned.

## Installation

You can [download a pre-built binary of Ploidy for your platform](https://github.com/linabutler/ploidy/releases/latest), or install from source with:

```sh
cargo install --locked ploidy
```

## Generating Rust code

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
| `--name <NAME>` | Set or override the generated package name. If not passed, and a package already exists in the output directory, defaults to the name of that package; otherwise, defaults to the name of the output directory |
| `--version <bump-major, bump-minor, bump-patch>` | If a package already exists in the output directory, increment its major, minor, or patch version. If not passed, keeps the existing package version. Ignored if the package doesn't exist yet |
