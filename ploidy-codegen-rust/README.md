# ploidy-codegen-rust

This crate is part of the [Ploidy](https://crates.io/crates/ploidy) OpenAPI code generator. It transforms [**ploidy-core**](https://crates.io/crates/ploidy-core) types into Rust syntax trees, pretty-prints them, and saves the output to disk.

⚠️ The **ploidy-codegen-rust** API isn't stable yet.

One of the goals of this crate is to support usage from [build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html), as an alternative to the `ploidy` CLI. This can be useful if you're generating an OpenAPI client as part of a larger Rust project, and don't need the complete crate that the CLI generates.
