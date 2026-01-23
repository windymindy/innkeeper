//! Configuration file parsing (HOCON format).

use std::path::Path;

use crate::common::error::ConfigError;
use crate::config::types::Config;
use hocon::HoconLoader;

/// Load configuration from a HOCON file.
pub fn load_config(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();

    HoconLoader::new()
        .load_file(path)
        .map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?
        .resolve()
        .map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })
}

/// Load configuration from a HOCON string.
pub fn load_config_str(content: &str) -> Result<Config, ConfigError> {
    HoconLoader::new()
        .load_str(content)
        .map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })?
        .resolve()
        .map_err(|e| ConfigError::ValidationError {
            message: e.to_string(),
        })
}
