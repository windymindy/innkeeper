# Innkeeper

A high-performance Discord-WoW chat bridge for Ascension private server, written in Rust.

Innkeeper is a complete rewrite of [WoWChat](https://github.com/fjaros/wowchat) specifically optimized for Ascension's WotLK 3.3.5a protocol with custom authentication.

## Features

- ✅ **Full Ascension Support**: Native support for Ascension's X25519/ChaCha20 authentication
- ✅ **Bidirectional Chat**: WoW ↔ Discord message relay with full Unicode support
- ✅ **Guild Integration**: Guild chat, officer chat, roster, MOTD, and events
- ✅ **Custom Channels**: Support for any WoW custom channels
- ✅ **Discord Commands**: `!who` for online members, `!gmotd` for guild MOTD
- ✅ **Message Filtering**: Regex-based filters for both directions
- ✅ **Auto-Reconnect**: Graceful handling of disconnections
- ✅ **Item Links**: Converts WoW item links to Ascension database URLs
- ✅ **Low Resource Usage**: Rust efficiency (~10-20MB RAM, minimal CPU)
- ✅ **Zero Dependencies**: Clientless design, no WoW client needed

## Quick Start

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs))
- Discord bot token ([create one](https://discord.com/developers/applications))
- Ascension WoW account

### Build from sources

```bash
# Clone the repository
git clone https://github.com/windymindy/innkeeper.git
cd innkeeper

# Build release binary
cargo build --release

# The binary will be in target/release/innkeeper
```

### Configuration

1. Copy the example configuration:
```bash
cp innkeeper.conf.example innkeeper.conf
```

2. Edit `innkeeper.conf` with your settings:
```hocon
discord {
    token = "YOUR_DISCORD_BOT_TOKEN"
    enable_dot_commands = true
}

wow {
    realmlist = "logon.project-ascension.com"
    realm = "Laughing Skull"
    account = "your_username"
    password = "your_password"
    character = "YourCharacterName"
}

chat {
    channels = [
        {
            direction = "both"
            wow {
                type = Guild
            }
            discord {
                channel = "guild-chat"  # Discord channel name or ID
            }
        }
    ]
}
```

3. Get your Discord channel name:
    - The channel name is the text after the # (e.g., "guild-chat" for #guild-chat)
    - Channel names are case-sensitive and must match exactly

### Running

```bash
# Run directly
cargo run --release

# Or use the binary
./target/release/innkeeper

# With custom config location
INNKEEPER_CONFIG=/path/to/config.conf ./target/release/innkeeper
```

### Environment Variables

You can override configuration values with environment variables:

```bash
export DISCORD_TOKEN="your_token_here"
export WOW_ACCOUNT="your_username"
export WOW_PASSWORD="your_password"
export WOW_CHARACTER="YourCharacter"

./target/release/innkeeper
```

## Configuration Reference

### Discord Settings

```hocon
discord {
    # Required: Your Discord bot token
    token = "YOUR_TOKEN"

    # Optional: Enable . prefix commands (default: true)
    enable_dot_commands = true

    # Optional: Whitelist of allowed dot commands (empty = all allowed if enabled)
    dot_commands_whitelist = ["help", "gm"]

    # Optional: Restrict commands to specific channels (empty = all channels)
    # Accepts channel names (strings) or channel IDs (integers)
    enable_commands_channels = ["bot-commands"]

    # Optional: Notify on failed tag/mention resolution (default: true)
    enable_tag_failed_notifications = true

    # Optional: Enable markdown in messages sent from WoW to Discord (default: false)
    enable_markdown = false
}
```

### WoW Settings

```hocon
wow {

    # Treat server's MotD as SYSTEM message (default: true)
    enable_server_motd = true

    # Realm list server (with optional port, default port: 3724)
    realmlist = "logon.project-ascension.com"

    # Realm name to connect to
    realm = "Laughing Skull"

    # Account credentials (or use WOW_ACCOUNT/WOW_PASSWORD env vars)
    account = "your_username"
    password = "your_password"

    # Character to log in with (or use WOW_CHARACTER env var)
    character = "YourCharacterName"
}
```

### Channel Mappings

```hocon
chat {
    channels = [
        {
            # Message direction (optional, default: "both")
            # - "both" (default)
            # - "wow_to_discord"
            # - "discord_to_wow"
            direction = "both"

            # WoW channel configuration
            wow = {
                # Channel type: Guild, Officer, Say, Yell, Emote, System, Channel, Whisper, Whispering
                type = "Guild"

                # Channel name (for custom "Channel" type)
                channel = "Trade"

                # Format string (optional)
                format = "[WoW] %user: %message"
            }

            # Discord channel configuration
            discord = {
                # Discord channel name or ID
                channel = "guild-chat"

                # Format string (optional)
                format = "[Discord] %user: %message"
            }
        }
    ]
}
```

### Message Filters

Global filters apply to all channels. You can also set per-channel filters on each
`wow {}` or `discord {}` block (see the example config for details).

Filter priority order (first non-disabled filter wins):
1. Discord channel filters (highest priority, applies to both directions)
2. WoW channel filters (applies to WoW -> Discord only)
3. Global filters (lowest priority, applies to all channels)

```hocon
filters {
    # Whether filtering is enabled (default: true)
    enabled = true

    # Regex patterns to filter (fancy-regex syntax, supports lookaheads/lookbehinds)
    patterns = [
        "^\\[System\\].*",           # System messages
        ".*has earned the achievement.*",  # Achievements
        "https?://.*"               # URLs
    ]
}
```

### Guild Events (Optional)

```hocon
guild {
    online = {
        enabled = true
        format = "%user has come online"
    }
    offline = {
        enabled = true
        format = "%user has gone offline"
    }
    # Other events: promoted, demoted, joined, left, removed, motd, achievement
}
```

### Guild Dashboard (Optional)

```hocon
guild-dashboard {
    enabled = true
    channel = "guild-dashboard"  # Discord channel name or ID for online member list
}
```

### Quirks (Optional)

```hocon
quirks {
    # Make the bot character sit (default: false)
    sit = false
}
```

## Discord Commands

Available commands (type in Discord):
- `!who` - List online guild members
- `!gmotd` - Show guild Message of the Day
- `!help` - Show help message

Dot commands (if enabled):
- `.help` - Shows WoW help
- `.gm on/off` - Toggle GM mode (if you have permissions)
- Any other WoW command

## Architecture

Innkeeper uses a clean, modular architecture with async message passing:

```
┌─────────────────────────────────────────────────────────────────┐
│                         main.rs                                  │
│  - Load config                                                   │
│  - Spawn Tokio runtime                                          │
│  - Initialize Discord + WoW connections                         │
└─────────────────────────────────────────────────────────────────┘
                               │
            ┌──────────────────┼──────────────────┐
            ▼                  ▼                  ▼
 ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
 │    Discord      │  │     Bridge      │  │   Protocol      │
 │    Bot Task     │  │   (Message      │  │   (WoW Tasks)   │
 │                 │  │   Routing)      │  │                 │
 │ - Event loop    │◄─┤ - Channel maps  │─►│ - Realm connect │
 │ - Commands      │  │ - Filtering     │  │ - Game connect  │
 │ - Send messages │  │ - Formatting    │  │ - Chat relay    │
 └─────────────────┘  └─────────────────┘  └─────────────────┘
```

- **Bridge**: Central message router with filtering, formatting, and channel mapping
- **Protocol Layer**: WotLK 3.3.5a + Ascension authentication
- **Discord Bot**: Serenity-based integration with slash/text commands
- **Message Flow**: Discord ↔ Bridge ↔ WoW with bidirectional filtering

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_auth_flow
```

All tests should pass.

### Building from Source

```bash
# Debug build (faster compile, slower runtime)
cargo build

# Release build (optimized)
cargo build --release

```

### Code Structure

```
src/
├── main.rs                 # Application entry point
├── bridge/                 # Message routing and orchestration
│   ├── orchestrator.rs     # Bridge struct - message flow orchestration
│   ├── channels.rs         # BridgeChannels, DiscordChannels, GameChannels
│   ├── state.rs            # BridgeState, ChannelConfig - shared state
│   ├── filter.rs           # Regex filtering
│   └── mod.rs
├── config/                 # Configuration loading and validation
│   ├── mod.rs
│   ├── parser.rs           # HOCON parsing
│   ├── types.rs            # Config structs
│   ├── validate.rs         # Configuration validation
│   └── env.rs              # Environment variable handling
├── protocol/               # WoW protocol implementation
│   ├── mod.rs
│   ├── realm/             # Realm server (authentication)
│   │   ├── mod.rs
│   │   ├── connector.rs    # Realm server connection
│   │   ├── handler.rs      # Packet handling
│   │   └── packets.rs      # Realm packet definitions
│   ├── game/              # Game server (packets, chat, guild)
│   │   ├── mod.rs
│   │   ├── connector.rs    # Game server connection
│   │   ├── handler.rs      # Packet handling (WotLK/Ascension)
│   │   ├── header.rs       # Header encryption (WotLK/Ascension variant)
│   │   ├── packets.rs      # Game packet definitions
│   │   ├── chat.rs         # Chat message handling
│   │   └── guild.rs        # Guild roster/events
│   └── packets/           # Packet codec and opcodes
│       ├── mod.rs
│       ├── opcodes.rs      # Packet opcode constants
│       └── codec.rs        # Encode/decode traits
├── game/                   # Game client logic
│   ├── mod.rs
│   ├── client.rs          # Game client main loop
│   └── formatter.rs       # Message formatting
├── discord/                # Discord bot integration
│   ├── mod.rs
│   ├── client.rs          # Discord bot setup
│   ├── handler.rs         # Message event handling
│   ├── commands.rs        # Slash/text commands (!who, etc)
│   ├── dashboard.rs       # Guild online member dashboard
│   └── resolver.rs        # Emoji, link, tag resolution
└── common/                 # Shared types and utilities
    ├── mod.rs
    ├── messages.rs         # Message types
    ├── types.rs            # Shared data structures
    └── resources.rs        # Zone names, class names, etc.
```

## Troubleshooting

### "Character not found"
- Check that the character name matches exactly (case-sensitive)
- Ensure the character is on the correct realm

### "Realm authentication failed"
- Verify your username and password are correct
- Check that realm host and port are correct
- Ascension may have changed their login server

### Discord bot doesn't respond
- Ensure bot has proper permissions in Discord
- Check that channel IDs are correct (enable Developer Mode)
- Verify the bot token is valid

## Performance

Innkeeper is designed for efficiency:

- **Memory**: ~10-20 MB RAM usage
- **CPU**: Minimal (<1% on modern hardware)
- **Network**: Only active traffic, no polling
- **Startup**: < 5 seconds to full connection

## Acknowledgments

- Based on [WoWChat](https://github.com/fjaros/wowchat) by fjaros
- Built for [Ascension](https://project-ascension.com/) community
- Powered by [Serenity](https://github.com/serenity-rs/serenity) Discord library

## Support

- **Issues**: [GitHub Issues](https://github.com/windymindy/innkeeper/issues)

---

**Note**: This is an unofficial tool. Use at your own risk. Automated gameplay tools may violate Ascension's Terms of Service. Innkeeper is designed as a chat bridge only and does not interact with gameplay.
