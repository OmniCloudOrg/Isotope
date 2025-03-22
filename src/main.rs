use anyhow::{Context, Result};
use log::{debug, info};
use std::process;

mod automation;
mod cli;
mod config;
mod core;
mod iso;
mod utils;

use cli::Command;
use core::builder::IsoBuilder;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();

    info!("Starting ISOtope v{}", env!("CARGO_PKG_VERSION"));
    debug!("Debug logging enabled");

    // Parse command line arguments
    let opts = cli::parse_args().context("Failed to parse command line arguments")?;
    debug!("Command line arguments: {:?}", opts);

    // Execute the requested command
    match opts.command {
        Command::Build(build_opts) => {
            info!("Building ISO from configuration file: {}", build_opts.config.display());
            
            // Parse and validate the configuration file
            let config = config::load_config(&build_opts.config)
                .context("Failed to load configuration file")?;
            
            // Create a builder for the ISO
            let mut builder = IsoBuilder::new(config);
            
            // Execute the build process
            builder.build()
                .context("Failed to build ISO")?;
            
            info!("ISO build completed successfully!");
        }
        Command::Validate(validate_opts) => {
            info!("Validating configuration file: {}", validate_opts.config.display());
            
            // Parse and validate the configuration file
            match config::validate_config(&validate_opts.config) {
                Ok(_) => {
                    info!("Configuration file is valid");
                }
                Err(e) => {
                    eprintln!("Configuration file is invalid: {}", e);
                    process::exit(1);
                }
            }
        }
        Command::Test(test_opts) => {
            info!("Testing ISO: {}", test_opts.iso.display());
            
            // Run the test process
            let test_result = core::tester::test_iso(&test_opts.iso, test_opts.vm_provider.as_deref())
                .context("Failed to test ISO")?;
            
            if test_result.success {
                info!("ISO test completed successfully!");
            } else {
                eprintln!("ISO test failed: {}", test_result.message.unwrap_or_default());
                process::exit(1);
            }
        }
        Command::Version => {
            println!("ISOtope v{}", env!("CARGO_PKG_VERSION"));
            println!("A flexible, OS-agnostic ISO builder for automated deployments");
            println!();
            println!("License: {}", env!("CARGO_PKG_LICENSE"));
            println!("Authors: {}", env!("CARGO_PKG_AUTHORS"));
        }
    }

    Ok(())
}