use std::borrow::Cow;
use std::collections::BTreeMap;

use miette::{Context, IntoDiagnostic, Result};

use ploidy::codegen::{rust, write_to_disk};
use ploidy::config::{Codegen, CodegenLanguage, Command, Main};
use ploidy::ir::IrSpec;
use ploidy::parse::Document;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    let Ok(main) = Main::parse().map_err(|err| err.exit());
    match main.command {
        Command::Codegen(Codegen {
            input,
            output,
            language: CodegenLanguage::Rust(config),
        }) => {
            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .with_context(|| format!("Failed to read `{}`", input.display()))?;

            let doc = Document::from_yaml(&source)
                .into_diagnostic()
                .context("Failed to parse OpenAPI document")?;

            println!("OpenAPI: {} (version {})", doc.info.title, doc.info.version);

            let spec = IrSpec::from_doc(&doc).into_diagnostic()?;

            let name = config
                .package
                .name
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| output.to_string_lossy());

            let context = rust::CodegenContext::new(
                &name,
                config.package.version_or_default(),
                config.package.license_or_default(),
                config.package.description.as_deref(),
                &spec,
            );

            println!("Writing generated code to `{}`...", output.display());

            println!("Generating `Cargo.toml`...");
            write_to_disk(&output, rust::CargoManifest::new(&context))?;

            println!("Generating `lib.rs`...");
            write_to_disk(&output, rust::CodegenLibrary)?;

            println!("Generating `error.rs`...");
            write_to_disk(&output, rust::CodegenErrorModule)?;

            println!("Generating {} types...", context.spec.schemas().count());
            rust::write_types_to_disk(&output, &context)?;

            let counts = context.spec.operations().fold(
                BTreeMap::<&str, usize>::new(),
                |mut counts, view| {
                    *counts.entry(view.op().resource).or_default() += 1;
                    counts
                },
            );
            println!(
                "Generating {} client methods for {} resources...",
                counts.values().copied().sum::<usize>(),
                counts.keys().count(),
            );
            rust::write_client_to_disk(&output, &context)?;

            println!("Generation complete");

            if config.check {
                println!("Running `cargo check`...");
                let status = std::process::Command::new("cargo")
                    .arg("check")
                    .arg("--all-targets")
                    .arg("--features")
                    .arg("full")
                    .current_dir(&output)
                    .status()
                    .into_diagnostic()?;

                if !status.success() {
                    miette::bail!("`cargo check` exited with status {status}");
                }
            }
        }
    }

    Ok(())
}
