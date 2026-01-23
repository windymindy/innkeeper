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
            0 => Some(Self::Say),
            1 => Some(Self::Party),
            2 => Some(Self::Raid),
            3 => Some(Self::Guild),
            4 => Some(Self::Officer),
            6 => Some(Self::Yell),
            7 => Some(Self::Whisper),
            8 => Some(Self::WhisperInform),
            10 => Some(Self::Emote),
            14 => Some(Self::Channel),
            17 => Some(Self::RaidLeader),
            38 => Some(Self::RaidWarning),
            39 => Some(Self::Battleground),
            40 => Some(Self::BattlegroundLeader),
            48 => Some(Self::Achievement),
            49 => Some(Self::GuildAchievement),
            _ => None,
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
