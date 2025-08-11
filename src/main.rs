use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};

mod automation;
mod cli;
mod config;
mod core;
mod iso;
mod utils;

use cli::Commands;
use config::IsotopeSpec;
use core::Builder;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(name = "isotope")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable OCR debug messages during screen text detection
    #[arg(long)]
    ocr_debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("isotope={},warn", log_level))
        .init();

    info!("Isotope v{} starting", env!("CARGO_PKG_VERSION"));

    let result = match cli.command {
        Commands::Build {
            spec_file,
            output,
            continue_from,
        } => {
            info!("Building ISO from specification: {}", spec_file.display());

            if let Some(step) = continue_from {
                info!("Continuing from step {}", step);
            }

            let spec = IsotopeSpec::from_file(&spec_file)
                .with_context(|| format!("Failed to load spec file: {}", spec_file.display()))?;

            let mut builder = Builder::new_with_ocr_debug(spec, cli.ocr_debug);
            builder.set_spec_file_path(spec_file.clone());

            if let Some(output_path) = output {
                builder.set_output_path(output_path);
            }

            if let Some(step) = continue_from {
                builder.set_continue_from_step(step);
            }

            builder.build().await
        }
        Commands::Validate { spec_file } => {
            info!("Validating specification: {}", spec_file.display());

            match IsotopeSpec::from_file(&spec_file) {
                Ok(spec) => {
                    info!("✓ Specification is valid");
                    spec.validate()
                }
                Err(e) => {
                    error!("✗ Specification is invalid: {}", e);
                    Err(e)
                }
            }
        }
        Commands::Test { spec_file } => {
            info!("Testing specification: {}", spec_file.display());

            let spec = IsotopeSpec::from_file(&spec_file)
                .with_context(|| format!("Failed to load spec file: {}", spec_file.display()))?;

            let builder = Builder::new_with_ocr_debug(spec, cli.ocr_debug);
            builder.test().await
        }
        Commands::Convert { input, output } => {
            info!("Converting {} to Isotope format", input.display());

            config::converter::convert_json_to_isotope(&input, &output)
                .with_context(|| "Failed to convert configuration")
        }
    };

    match result {
        Ok(_) => {
            info!("✓ Operation completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("✗ Operation failed: {}", e);
            std::process::exit(1);
        }
    }
}
