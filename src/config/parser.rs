//! Configuration file parsing (HOCON format).

use std::path::Path;

use crate::config::types::Config;
use anyhow::{anyhow, Context, Result};

/// Load configuration from a HOCON file.
pub fn load_config(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref();

    hocon_rs::Config::load(path, None)
        .with_context(|| format!("Failed to load config file: {}", path.display()))
}

/// Load configuration from a HOCON string.
pub fn load_config_str(content: &str) -> Result<Config> {
    hocon_rs::Config::parse_str(content, None).map_err(|e| anyhow!("Failed to parse config: {}", e))
}
