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
        errors.push(
            "discord.token is required (set in config or use DISCORD_TOKEN env var)".to_string(),
        );
    } else if config.discord.token == "YOUR_DISCORD_TOKEN_HERE" {
        errors.push("discord.token has not been configured (still using placeholder)".to_string());
    }

    // Validate WoW config
    if config.wow.account.is_empty() {
        errors
            .push("wow.account is required (set in config or use WOW_ACCOUNT env var)".to_string());
    }

    if config.wow.password.is_empty() {
        errors.push(
            "wow.password is required (set in config or use WOW_PASSWORD env var)".to_string(),
        );
    }

    if config.wow.character.is_empty() {
        errors.push(
            "wow.character is required (set in config or use WOW_CHARACTER env var)".to_string(),
        );
    } else if config.wow.character.len() < 2 || config.wow.character.len() > 12 {
        errors.push(format!(
            "wow.character must be 2-12 characters (got {})",
            config.wow.character.len()
        ));
    }

    // Validate realm config
    if config.wow.realmlist.is_empty() {
        errors.push("wow.realmlist is required".to_string());
    }
    if config.wow.realm.is_empty() {
        errors.push("wow.realm is required".to_string());
    }

    // Validate channel mappings
    let chat = &config.chat;
    for (i, mapping) in chat.channels.iter().enumerate() {
        if mapping.wow.channel_type.is_empty() {
            errors.push(format!("chat.channels[{}].wow.type is required", i));
        }

        // Validate channel type
        let valid_types = [
            "Guild", "Officer", "Say", "Yell", "Emote", "System", "Channel", "Whisper",
        ];
        if !valid_types.contains(&mapping.wow.channel_type.as_str()) {
            errors.push(format!(
                "chat.channels[{}].wow.type '{}' is invalid (use: {})",
                i,
                mapping.wow.channel_type,
                valid_types.join(", ")
            ));
        }

        // Custom channels need a channel name
        if mapping.wow.channel_type == "Channel" && mapping.wow.channel.is_none() {
            errors.push(format!(
                "chat.channels[{}].wow.channel is required when type is 'Channel'",
                i
            ));
        }

        // Discord channel name is required
        if mapping.discord.channel.is_empty() {
            errors.push(format!("chat.channels[{}].discord.channel is required", i));
        }

        // Validate direction
        let valid_directions = ["both", "wow_to_discord", "discord_to_wow"];
        if !valid_directions.contains(&mapping.direction.as_str()) {
            errors.push(format!(
                "chat.channels[{}].direction '{}' is invalid (use: {})",
                i,
                mapping.direction,
                valid_directions.join(", ")
            ));
        }
    }

    if chat.channels.is_empty() {
        errors.push("chat.channels is empty - no message routing configured".to_string());
    }

    // Validate filter patterns (try to compile them)
    if let Some(ref filters) = config.filters {
        if let Some(ref patterns) = filters.patterns {
            for (i, pattern) in patterns.iter().enumerate() {
                if regex::Regex::new(pattern).is_err() {
                    errors.push(format!(
                        "filters.patterns[{}] is not a valid regex: '{}'",
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
        && !config.wow.account.is_empty()
        && !config.wow.password.is_empty()
        && !config.wow.character.is_empty()
        && !config.wow.realmlist.is_empty()
        && !config.wow.realm.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::GuildDashboardConfig;
    use crate::config::types::*;

    fn make_valid_config() -> Config {
        Config {
            discord: DiscordConfig {
                token: "valid_token_here".to_string(),
                enable_dot_commands: true,
                dot_commands_whitelist: None,
                enable_commands_channels: None,
                enable_tag_failed_notifications: false,
            },
            wow: WowConfig {
                platform: "Mac".to_string(),
                enable_server_motd: true,
                version: "3.3.5".to_string(),
                realm_build: None,
                game_build: None,
                realmlist: "logon.project-ascension.com".to_string(),
                realm: "Laughing Skull".to_string(),
                account: "testuser".to_string(),
                password: "testpass".to_string(),
                character: "TestChar".to_string(),
            },
            guild: GuildEventsConfig::default(),
            chat: ChatConfig {
                channels: vec![ChannelMapping {
                    direction: "both".to_string(),
                    wow: WowChannelConfig {
                        channel_type: "Guild".to_string(),
                        channel: None,
                        format: Some("[%user]: %message".to_string()),
                    },
                    discord: DiscordChannelConfig {
                        channel: "guild-chat".to_string(),
                        format: Some("[%user]: %message".to_string()),
                    },
                }],
            },
            filters: None,
            guild_dashboard: GuildDashboardConfig::default(),
            quirks: QuirksConfig::default(),
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
            enabled: true,
            patterns: Some(vec!["[invalid".to_string()]),
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
        config.chat.channels[0].direction = "invalid_direction".to_string();

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
