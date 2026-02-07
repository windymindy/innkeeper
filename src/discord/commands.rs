//! Discord bot commands (!who, !gmotd, etc).
//!
//! Handles command parsing and execution for Discord commands.

use serenity::model::channel::Message;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::common::messages::CommandResponseData;

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
    pub content: CommandResponseData,
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
