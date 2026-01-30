//! Shared types used across the application.

use crate::common::resources::{Class, Race};

/// Unique identifier for a WoW character.
pub type Guid = u64;

/// Represents a player character.
#[derive(Debug, Clone)]
pub struct Player {
    pub guid: Guid,
    pub name: String,
    pub level: u8,
    pub class: Option<Class>,
    pub race: Option<Race>,
    pub zone_id: u32,
}

/// Represents a guild member.
#[derive(Debug, Clone)]
pub struct GuildMember {
    pub guid: Guid,
    pub name: String,
    pub level: u8,
    pub class: Option<Class>,
    pub rank: u8,
    pub rank_name: String,
    pub zone_id: u32,
    pub online: bool,
    pub note: String,
    pub officer_note: String,
}

/// Type of chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatType {
    Say,
    Party,
    Raid,
    Guild,
    Officer,
    Whisper,
    WhisperInform,
    Emote,
    Channel,
    System,
    Yell,
    RaidLeader,
    RaidWarning,
    Battleground,
    BattlegroundLeader,
    Achievement,
    GuildAchievement,
}

impl ChatType {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0x00 => Some(Self::System),
            0x01 => Some(Self::Say),
            0x02 => Some(Self::Party),
            0x03 => Some(Self::Raid),
            0x04 => Some(Self::Guild),
            0x05 => Some(Self::Officer),
            0x06 => Some(Self::Yell),
            0x07 => Some(Self::Whisper),
            0x09 => Some(Self::WhisperInform),
            0x0A => Some(Self::Emote),
            0x11 => Some(Self::Channel),
            0x28 => Some(Self::RaidLeader),
            0x29 => Some(Self::RaidWarning),
            0x2C => Some(Self::Battleground),
            0x2D => Some(Self::BattlegroundLeader),
            0x30 => Some(Self::Achievement),
            0x31 => Some(Self::GuildAchievement),
            _ => None,
        }
    }

    /// Convert ChatType to its protocol wire value.
    /// This is the inverse of from_id().
    pub fn to_id(&self) -> u8 {
        match self {
            Self::System => 0x00,
            Self::Say => 0x01,
            Self::Party => 0x02,
            Self::Raid => 0x03,
            Self::Guild => 0x04,
            Self::Officer => 0x05,
            Self::Yell => 0x06,
            Self::Whisper => 0x07,
            Self::WhisperInform => 0x09,
            Self::Emote => 0x0A,
            Self::Channel => 0x11,
            Self::RaidLeader => 0x28,
            Self::RaidWarning => 0x29,
            Self::Battleground => 0x2C,
            Self::BattlegroundLeader => 0x2D,
            Self::Achievement => 0x30,
            Self::GuildAchievement => 0x31,
        }
    }
}

/// A chat message from WoW.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub chat_type: ChatType,
    pub language: u32,
    pub sender_guid: Guid,
    pub sender_name: String,
    pub channel_name: Option<String>,
    pub content: String,
}

/// Guild information.
#[derive(Debug, Clone, Default)]
pub struct GuildInfo {
    pub name: String,
    pub motd: String,
    pub info: String,
    pub members: Vec<GuildMember>,
}

/// Guild event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuildEvent {
    Promotion,
    Demotion,
    Motd,
    Joined,
    Left,
    Removed,
    SignedOn,
    SignedOff,
}

impl GuildEvent {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Promotion),
            1 => Some(Self::Demotion),
            2 => Some(Self::Motd),
            3 => Some(Self::Joined),
            4 => Some(Self::Left),
            5 => Some(Self::Removed),
            12 => Some(Self::SignedOn),
            13 => Some(Self::SignedOff),
            _ => None,
        }
    }
}
