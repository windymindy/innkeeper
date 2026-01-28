//! Environment variable overrides for configuration.
//!
//! Supports overriding config values with environment variables:
//! - `INNKEEPER_DISCORD_TOKEN` - Discord bot token
//! - `INNKEEPER_WOW_USERNAME` - WoW account username
//! - `INNKEEPER_WOW_PASSWORD` - WoW account password
//! - `INNKEEPER_WOW_CHARACTER` - Character name
//! - `INNKEEPER_REALM_HOST` - Realm server host
//! - `INNKEEPER_REALM_PORT` - Realm server port
//! - `INNKEEPER_REALM_NAME` - Realm name to connect to

use std::env;

use crate::config::types::Config;

/// Environment variable prefix for all config overrides.
const ENV_PREFIX: &str = "INNKEEPER";

/// Apply environment variable overrides to a config.
///
/// This allows sensitive values like tokens and passwords to be
/// provided via environment variables instead of the config file.
pub fn apply_env_overrides(mut config: Config) -> Config {
    // Discord token
    if let Ok(token) = env::var(format!("{}_DISCORD_TOKEN", ENV_PREFIX)) {
        config.discord.token = token;
    }

    // WoW credentials
    if let Ok(username) = env::var(format!("{}_WOW_USERNAME", ENV_PREFIX)) {
        config.wow.account.username = username;
    }
    if let Ok(password) = env::var(format!("{}_WOW_PASSWORD", ENV_PREFIX)) {
        config.wow.account.password = password;
    }
    if let Ok(character) = env::var(format!("{}_WOW_CHARACTER", ENV_PREFIX)) {
        config.wow.character = character;
    }

    // Realm settings
    if let Ok(host) = env::var(format!("{}_REALM_HOST", ENV_PREFIX)) {
        config.wow.realm.host = host;
    }
    if let Ok(port) = env::var(format!("{}_REALM_PORT", ENV_PREFIX)) {
        if let Ok(port) = port.parse() {
            config.wow.realm.port = port;
        }
    }
    if let Ok(name) = env::var(format!("{}_REALM_NAME", ENV_PREFIX)) {
        config.wow.realm.name = name;
    }

    // Discord guild ID
    if let Ok(guild_id) = env::var(format!("{}_DISCORD_GUILD_ID", ENV_PREFIX)) {
        if let Ok(id) = guild_id.parse() {
            config.discord.guild_id = Some(id);
        }
    }

    config
}

/// Check if any required environment variables are set but empty.
///
/// Returns a list of variable names that are set but empty.
pub fn check_empty_env_vars() -> Vec<String> {
    let vars = [
        format!("{}_DISCORD_TOKEN", ENV_PREFIX),
        format!("{}_WOW_USERNAME", ENV_PREFIX),
        format!("{}_WOW_PASSWORD", ENV_PREFIX),
    ];

    vars.into_iter()
        .filter(|var| env::var(var).map(|v| v.is_empty()).unwrap_or(false))
        .collect()
}

/// Get the config file path from environment or use default.
///
/// Checks `INNKEEPER_CONFIG` environment variable, otherwise returns "innkeeper.conf".
pub fn get_config_path() -> String {
    env::var(format!("{}_CONFIG", ENV_PREFIX)).unwrap_or_else(|_| "innkeeper.conf".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::*;

    fn make_test_config() -> Config {
        Config {
            wow: WowConfig {
                realm: RealmConfig {
                    host: "localhost".to_string(),
                    port: 3724,
                    name: "Test".to_string(),
                },
                account: AccountConfig {
                    username: "test".to_string(),
                    password: "test".to_string(),
                },
                character: "TestChar".to_string(),
            },
            discord: DiscordConfig {
                token: "original_token".to_string(),
                guild_id: None,
                enable_dot_commands: None,
            },
            guild: None,
            chat: None,
            filters: None,
        }
    }

    #[test]
    fn test_env_prefix() {
        assert_eq!(ENV_PREFIX, "INNKEEPER");
    }

    #[test]
    fn test_get_config_path_default() {
        // Clear the env var first
        env::remove_var("INNKEEPER_CONFIG");
        assert_eq!(get_config_path(), "innkeeper.conf");
    }

    #[test]
    fn test_apply_env_overrides_no_vars() {
        // Clear all relevant env vars
        env::remove_var("INNKEEPER_DISCORD_TOKEN");
        env::remove_var("INNKEEPER_WOW_USERNAME");

        let config = make_test_config();
        let result = apply_env_overrides(config);

        // Should remain unchanged
        assert_eq!(result.discord.token, "original_token");
        assert_eq!(result.wow.account.username, "test");
    }
}
