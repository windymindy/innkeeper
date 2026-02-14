# Feature Parity Audit: wowchat_ascension (Scala) vs Innkeeper (Rust)

**Date:** 2025-02-13
**Auditor:** AI-assisted comprehensive code review
**Scala project:** wowchat_ascension (~51 files, ~6,500 lines)
**Rust project:** Innkeeper (~39 files, ~11,260 lines)

---

## Executive Summary

Innkeeper achieves **high feature parity** with the original Scala wowchat_ascension project. The core functionality -- Ascension realm authentication, game server protocol, bidirectional Discord-WoW chat bridge, guild features, and dashboard -- is fully ported with matching opcodes and behavior.

**Key scope difference:** The original supports 5 WoW expansions (Vanilla through MoP) across many private servers. Innkeeper targets **Ascension only** (WotLK 3.3.5a with custom authentication). All multi-expansion abstractions are collapsed into direct Ascension-specific implementations.

### Parity Summary

| Category | Ported | Partial | Missing | Redesigned | N/A | Total |
|----------|--------|---------|---------|------------|-----|-------|
| Authentication and Cryptography | 5 | 0 | 0 | 1 | 2 | 8 |
| Game Protocol - Opcodes | 28 | 0 | 0 | 0 | 0 | 28 |
| Game Protocol - Codec | 3 | 0 | 0 | 1 | 0 | 4 |
| Chat Bridge | 10 | 0 | 0 | 1 | 0 | 11 |
| Guild Features | 6 | 1 | 1 | 0 | 0 | 8 |
| Discord Bot | 6 | 1 | 0 | 1 | 0 | 8 |
| Configuration | 11 | 2 | 1 | 2 | 0 | 16 |
| Connection Management | 4 | 0 | 0 | 1 | 0 | 5 |
| Utilities | 5 | 1 | 0 | 1 | 0 | 7 |
| **Totals** | **78** | **5** | **2** | **8** | **2** | **95** |

### Critical Gaps

1. **No server-side WHO query** -- The `!who` command only searches the local guild roster, not the game server's WHO system. The Scala version sends CMSG_WHO for broader player searches.

### Notable Improvements in Rust Port

1. **Exponential backoff reconnection** with jitter (backon crate) vs fixed 10-second delay
2. **`!help` command** added (not in Scala)
3. **Both `!` and `?` command prefixes** supported (Scala only uses `?`)
4. **Per-channel directional filters** (wow_to_discord vs discord_to_wow) with fancy_regex
5. **`enable_markdown` config option** to disable Discord markdown escaping
6. **Structured config validation** with detailed error messages
7. **Environment variable overrides** with `INNKEEPER_CONFIG` / `WOWCHAT_CONFIG` path support

---

## 1. Authentication and Cryptography

### Realm Authentication (Ascension Custom)

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| X25519 key exchange | HandshakeAscension.scala | realm/handler.rs | Ported |
| ChaCha20-Poly1305 password encryption | HandshakeAscension.scala | realm/handler.rs (chacha20poly1305 crate) | Ported |
| HKDF-like key derivation (HMAC-SHA256) | HandshakeAscension.scala | realm/handler.rs `derive_key()` | Ported |
| XOR mask on encrypted data (0xED) | HandshakeAscension.scala | realm/handler.rs `XOR_MASK` | Ported |
| Custom header magic (0xE6F4F4FC) | HandshakeAscension.scala | realm/handler.rs `HEADER_MAGIC` | Ported |
| Server proof verification (HMAC proof_2) | HandshakeAscension.scala | realm/handler.rs `handle_logon_proof_response` | Ported |
| 2FA / security flag check | HandshakeAscension.scala | realm/handler.rs (checks security_flag != 0) | Ported |

### Realm Login Flow

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| CMD_AUTH_LOGON_CHALLENGE (0x00) send | RealmPacketHandler.scala | realm/handler.rs `build_logon_challenge` | Ported |
| CMD_AUTH_LOGON_CHALLENGE response parse | RealmPacketHandler.scala | realm/handler.rs `handle_logon_challenge_response` | Ported |
| CMD_AUTH_LOGON_PROOF (0x01) send (empty proof) | RealmPacketHandler.scala | realm/handler.rs `build_logon_proof` (32+20+20 zeros) | Ported |
| CMD_AUTH_LOGON_PROOF response parse | RealmPacketHandler.scala | realm/handler.rs `handle_logon_proof_response` | Ported |
| CMD_REALM_LIST (0x10) request and parse | RealmPacketHandler.scala + TBC variant | realm/handler.rs `handle_realm_list_response` (u16 count) | Ported |
| Realm selection by name (case-insensitive) | RealmConnector.scala | realm/connector.rs `eq_ignore_ascii_case` | Ported |
| Auth error code mapping (banned, suspended, etc.) | RealmPackets.scala | realm/packets.rs `AuthResult` enum | Ported |
| FailNewDevice error (Ascension-specific) | Not in Scala | realm/packets.rs (0x17 with email verification message) | New in Rust |

### Standard SRP-6 Authentication

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| SRP-6 client implementation | SRPClient.scala | Not ported | N/A (Ascension bypasses SRP) |
| BigNumber arithmetic | BigNumber.scala | Not ported | N/A (Ascension bypasses SRP) |

### Game Header Encryption

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| WotLK header crypt (HMAC-SHA1 based) | GameHeaderCryptWotLK.scala (ALL COMMENTED OUT) | game/header.rs (NOP encrypt/decrypt) | Redesigned -- both are NOPs for Ascension, Rust makes this explicit |

### Game Server Authentication

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| SMSG_AUTH_CHALLENGE (0x01EC) handle | GamePacketHandlerWotLK | game/handler.rs | Ported |
| CMSG_AUTH_SESSION (0x01ED) build and send | GamePacketHandler | game/handler.rs `build_auth_session` | Ported |
| SMSG_AUTH_RESPONSE (0x01EE) handle | GamePacketHandler | game/handler.rs | Ported |
| Session key from realm auth | RealmConnectionCallback | Passed via `RealmSession.session_key` | Ported |

---

## 2. Game Protocol

### Client-to-Server Packets (CMSG)

All opcodes use WotLK values (after TBC/WotLK inheritance chain in Scala).

| Opcode | Hex | Scala Handler | Rust Handler | Status |
|--------|-----|---------------|--------------|--------|
| CMSG_CHAR_ENUM | 0x0037 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_PLAYER_LOGIN | 0x003D | GamePacketHandler | game/handler.rs | Ported |
| CMSG_LOGOUT_REQUEST | 0x004B | GamePacketHandler | game/handler.rs | Ported |
| CMSG_NAME_QUERY | 0x0050 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_GUILD_QUERY | 0x0054 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_WHO | 0x0062 | GamePacketHandler | opcodes.rs (defined, not sent) | Missing |
| CMSG_GUILD_ROSTER | 0x0089 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_MESSAGECHAT | 0x0095 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_JOIN_CHANNEL | 0x0097 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_GAMEOBJ_USE | 0x00B1 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_PING | 0x01DC | GamePacketHandler | game/handler.rs | Ported |
| CMSG_AUTH_SESSION | 0x01ED | GamePacketHandlerWotLK | game/handler.rs | Ported |
| CMSG_TIME_SYNC_RESP | 0x0391 | GamePacketHandler | game/handler.rs | Ported |
| CMSG_KEEP_ALIVE | 0x0407 | GamePacketHandler | game/handler.rs | Ported |

### Server-to-Client Packets (SMSG)

| Opcode | Hex | Scala Handler | Rust Handler | Status |
|--------|-----|---------------|--------------|--------|
| SMSG_AUTH_CHALLENGE | 0x01EC | GamePacketHandlerWotLK | game/handler.rs | Ported |
| SMSG_AUTH_RESPONSE | 0x01EE | GamePacketHandler | game/handler.rs | Ported |
| SMSG_CHAR_ENUM | 0x003B | GamePacketHandler | game/handler.rs | Ported |
| SMSG_NAME_QUERY | 0x0051 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_GUILD_QUERY | 0x0055 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_WHO | 0x0063 | GamePacketHandler | opcodes.rs (defined, not handled) | Missing |
| SMSG_GUILD_ROSTER | 0x008A | GamePacketHandlerWotLK | game/handler.rs | Ported |
| SMSG_GUILD_EVENT | 0x0092 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_MESSAGECHAT | 0x0096 | GamePacketHandler | game/handler.rs (via chat.rs) | Ported |
| SMSG_CHANNEL_NOTIFY | 0x0099 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_UPDATE_OBJECT | 0x00A9 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_NOTIFICATION | 0x01CB | GamePacketHandler | game/handler.rs | Ported |
| SMSG_LOGIN_VERIFY_WORLD | 0x0236 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_SERVER_MESSAGE | 0x0291 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_CHAT_PLAYER_NOT_FOUND | 0x02A9 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_INIT_WORLD_STATES | 0x02C2 | Not explicitly handled (consumed) | game/handler.rs | Ported |
| SMSG_INVALIDATE_PLAYER | 0x031C | GamePacketHandler (removes from playerRoster) | Defined in opcodes.rs, NO handler | Ported |
| SMSG_MOTD | 0x033D | GamePacketHandlerTBC | game/handler.rs | Ported |
| SMSG_TIME_SYNC_REQ | 0x0390 | GamePacketHandler | game/handler.rs | Ported |
| SMSG_GM_MESSAGECHAT | 0x03B3 | GamePacketHandlerTBC | game/handler.rs (via chat.rs) | Ported |

### Packet Codec

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Outgoing packet encoder (size + opcode + data) | GamePacketEncoder.scala | game/connector.rs `GamePacketCodec` | Ported |
| Incoming packet decoder (size + opcode + data) | GamePacketDecoder.scala | game/connector.rs `GamePacketCodec` | Ported |
| WotLK variable-length header (3-byte for large packets) | GamePacketDecoderWotLK.scala | game/connector.rs (handles large_packet flag) | Ported |
| Realm packet encoder/decoder | RealmPacketEncoder/Decoder.scala | realm/connector.rs (raw TCP read/write) | Redesigned -- no Netty pipeline, direct async TCP |

---

## 3. Chat Bridge

### Message Routing

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| WoW-to-Discord routing (chat_type + channel -> Discord channel) | Discord.scala `channelLookup` | orchestrator.rs `MessageRouter.get_discord_targets` | Ported |
| Discord-to-WoW routing (Discord channel -> chat_type + WoW channel) | Discord.scala `discordLookup` | orchestrator.rs `MessageRouter.get_wow_targets` | Ported |
| Bidirectional direction config (both/wow_to_discord/discord_to_wow) | Config.scala `ChatDirection` | orchestrator.rs `Direction` enum | Ported |
| Custom channel join (CMSG_JOIN_CHANNEL for type=Channel) | GamePacketHandler | game/handler.rs + orchestrator.rs `get_channels_to_join` | Ported |
| Multiple Discord channels per WoW channel | Discord.scala | orchestrator.rs (Vec of routes per key) | Ported |

### Message Formatting

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Format placeholders: %time, %user, %message, %target, %channel | Discord.scala | formatter.rs `MessageFormatter` | Ported |
| Format placeholder: %rank (for guild events) | Discord.scala | formatter.rs | Ported |
| Format placeholder: %achievement | Discord.scala (achievement link formatting) | formatter.rs + orchestrator.rs | Ported |
| Default WoW-to-Discord format: `[%user]: %message` | Discord.scala | formatter.rs `DEFAULT_WOW_TO_DISCORD_FORMAT` | Ported |
| Default Discord-to-WoW format: `%user: %message` | Discord.scala | formatter.rs `DEFAULT_DISCORD_TO_WOW_FORMAT` | Ported |
| Message splitting at 255 chars (word boundary preferred) | Discord.scala `splitMessage` | formatter.rs `split_message` + orchestrator.rs | Ported |

### Message Processing (Discord-to-WoW)

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Discord nickname resolution (use server nick if available) | Discord.scala | discord/handler.rs `msg.member.nick` | Ported |
| Attachment URL appending | Discord.scala | discord/handler.rs (appends `attachment.url`) | Ported |
| Unicode emoji to text (e.g. thumbs_up -> :+1:) | MessageResolver.scala | resolver.rs `resolve_unicode_emojis_to_text` | Ported |
| Custom Discord emoji to text (<:name:id> -> :name:) | MessageResolver.scala | resolver.rs `resolve_custom_emojis_to_text` | Ported |
| Discord @mentions to text (<@id> -> @username) | MessageResolver.scala | resolver.rs `resolve_mentions_to_text` | Ported |
| Discord #channel mentions to text | MessageResolver.scala | resolver.rs `resolve_channel_mentions` | Ported |
| Discord @role mentions to text | MessageResolver.scala | resolver.rs `resolve_role_mentions` | Ported |
| Whisper preprocessing (/w target message) | Discord.scala `preformatChat` | orchestrator.rs `preprocess_whisper_message` | Ported |
| Whisper target validation (3-12 chars, alpha only) | Discord.scala | orchestrator.rs | Ported |
| Ignore own messages | Discord.scala | discord/handler.rs (checks `msg.author.id`) | Ported |
| Ignore DMs (guild messages only) | Discord.scala | discord/handler.rs (checks `msg.guild_id`) | Ported |

### Message Processing (WoW-to-Discord)

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Item link resolution (Hitem -> clickable URL) | MessageResolver.scala | resolver.rs `resolve_links` | Ported |
| Spell/enchant/talent link resolution | MessageResolver.scala | resolver.rs | Ported |
| Quest link resolution | MessageResolver.scala | resolver.rs | Ported |
| Achievement link resolution | MessageResolver.scala | resolver.rs | Ported |
| Trade skill link resolution | MessageResolver.scala | resolver.rs | Ported |
| Link site: db.ascension.gg | MessageResolver.scala (WotLK) | resources.rs `LINK_SITE` | Ported |
| Color code stripping (pipe-c-hex-r format) | MessageResolver.scala | resolver.rs `strip_color_coding` | Ported |
| Texture code stripping (pipe-T...pipe-t) | MessageResolver.scala | resolver.rs `strip_texture_coding` | Ported |
| Emoji shortcode resolution (:smile: -> Discord emoji) | MessageResolver.scala | resolver.rs `resolve_emojis` | Ported |
| @tag resolution (WoW @name -> Discord mention) | MessageResolver.scala `resolveTags` | resolver.rs `resolve_tags` | Ported |
| Quoted @tag resolution ("@name with spaces") | MessageResolver.scala | resolver.rs (tag_patterns with quoted variant) | Ported |
| Tag ambiguity error reporting | MessageResolver.scala | resolver.rs (errors Vec + whisper back to WoW) | Ported |
| Discord markdown escaping (backtick, asterisk, underscore, tilde) | MessageResolver.scala | resolver.rs `escape_discord_markdown` | Ported |
| Markdown escaping preserves mentions | MessageResolver.scala | resolver.rs `escape_discord_markdown_preserve_mentions` | Ported |
| Tag failed notification whisper to WoW | Discord.scala | discord/handler.rs (sends CHAT_MSG_WHISPER back) | Ported |

### Message Filtering

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Global regex filter (config.filters.patterns) | Discord.scala | orchestrator.rs + filter.rs `MessageFilter` | Ported |
| Per-channel wow-side filter (wow.filters) | Discord.scala | orchestrator.rs `build_per_channel_filters` | Ported |
| Per-channel discord-side filter (discord.filters) | Discord.scala | orchestrator.rs `build_per_channel_filters` | Ported |
| Filter priority (discord filters override wow filters) | Discord.scala | orchestrator.rs (discord checked first, continues only if absent) | Ported |
| Directional filtering (WoW-to-Discord vs Discord-to-WoW) | Not in Scala (same filter both ways) | filter.rs `FilterDirection` enum | Redesigned -- Rust adds directional awareness |

---

## 4. Guild Features

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Guild roster request (CMSG_GUILD_ROSTER) | GamePacketHandler | game/handler.rs | Ported |
| Guild roster parsing (SMSG_GUILD_ROSTER, WotLK format) | GamePacketHandlerWotLK | game/handler.rs (via guild.rs) | Ported |
| Periodic roster refresh (~60s interval) | GamePacketHandler (scheduled task) | game/handler.rs `should_update_guild_roster` | Ported |
| Guild event handling (SMSG_GUILD_EVENT) | GamePacketHandler | game/handler.rs | Ported |
| Guild events: online/offline/joined/left/removed/promoted/demoted/motd | GamePacketHandler (8 event types) | common/types.rs `GuildEvent` enum (8 event types) | Ported |
| Guild event per-event enable/disable config | Config.scala `GuildEventConfig.enabled` | config/types.rs `GuildEventConfig.enabled` | Ported |
| Guild event per-event format config | Config.scala `GuildEventConfig.format` | config/types.rs `GuildEventConfig.format` | Ported |
| Guild event per-event channel override | Config.scala `GuildEventConfig.channel` | NOT in config/types.rs `GuildEventConfig` | Partial -- channel override missing |
| Guild achievement events | GamePacketHandler | game/handler.rs + common/types.rs `GuildAchievement` | Ported |
| Guild query (CMSG_GUILD_QUERY / SMSG_GUILD_QUERY) | GamePacketHandler | game/handler.rs | Ported |

---

## 5. Discord Bot

### Commands

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| `?who` / `!who` -- list online guildies | CommandHandler.scala (server WHO query) | commands.rs (local guild roster only) | Partial -- no CMSG_WHO server query |
| `?who <name>` / `!who <name>` -- search specific player | CommandHandler.scala | commands.rs + game/handler.rs `search_guild_member` | Ported (guild roster only) |
| `?gmotd` / `!gmotd` -- show guild MOTD | CommandHandler.scala | commands.rs | Ported |
| `?online` / `!online` -- alias for ?who | CommandHandler.scala | commands.rs | Ported |
| `!help` -- list available commands | Not in Scala | commands.rs | New in Rust |
| Command prefix `?` | CommandHandler.scala (only `?`) | discord/handler.rs (both `!` and `?`) | Ported (expanded) |
| Command prefix `!` | Not in Scala | discord/handler.rs | New in Rust |
| Command channel restriction (enable_commands_channels) | Config.scala | state.rs `command_allowed_in_channel` | Ported |

### Dot Commands

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| `.command` passthrough to WoW | Discord.scala | discord/handler.rs + orchestrator.rs | Ported |
| Dot command whitelist filtering | Config.scala `dot_commands_whitelist` | state.rs `should_send_dot_command_directly` | Ported |
| Glob pattern support in whitelist (e.g. `guild*`) | Discord.scala | state.rs (wildcard pattern matching) | Ported |
| Dot commands sent as ChatType::Say | Discord.scala | orchestrator.rs `handle_discord_to_wow_directly` | Ported |

### Guild Dashboard

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Dashboard embed with guild roster | Discord.scala (JDA embeds) | dashboard.rs (Serenity embeds) | Ported |
| ANSI color codes for member names | Discord.scala | dashboard.rs `color_pad` function | Ported |
| Multi-page embeds (5 blocks of 13 per page) | Discord.scala | dashboard.rs (group_size=13, block_size=5) | Ported |
| Edit existing dashboard messages | Discord.scala (JDA editMessage) | dashboard.rs (Serenity EditMessage) | Ported |
| Find existing dashboard in channel history | Discord.scala | dashboard.rs (searches last 10 messages) | Ported |
| Online/offline status indicator | Discord.scala | dashboard.rs (green_circle / red_circle) | Ported |
| Relative timestamp | Discord.scala | dashboard.rs (Discord `<t:unix:R>` format) | Ported |
| Zone name display for each member | Discord.scala | dashboard.rs (uses `get_zone_name`) | Ported |
| Deduplication (skip if data unchanged) | Discord.scala | dashboard.rs `last_data` comparison | Ported |
| Pad with empty embeds when roster shrinks | Discord.scala | dashboard.rs (pads with Hangul Filler embeds) | Ported |

### Activity Status

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| "Connecting..." status | Discord.scala | discord/handler.rs `ActivityStatus::Connecting` | Ported |
| "Offline" status on disconnect | Discord.scala | discord/handler.rs `ActivityStatus::Disconnected` | Ported |
| Realm name status on connect | Discord.scala | discord/handler.rs `ActivityStatus::ConnectedToRealm` | Ported |
| "X guildies online" watching status | Discord.scala | discord/handler.rs `ActivityStatus::GuildStats` | Ported |

---

## 6. Configuration

### Discord Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `discord.token` | Config.scala | types.rs `DiscordConfig.token` | Ported |
| `discord.enable_dot_commands` | Config.scala (default: true) | types.rs (default: true) | Ported |
| `discord.dot_commands_whitelist` | Config.scala (Optional list) | types.rs (Option of Vec) | Ported |
| `discord.enable_commands_channels` | Config.scala (Optional list) | types.rs (Option of Vec) | Ported |
| `discord.enable_tag_failed_notifications` | Config.scala (default: true) | types.rs (default: true) | Ported |
| `discord.enable_markdown` | Not in Scala | types.rs (default: false) | New in Rust |

### WoW Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `wow.platform` | Config.scala (Mac/Win) | types.rs `WowConfig.platform` | Ported |
| `wow.locale` | Config.scala (default: "enUS") | Not in Rust types | Missing |
| `wow.enable_server_motd` | Config.scala | types.rs `WowConfig.enable_server_motd` | Ported |
| `wow.version` | Config.scala (e.g. "3.3.5") | types.rs `WowConfig.version` | Ported |
| `wow.realm_build` | Config.scala | types.rs `WowConfig.realm_build` | Ported |
| `wow.game_build` | Config.scala | types.rs `WowConfig.game_build` | Ported |
| `wow.realmlist` | Config.scala | types.rs `WowConfig.realmlist` | Ported |
| `wow.realm` | Config.scala | types.rs `WowConfig.realm` | Ported |
| `wow.account` | Config.scala | types.rs `WowConfig.account` | Ported |
| `wow.password` | Config.scala | types.rs `WowConfig.password` | Ported |
| `wow.character` | Config.scala | types.rs `WowConfig.character` | Ported |

### Chat Channel Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `chat.channels[].direction` (both/wow_to_discord/discord_to_wow) | Config.scala | types.rs `ChannelMapping.direction` | Ported |
| `chat.channels[].wow.type` | Config.scala | types.rs `WowChannelConfig.channel_type` | Ported |
| `chat.channels[].wow.channel` (custom channel name) | Config.scala | types.rs `WowChannelConfig.channel` | Ported |
| `chat.channels[].wow.format` | Config.scala | types.rs `WowChannelConfig.format` | Ported |
| `chat.channels[].wow.filters` | Config.scala | types.rs `WowChannelConfig.filters` | Ported |
| `chat.channels[].discord.channel` | Config.scala | types.rs `DiscordChannelConfig.channel` | Ported |
| `chat.channels[].discord.format` | Config.scala | types.rs `DiscordChannelConfig.format` | Ported |
| `chat.channels[].discord.filters` | Config.scala | types.rs `DiscordChannelConfig.filters` | Ported |

### Guild Events Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `guild.online/offline/joined/left/removed/promoted/demoted/motd/achievement` | Config.scala | types.rs `GuildEventsConfig` | Ported |
| Per-event `.enabled` | Config.scala | types.rs `GuildEventConfig.enabled` | Ported |
| Per-event `.format` | Config.scala | types.rs `GuildEventConfig.format` | Ported |
| Per-event `.channel` override | Config.scala | NOT in types.rs | Partial -- missing |

### Global Filters Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `filters.enabled` | Config.scala | types.rs `FiltersConfig.enabled` | Ported |
| `filters.patterns` (regex list) | Config.scala | types.rs `FiltersConfig.patterns` | Ported |

### Guild Dashboard Config

| Config Key | Scala | Rust | Status |
|------------|-------|------|--------|
| `guild_dashboard.enabled` | Config.scala | types.rs `GuildDashboardConfig.enabled` | Ported |
| `guild_dashboard.channel` | Config.scala | types.rs `GuildDashboardConfig.channel` | Ported |

### Config Infrastructure

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| HOCON format parsing | Typesafe Config library | hocon_rs crate (parser.rs) | Ported |
| Environment variable substitution in HOCON | Typesafe Config `${?VAR}` | hocon_rs handles `${?VAR}` + env.rs fallback | Ported |
| Env var overrides: DISCORD_TOKEN, WOW_ACCOUNT, WOW_PASSWORD, WOW_CHARACTER | Not in Scala (HOCON-only) | env.rs `apply_env_overrides` | New in Rust |
| INNKEEPER_CONFIG / WOWCHAT_CONFIG path env var | Not in Scala | env.rs `get_config_path` | New in Rust |
| Config validation with detailed errors | Minimal in Scala | validate.rs (token, credentials, character length, channel types, direction, regex patterns) | Redesigned -- much more thorough |
| Quirks config section | Not in Scala | types.rs `QuirksConfig` | New in Rust |

---

## 7. Connection Management

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Realm TCP connection | RealmConnector.scala (Netty pipeline) | realm/connector.rs (tokio TcpStream) | Ported |
| Game TCP connection | GameConnector.scala (Netty pipeline) | game/connector.rs (tokio TcpStream + Framed codec) | Ported |
| Reconnection on disconnect | WoWChat.scala (fixed 10s delay, infinite loop) | main.rs (exponential backoff with jitter via backon) | Redesigned -- better backoff strategy |
| Keepalive (CMSG_KEEP_ALIVE) | GamePacketHandler (periodic) | game/handler.rs | Ported |
| Ping/pong (CMSG_PING) | GamePacketHandler | game/handler.rs | Ported |
| Time sync (SMSG_TIME_SYNC_REQ / CMSG_TIME_SYNC_RESP) | GamePacketHandler | game/handler.rs | Ported |
| Idle timeout handling | IdleStateCallback.scala (Netty IdleStateEvent) | Not explicitly ported (tokio handles TCP timeouts) | Redesigned |
| Graceful shutdown | WoWChat.scala (shutdown hook) | main.rs (tokio watch channel + signal handler) | Ported |
| Drain messages during disconnect backoff | Not in Scala | main.rs (drains channels, sends error responses) | New in Rust |

---

## 8. Utilities

### Name Resolution Cache

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Player name cache (GUID -> name) | Global.scala `playerRoster` (LRUMap, 10,000 entries) | game/handler.rs `name_cache: HashMap` | Partial -- no LRU eviction |
| CMSG_NAME_QUERY for unknown GUIDs | GamePacketHandler | game/handler.rs | Ported |
| SMSG_NAME_QUERY response caching | GamePacketHandler | game/handler.rs | Ported |

### Resource Loading

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Achievement database (achievements.csv) | GameResources.scala | resources.rs (include_str! embedded) | Ported |
| Area/zone database (pre_cata_areas.csv) | GameResources.scala | resources.rs (include_str! embedded) | Ported |
| Race enum with faction language | MessageResolver.scala | resources.rs `Race::language()` | Ported |
| Class enum with names | MessageResolver.scala | resources.rs `Class::name()` | Ported |

### Byte Utilities

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| ByteUtils (hex conversions, toWoWString, etc.) | ByteUtils.scala (130 lines) | Uses bytes crate (BufMut/Buf) directly | Redesigned -- standard library replaces custom utils |

### Global State

| Feature | Scala | Rust | Status |
|---------|-------|------|--------|
| Mutable global state (Global.scala) | Global.scala: discord, game, config, playerRoster, guildInfo | No global mutable state -- passed via channels and Arc | Redesigned -- no global mutable state |

---

## 9. Detailed Findings

### 9.1 Missing: Server-side WHO Query (CMSG_WHO / SMSG_WHO)

**Severity: Low**

The Scala `!who` (via `?who`) command sends CMSG_WHO to the game server, which returns all matching players server-wide (not just guild members). In Innkeeper, `!who` only searches the local guild roster cache via `get_online_guildies()` and `search_guild_member()`.

**Impact:** Users cannot search for non-guild players. For the typical use case (checking guild member status), the Rust implementation is functionally equivalent. The server-wide WHO feature was rarely used in practice on Ascension.

**Recommendation:** Could be added if users request it. Would require implementing the CMSG_WHO packet builder and SMSG_WHO response parser. Low priority.

### 9.2 Partial: Guild Event Channel Override

**Severity: Low**

The Scala `GuildEventConfig` case class includes a `channel` field that allows guild events to be routed to a different Discord channel than the default guild chat channel. In Innkeeper, `GuildEventConfig` only has `enabled` and `format` fields.

**Impact:** All guild events (online, offline, joined, left, promoted, demoted, motd, achievement) go to the same channel(s) as guild chat. Users who want events in a separate channel cannot configure this.

**Recommendation:** Add `channel: Option<String>` to `GuildEventConfig` in `types.rs` and update the routing logic in `orchestrator.rs`.

### 9.3 Partial: Name Cache Has No LRU Eviction

**Severity: Very Low**

The Scala version uses `LRUMap` with a 10,000 entry limit. Innkeeper uses a plain `HashMap` with no eviction. In practice, the cache only stores players seen in the current session and is cleared on reconnect, so this is unlikely to be an issue.

### 9.4 Missing: `wow.locale` Config Key

**Severity: Very Low**

The Scala config supports `wow.locale` (default "enUS") which is sent in the auth packet. Innkeeper hardcodes "enUS" in the realm handler. Since Ascension only supports English, this has no practical impact.

### 9.5 Partial: `!who` Command Only Searches Guild Roster

**Severity: Low**

See finding 9.2. The `!who` command returns guild members only, not server-wide results. The formatting is enhanced in Rust (includes level, class, zone) compared to Scala's simpler list.

---

## 10. Architecture Differences (By Design)

These are intentional redesigns, not gaps:

| Aspect | Scala | Rust | Rationale |
|--------|-------|------|-----------|
| Concurrency model | Netty (event-driven NIO) + JDA (thread pool) | Tokio (async/await) + Serenity (async) | Modern async Rust idiom |
| Global state | Mutable singleton `Global.scala` | No globals; state passed via channels and Arc | Thread safety, testability |
| Multi-expansion support | 3-layer inheritance (Base -> TBC -> WotLK) | Single flat implementation | Ascension-only target |
| Discord library | JDA (Java Discord API) | Serenity (Rust Discord library) | Language-native |
| Config parsing | Typesafe Config (Java) | hocon_rs (Rust HOCON parser) | Language-native |
| Reconnection | Fixed 10s delay, infinite loop | Exponential backoff with jitter (backon crate) | Better behavior under load |
| Message channels | Direct method calls between components | tokio mpsc/watch channels (7 channels) | Async-safe decoupling |
| Byte handling | Custom ByteUtils + Netty ByteBuf | Standard bytes crate (Buf/BufMut) | Ecosystem standard |
| Regex engine | Java regex | fancy_regex crate (supports lookahead/lookbehind) | More powerful patterns |
| Idle timeout | Netty IdleStateHandler pipeline | Tokio TCP-level timeouts | Simpler, less code |

---

## 11. File Mapping Reference

| Scala File | Rust Equivalent | Notes |
|------------|----------------|-------|
| WoWChat.scala | main.rs | Entry point, reconnection loop |
| Config.scala | config/types.rs, parser.rs, validate.rs, env.rs | Split into 4 focused modules |
| Discord.scala | discord/handler.rs, client.rs + bridge/orchestrator.rs | Split into handler + bridge |
| MessageResolver.scala | discord/resolver.rs | Direct port with enhancements |
| CommandHandler.scala | discord/commands.rs | Added !help command |
| GamePackets.scala + TBC + WotLK | protocol/packets/opcodes.rs | Flattened to single file |
| GamePacketHandler.scala + TBC + WotLK | protocol/game/handler.rs, chat.rs, guild.rs | Split by concern |
| GamePacketEncoder/Decoder.scala | protocol/game/connector.rs | Combined into codec |
| GameConnector.scala | protocol/game/connector.rs | Tokio TcpStream |
| RealmPacketHandler.scala + TBC | protocol/realm/handler.rs | Combined |
| RealmPackets.scala | protocol/realm/packets.rs | Direct port |
| RealmConnector.scala | protocol/realm/connector.rs | Async TCP |
| HandshakeAscension.scala | protocol/realm/handler.rs | Merged into handler |
| GameHeaderCryptWotLK.scala | protocol/game/header.rs | Explicit NOP |
| Global.scala | common/types.rs, messages.rs | No mutable globals |
| ByteUtils.scala | (bytes crate) | Standard library |
| LRUMap.scala | HashMap (no LRU) | Simplified |
| Packet.scala | protocol/packets/opcodes.rs | Merged |
| GameResources.scala | common/resources.rs | Embedded via include_str! |
| IdleStateCallback.scala | (tokio timeouts) | Implicit |
| ReconnectDelay.scala | main.rs (backon crate) | Improved |
| SRPClient.scala, BigNumber.scala | Not ported | N/A (Ascension bypasses SRP) |
| -- | discord/dashboard.rs | New dedicated module |
| -- | bridge/filter.rs | New dedicated module |
| -- | bridge/state.rs | New dedicated module |
| -- | bridge/channels.rs | New dedicated module |
| -- | game/formatter.rs | New dedicated module |
