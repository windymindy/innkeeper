//! Configuration validation.
//!
//! Validates configuration values and provides helpful error messages.

use crate::common::error::ConfigError;
use crate::config::types::Config;

/// Validate a configuration and return detailed errors.
pub fn validate_config(config: &Config) -> Result<(), ConfigError> {
    let mut errors = Vec::new();

    // Validate Discord config
    if config.discord.token.is_empty() {
        errors.push("discord.token is required".to_string());
    }
    if config.discord.token == "YOUR_DISCORD_TOKEN_HERE" {
        errors.push("discord.token has not been configured (still using placeholder)".to_string());
    }

    // Validate WoW config
    if config.wow.account.username.is_empty() {
        errors.push("wow.account.username is required".to_string());
    }
    if config.wow.account.password.is_empty() {
        errors.push("wow.account.password is required".to_string());
    }
    if config.wow.character.is_empty() {
        errors.push("wow.character is required".to_string());
    }
    if config.wow.character.len() < 2 || config.wow.character.len() > 12 {
        errors.push(format!(
            "wow.character must be 2-12 characters (got {})",
            config.wow.character.len()
        ));
    }

    // Validate realm config
    if config.wow.realm.host.is_empty() {
        errors.push("wow.realm.host is required".to_string());
    }
    if config.wow.realm.name.is_empty() {
        errors.push("wow.realm.name is required".to_string());
    }
    if config.wow.realm.port == 0 {
        errors.push("wow.realm.port must be non-zero".to_string());
    }

    // Validate channel mappings
    if let Some(ref chat) = config.chat {
        for (i, mapping) in chat.channels.iter().enumerate() {
            if mapping.wow.is_empty() {
                errors.push(format!("chat.channels[{}].wow is required", i));
            }
            if mapping.discord == 0 {
                errors.push(format!("chat.channels[{}].discord must be non-zero", i));
            }
            if let Some(ref direction) = mapping.direction {
                let valid_directions = ["both", "wow_to_discord", "discord_to_wow", "w2d", "d2w"];
                if !valid_directions.contains(&direction.to_lowercase().as_str()) {
                    errors.push(format!(
                        "chat.channels[{}].direction '{}' is invalid (use: both, wow_to_discord, discord_to_wow)",
                        i, direction
                    ));
                }
            }
        }

        if chat.channels.is_empty() {
            errors.push("chat.channels is empty - no message routing configured".to_string());
        }
    }

    // Validate filter patterns (try to compile them)
    if let Some(ref filters) = config.filters {
        if let Some(ref patterns) = filters.wow_to_discord {
            for (i, pattern) in patterns.iter().enumerate() {
                if regex::Regex::new(pattern).is_err() {
                    errors.push(format!(
                        "filters.wow_to_discord[{}] is not a valid regex: '{}'",
                        i, pattern
                    ));
                }
            }
        }
        if let Some(ref patterns) = filters.discord_to_wow {
            for (i, pattern) in patterns.iter().enumerate() {
                if regex::Regex::new(pattern).is_err() {
                    errors.push(format!(
                        "filters.discord_to_wow[{}] is not a valid regex: '{}'",
                        i, pattern
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ConfigError::ValidationError {
            message: errors.join("\n"),
        })
    }
}

/// Quick check if config has the minimum required fields populated.
pub fn has_required_fields(config: &Config) -> bool {
    !config.discord.token.is_empty()
        && !config.wow.account.username.is_empty()
        && !config.wow.account.password.is_empty()
        && !config.wow.character.is_empty()
        && !config.wow.realm.host.is_empty()
        && !config.wow.realm.name.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::*;

    fn make_valid_config() -> Config {
        Config {
            wow: WowConfig {
                realm: RealmConfig {
                    host: "localhost".to_string(),
                    port: 3724,
                    name: "Ascension".to_string(),
                },
                account: AccountConfig {
                    username: "testuser".to_string(),
                    password: "testpass".to_string(),
                },
                character: "TestChar".to_string(),
            },
            discord: DiscordConfig {
                token: "valid_token_here".to_string(),
                guild_id: Some(123456789),
                enable_dot_commands: Some(true),
            },
            guild: None,
            chat: Some(ChatConfig {
                channels: vec![ChannelMapping {
                    wow: "guild".to_string(),
                    discord: 987654321,
                    direction: None,
                    format: None,
                }],
            }),
            filters: None,
        }
    }

    #[test]
    fn test_valid_config_passes() {
        let config = make_valid_config();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_empty_token_fails() {
        let mut config = make_valid_config();
        config.discord.token = String::new();

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("discord.token"));
    }

    #[test]
    fn test_placeholder_token_fails() {
        let mut config = make_valid_config();
        config.discord.token = "YOUR_DISCORD_TOKEN_HERE".to_string();

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("placeholder"));
    }

    #[test]
    fn test_short_character_name_fails() {
        let mut config = make_valid_config();
        config.wow.character = "A".to_string();

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("2-12 characters"));
    }

    #[test]
    fn test_invalid_regex_filter_fails() {
        let mut config = make_valid_config();
        config.filters = Some(FiltersConfig {
            wow_to_discord: Some(vec!["[invalid".to_string()]),
            discord_to_wow: None,
        });

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not a valid regex"));
    }

    #[test]
    fn test_invalid_direction_fails() {
        let mut config = make_valid_config();
        if let Some(ref mut chat) = config.chat {
            chat.channels[0].direction = Some("invalid_direction".to_string());
        }

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("direction"));
    }

    #[test]
    fn test_has_required_fields() {
        let config = make_valid_config();
        assert!(has_required_fields(&config));

        let mut invalid = make_valid_config();
        invalid.discord.token = String::new();
        assert!(!has_required_fields(&invalid));
    }
}
