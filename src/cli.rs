use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "isotope")]
#[command(about = "A flexible, OS-agnostic ISO builder for automated deployments", long_about = None)]
#[command(version, author, propagate_version = true)]
pub struct Opts {
    /// Set verbosity level (can be repeated for more verbosity)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build an ISO from a configuration file
    Build(BuildOpts),
    
    /// Validate a configuration file without building
    Validate(ValidateOpts),
    
    /// Test an ISO in a virtual machine
    Test(TestOpts),
    
    /// Display version information
    Version,
}

#[derive(Debug, Args)]
pub struct BuildOpts {
    /// Path to the configuration file
    #[arg(name = "CONFIG")]
    pub config: PathBuf,
    
    /// Skip validation checks
    #[arg(long)]
    pub skip_validation: bool,
    
    /// Skip testing the resulting ISO
    #[arg(long)]
    pub skip_test: bool,
    
    /// Don't clean up temporary files
    #[arg(long)]
    pub no_cleanup: bool,
    
    /// Override environment variables from the command line (KEY=VALUE)
    #[arg(short, long, value_name = "KEY=VALUE")]
    pub env: Vec<String>,
    
    /// Override output path
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ValidateOpts {
    /// Path to the configuration file
    #[arg(name = "CONFIG")]
    pub config: PathBuf,
    
    /// Strict validation mode
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Args)]
pub struct TestOpts {
    /// Path to the ISO file
    #[arg(name = "ISO")]
    pub iso: PathBuf,
    
    /// VM provider to use (qemu, virtualbox, vmware)
    #[arg(long)]
    pub vm_provider: Option<String>,
    
    /// Memory to allocate to the VM
    #[arg(long)]
    pub memory: Option<String>,
    
    /// Number of CPUs to allocate to the VM
    #[arg(long)]
    pub cpus: Option<u8>,
    
    /// Timeout for the test in seconds (default: 3600)
    #[arg(long, default_value = "3600")]
    pub timeout: u64,
    
    /// Display GUI during testing
    #[arg(long)]
    pub gui: bool,
}

/// Parse command line arguments
pub fn parse_args() -> Result<Opts> {
    let opts = Opts::parse();
    
    // Configure logging verbosity based on command line arguments
    match opts.verbose {
        0 => std::env::set_var("RUST_LOG", "info"),
        1 => std::env::set_var("RUST_LOG", "debug"),
        _ => std::env::set_var("RUST_LOG", "trace"),
    }
    
    Ok(opts)
}