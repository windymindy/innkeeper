//! Discord bot commands (!who, !gmotd, etc).
//!
//! Handles command parsing and execution for Discord commands.

use serenity::model::channel::Message;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Commands that can be sent to the WoW handler.
#[derive(Debug, Clone)]
pub enum WowCommand {
    /// Request guild roster (!who or !who <name>).
    Who { args: Option<String>, reply_channel: u64 },
    /// Request guild MOTD (!gmotd).
    GuildMotd { reply_channel: u64 },
}

/// Responses from the WoW handler.
#[derive(Debug, Clone)]
pub struct CommandResponse {
    pub channel_id: u64,
    pub content: String,
}

impl CommandResponse {
    pub fn new(channel_id: u64, content: String) -> Self {
        Self { channel_id, content }
    }
}

/// Command handler for Discord bot.
pub struct CommandHandler {
    /// Channel to send commands to WoW handler.
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
}

impl CommandHandler {
    pub fn new(command_tx: mpsc::UnboundedSender<WowCommand>) -> Self {
        Self { command_tx }
    }

    /// Parse and execute a command from Discord.
    ///
    /// Returns `true` if the message was a command, `false` otherwise.
    pub async fn handle_command(
        &self,
        ctx: &Context,
        msg: &Message,
        content: &str,
    ) -> anyhow::Result<bool> {
        if content.len() > 100 {
            return Ok(false);
        }
        if !content.starts_with('!') && !content.starts_with('?') {
            return Ok(false);
        }

        let parts: Vec<&str> = content[1..].splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();
        let args = parts.get(1).map(|s| s.trim().to_string());

        debug!("Processing command: {} with args: {:?}", command, args);

        match command.as_str() {
            "who" | "online" => {
                self.handle_who(ctx, msg, args).await?;
                Ok(true)
            }
            "gmotd" => {
                self.handle_gmotd(ctx, msg).await?;
                Ok(true)
            }
            "help" => {
                self.handle_help(ctx, msg).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Handle !who command.
    async fn handle_who(
        &self,
        ctx: &Context,
        msg: &Message,
        args: Option<String>,
    ) -> anyhow::Result<()> {
        info!("!who command from {} with args: {:?}", msg.author.name, args);

        let command = WowCommand::Who {
            args,
            reply_channel: msg.channel_id.get(),
        };

        if self.command_tx.send(command).is_err() {
            msg.channel_id
                .say(&ctx.http, "Error: Not connected to WoW server.")
                .await?;
        } else {
            // The response will be sent asynchronously
            msg.react(&ctx.http, '👀').await.ok();
        }

        Ok(())
    }

    /// Handle !gmotd command.
    async fn handle_gmotd(&self, ctx: &Context, msg: &Message) -> anyhow::Result<()> {
        info!("!gmotd command from {}", msg.author.name);

        let command = WowCommand::GuildMotd {
            reply_channel: msg.channel_id.get(),
        };

        if self.command_tx.send(command).is_err() {
            msg.channel_id
                .say(&ctx.http, "Error: Not connected to WoW server.")
                .await?;
        } else {
            msg.react(&ctx.http, '📜').await.ok();
        }

        Ok(())
    }

    /// Handle !help command.
    async fn handle_help(&self, ctx: &Context, msg: &Message) -> anyhow::Result<()> {
        let help_text = r#"**Available Commands:**
• `!who` - List online guild members
• `!who <name>` - Search for a player
• `!gmotd` - Show guild Message of the Day
• `!help` - Show this help message"#;

        msg.channel_id.say(&ctx.http, help_text).await?;
        Ok(())
    }
}

/// Format a !who response with guild members.
pub fn format_who_response(
    members: &[(String, u8, String)], // (name, level, zone)
    guild_name: Option<&str>,
) -> String {
    if members.is_empty() {
        return "No guild members online.".to_string();
    }

    let count = members.len();
    let header = if let Some(name) = guild_name {
        format!("**{}** - {} member{} online:", name, count, if count == 1 { "" } else { "s" })
    } else {
        format!("{} guild member{} online:", count, if count == 1 { "" } else { "s" })
    };

    let mut lines = vec![header];

    for (name, level, zone) in members {
        lines.push(format!("• **{}** (Lvl {}) - {}", name, level, zone));
    }

    lines.join("\n")
}

/// Format a !who search response for a specific player.
pub fn format_who_search_response(
    player_name: &str,
    found: Option<&(String, u8, String, Option<String>)>, // (name, level, zone, guild)
) -> String {
    match found {
        Some((name, level, zone, guild)) => {
            let guild_str = guild.as_ref().map(|g| format!(" <{}>", g)).unwrap_or_default();
            format!("**{}**{} - Level {} in {}", name, guild_str, level, zone)
        }
        None => format!("Player '{}' is not currently online.", player_name),
    }
}

/// Format a !gmotd response.
pub fn format_gmotd_response(motd: Option<&str>, guild_name: Option<&str>) -> String {
    match motd {
        Some(m) if !m.is_empty() => {
            if let Some(name) = guild_name {
                format!("**{}** MOTD:\n{}", name, m)
            } else {
                format!("Guild MOTD:\n{}", m)
            }
        }
        _ => "No guild MOTD set.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_who_response() {
        let members = vec![
            ("Alice".to_string(), 80, "Dalaran".to_string()),
            ("Bob".to_string(), 75, "Stormwind".to_string()),
        ];

        let response = format_who_response(&members, Some("Test Guild"));
        assert!(response.contains("Test Guild"));
        assert!(response.contains("2 members online"));
        assert!(response.contains("Alice"));
        assert!(response.contains("Bob"));
    }

    #[test]
    fn test_format_who_search_response_found() {
        let player = ("TestPlayer".to_string(), 80, "Dalaran".to_string(), Some("Cool Guild".to_string()));
        let response = format_who_search_response("TestPlayer", Some(&player));
        assert!(response.contains("TestPlayer"));
        assert!(response.contains("Cool Guild"));
        assert!(response.contains("80"));
    }

    #[test]
    fn test_format_who_search_response_not_found() {
        let response = format_who_search_response("UnknownPlayer", None);
        assert!(response.contains("UnknownPlayer"));
        assert!(response.contains("not currently online"));
    }

    #[test]
    fn test_format_gmotd_response() {
        let response = format_gmotd_response(Some("Welcome to the guild!"), Some("Awesome Guild"));
        assert!(response.contains("Awesome Guild"));
        assert!(response.contains("Welcome to the guild!"));
    }
}
