//! Environment variable overrides for configuration.
//!
//! Supports overriding config values with environment variables (wowchat style):
//! - `DISCORD_TOKEN` - Discord bot token
//! - `WOW_ACCOUNT` - WoW account username
//! - `WOW_PASSWORD` - WoW account password
//! - `WOW_CHARACTER` - Character name
//!
//! Note: The HOCON parser handles `${?VAR}` syntax automatically.
//! This module provides additional fallback support.

use std::env;

use crate::config::types::Config;

/// Apply environment variable overrides to a config.
///
/// This provides environment variable support:
/// - DISCORD_TOKEN
/// - WOW_ACCOUNT
/// - WOW_PASSWORD
/// - WOW_CHARACTER
///
/// These are applied after HOCON parsing, so they override any values
/// that weren't set via HOCON's ${?VAR} syntax.
pub fn apply_env_overrides(mut config: Config) -> Config {
    // Discord token (only if not already set)
    if config.discord.token.is_empty() {
        if let Ok(token) = env::var("DISCORD_TOKEN") {
            if !token.is_empty() {
                config.discord.token = token;
            }
        }
    }

    // WoW credentials (only if not already set)
    if config.wow.account.is_empty() {
        if let Ok(account) = env::var("WOW_ACCOUNT") {
            if !account.is_empty() {
                config.wow.account = account;
            }
        }
    }

    if config.wow.password.is_empty() {
        if let Ok(password) = env::var("WOW_PASSWORD") {
            if !password.is_empty() {
                config.wow.password = password;
            }
        }
    }

    if let Ok(character) = env::var("WOW_CHARACTER") {
        if !character.is_empty() {
            config.wow.character = character;
        }
    }

    config
}

/// Check if required configuration values are present.
///
/// Returns a list of missing required fields.
pub fn check_missing_required(config: &Config) -> Vec<String> {
    let mut missing = Vec::new();

    // Check Discord token
    if config.discord.token.is_empty() {
        missing.push("discord.token (or DISCORD_TOKEN env var)".to_string());
    }

    // Check WoW credentials
    if config.wow.account.is_empty() {
        missing.push("wow.account (or WOW_ACCOUNT env var)".to_string());
    }

    if config.wow.password.is_empty() {
        missing.push("wow.password (or WOW_PASSWORD env var)".to_string());
    }

    if config.wow.character.is_empty() {
        missing.push("wow.character (or WOW_CHARACTER env var)".to_string());
    }

    if config.wow.realmlist.is_empty() {
        missing.push("wow.realmlist".to_string());
    }

    if config.wow.realm.is_empty() {
        missing.push("wow.realm".to_string());
    }

    missing
}

/// Get the config file path from environment or use default.
///
/// Checks `INNKEEPER_CONFIG` or `WOWCHAT_CONFIG` environment variable,
/// otherwise returns "innkeeper.conf".
pub fn get_config_path() -> String {
    env::var("INNKEEPER_CONFIG")
        .or_else(|_| env::var("WOWCHAT_CONFIG"))
        .unwrap_or_else(|_| "innkeeper.conf".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ChatConfig;
    use crate::config::types::GuildDashboardConfig;
    use crate::config::types::*;

    fn make_test_config() -> Config {
        Config {
            discord: DiscordConfig {
                token: "original_token".to_string(),
                enable_dot_commands: false,
                dot_commands_whitelist: None,
                enable_commands_channels: None,
                enable_tag_failed_notifications: false,
                enable_markdown: false,
            },
            wow: WowConfig {
                platform: "Mac".to_string(),
                enable_server_motd: false,
                version: "3.3.5".to_string(),
                realm_build: None,
                game_build: None,
                realmlist: "localhost".to_string(),
                realm: "Test".to_string(),
                account: "test".to_string(),
                password: "test".to_string(),
                character: "TestChar".to_string(),
            },
            guild: GuildEventsConfig::default(),
            chat: ChatConfig::default(),
            filters: None,
            guild_dashboard: GuildDashboardConfig::default(),
            quirks: QuirksConfig::default(),
        }
    }

    #[test]
    fn test_get_config_path_default() {
        // Clear the env vars first
        env::remove_var("INNKEEPER_CONFIG");
        env::remove_var("WOWCHAT_CONFIG");
        assert_eq!(get_config_path(), "innkeeper.conf");
    }

    #[test]
    fn test_apply_env_overrides_no_vars() {
        // Clear all relevant env vars
        env::remove_var("DISCORD_TOKEN");
        env::remove_var("WOW_ACCOUNT");

        let config = make_test_config();
        let result = apply_env_overrides(config);

        // Should remain unchanged
        assert_eq!(result.discord.token, "original_token".to_string());
        assert_eq!(result.wow.account, "test".to_string());
    }

    #[test]
    fn test_check_missing_required() {
        let config = make_test_config();
        let missing = check_missing_required(&config);
        assert!(missing.is_empty(), "Should have all required fields");
    }
}
