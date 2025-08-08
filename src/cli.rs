use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Commands {
    /// Build an ISO from an Isotope specification
    Build {
        /// Path to the Isotope specification file
        spec_file: PathBuf,
        /// Output path for the generated ISO (overrides spec)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Validate an Isotope specification
    Validate {
        /// Path to the Isotope specification file
        spec_file: PathBuf,
    },
    /// Test an Isotope specification in a VM
    Test {
        /// Path to the Isotope specification file
        spec_file: PathBuf,
    },
    /// Convert a JSON config to Isotope format
    Convert {
        /// Input JSON file path
        input: PathBuf,
        /// Output Isotope file path
        output: PathBuf,
    },
}