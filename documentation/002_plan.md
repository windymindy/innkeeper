# WoWChat Rust Port - Implementation Checklist

Track progress here. Update status as work proceeds.

## Legend
- [ ] Not started
- [~] In progress
- [x] Complete
- [-] Skipped/Not needed

---

## Phase 1: Core Infrastructure

- [x] Create Cargo.toml with dependencies
- [x] Set up directory structure
- [x] Implement error types (`src/common/error.rs`)
- [x] Set up tracing/logging
- [x] Define opcode constants (`src/protocol/packets/opcodes.rs`)
- [x] Create shared types (Player, GuildMember, ChatMessage, etc.)

## Phase 2: Realm Connection

- [x] TCP connector with Tokio (`src/protocol/realm/connector.rs`)
- [x] Realm packet codec (encode/decode)
- [x] AUTH_LOGON_CHALLENGE handling
- [x] AUTH_LOGON_PROOF handling (Ascension variant)
- [x] Realm list parsing
- [x] Session key extraction and storage
- [x] Reconnection with exponential backoff

## Phase 3: Game Connection

- [x] Game server TCP connector (`src/protocol/game/connector.rs`)
- [x] Game packet codec (WotLK/Ascension variant)
- [x] SMSG_AUTH_CHALLENGE handling
- [x] CMSG_AUTH_SESSION sending
- [x] SMSG_AUTH_RESPONSE handling
- [x] CMSG_CHAR_ENUM / SMSG_CHAR_ENUM
- [x] CMSG_PLAYER_LOGIN / SMSG_LOGIN_VERIFY_WORLD
- [x] CMSG_PING keep-alive loop

## Phase 4: Chat & Guild

- [x] SMSG_MESSAGECHAT parsing (`src/protocol/game/chat.rs`)
- [x] CMSG_MESSAGECHAT sending
- [x] Channel join (CMSG_JOIN_CHANNEL)
- [x] SMSG_CHANNEL_NOTIFY handling
- [x] SMSG_GUILD_ROSTER parsing (`src/protocol/game/guild.rs`)
- [x] SMSG_GUILD_EVENT handling
- [x] SMSG_GUILD_QUERY handling
- [x] CMSG_NAME_QUERY / SMSG_NAME_QUERY for player names

## Phase 5: Discord Integration

- [ ] Choose Discord library (Serenity vs Twilight)
- [x] Bot connection and authentication setup (`src/discord/bot.rs`)
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

- [x] HOCON parser (`src/config/parser.rs`)
- [x] Config types with serde (`src/config/types.rs`)
- [ ] Environment variable overrides (DISCORD_TOKEN, WOW_ACCOUNT, etc.)
- [ ] Config validation

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

- [ ] Unit tests: Header encryption (NOP)
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

### 2026-01-26 (Session 2)
- Phase 4 complete: Chat and Guild message handling implemented.
- **chat.rs**: Added `SMSG_MESSAGECHAT` parsing, `CMSG_MESSAGECHAT` sending, `CMSG_JOIN_CHANNEL`, `SMSG_CHANNEL_NOTIFY`, name query packets.
- **guild.rs**: Added `SMSG_GUILD_ROSTER` parsing, `SMSG_GUILD_EVENT` handling, `SMSG_GUILD_QUERY` parsing.
- Updated `GameHandler` with full chat/guild handling methods and player name cache.
- All packets include proper serialization/deserialization matching Scala reference.

### 2026-01-26
- Phase 3 complete: Game connection, authentication, and character selection implemented.
- Packets: `SMSG_AUTH_CHALLENGE`, `CMSG_AUTH_SESSION` (SHA1+AddonInfo), `SMSG_CHAR_ENUM`, `CMSG_PLAYER_LOGIN`.
- Verified `ADDON_INFO` hex payload matches Scala source (length 685).
- Phase 1 & 2 complete.
- Ascension authentication (X25519/ChaCha20) implemented and verified via `cargo check`.
- Game packet codec implemented (handles standard and large WotLK packets).
- HOCON configuration support verified with `hocon` crate.

### 2026-01-23
- Design document created
- Decided on Ascension-only support
- Full rewrite approach selected
- HOCON config format kept

---

*Last updated: 2026-01-26*
