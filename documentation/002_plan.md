# WoWChat Rust Port - Implementation Checklist

Track progress here. Update status as work proceeds.

## Legend
- [ ] Not started
- [~] In progress
- [x] Complete
- [-] Skipped/Not needed

---

## Phase 1: Core Infrastructure

- [ ] Create Cargo.toml with dependencies
- [ ] Set up directory structure
- [ ] Implement error types (`src/common/error.rs`)
- [ ] Set up tracing/logging
- [ ] Define opcode constants (`src/protocol/packets/opcodes.rs`)
- [ ] Create shared types (Player, GuildMember, ChatMessage, etc.)

## Phase 2: Realm Connection

- [ ] TCP connector with Tokio (`src/protocol/realm/connector.rs`)
- [ ] Realm packet codec (encode/decode)
- [ ] AUTH_LOGON_CHALLENGE handling
- [ ] AUTH_LOGON_PROOF handling (Ascension variant)
- [ ] Realm list parsing
- [ ] Session key extraction and storage
- [ ] Reconnection with exponential backoff

## Phase 3: Game Connection

- [ ] Game server TCP connector (`src/protocol/game/connector.rs`)
- [ ] Game packet codec with encryption
- [ ] SMSG_AUTH_CHALLENGE handling
- [ ] CMSG_AUTH_SESSION sending
- [ ] SMSG_AUTH_RESPONSE handling
- [ ] CMSG_CHAR_ENUM / SMSG_CHAR_ENUM
- [ ] CMSG_PLAYER_LOGIN / SMSG_LOGIN_VERIFY_WORLD
- [ ] CMSG_PING keep-alive loop

## Phase 4: Chat & Guild

- [ ] SMSG_MESSAGECHAT parsing (`src/protocol/game/chat.rs`)
- [ ] CMSG_MESSAGECHAT sending
- [ ] Channel join (CMSG_JOIN_CHANNEL)
- [ ] SMSG_CHANNEL_NOTIFY handling
- [ ] SMSG_GUILD_ROSTER parsing (`src/protocol/game/guild.rs`)
- [ ] SMSG_GUILD_EVENT handling
- [ ] SMSG_GUILD_QUERY handling
- [ ] CMSG_NAME_QUERY / SMSG_NAME_QUERY for player names

## Phase 5: Discord Integration

- [ ] Choose Discord library (Serenity vs Twilight)
- [ ] Bot connection and authentication (`src/discord/bot.rs`)
- [ ] Message received event handler (`src/discord/handler.rs`)
- [ ] Message sending to channels
- [ ] !who command (`src/discord/commands.rs`)
- [ ] !gmotd command
- [ ] Emoji resolution (`src/discord/resolver.rs`)
- [ ] @mention resolution
- [ ] Link transformation (item links, etc.)

## Phase 6: Bridge & Message Routing

- [ ] Channel mapping (WoW channel <-> Discord channel)
- [ ] WoW -> Discord message routing (`src/bridge/router.rs`)
- [ ] Discord -> WoW message routing
- [ ] Message formatting (`src/bridge/formatter.rs`)
- [ ] Regex filtering (`src/bridge/filter.rs`)
- [ ] Dot command passthrough (.commands)

## Phase 9: Configuration

- [ ] TOML parser (`src/config/parser.rs`)
- [ ] Config types with serde (`src/config/types.rs`)
- [ ] Environment variable overrides (DISCORD_TOKEN, WOW_ACCOUNT, etc.)
- [ ] Config validation
- [ ] Migration notes for HOCON -> TOML

## Phase 10: Polish & Release

- [ ] Graceful shutdown handling (SIGINT/SIGTERM)
- [ ] Comprehensive error messages
- [ ] README.md with setup instructions
- [ ] Docker support (optional)
- [ ] GitHub Actions CI/CD
- [ ] Release binaries (Windows, Linux, macOS)
- [ ] Performance testing (RAM, latency)
- [ ] Documentation

---

## Testing Checklist

- [ ] Unit tests: Header encryption
- [ ] Unit tests: Packet serialization
- [ ] Unit tests: Message formatting
- [ ] Integration test: Mock realm server
- [ ] Integration test: Mock game server
- [ ] Manual test: Connect to Ascension
- [ ] Manual test: Chat relay bidirectional
- [ ] Manual test: Reconnection after disconnect

---

## Notes

_Add implementation notes, issues discovered, and decisions made during development here._

### 2026-01-23
- Design document created
- Decided on Ascension-only support
- Full rewrite approach selected
- HOCON config format kept

---

*Last updated: 2026-01-23*
