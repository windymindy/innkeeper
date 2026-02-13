# Innkeeper

**Innkeeper** is a rust port of wowchat_ascension, fork of WoWChat for Ascension, clientless Discord-WoW chat bridge bot.
It connects to a World of Warcraft private server as a game client and relays messages between WoW guild/channels and Discord channels.

## Working on This Project

The local copy of the original repository is located in the ../wowchat_ascension folder in the parent directory.

**Original:** Scala/JVM implementation (~6,500 lines)
**Current State:** Rust/Tokio rewrite (~11,000 lines across 39 files)

### Before Implementation
1. Read `documentation/001_design.md` for full architecture
2. Reference Scala source files when implementing Rust equivalents

## Resources

- **Ascension Server:** Custom WotLK-based private server
- **WoW Protocol Docs:** [wowdev.wiki](https://wowdev.wiki/)
- **Original Repo:** https://github.com/windymindy/wowchat_ascension/
- **Upstream:** https://github.com/fjaros/wowchat

---
