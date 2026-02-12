use std::time::{SystemTime, UNIX_EPOCH};
use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage, EditMessage, Message, MessageId};
use tracing::{error, info};
use crate::common::messages::GuildDashboardData;
use crate::config::types::GuildDashboardConfig;
use crate::common::resources::get_zone_name;

pub struct DashboardRenderer {
    config: GuildDashboardConfig,
    message_ids: Vec<MessageId>,
    last_data: Option<GuildDashboardData>,
}

impl DashboardRenderer {
    pub fn new(config: GuildDashboardConfig) -> Self {
        Self {
            config,
            message_ids: Vec::new(),
            last_data: None,
        }
    }

    pub async fn update(&mut self, ctx: &Context, channel_id: Option<ChannelId>, data: GuildDashboardData) {
        if !self.config.enabled {
            return;
        }

        // We need a valid channel ID to proceed
        let channel_id = match channel_id {
            Some(id) => id,
            None => {
                return;
            }
        };

        // Deduplicate updates
        if let Some(last) = &self.last_data {
            if last == &data {
                return;
            }
        }

        info!("Updating guild dashboard for {} members", data.members.len());

        // Format the dashboard
        let mut embeds = self.format_dashboard(&data);

        // If we don't have message IDs, try to find them in history
        if self.message_ids.is_empty() {
            let title = format!("{} — {}", data.guild_name, data.realm);
            if let Ok(messages) = channel_id.messages(&ctx.http, serenity::all::GetMessages::new().limit(10)).await {
                let self_id = ctx.cache.current_user().id;

                let mut found_messages: Vec<Message> = messages.into_iter()
                    .rev() // Oldest first (like Scala .reverse)
                    .skip_while(|m| {
                        // Skip until we find the first dashboard message (first page with title)
                        !(m.author.id == self_id
                            && !m.embeds.is_empty()
                            && m.embeds[0].title.as_ref().map_or(false, |t| t == &title))
                    })
                    .take_while(|m| {
                        // Take all contiguous messages authored by us with embeds
                        // First page has the title, subsequent pages have null title but are contiguous
                        m.author.id == self_id
                            && !m.embeds.is_empty()
                            && (m.embeds[0].title.as_ref().map_or(true, |t| t == &title || t.is_empty())
                                || m.embeds[0].description.as_ref().map_or(false, |d|
                                    d.contains("```ansi") || d == "\u{3164}"))
                            && m.embeds[0].description.as_ref().map_or(false, |d| !d.is_empty())
                    })
                    .collect();

                //found_messages.sort_by_key(|m| m.timestamp);
                self.message_ids = found_messages.iter().map(|m| m.id).collect();

                if !self.message_ids.is_empty() {
                    info!("Found {} existing dashboard messages", self.message_ids.len());
                }
            }
        }

        // Pad embeds if we have fewer new pages than existing messages
        // This prevents deleting messages when the roster shrinks
        if !self.message_ids.is_empty() && embeds.len() < self.message_ids.len() {
            let needed = self.message_ids.len() - embeds.len();
            for _ in 0..needed {
                embeds.push(CreateEmbed::new().description("\u{3164}"));
            }
        }

        // Send or Edit
        if self.message_ids.len() == embeds.len() {
            // Update existing messages
            for (i, embed) in embeds.into_iter().enumerate() {
                let msg_id = self.message_ids[i];
                let builder = EditMessage::new().embed(embed);
                if let Err(e) = channel_id.edit_message(&ctx.http, msg_id, builder).await {
                    error!("Failed to edit dashboard message: {}", e);
                    self.message_ids.clear(); // Force re-send next time
                    return; // Don't update last_data so we retry
                }
            }
        } else {
            // Count mismatch (new pages added), delete old and send new
            if !self.message_ids.is_empty() {
                info!("Recreating dashboard messages (count increased: {} -> {})", self.message_ids.len(), embeds.len());
                for msg_id in &self.message_ids {
                    let _ = channel_id.delete_message(&ctx.http, *msg_id).await;
                }
                self.message_ids.clear();
            }

            for embed in embeds {
                let builder = CreateMessage::new().embed(embed);
                match channel_id.send_message(&ctx.http, builder).await {
                    Ok(msg) => self.message_ids.push(msg.id),
                    Err(e) => {
                        error!("Failed to send dashboard message: {}", e);
                    }
                }
            }
        }

        // Only update last_data if we successfully performed the update (or at least tried to send new ones)
        if !self.message_ids.is_empty() {
            self.last_data = Some(data.clone());
        }
    }

    pub async fn set_offline(&mut self, ctx: &Context, channel_id: Option<ChannelId>) {
        if let Some(mut data) = self.last_data.clone() {
            if data.online {
                data.online = false;
                self.update(ctx, channel_id, data).await;
            }
        }
    }

    fn format_dashboard(&self, data: &GuildDashboardData) -> Vec<CreateEmbed> {
        let title = format!("{} — {}", data.guild_name, data.realm);
        let group_size = 13;
        let block_size = 5;

        // Prepare data rows: (Name, Level, Area)
        // This unifies the logic for empty and non-empty states, ensuring the empty state
        // uses the exact same padding/height/coloring logic as regular members.
        let all_rows: Vec<(String, String, String)> = if data.members.is_empty() {
            vec![("—".to_string(), "".to_string(), "".to_string())]
        } else {
            data.members.iter().map(|m| (
                m.name.clone(),
                m.level.to_string(),
                get_zone_name(m.zone_id).to_string()
            )).collect()
        };

        // Generate blocks
        // Group into chunks of 13
        let blocks: Vec<String> = all_rows.chunks(group_size).map(|chunk| {
            let lines: Vec<String> = chunk.iter().map(|(name, level, area)| {
                // Name: Truncate to 12, Pad to 13 (using color_pad logic)
                // Level: Truncate to 3, Pad to 3
                // Area: Truncate to 24, Pad to 24

                // Note: The original logic truncates BEFORE padding.
                let name_fmt = color_pad(&truncate(name, 12), 13);
                let level_fmt = pad1(&truncate(level, 3), 3);
                let area_fmt = pad1(&truncate(area, 24), 24);

                format!("{}{}{}", name_fmt, level_fmt, area_fmt)
            }).collect();

            let mut content = lines.join("\n");

            // Pad with empty lines if chunk is smaller than group_size
            // Scala: .padTo(group, "\u3164")
            // This ensures every block is exactly 13 lines high
            let remaining = group_size - chunk.len();
            if remaining > 0 {
                let padding = std::iter::repeat("\u{3164}").take(remaining).collect::<Vec<_>>().join("\n");
                content.push('\n');
                content.push_str(&padding);
            }

            format!("```ansi\n{}\n```", content)
        }).collect();

        let mut embeds = Vec::new();

        // Split blocks into embeds (5 blocks per embed)
        for (i, chunk) in blocks.chunks(block_size).enumerate() {
            let mut embed = CreateEmbed::new();
            let desc_content = chunk.join("");

            if i == 0 {
                embed = embed.title(&title);
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let status_icon = if data.online { ":green_circle:" } else { ":red_circle:" };
                let status_text = if data.online { "online" } else { "were online" };
                let count = data.members.len();

                let mut description = format!("{} {} {}", status_icon, count, status_text);
                // Pad description to 28 chars using pad2 (Hangul Filler)
                description = format!("{}<t:{}:R>", pad2(&description, 28), timestamp);

                // Header line: pad2 separators match Scala exactly
                // Scala: pad2("", 3) + "**Name**" + pad2("", 3) + "**Level**" + pad2("", 3) + "**Area**"
                let header_line = format!("{}**Name**{}**Level**{}**Area**",
                    pad2("", 3), pad2("", 3), pad2("", 3)
                );

                embed = embed.description(format!("{}\n\n{}\n{}", description, header_line, desc_content));
            } else {
                embed = embed.description(desc_content);
            }

            embeds.push(embed);
        }

        embeds
    }
}

// Helpers

fn pad1(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if width > len {
        format!("{}{}", value, "\u{00a0}".repeat(width - len))
    } else {
        value.to_string()
    }
}

fn pad2(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if width > len {
        format!("{}{}", value, "\u{3164}".repeat(width - len))
    } else {
        value.to_string()
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        s.chars().take(max_chars).collect()
    } else {
        s.to_string()
    }
}

fn color_pad(value: &str, width: usize) -> String {
    if value.chars().count() < 2 {
        return pad1(value, width);
    }

    let colors = [
        /*"30",*/ "31", "32", "33", "34", "35", "36", "37",
        /*"40;30",*/ "40;31", "40;32", "40;33", "40;34", "40;35", "40;36", "40;37",
        "41;30", /*"41;31", "41;32", "41;33", "41;34", "41;35", "41;36",*/ "41;37",
        /*"42;30", "42;31", "42;32", "42;33", "42;34", "42;35", "42;36", "42;37",*/
        /*"43;30", "43;31", "43;32", "43;33", "43;34", "43;35", "43;36", "43;37",*/
        /*"44;30", "44;31", "44;32", "44;33", "44;34", "44;35", "44;36", "44;37",*/
        /*"45;30", "45;31", "45;32", "45;33", "45;34", "45;35", "45;36", "45;37",*/
        "46;30", "46;31", /*"46;32", "46;33", "46;34",*/ "46;35", /*"46;36",*/ "46;37",
        "47;30", "47;31", "47;32", "47;33", "47;34", "47;35", "47;36", /*"47;37"*/
    ];

    let first = value.chars().next().unwrap_or_default() as usize;
    let last = value.chars().last().unwrap_or_default() as usize;
    // Scala uses string length (UTF-16 code units), Rust len() is bytes
    // We want character count to match logic better, though Scala logic is actually code units
    let len = value.chars().count();
    let idx = (first + last + len) % colors.len();
    let color = colors[idx];

    let value_len = value.chars().count();
    let padding = if width > value_len {
        "\u{00a0}".repeat(width - value_len)
    } else {
        String::new()
    };

    format!("\u{001b}[{}m{}\u{001b}[0m{}", color, value, padding)
}
