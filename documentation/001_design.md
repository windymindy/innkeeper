# Innkeeper Design

**Date:** 2026-01-23  
**Status:** Approved  
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
| **Discord Library** | TBD (Serenity or Twilight) | Research during implementation |
| **Config Format** | HOCON | Keep configs compatible |
| **Networking** | tokio + bytes | Zero-copy parsing, efficient buffer management |

---

## 3. Architecture

### 3.1 Directory Structure

```
innkeeper/
├── Cargo.toml
├── src/
│   ├── main.rs                 # Entry point, runtime setup
│   │
│   ├── config/
│   │   ├── mod.rs
│   │   ├── parser.rs           # HOCON parsing
│   │   └── types.rs            # Config structs
│   │
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── realm/
│   │   │   ├── mod.rs
│   │   │   ├── connector.rs    # Realm server connection
│   │   │   ├── handler.rs      # Packet handling
│   │   │   └── packets.rs      # Realm packet definitions
│   │   │
│   │   ├── game/
│   │   │   ├── mod.rs
│   │   │   ├── connector.rs    # Game server connection
│   │   │   ├── handler.rs      # Packet handling (WotLK/Ascension)
│   │   │   ├── header.rs       # Header encryption (WotLK/Ascension variant)
│   │   │   ├── packets.rs      # Game packet definitions
│   │   │   ├── chat.rs         # Chat message handling
│   │   │   └── guild.rs        # Guild roster/events
│   │   │
│   │   │
│   │   └── packets/
│   │       ├── mod.rs
│   │       ├── opcodes.rs      # Packet opcode constants
│   │       └── codec.rs        # Encode/decode traits
│   │
│   ├── discord/
│   │   ├── mod.rs
│   │   ├── bot.rs              # Discord bot setup
│   │   ├── handler.rs          # Message event handling
│   │   ├── commands.rs         # Slash/text commands (!who, etc)
│   │   └── resolver.rs         # Emoji, link, tag resolution
│   │
│   ├── game/
│   │   ├── mod.rs
│   │   ├── router.rs           # Message routing logic
│   │   ├── formatter.rs        # Message formatting
│   │   └── filter.rs           # Message filtering (regex)
│   │
│   └── common/
│       ├── mod.rs
│       ├── error.rs            # Error types (thiserror)
│       ├── reconnect.rs        # Exponential backoff
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
│    Discord      │  │     Game        │  │   Protocol      │
│    Bot Task     │  │   (Channels)    │  │   (WoW Tasks)   │
├─────────────────┤  ├─────────────────┤  ├─────────────────┤
│ - Event loop    │  │ - wow_tx/rx     │  │ - Realm connect │
│ - Commands      │◄─┤ - discord_tx/rx │─►│ - Game connect  │
│ - Send messages │  │ - Routing       │  │ - Chat relay    │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

### 3.3 Message Flow

```
Discord User types message
         │
         ▼
┌─────────────────┐
│ Discord Handler │ ── filters, formats ──►┌─────────────┐
└─────────────────┘                        │   Game      │
                                           │  (mpsc tx)  │
                                           └──────┬──────┘
                                                  │
                                                  ▼
                                           ┌─────────────┐
                                           │ Game Handler│
                                           │ (mpsc rx)   │
                                           └──────┬──────┘
                                                  │
                                                  ▼
                                           ┌─────────────┐
                                           │  WoW Server │
                                           └─────────────┘

(Reverse flow for WoW → Discord)
```

---

## 4. Technical Details

### 4.1 Dependencies (Cargo.toml)

```toml
[package]
name = "wowchat-rs"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# Networking & serialization
bytes = "1"
tokio-util = { version = "0.7", features = ["codec"] }

# Cryptography
sha1 = "0.10"

# Discord (choose one during implementation)
serenity = { version = "0.12", features = ["client", "gateway"] }
# OR: twilight-gateway, twilight-http

# Configuration
serde = { version = "1", features = ["derive"] }

# Utilities
thiserror = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
regex = "1"

# Optional: emoji handling
emojis = "0.6"
```

### 4.2 Cryptography Implementation

#### Realm protocol
Ascension version does not use `SRPClient.scala`
Port from `HandshakeAscension.scala`.

#### Header Encryption (crypto/header.rs)
Ascension version does not use HMAC-SHA1 based header encryption.
Port from game packet encoder and decoder.

### 4.3 Packet Codec

```rust
// Example packet structure
pub struct Packet {
    pub opcode: u16,
    pub payload: Bytes,
}

// Decoder for incoming packets
impl Decoder for GamePacketCodec {
    type Item = Packet;
    type Error = ProtocolError;
    
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 1. Check minimum header size
        // 2. Decrypt header if encryption enabled
        // 3. Read size + opcode
        // 4. Wait for full payload
        // 5. Return packet
    }
}
```

### 4.4 Ascension-Specific Protocol

Based on `HandshakeAscension.scala`, Ascension has custom:
- Modified realm handshake (different packet structure)
- Custom authentication flow
- Same WotLK game protocol otherwise
- Warden is not required.

Key file to reference: `src/main/scala/wowchat/realm/HandshakeAscension.scala`

---

## 5. Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Project setup (Cargo.toml, directory structure)
- [ ] Configuration parser (HOCON)
- [ ] Error types and logging setup
- [ ] Basic types (opcodes, constants)

### Phase 2: Realm Connection
- [ ] TCP connection with Tokio
- [ ] Realm packet codec
- [ ] Authentication handshake (ChaCha20Poly1305 and HmacSHA256 Ascension variant)
- [ ] Realm list parsing
- [ ] Session key extraction

### Phase 3: Game Connection
- [ ] Game server connection
- [ ] Header and packet encoder and decoder without SRP-6a RC4 encryption
- [ ] Auth challenge/response
- [ ] Character enumeration
- [ ] World login
- [ ] Keep-alive / ping handling

### Phase 4: Chat & Guild
- [ ] Chat message parsing (SMSG_MESSAGECHAT)
- [ ] Chat message sending (CMSG_MESSAGECHAT)
- [ ] Guild roster (SMSG_GUILD_ROSTER)
- [ ] Guild events (online/offline/motd)
- [ ] Channel join/leave

### Phase 5: Discord Integration
- [ ] Bot connection and event loop
- [ ] Message receiving
- [ ] Message sending
- [ ] Commands (!who, !gmotd)
- [ ] Emoji/mention resolution

### Phase 6: Polish
- [ ] Message routing (bidirectional)
- [ ] Message formatting
- [ ] Filtering (regex patterns)
- [ ] Reconnection logic
- [ ] Guild dashboard
- [ ] Testing and documentation

---

## 6. Migration Notes

### 6.1 Dropped Features

Since we're targeting Ascension only:
- Vanilla, TBC, Original WoTLK Cataclysm, MoP packet handlers (removed)
- Version-specific branching (simplified)

### 6.2 Key Files to Reference

| Rust Module | Scala Source |
|-------------|--------------|
| `protocol/realm/handler.rs` | `realm/RealmPacketHandler.scala`, `realm/HandshakeAscension.scala` |
| `protocol/game/handler.rs` | `game/GamePacketHandlerWotLK.scala` |
| `protocol/crypto/header.rs` | `game/GameHeaderCryptWotLK.scala` |
| `discord/bot.rs` | `discord/Discord.scala` |
| `config/parser.rs` | `common/Config.scala` |

---

## 7. Testing Strategy

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

## 8. Success Criteria

| Metric | Target |
|--------|--------|
| RAM usage | < 30MB |
| Binary size | < 15MB |
| Startup time | < 1 second |
| Message latency | < 100ms (network permitting) |
| Feature parity | All relevant features from original |

---

## 9. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Ascension protocol changes | Document protocol thoroughly, keep Scala version as reference |
| Discord library instability | Abstract Discord behind trait, allow swapping implementations |

---

*Document created: 2026-01-23*  
*Last updated: 2026-01-23*
