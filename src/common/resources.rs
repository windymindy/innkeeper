//! Game resources: zone names, class names, race names, etc.

/// WoW character classes (WotLK).
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
            Self::Druid => "Druid",
        }
    }
}

/// WoW character races (WotLK).
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
            Self::BloodElf => "Blood Elf",
            Self::Draenei => "Draenei",
        }
    }
}
