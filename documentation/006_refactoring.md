# Refactoring Plan

Full code review of the innkeeper codebase. Findings grouped by severity, each with enough context to pick up and fix independently.

---

## CRITICAL - Security

### C1. No input length validation on network packet parsing

**Files:** `src/protocol/game/handler.rs` (lines 866-1002, `parse_movement`, `parse_update_fields`, `handle_update_object`)

**Problem:** Many `buf.get_*()` and `buf.advance()` calls lack `buf.remaining()` checks. A malformed or malicious server packet will cause a panic (the `bytes` crate panics on underflow). This crashes the entire bot.

**Examples:**
- `parse_movement` line 890: `buf.get_u16_le()` without checking `buf.remaining() >= 2`
- `parse_movement` line 934: `buf.advance(9 * 4)` (36 bytes) without any remaining check
- `parse_update_fields` line 879: `buf.advance((set_bits * 4) as usize)` where `set_bits` comes from packet data - attacker-controlled advance length

**Fix:** Wrap every `buf.get_*` / `buf.advance` in remaining checks, or better yet, create a safe wrapper that returns `Result` on underflow instead of panicking. Consider a `SafeBuf` newtype around `Bytes` that returns `Err` instead of panicking.

---

### C2. Unbounded `pending_messages` HashMap (memory leak)

**File:** `src/protocol/game/handler.rs:57`

**Problem:** `pending_messages: HashMap<u64, Vec<ChatMessage>>` grows without limit. When a name query is sent for an unknown GUID but the server never responds, the pending messages for that GUID are never drained. Over time (hours/days of uptime), this leaks memory.

**Fix:** Add a TTL or max-size cap. For example:
- Evict entries older than 30 seconds
- Cap at 100 pending GUIDs, dropping oldest
- Store `(Instant, Vec<ChatMessage>)` and periodically sweep

---

### C3. `read_cstring` has no max length guard

**Files:** `src/protocol/game/packets.rs:264`, `src/protocol/game/chat.rs:560`, `src/protocol/game/guild.rs:375`

**Problem:** All three copies of `read_cstring` loop until they find a null byte or exhaust the buffer. A malformed packet with no null terminator and a large payload will allocate a huge `Vec<u8>` before returning.

**Fix:** Add a `max_len` parameter (e.g., 256 for player names, 4096 for guild info). Return an error if the limit is exceeded.

---

### C4. Credentials logged at INFO level

**File:** `src/main.rs:52`

**Problem:** `info!("  WoW Account: {}", config.wow.account)` logs the WoW account name at INFO level. In containerized/cloud deployments, INFO logs are typically collected and stored, exposing credentials.

**Fix:** Log at DEBUG level, or redact to first/last character only.

---

## HIGH - Bugs and Hangs

### H1. `split_message` panics on multi-byte UTF-8

**File:** `src/game/formatter.rs:168`

**Problem:** `let chunk = &remaining[..max_len]` slices by byte offset. If `max_len` falls in the middle of a multi-byte UTF-8 character (e.g., emoji, accented character), this panics with "byte index is not a char boundary".

**Fix:** Use `remaining.char_indices()` to find the last char boundary at or before `max_len`, or use `remaining.floor_char_boundary(max_len)` (nightly), or a manual scan:
```rust
let split_at = remaining[..max_len]
    .char_indices()
    .last()
    .map(|(i, c)| i + c.len_utf8())
    .unwrap_or(max_len);
```

---

### H2. All channels are unbounded (backpressure missing)

**File:** `src/bridge/channels.rs:74-103`

**Problem:** Every channel in `ChannelBundle::new()` uses `mpsc::unbounded_channel()`. If the Discord side disconnects or is slow, messages from WoW accumulate without limit. The `wow_tx` channel is particularly risky since it carries all chat messages from a potentially busy server.

**Fix:** Use bounded channels (e.g., `mpsc::channel(256)`) for the high-volume paths (`wow_tx`, `outgoing_wow_tx`). Use `try_send` or `send().await` with timeouts. Keep unbounded only for low-volume control channels (shutdown, commands).

---

### H3. `std::process::exit(1)` in async context

**File:** `src/main.rs:146`

**Problem:** `std::process::exit(1)` kills the process immediately without running destructors, flushing log buffers, or cleaning up tokio tasks. Discord bot may not disconnect cleanly.

**Fix:** Return an error from `main()` instead, or use `panic!()` which at least unwinds.

---

### H4. `handle_shutdown` sleeps 2s unconditionally

**File:** `src/game/client.rs:573`

**Problem:** After sending CMSG_LOGOUT_REQUEST, the code does `tokio::time::sleep(Duration::from_secs(2)).await` regardless of whether SMSG_LOGOUT_COMPLETE has already arrived. This adds unnecessary delay to every shutdown.

**Fix:** Use `tokio::select!` to wait for either SMSG_LOGOUT_COMPLETE on the stream or the 2s timeout, whichever comes first.

---

### H5. Race condition in guild roster request timing

**File:** `src/protocol/game/handler.rs:411-413`

**Problem:** `request_guild_roster()` updates `last_roster_request = Some(Instant::now())` BEFORE the caller actually sends the packet. If `connection.send()` fails (network error), the timestamp is already set, so the next roster request is delayed by 60 seconds even though no request was actually sent.

**Fix:** Move the timestamp update to after successful send, or have `request_guild_roster` only build the packet and let the caller update the timestamp on success.

---

### H6. `connected` variable is dead logic

**File:** `src/main.rs:170-232`

**Problem:** `connected` is set to `true` at line 206, then the code enters a block that unconditionally sets it back to `false` at line 232. The `if connected { connected = false; continue; }` block at lines 184-188 is unreachable. The compiler warns about this.

**Fix:** Remove the `connected` variable entirely. The loop structure already handles reconnection via the `match connect_and_authenticate` result.

---

## MEDIUM - Code Duplication

### M1. `read_cstring` duplicated 3 times

**Files:**
- `src/protocol/game/packets.rs:264`
- `src/protocol/game/chat.rs:560`
- `src/protocol/game/guild.rs:375`

**Problem:** Three identical implementations of `read_cstring(buf: &mut Bytes) -> Result<String>`.

**Fix:** Move to `src/protocol/packets/codec.rs` (or a new `src/protocol/util.rs`) and import everywhere. Add the max-length guard (see C3) at the same time.

---

### M2. `make_test_config()` duplicated in 4 test modules

**Files:**
- `src/config/validate.rs:149`
- `src/config/env.rs:114`
- `src/bridge/orchestrator.rs:775`
- `src/game/client.rs:698`

**Problem:** Near-identical `make_test_config()` / `make_valid_config()` functions in every test module.

**Fix:** Create a `#[cfg(test)]` helper module in `src/config/types.rs` or a `src/test_helpers.rs` with a shared `make_test_config()`.

---

### M3. Markdown conditional duplication in `format_who_list` and `format_who_search`

**File:** `src/bridge/orchestrator.rs:359-453`

**Problem:** Both methods have `if enable_markdown { format!("**{}**...") } else { format!("{}...") }` branches that differ only by whether `**` wrapping is present.

**Fix:** Extract a `fn bold(s: &str, enable_markdown: bool) -> String` helper.

---

## MEDIUM - Module Organization

### M4. `common/mod.rs` re-exports from `bridge`

**File:** `src/common/mod.rs:18`

**Problem:** `pub use crate::bridge::GameChannels;` makes `common` depend on `bridge`, but `bridge` also depends on `common`. This creates a logical circular dependency. `common` should be a leaf module that others depend on, not the other way around.

**Fix:** Remove this re-export. Have consumers import `GameChannels` from `bridge` directly.

---

### M5. `game/mod.rs` re-exports `Bridge` from `bridge`

**File:** `src/game/mod.rs:14`

**Problem:** `pub use crate::bridge::Bridge;` - the comment says "for backwards compatibility" but this is misleading. `Bridge` belongs to `bridge`, not `game`.

**Fix:** Remove this re-export. Import `Bridge` from `bridge` directly where needed.

---

### M6. `Direction::from_str` shadows the `FromStr` trait

**File:** `src/bridge/orchestrator.rs:568`

**Problem:** `Direction` has an inherent `from_str` method that shadows the standard `FromStr` trait convention
