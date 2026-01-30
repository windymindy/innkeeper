# Innkeeper Refactoring Plan

**Date:** 2026-01-30  
**Status:** Draft  
**Goal:** Improve code quality, remove duplication, and ensure correctness against original Scala implementation

---

## 1. Executive Summary

After thorough comparison with the original `wowchat_ascension` Scala codebase (~6,500 lines), this document identifies issues, duplications, and missing pieces in the Innkeeper Rust port (~3,000 lines). The refactoring is organized into priority tiers.

---

## 2. Critical Issues (Priority 1 - Must Fix)

### 2.1 Duplicate Type Definitions

**Problem:** Multiple definitions of the same message types across modules creates maintenance burden and potential inconsistencies.

| Type | Locations | Issue |
|------|-----------|-------|
| `OutgoingWowMessage` | `discord/handler.rs:36-41`, `game/bridge.rs:72` | Identical struct defined twice, re-exported |
| `IncomingWowMessage` | `discord/handler.rs:44-50`, `game/bridge.rs` (via From) | Related but separate types |
| `WowMessage` | `game/bridge.rs:19-31` | Similar to IncomingWowMessage |
| `ChannelConfig` | `discord/handler.rs:24-32`, `discord/client.rs:20` | Same name, different contexts |

**Solution:**
1. Create `src/common/messages.rs` with canonical message type definitions
2. All modules import from this single source
3. Remove duplicate definitions

### 2.2 Redundant Channel Creation in main.rs

**Problem:** `main.rs:172-188` creates `game_bridge_channels` twice with overlapping logic, shadowing the earlier definition from line 81.

```rust
// Line 81 - First creation
let (game_bridge_channels, game_wow_rx) = BridgeChannels::new();

// Lines 172-188 - Creates ANOTHER set, shadowing the first
let game_bridge_channels = {
    let (game_discord_tx, game_discord_rx) = mpsc::unbounded_channel();
    // ... more channels ...
    BridgeChannels { ... }
};
```

**Solution:**
1. Remove the second redundant channel creation block
2. Use the channels from `BridgeChannels::new()` directly
3. Clean up unused channel receivers (`_game_cmd_rx`, `dummy_rx`)

### 2.3 Missing `Goblin` Race in resources.rs

**Problem:** `resources.rs` is missing the Goblin race (ID 9) which exists in the original Scala code.

```scala
// Original GamePackets.scala:126
val RACE_GOBLIN = 0x09
```

**Solution:** Add Goblin = 9 to the Race enum in `common/resources.rs`.

### 2.4 Missing `Monk` Class in resources.rs

**Problem:** `resources.rs` is missing the Monk class (ID 10) which exists in the original Scala code. While Monk doesn't exist in WotLK, Ascension may have custom classes.

```scala
// Original GamePackets.scala:174
val CLASS_MONK = 0x0A
```

**Solution:** Add Monk = 10 to the Class enum in `common/resources.rs` for completeness.

---

## 3. Structural Issues (Priority 2 - Should Fix)

### 3.1 Inconsistent Module Organization

**Problem:** The `discord/` and `game/` modules have overlapping responsibilities:
- `game/bridge.rs` handles Discord-WoW bridging
- `discord/handler.rs` also handles bridging
- Both have message routing logic

**Solution:**
Create a clearer separation:
```
src/
├── bridge/              # NEW: Unified bridge module
│   ├── mod.rs
│   ├── messages.rs      # Canonical message types
│   ├── channels.rs      # Channel management (from bridge.rs)
│   └── state.rs         # Shared state (from handler.rs BridgeState)
├── discord/
│   ├── mod.rs
│   ├── client.rs        # Discord connection only
│   ├── handler.rs       # Discord event handling only
│   └── commands.rs      # Command parsing
└── game/
    ├── mod.rs
    ├── client.rs        # Game connection only
    ├── router.rs        # Channel routing
    └── formatter.rs     # Message formatting and filtering
```

### 3.2 BridgeChannels Has Too Many Fields

**Problem:** `BridgeChannels` in `game/bridge.rs:76-95` has 9 channel fields, making it unwieldy and error-prone.

**Solution:** Split into logical groups:
```rust
pub struct GameChannels {
    pub wow_tx: Sender<WowMessage>,
    pub outgoing_wow_rx: Receiver<OutgoingWowMessage>,
}

pub struct DiscordChannels {
    pub discord_tx: Sender<DiscordMessage>,
    pub discord_rx: Receiver<DiscordMessage>,
}

pub struct CommandChannels {
    pub command_rx: Receiver<BridgeCommand>,
    pub command_response_tx: Sender<CommandResponse>,
}
```

### 3.3 Hardcoded Chat Type Constants in client.rs

**Problem:** `client.rs` uses hardcoded magic numbers instead of the constants defined in `chat.rs`:

```rust
// client.rs:189
chat_type: 0x04, // CHAT_MSG_GUILD

// client.rs:204, 219, 264
chat_type: 0x00, // CHAT_MSG_SYSTEM
```

**Solution:** Use `chat_events::CHAT_MSG_GUILD` and `chat_events::CHAT_MSG_SYSTEM` constants.

---

## 4. Code Quality Issues (Priority 3 - Nice to Have)

### 4.1 Unused Code

**Unused imports and functions to remove:**

| Location | Item | Reason |
|----------|------|--------|
| `game/bridge.rs:7` | `serenity::all::ChannelId` | Only used in unused `ChannelResolver` type |
| `game/bridge.rs:126` | `ChannelResolver` type alias | Never actually used |
| `game/bridge.rs:396-415` | `run_command_response_loop` | Duplicate of logic in main.rs |
| `discord/handler.rs:414-428` | `create_bridge_channels` function | Not used, duplicates `BridgeChannels::new()` |
| `game/client.rs:3-4` | `Arc`, `AsyncRead/Write` imports | Partially unused |
| `game/client.rs:20` | `FramedParts` import | Unused |

### 4.2 Missing AuthResponseCodes Constants

**Problem:** The original Scala code has comprehensive `AuthResponseCodes` in `GamePackets.scala:209-284` but Rust port only handles success/failure without detailed error codes.

**Solution:** Add `auth_response_codes` module to `protocol/game/packets.rs` or `protocol/packets/opcodes.rs`:

```rust
pub mod auth_response {
    pub const AUTH_OK: u8 = 0x0C;
    pub const AUTH_FAILED: u8 = 0x0D;
    pub const AUTH_REJECT: u8 = 0x0E;
    // ... rest from GamePackets.scala
}
```

### 4.3 Missing ChatChannelIds Constants

**Problem:** Original has `ChatChannelIds` object with `GENERAL`, `TRADE`, `LOCAL_DEFENSE`, etc. These aren't in the Rust port.

```scala
// Original GamePackets.scala:333-355
object ChatChannelIds {
    val GENERAL = 0x01
    val TRADE = 0x02
    // ...
}
```

**Solution:** Add to `protocol/game/chat.rs`:
```rust
pub mod channel_ids {
    pub const GENERAL: u32 = 0x01;
    pub const TRADE: u32 = 0x02;
    pub const LOCAL_DEFENSE: u32 = 0x16;
    pub const WORLD_DEFENSE: u32 = 0x17;
    pub const GUILD_RECRUITMENT: u32 = 0x19; // TBC/WotLK
    pub const LOOKING_FOR_GROUP: u32 = 0x1A;
}
```

### 4.4 Inconsistent Error Handling Pattern

**Problem:** Some functions return `Result<T, Box<dyn Error>>` while others use specific error types from `common/error.rs`.

**Solution:** Switch to anyhow crate style error handling.

### 4.5 Test Config Duplication

**Problem:** `make_test_config()` is duplicated in multiple test modules:
- `config/validate.rs:140-180`
- `config/env.rs:114-141`
- `game/bridge.rs:425-467`
- `game/client.rs:353-380`
- `discord/client.rs` (indirectly via handler tests)

**Solution:** Create `src/common/test_utils.rs` (only compiled with `#[cfg(test)]`) with shared test fixtures.

---

## 5. Missing Features vs Original

### 5.1 Per-Channel Filters

**Problem:** Original supports filters at the channel level, Rust only has global filters.

```scala
// Original Config.scala:21
case class WowChannelConfig(..., filters: Option[FiltersConfig])
case class DiscordChannelConfig(..., filters: Option[FiltersConfig])
```

**Impact:** Medium - Users might want different filter rules per channel.

**Decision:** Document as enhancement opportunity.

---

## 6. Implementation Order

### Phase 1: Critical Fixes (1-2 hours)
1. [ ] Create `common/messages.rs` with canonical message types
2. [ ] Remove duplicate type definitions
3. [ ] Fix main.rs double channel creation
4. [ ] Add missing Goblin race and Monk class
5. [ ] Replace hardcoded chat type constants

### Phase 2: Structural Improvements (2-3 hours)
1. [ ] Split BridgeChannels into logical groups
2. [ ] Remove unused code
3. [ ] Add missing constants (AuthResponseCodes, ChatChannelIds)

### Phase 3: Code Quality (1-2 hours)
1. [ ] Standardize error handling
2. [ ] Create shared test utilities
3. [ ] Add documentation for limitations

### Phase 4: Optional Enhancements (future)
1. [ ] Per-channel filter support
2. [ ] Restructure module organization

---

## 7. Files to Modify

| File | Changes |
|------|---------|
| `src/common/mod.rs` | Add `messages` module |
| `src/common/messages.rs` | NEW - Canonical message types |
| `src/common/resources.rs` | Add Goblin race, Monk class |
| `src/common/types.rs` | Potentially merge with messages.rs |
| `src/main.rs` | Remove duplicate channel creation |
| `src/game/bridge.rs` | Remove duplicate types, use common/messages |
| `src/game/client.rs` | Use chat_events constants |
| `src/discord/handler.rs` | Remove duplicate types, use common/messages |
| `src/discord/mod.rs` | Update re-exports |
| `src/game/mod.rs` | Update re-exports |
| `src/protocol/game/chat.rs` | Add channel_ids module |
| `src/protocol/packets/opcodes.rs` | Add auth_response module |

---

## 8. Verification

After refactoring:
1. All 55 existing tests must pass
2. `cargo build` must succeed with no new warnings
3. `cargo clippy` should show improvement (fewer warnings)
4. Manual test against Ascension server (if available)

---

*Document created: 2026-01-30*
