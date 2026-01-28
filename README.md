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
- ✅ **Dot Commands**: Send WoW commands from Discord (`.help`, `.gm on`, etc.)
- ✅ **Auto-Reconnect**: Graceful handling of disconnections
- ✅ **Item Links**: Converts WoW item links to Ascension database URLs
- ✅ **Low Resource Usage**: Rust efficiency (~10-20MB RAM, minimal CPU)
- ✅ **Zero Dependencies**: Clientless design, no WoW client needed

## Quick Start

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs))
- Discord bot token ([create one](https://discord.com/developers/applications))
- Ascension WoW account

### Installation

```bash
# Clone the repository
git clone https://github.com/anomalyco/innkeeper.git
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
    realm {
        host = "logon.project-ascension.com"
        port = 3724
        name = "Laughing Skull"  # or "Sargeras"
    }

    account {
        username = "your_username"
        password = "your_password"
    }

    character = "YourCharacterName"
}

chat {
    channels = [
        {
            wow = "guild"
            discord = 123456789012345678  # Your Discord channel ID
            direction = "both"
        }
    ]
}
```

3. Get your Discord channel ID:
   - Enable Developer Mode in Discord (User Settings → Advanced → Developer Mode)
   - Right-click your channel → Copy ID

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
export INNKEEPER_DISCORD_TOKEN="your_token_here"
export INNKEEPER_WOW_USERNAME="your_username"
export INNKEEPER_WOW_PASSWORD="your_password"
export INNKEEPER_WOW_CHARACTER="YourCharacter"

./target/release/innkeeper
```

## Configuration Reference

### Discord Settings

```hocon
discord {
    # Required: Your Discord bot token
    token = "YOUR_TOKEN"

    # Optional: Restrict bot to specific guild
    guild_id = 123456789012345678

    # Optional: Enable . prefix commands (default: false)
    enable_dot_commands = true
}
```

### WoW Settings

```hocon
wow {
    realm {
        host = "logon.project-ascension.com"
        port = 3724
        name = "Laughing Skull"  # Exact realm name
    }

    account {
        username = "your_username"
        password = "your_password"
    }

    # Character to log in with
    character = "YourCharacterName"
}
```

### Channel Mappings

```hocon
chat {
    channels = [
        {
            # WoW channel types:
            # - "guild" - Guild chat
            # - "officer" - Officer chat
            # - "say" - Say chat
            # - "yell" - Yell chat
            # - "emote" - Emotes
            # - Any custom channel name
            wow = "guild"

            # Discord channel ID (right-click → Copy ID)
            discord = 123456789012345678

            # Message direction (optional)
            # - "both" (default)
            # - "wow_to_discord" or "w2d"
            # - "discord_to_wow" or "d2w"
            direction = "both"

            # Custom format (optional)
            # Placeholders: %time, %user, %message, %channel
            format = "[%time] %user: %message"
        }
    ]
}
```

### Message Filters

```hocon
filters {
    # Block messages from WoW to Discord
    wow_to_discord = [
        "^\\[System\\].*",           # System messages
        ".*has earned the achievement.*"  # Achievements
    ]

    # Block messages from Discord to WoW
    discord_to_wow = [
        "https?://.*"  # URLs
    ]
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

Innkeeper uses a clean, modular architecture:

```
┌─────────────┐         ┌──────────┐         ┌─────────────┐
│   Discord   │ ←──────→ │  Bridge  │ ←──────→ │  WoW Server │
│     Bot     │  Messages│  Router  │  Packets │   Client    │
└─────────────┘         └──────────┘         └─────────────┘
                             │
                             ↓
                    ┌──────────────────┐
                    │ Filter/Formatter │
                    └──────────────────┘
```

- **Bridge**: Central message router with filtering and formatting
- **GameClient**: Async WoW protocol implementation
- **Discord Bot**: Serenity-based Discord integration
- **Protocol Layer**: WotLK 3.3.5a + Ascension authentication

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

All 52 tests should pass.

### Building from Source

```bash
# Debug build (faster compile, slower runtime)
cargo build

# Release build (optimized)
cargo build --release

# With specific features
cargo build --release --features "some-feature"
```

### Code Structure

```
src/
├── main.rs                 # Application entry point
├── config/                 # Configuration loading and validation
├── protocol/               # WoW protocol implementation
│   ├── realm/             # Realm server (authentication)
│   ├── game/              # Game server (packets, chat, guild)
│   └── packets/           # Packet codec and opcodes
├── game/                   # Game logic
│   ├── client.rs          # Game client main loop
│   ├── bridge.rs          # Message routing orchestrator
│   ├── router.rs          # Channel mapping
│   ├── formatter.rs       # Message formatting
│   └── filter.rs          # Regex filtering
├── discord/                # Discord bot integration
└── common/                 # Shared types and utilities
```

## Troubleshooting

### "Character not found"
- Check that the character name matches exactly (case-sensitive)
- Ensure the character is on the correct realm

### "Realm authentication failed"
- Verify your username and password are correct
- Check that realm host and port are correct
- Ascension may have changed their login server

### "Failed to send message to WoW"
- Message might be too long (max 255 characters in WoW)
- Character might not have permission to speak in that channel
- Check that you've joined the channel (for custom channels)

### Discord bot doesn't respond
- Ensure bot has proper permissions in Discord
- Check that channel IDs are correct (enable Developer Mode)
- Verify the bot token is valid

### Connection keeps dropping
- Ascension may have stricter anti-bot measures
- Check your internet connection stability
- Review logs for specific disconnect reasons

## Performance

Innkeeper is designed for efficiency:

- **Memory**: ~10-20 MB RAM usage
- **CPU**: Minimal (<1% on modern hardware)
- **Network**: Only active traffic, no polling
- **Startup**: < 5 seconds to full connection

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass (`cargo test`)
5. Submit a pull request

## Acknowledgments

- Based on [WoWChat](https://github.com/fjaros/wowchat) by fjaros
- Built for [Ascension](https://project-ascension.com/) community
- Powered by [Serenity](https://github.com/serenity-rs/serenity) Discord library

## Support

- **Issues**: [GitHub Issues](https://github.com/anomalyco/innkeeper/issues)
- **Discord**: [Ascension Discord](https://discord.gg/ascension)

---

**Note**: This is an unofficial tool. Use at your own risk. Automated gameplay tools may violate Ascension's Terms of Service. Innkeeper is designed as a chat bridge only and does not interact with gameplay.
