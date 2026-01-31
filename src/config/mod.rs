//! Configuration parsing, validation, and environment variable overrides.
//!
//! # Example
//!
//! ```ignore
//! use innkeeper::config::{load_and_validate, get_config_path};
//!
//! let config = load_and_validate(get_config_path())?;
//! ```

pub mod env;
pub mod parser;
pub mod types;
pub mod validate;

pub use env::{apply_env_overrides, get_config_path};
pub use parser::load_config;
pub use types::*;
pub use validate::validate_config;

use crate::common::error::ConfigError;

/// Load, apply environment overrides, and validate configuration.
///
/// This is the main entry point for loading configuration.
pub fn load_and_validate(path: impl AsRef<std::path::Path>) -> Result<Config, ConfigError> {
    // Load from file (HOCON parser handles ${?VAR} env var substitution automatically)
    let config = load_config(path)?;

    // Apply environment variable overrides (fallback for vars not in ${?VAR} syntax)
    let config = apply_env_overrides(config);

    // Validate
    validate_config(&config)?;

    Ok(config)
}

/// Load configuration with automatic path detection.
///
/// Uses `WOWCHAT_CONFIG`, `INNKEEPER_CONFIG` environment variable,
/// or "innkeeper.conf" as default.
pub fn load_default() -> Result<Config, ConfigError> {
    load_and_validate(get_config_path())
}
