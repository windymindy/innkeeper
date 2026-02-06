//! Game resources: zone names, class names, race names, etc.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Link site for item/spell/quest/achievement links.
pub const LINK_SITE: &str = "https://db.ascension.gg";

/// WoW character classes (WotLK/Ascension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Class {
    Warrior = 1,
    Paladin = 2,
    Hunter = 3,
    Rogue = 4,
    Priest = 5,
    DeathKnight = 6,
    Shaman = 7,
    Mage = 8,
    Warlock = 9,
    Monk = 10, // Added for completeness (Ascension may have custom classes)
    Druid = 11,
}

impl Class {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::Warrior),
            2 => Some(Self::Paladin),
            3 => Some(Self::Hunter),
            4 => Some(Self::Rogue),
            5 => Some(Self::Priest),
            6 => Some(Self::DeathKnight),
            7 => Some(Self::Shaman),
            8 => Some(Self::Mage),
            9 => Some(Self::Warlock),
            10 => Some(Self::Monk),
            11 => Some(Self::Druid),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Warrior => "Warrior",
            Self::Paladin => "Paladin",
            Self::Hunter => "Hunter",
            Self::Rogue => "Rogue",
            Self::Priest => "Priest",
            Self::DeathKnight => "Death Knight",
            Self::Shaman => "Shaman",
            Self::Mage => "Mage",
            Self::Warlock => "Warlock",
            Self::Monk => "Monk",
            Self::Druid => "Druid",
        }
    }
}

/// WoW character races (WotLK/Ascension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Race {
    Human = 1,
    Orc = 2,
    Dwarf = 3,
    NightElf = 4,
    Undead = 5,
    Tauren = 6,
    Gnome = 7,
    Troll = 8,
    Goblin = 9, // Added - exists in original Scala
    BloodElf = 10,
    Draenei = 11,
}

impl Race {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::Human),
            2 => Some(Self::Orc),
            3 => Some(Self::Dwarf),
            4 => Some(Self::NightElf),
            5 => Some(Self::Undead),
            6 => Some(Self::Tauren),
            7 => Some(Self::Gnome),
            8 => Some(Self::Troll),
            9 => Some(Self::Goblin),
            10 => Some(Self::BloodElf),
            11 => Some(Self::Draenei),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Human => "Human",
            Self::Orc => "Orc",
            Self::Dwarf => "Dwarf",
            Self::NightElf => "Night Elf",
            Self::Undead => "Undead",
            Self::Tauren => "Tauren",
            Self::Gnome => "Gnome",
            Self::Troll => "Troll",
            Self::Goblin => "Goblin",
            Self::BloodElf => "Blood Elf",
            Self::Draenei => "Draenei",
        }
    }

    /// Get the default language for this race (for sending chat messages).
    pub fn language(&self) -> u32 {
        match self {
            // Horde races speak Orcish
            Self::Orc
            | Self::Undead
            | Self::Tauren
            | Self::Troll
            | Self::BloodElf
            | Self::Goblin => 1, // LANG_ORCISH
            // Alliance races speak Common
            _ => 7, // LANG_COMMON
        }
    }
}

// ============================================================================
// Achievement Database
// ============================================================================

/// Achievement database loaded from achievements.csv.
static ACHIEVEMENTS: OnceLock<HashMap<u32, String>> = OnceLock::new();

/// Load achievements from embedded CSV data.
fn load_achievements() -> HashMap<u32, String> {
    // Embedded achievements.csv content (from wowchat_ascension)
    let csv_data = include_str!("../../resources/achievements.csv");

    csv_data
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ',');
            let id = parts.next()?.parse::<u32>().ok()?;
            let name = parts.next()?.to_string();
            Some((id, name))
        })
        .collect()
}

/// Get the achievements database, loading it if necessary.
pub fn get_achievements() -> &'static HashMap<u32, String> {
    ACHIEVEMENTS.get_or_init(load_achievements)
}

/// Get an achievement name by ID.
pub fn get_achievement_name(id: u32) -> Option<&'static str> {
    get_achievements().get(&id).map(|s: &String| s.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_achievements_loads() {
        let achievements = get_achievements();

        // Should have loaded at least some achievements
        assert!(!achievements.is_empty());

        // Check for known achievement
        assert!(achievements.contains_key(&6)); // Level 10
        assert_eq!(achievements.get(&6), Some(&"Level 10".to_string()));
    }

    #[test]
    fn test_get_achievement_name() {
        // Test known achievement
        assert_eq!(get_achievement_name(6), Some("Level 10"));

        // Test unknown achievement
        assert_eq!(get_achievement_name(999999999), None);
    }
}
