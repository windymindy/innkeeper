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

### 5.1 Per-Channel Filters ✅ IMPLEMENTED (2026-02-01)

**Status:** COMPLETE - Per-channel filter support has been fully implemented!

**Implementation:**

1. **Config Types Updated** (`src/config/types.rs`):
   - Added `filters: Option<FiltersConfig>` field to `WowChannelConfig`
   - Added `filters: Option<FiltersConfig>` field to `DiscordChannelConfig`

2. **MessageRouter Enhanced** (`src/game/router.rs`):
   - Updated `Route` struct to include per-route `filter: MessageFilter`
   - Added `build_route_filter()` function that merges WoW and Discord filter configs
   - Priority order: Discord filters (both directions) > WoW filters (WoW->Discord only) > global filters

3. **Bridge Updated** (`src/game/bridge.rs`):
   - Removed global filter field (now handled per-route)
   - Updated `handle_discord_to_wow()` to use `route.filter.should_filter_discord_to_wow()`
   - Cleaned up unused imports

4. **Tests Added** (`src/game/router.rs`):
   - `test_per_channel_filter_wow_to_discord`: Tests WoW-side filters
   - `test_per_channel_filter_discord_priority`: Tests Discord filters take priority
   - `test_per_channel_filter_disabled`: Tests disabled filter behavior


---

## 6. Implementation Order

### Phase 1: Critical Fixes ✅ COMPLETED (2026-01-30)
1. [x] Create `common/messages.rs` with canonical message types
2. [x] Remove duplicate type definitions
3. [x] Fix main.rs double channel creation
4. [x] Add missing Goblin race and Monk class
5. [x] Replace hardcoded chat type constants

### Phase 2: Structural Improvements ✅ COMPLETED (2026-01-30)
1. [x] Remove unused code (ChannelResolver, loop functions, create_bridge_channels)
2. [x] Add missing constants (AuthResponseCodes, ChatChannelIds)
3. [-] Split BridgeChannels into logical groups (DEFERRED - working well as-is)

### Phase 3: Code Quality ✅ COMPLETED (2026-02-01)
1. [x] Remove unused imports from config/mod.rs, game/router.rs, protocol/game modules
2. [x] Remove unused variable assignments (resolved_count in discord/handler.rs)
3. [x] Add #[allow(dead_code)] to unused error types and type aliases for future use
4. [x] Standardize error handling with anyhow crate
5. [ ] Create shared test utilities (DEFERRED - low priority)

### Phase 4: Optional Enhancements ✅ COMPLETED (2026-02-01)
1. [x] Per-channel filter support
2. [ ] Restructure module organization (DEFERRED - low priority)

---

## 7. Implementation Summary (2026-01-30)

### Completed Work

**Phase 1 - All Critical Fixes Completed:**
- Created `src/common/messages.rs` as single source of truth for message types
- Removed duplicate `OutgoingWowMessage`, `IncomingWowMessage`, `WowMessage`, `BridgeCommand` definitions
- Cleaned up `BridgeChannels::new()` API to return `(BridgeChannels, wow_rx, command_tx, command_response_rx)`
- Fixed `main.rs` double channel creation bug (lines 81 and 172-188 issue resolved)
- Added `Goblin` (race ID 9) and `Monk` (class ID 10) to `common/resources.rs`
- Added `language()` helper method to Race enum
- Replaced all hardcoded `0x04` and `0x00` with `chat_events::CHAT_MSG_GUILD` and `chat_events::CHAT_MSG_SYSTEM`

**Phase 2 - Structural Improvements Completed:**
- Removed unused `ChannelResolver` type alias and `channel_resolver` field from Bridge
- Removed unused `set_channel_resolver()` method
- Removed unused `run_wow_to_discord_loop()`, `run_discord_to_wow_loop()`, `run_command_response_loop()` functions
- Removed unused `create_bridge_channels()` function from `discord/handler.rs`
- Removed broken `handle_wow_to_discord()` method (Discord handler now does this directly)
- Added `auth_response` module with all 23 auth response codes and helper functions
- Added `channel_ids` module with standard WoW channel IDs (GENERAL, TRADE, etc.)

**Test Results:**
- All 55 existing tests pass ✅
- Build succeeds with only minor unused import warnings
- No new clippy warnings introduced

### Files Modified

| File | Changes Made |
|------|--------------|
| `src/common/mod.rs` | Added `messages` module, re-exported message types |
| `src/common/messages.rs` | **NEW** - Canonical message types and BridgeChannels |
| `src/common/resources.rs` | Added Goblin, Monk, language() method |
| `src/main.rs` | Refactored to use new BridgeChannels API, removed duplication |
| `src/game/bridge.rs` | Removed duplicates, unused code, re-exports from common |
| `src/game/client.rs` | Replaced hardcoded constants, updated imports |
| `src/game/mod.rs` | Updated re-exports to use common |
| `src/discord/handler.rs` | Removed duplicates, removed create_bridge_channels |
| `src/discord/client.rs` | Updated imports to use common |
| `src/discord/mod.rs` | Updated re-exports |
| `src/protocol/game/chat.rs` | Added channel_ids module |
| `src/protocol/packets/opcodes.rs` | Added auth_response module |

---

## 8. Phase 3 Implementation Summary (2026-01-31)

### Completed Work

**Code Quality Improvements:**
- Removed unused imports from:
  - `src/config/mod.rs`: `load_config_str`, `has_required_fields`
  - `src/game/router.rs`: `ChannelMapping` (kept in tests where needed)
  - `src/protocol/game/mod.rs`: Removed unused re-exports
  - `src/protocol/game/handler.rs`: Removed unused `chat_events` import
- Removed unused variable `resolved_count` from `discord/handler.rs:64-99`
- Added `#[allow(dead_code)]` to unused error types and type aliases for future use:
  - `AppError`, `DiscordError`, `Result<T>`, `ProtocolResult<T>`, `ConnectionResult<T>`, `DiscordResult<T>`
  - Unused error variants: `ParseError`, `MissingField`, `InvalidValue`, `CharacterNotFound`, `RealmNotFound`, `DecryptionError`, `Timeout`, `MaxReconnectAttempts`

**Test Results:**
- All 55 existing tests pass ✅
- Build succeeds with only non-critical warnings (dead_code for future features)
- Significantly reduced unused import warnings (from 10+ to remaining platform-specific ones)

### Files Modified

| File | Changes Made |
|------|--------------|
| `src/config/mod.rs` | Removed unused imports |
| `src/game/router.rs` | Removed unused import from top, kept in test module |
| `src/protocol/game/mod.rs` | Cleaned up re-exports |
| `src/protocol/game/handler.rs` | Removed unused import |
| `src/discord/handler.rs` | Removed unused `resolved_count` variable |
| `src/common/error.rs` | Added `#[allow(dead_code)]` to future-use error types |

---

## 9. Remaining Work

### Deferred Items (Low Priority)

**BridgeChannels Simplification:**
- Current structure works well, decided not to split further
- Future enhancement if complexity grows

**Error Handling Standardization:**
- Currently works adequately
- Would benefit from anyhow crate adoption
- Not critical for functionality

**Test Config Duplication:**
- Test fixtures are duplicated but isolated
- Low impact on maintenance
- Can consolidate if tests expand significantly

**Module Restructuring:**
- Current organization is functional
- Major restructure would be high-risk, low-reward
- Consider only if adding significant new features

**Remaining Warnings (Non-Critical):**
- Dead code warnings for fields/structs reserved for future features (guild dashboard, quirks, etc.)
- These are intentionally kept for completeness and future use
- Platform-specific unused imports that may be needed in different build configurations

---

## 10. Known Limitations (Documented)

1. **No locale support** - Ascension uses enUS only
2. **Version/build hardcoded** - 3.3.5 only (correct for Ascension)
3. **Per-channel filters don't override global filters** - They supplement them (per-channel wins)
4. **handle_wow_to_discord removed** - Discord handler does forwarding directly

---

## 11. Verification

**Build Status:** ✅ PASSING
```
cargo build --release
```
- No compilation errors
- Minimal non-critical warnings (dead code for future features)

**Test Status:** ✅ ALL PASSING (58/58)
```
cargo test
```
- All 58 tests pass (3 new tests added for per-channel filters)
- Test coverage maintained
- No regressions introduced

**Clippy:** ✅ CLEAN
- Dead code warnings are intentional (future features)
- No new issues introduced by refactoring

---

## 12. Conclusion

**Status:** Phases 1, 2, and 3 Complete ✅

The codebase now has:
- Single source of truth for message types
- No duplicate type definitions
- Cleaner channel creation API
- Complete WotLK/Ascension resource definitions
- Proper use of named constants instead of magic numbers
- Removed dead/unused code
- Cleaned up unused imports and variables
- **Per-channel filter support** (Discord filters apply to both directions, WoW filters to WoW->Discord)

All 58 tests pass and the code compiles without errors. The refactoring maintains backward compatibility while improving code quality and adding new functionality.

## 13. Phase 3 Error Handling Update (2026-02-01)

### Standardized Error Handling with anyhow

**Problem Solved:** 4.4 Inconsistent Error Handling Pattern

**Benefits:**
- Consistent error handling across the codebase
- Better error messages with context
- Easier error propagation using `?` operator
- No more `Box<dyn Error>` type erasure

**Verification:**
- Build: ✅ PASSING (no errors, only expected dead code warnings)
- Tests: ✅ ALL 58 TESTS PASSING
- No functional changes, purely refactoring

---

*Document created: 2026-01-30*  
*Last updated: 2026-02-01 (Session 7 - Error Handling Standardization)*
