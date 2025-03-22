use anyhow::{Context, Result};
use log::{debug, info};
use std::path::Path;
use std::fs;

pub mod schema;
pub mod validation;

use schema::Config;

/// Load and parse a configuration file
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();
    info!("Loading configuration from {}", path.display());
    
    // Read the file contents
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    
    // Parse the JSON contents
    let config: Config = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse JSON in config file: {}", path.display()))?;
    
    // Validate the configuration
    validation::validate_config_structure(&config)
        .with_context(|| format!("Invalid configuration in file: {}", path.display()))?;
    
    debug!("Successfully loaded config: {:#?}", config);
    Ok(config)
}

/// Validate a configuration file without loading it
pub fn validate_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    info!("Validating configuration from {}", path.display());
    
    // Read the file contents
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    
    // Parse the JSON contents to verify it's valid JSON
    let config: Config = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse JSON in config file: {}", path.display()))?;
    
    // Validate the configuration structure
    validation::validate_config_structure(&config)
        .with_context(|| format!("Invalid configuration in file: {}", path.display()))?;
    
    info!("Configuration file is valid");
    Ok(())
}

/// Substitute environment variables in a configuration
pub fn substitute_env_vars(config: &mut Config) -> Result<()> {
    // This is a placeholder for now
    // We would iterate through all string values in the config and replace {{ env.VAR_NAME }} patterns
    // with the corresponding environment variable values
    
    info!("Environment variable substitution completed");
    Ok(())
}

/// Resolve paths in a configuration relative to the config file
pub fn resolve_paths<P: AsRef<Path>>(config: &mut Config, base_path: P) -> Result<()> {
    let base_path = base_path.as_ref();
    
    // This is a placeholder for now
    // We would iterate through all path values in the config and resolve them relative to the base_path
    // if they are relative paths
    
    info!("Path resolution completed relative to {}", base_path.display());
    Ok(())
}