# Innkeeper Design

**Date:** 2026-01-23  
**Status:** Active Development
**Goal:** Full rewrite from Scala/JVM to Rust/Tokio for reduced RAM usage and elimination of JVM dependency

---

## 1. Project Overview

### 1.1 Current State (Scala)
- **Language:** Scala 2.12 on JVM 21
- **Codebase:** ~6,500 lines across 52 files
- **Dependencies:**
  - Netty 4.2.3 (async networking)
  - JDA 5.6.1 (Discord)
  - Typesafe Config (HOCON)
  - Bouncy Castle (cryptography)
- **RAM Usage:** 150-300MB typical (JVM overhead)
- **Supports:** 5 WoW expansions (Vanilla, TBC, WotLK, Cataclysm, MoP)

### 1.2 Target State (Rust)
- **Language:** Rust (stable, latest edition)
- **Runtime:** Tokio async runtime
- **Target RAM:** 10-30MB
- **Target binary:** Single static executable, no runtime dependencies
- **Supports:** Ascension only (WotLK-based, 3.3.5a protocol with custom modifications)

---

## 2. Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Expansion Support** | Ascension only (WotLK-based) | Reduces complexity by ~40%, focuses on actual use case |
| **Migration Strategy** | Full rewrite | Clean Rust architecture, not constrained by Scala patterns, separate repository |
| **Async Runtime** | Tokio | Industry standard, excellent ecosystem |
| **Discord Library** | Serenity 0.12 | Mature, well-documented, good async support |
| **Config Format** | HOCON | Keep configs compatible |
| **Networking** | tokio + bytes | Zero-copy parsing, efficient buffer management |

---

## 3. Architecture

### 3.1 Directory Structure

```
innkeeper/
├── Cargo.toml
├── src/
│   ├── main.rs                 # Application entry point
│   │
│   ├── bridge/                 # Message routing and orchestration
│   │   ├── orchestrator.rs     # Bridge struct - message flow orchestration
│   │   ├── channels.rs         # BridgeChannels, DiscordChannels, GameChannels
│   │   ├── state.rs            # BridgeState, ChannelConfig - shared state
│   │   ├── filter.rs           # Regex filtering
│   │   └── mod.rs
│   │
│   ├── config/                 # Configuration loading and validation
│   │   ├── mod.rs
│   │   ├── parser.rs           # HOCON parsing
│   │   ├── types.rs            # Config structs
│   │   ├── validate.rs         # Configuration validation
│   │   └── env.rs              # Environment variable handling
│   │
│   ├── protocol/               # WoW protocol implementation
│   │   ├── mod.rs
│   │   ├── realm/             # Realm server (authentication)
│   │   │   ├── mod.rs
│   │   │   ├── connector.rs    # Realm server connection
│   │   │   ├── handler.rs      # Packet handling
│   │   │   └── packets.rs      # Realm packet definitions
│   │   │
│   │   ├── game/              # Game server (packets, chat, guild)
│   │   │   ├── mod.rs
│   │   │   ├── connector.rs    # Game server connection
│   │   │   ├── handler.rs      # Packet handling (WotLK/Ascension)
│   │   │   ├── header.rs       # Header encryption (WotLK/Ascension variant)
│   │   │   ├── packets.rs      # Game packet definitions
│   │   │   ├── chat.rs         # Chat message handling
│   │   │   └── guild.rs        # Guild roster/events
│   │   │
│   │   └── packets/           # Packet codec and opcodes
│   │       ├── mod.rs
│   │       ├── opcodes.rs      # Packet opcode constants
│   │       └── codec.rs        # Encode/decode traits
│   │
│   ├── game/                   # Game client logic
│   │   ├── mod.rs
│   │   ├── client.rs          # Game client main loop
│   │   └── formatter.rs       # Message formatting
│   │
│   ├── discord/                # Discord bot integration
│   │   ├── mod.rs
│   │   ├── client.rs          # Discord bot setup
│   │   ├── handler.rs         # Message event handling
│   │   ├── commands.rs        # Slash/text commands (!who, etc)
│   │   ├── dashboard.rs       # Guild online member dashboard
│   │   └── resolver.rs        # Emoji, link, tag resolution
│   │
│   └── common/                 # Shared types and utilities
│       ├── mod.rs
│       ├── messages.rs         # Message types
│       ├── types.rs            # Shared data structures
│       └── resources.rs        # Zone names, class names, etc.
```

### 3.2 Component Diagram

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

### 3.3 Message Flow

```
Discord User types message
          │
          ▼
┌─────────────────┐     ┌─────────────────┐
│ Discord Handler │ ──► │     Bridge      │ ── filters, formats ──►┌─────────────┐
└─────────────────┘     │   (mpsc tx)     │                        │   Game      │
                        │ - Channel maps  │                        │  (mpsc tx)  │
                        │ - Filtering     │                        └──────┬──────┘
                        └──────┬─────────┘                               │
                               │                                         ▼
                               ▼                                  ┌─────────────┐
                        ┌─────────────┐                          │ Game Handler│
                        │ Bridge Cmd  │                          │ (mpsc rx)   │
                        │ Processor   │                          └──────┬──────┘
                        └─────────────┘                               │
                                                                     ▼
                                                              ┌─────────────┐
                                                              │  WoW Server │
                                                              └─────────────┘

(Reverse flow for WoW → Discord: WoW Server → Game Handler → Bridge → Discord Handler)
```

---

## 4. Technical Details

### 4.1 Dependencies

Dependencies are defined in `Cargo.toml`. Key dependencies include:
- **tokio** - Async runtime
- **bytes** - Zero-copy buffer management
- **serenity** - Discord library
- **serde** - Configuration serialization
- **anyhow** - Error handling
- **tracing** - Logging
- **fancy-regex** - Message filtering (supports lookaheads/lookbehinds)

### 4.2 Cryptography Implementation

#### Realm protocol
Ascension version does not use `SRPClient.scala`
Port from `HandshakeAscension.scala`.

#### Header Encryption (protocol/game/header.rs)
Ascension version does not use HMAC-SHA1 based header encryption.
Port from game packet encoder and decoder.

### 4.3 Ascension-Specific Protocol

Based on `HandshakeAscension.scala`, Ascension has custom:
- Modified realm handshake (different packet structure)
- Custom authentication flow
- Same WotLK game protocol otherwise
- Warden is not required.

Key file to reference: `src/main/scala/wowchat/realm/HandshakeAscension.scala`

---

## 5. Migration Notes

### 5.1 Dropped Features

Since we're targeting Ascension only:
- Vanilla, TBC, Original WoTLK Cataclysm, MoP packet handlers (removed)
- Version-specific branching (simplified)

### 5.2 Key Files to Reference

| Rust Module | Scala Source |
|-------------|--------------|
| `protocol/realm/handler.rs` | `realm/RealmPacketHandler.scala`, `realm/HandshakeAscension.scala` |
| `protocol/game/handler.rs` | `game/GamePacketHandlerWotLK.scala` |
| `protocol/game/header.rs` | `game/GameHeaderCryptWotLK.scala` |
| `discord/client.rs` | `discord/Discord.scala` |
| `config/parser.rs` | `common/Config.scala` |

---

## 6. Testing Strategy

### Unit Tests
- Cryptography functions (known test vectors)
- Packet serialization/deserialization
- Message formatting

### Integration Tests
- Mock WoW server for protocol testing
- Discord API mocking

### Manual Testing
- Connect to actual Ascension server
- Verify chat relay works bidirectionally
- Test reconnection scenarios

---

## 7. Success Criteria

| Metric | Target |
|--------|--------|
| RAM usage | < 30MB |
| Binary size | < 15MB |
| Startup time | < 1 second |
| Message latency | < 100ms (network permitting) |
| Feature parity | All relevant features from original |

---

## 8. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Ascension protocol changes | Document protocol thoroughly, keep Scala version as reference |
| Discord library instability | Abstract Discord behind trait, allow swapping implementations |

---

*Document created: 2026-01-23*  
*Last updated: 2026-02-02*
