//! Exact 61-hero CFF0 `AbilitySet` slot index table documented in `docs/native-abilities.md`.
//!
//! Each decrypted v7 hero `ActorDef` record stores an ordered `PTCH` pointer array of
//! `AbilityDef` records whose 0-based indices match wire `0x043A` learn grants and
//! `0x0412`/`0x0415`/`0x0416` ability selectors. Across all 61 `HeroManifest` entries,
//! 41 heroes use standard `(A=0, B=1, C=2)` indices and 20 heroes have shifted,
//! sub-ability, or dual-form indices.

use crate::EntityTemplate;
use serde::{Deserialize, Serialize};

/// Primary hero ability slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AbilitySlot {
    A,
    B,
    C,
}

impl AbilitySlot {
    pub const ALL: [Self; 3] = [Self::A, Self::B, Self::C];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
        }
    }

    pub const fn ordinal(self) -> usize {
        self as usize
    }
}

/// Ability bar form for heroes with dual-form CFF0 primary slots (Malene).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AbilityForm {
    Primary,
    Alternate,
}

/// Semantic role of an entry inside a hero's CFF0 `AbilitySet` array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityEntryKind {
    /// Primary A/B/C slot (including Malene Light form `A1`/`B1`/`C1` and Ozo `A1`).
    Primary(AbilitySlot),
    /// Malene Shadow form slot (`A2`, `B2`, `C2`).
    DualForm(AbilitySlot),
    /// Combo stage, recast, cancel, or empowered variant associated with a slot.
    SubAbility(AbilitySlot),
    /// Attack override, perk, eat/spit, disabled placeholder, or talent entry inside `AbilitySet`.
    Auxiliary,
}

/// One 0-based slot in a hero's CFF0 `AbilitySet` array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilitySetEntry {
    pub index: u8,
    pub ability_id: &'static str,
    pub kind: AbilityEntryKind,
}

/// Complete CFF0 `AbilitySet` slot layout for one hero template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroAbilitySlots {
    pub template: EntityTemplate,
    pub hero_id: &'static str,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    /// Alternate-form `[A2, B2, C2]` indices when the hero is dual-form (`Malene`).
    pub dual_form: Option<[u8; 3]>,
    /// Ordered entries matching CFF0 `AbilitySet` indices `0..entries.len()`.
    pub entries: &'static [AbilitySetEntry],
}

impl HeroAbilitySlots {
    /// Returns the primary CFF0 `AbilitySet` index for `slot`.
    pub const fn index(&self, slot: AbilitySlot) -> u8 {
        match slot {
            AbilitySlot::A => self.a,
            AbilitySlot::B => self.b,
            AbilitySlot::C => self.c,
        }
    }

    /// Returns the CFF0 `AbilitySet` index for `(slot, form)`.
    pub const fn index_for_form(&self, slot: AbilitySlot, form: AbilityForm) -> Option<u8> {
        match form {
            AbilityForm::Primary => Some(self.index(slot)),
            AbilityForm::Alternate => match self.dual_form {
                Some(alt) => Some(alt[slot.ordinal()]),
                None => None,
            },
        }
    }

    /// Returns `true` for the 41 standard `(A=0, B=1, C=2)` single-form heroes.
    pub const fn is_standard(&self) -> bool {
        self.a == 0 && self.b == 1 && self.c == 2 && self.dual_form.is_none()
    }

    /// Returns `true` when the hero uses dual-form interleaved slots (`Malene`).
    pub const fn is_dual_form(&self) -> bool {
        self.dual_form.is_some()
    }

    /// Returns the `abilities.json` `ability_id` for primary `slot`.
    pub fn ability_id(&self, slot: AbilitySlot) -> &'static str {
        self.entries[usize::from(self.index(slot))].ability_id
    }

    /// Returns the `abilities.json` `ability_id` for `(slot, form)`.
    pub fn ability_id_for_form(
        &self,
        slot: AbilitySlot,
        form: AbilityForm,
    ) -> Option<&'static str> {
        let index = self.index_for_form(slot, form)?;
        self.entries
            .get(usize::from(index))
            .map(|entry| entry.ability_id)
    }

    /// Returns the `AbilitySetEntry` at `index`, if within this hero's CFF0 `AbilitySet`.
    pub fn entry(&self, index: u8) -> Option<&'static AbilitySetEntry> {
        self.entries.get(usize::from(index))
    }

    /// Resolves a primary `AbilitySet` index back to `AbilitySlot` (excluding alternate forms).
    pub const fn primary_slot_for_index(&self, index: u8) -> Option<AbilitySlot> {
        if index == self.a {
            Some(AbilitySlot::A)
        } else if index == self.b {
            Some(AbilitySlot::B)
        } else if index == self.c {
            Some(AbilitySlot::C)
        } else {
            None
        }
    }

    /// Resolves a primary or dual-form `AbilitySet` index back to `(AbilitySlot, AbilityForm)`.
    pub const fn slot_for_index(&self, index: u8) -> Option<(AbilitySlot, AbilityForm)> {
        if let Some(slot) = self.primary_slot_for_index(index) {
            return Some((slot, AbilityForm::Primary));
        }
        if let Some([a2, b2, c2]) = self.dual_form {
            if index == a2 {
                return Some((AbilitySlot::A, AbilityForm::Alternate));
            } else if index == b2 {
                return Some((AbilitySlot::B, AbilityForm::Alternate));
            } else if index == c2 {
                return Some((AbilitySlot::C, AbilityForm::Alternate));
            }
        }
        None
    }

    /// Finds the 0-based `AbilitySet` index for an exact `abilities.json` `ability_id`.
    pub fn index_of(&self, ability_id: &str) -> Option<u8> {
        self.entries
            .iter()
            .find(|entry| entry.ability_id == ability_id)
            .map(|entry| entry.index)
    }
}

const fn entry(index: u8, ability_id: &'static str, kind: AbilityEntryKind) -> AbilitySetEntry {
    AbilitySetEntry {
        index,
        ability_id,
        kind,
    }
}

macro_rules! standard_hero {
    ($template:ident, $hero_id:literal, $prefix:literal) => {
        HeroAbilitySlots {
            template: EntityTemplate::$template,
            hero_id: $hero_id,
            a: 0,
            b: 1,
            c: 2,
            dual_form: None,
            entries: &[
                entry(
                    0,
                    concat!("Ability__", $prefix, "__A"),
                    AbilityEntryKind::Primary(AbilitySlot::A),
                ),
                entry(
                    1,
                    concat!("Ability__", $prefix, "__B"),
                    AbilityEntryKind::Primary(AbilitySlot::B),
                ),
                entry(
                    2,
                    concat!("Ability__", $prefix, "__C"),
                    AbilityEntryKind::Primary(AbilitySlot::C),
                ),
            ],
        }
    };
    ($template:ident, $hero_id:literal) => {
        standard_hero!($template, $hero_id, $hero_id)
    };
}

macro_rules! custom_hero {
    (
        $template:ident,
        $hero_id:literal,
        ($a:literal, $b:literal, $c:literal),
        $dual_form:expr,
        [$(($idx:literal, $id:expr, $kind:expr)),+ $(,)?]
    ) => {
        HeroAbilitySlots {
            template: EntityTemplate::$template,
            hero_id: $hero_id,
            a: $a,
            b: $b,
            c: $c,
            dual_form: $dual_form,
            entries: &[$(entry($idx, $id, $kind)),+],
        }
    };
}

use AbilityEntryKind::{Auxiliary, DualForm, Primary, SubAbility};
use AbilitySlot::{A, B, C};

/// Complete CFF0 `AbilitySet` slot table across all 61 `HeroManifest` templates.
const HERO_ABILITY_SLOTS: [HeroAbilitySlots; 61] = [
    // 41 standard heroes: (A=0, B=1, C=2)
    standard_hero!(Adagio, "Adagio"),
    standard_hero!(Alpha, "Alpha"),
    standard_hero!(Amael, "Amael"),
    standard_hero!(Ardan, "Ardan"),
    standard_hero!(Baptiste, "Baptiste"),
    standard_hero!(Blackfeather, "Blackfeather"),
    standard_hero!(Caine, "Caine"),
    standard_hero!(Celeste, "Celeste"),
    standard_hero!(Churnwalker, "Churnwalker"),
    standard_hero!(Flicker, "Flicker"),
    standard_hero!(Fortress, "Fortress"),
    standard_hero!(Grace, "Grace"),
    standard_hero!(Hero009, "Hero009", "Krul"),
    standard_hero!(Skaarf, "Hero010", "Skaarf"),
    standard_hero!(Rona, "Hero016", "Rona"),
    standard_hero!(Hero062, "Hero062"),
    standard_hero!(Hero999, "Hero999"),
    standard_hero!(Inara, "Inara"),
    standard_hero!(Ishtar, "Ishtar"),
    standard_hero!(Joule, "Joule"),
    standard_hero!(Karas, "Karas"),
    standard_hero!(Kensei, "Kensei"),
    standard_hero!(Kestrel, "Kestrel"),
    standard_hero!(Kinetic, "Kinetic"),
    standard_hero!(Koshka, "Koshka"),
    standard_hero!(Lance, "Lance"),
    standard_hero!(LanceBallLance, "LanceBall_Lance"),
    standard_hero!(Leo, "Leo"),
    standard_hero!(Lorelai, "Lorelai"),
    standard_hero!(Magnus, "Magnus"),
    standard_hero!(Miho, "Miho"),
    standard_hero!(Petal, "Petal"),
    standard_hero!(Phinn, "Phinn"),
    standard_hero!(Reim, "Reim"),
    standard_hero!(Reza, "Reza"),
    standard_hero!(Ringo, "Ringo"),
    standard_hero!(Sanfeng, "Sanfeng"),
    standard_hero!(Sayoc, "Sayoc", "Taka"),
    standard_hero!(Shin, "Shin"),
    standard_hero!(Vox, "Vox"),
    standard_hero!(Warhawk, "Warhawk"),
    // 20 non-standard heroes from `docs/native-abilities.md`
    custom_hero!(
        Anka,
        "Anka",
        (0, 2, 3),
        None,
        [
            (0, "Ability__Anka__A", Primary(A)),
            (1, "Ability__Anka__A2", SubAbility(A)),
            (2, "Ability__Anka__B", Primary(B)),
            (3, "Ability__Anka__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Baron,
        "Baron",
        (1, 2, 3),
        None,
        [
            (0, "Ability__Baron__QuickAttack", Auxiliary),
            (1, "Ability__Baron__A", Primary(A)),
            (2, "Ability__Baron__B", Primary(B)),
            (3, "Ability__Baron__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Catherine,
        "Catherine",
        (1, 2, 3),
        None,
        [
            (0, "Ability__Catherine__A_DefaultAttack", Auxiliary),
            (1, "Ability__Catherine__A", Primary(A)),
            (2, "Ability__Catherine__B", Primary(B)),
            (3, "Ability__Catherine__C", Primary(C)),
            (4, "Ability__Catherine__B_ReflectDamage", SubAbility(B)),
        ]
    ),
    custom_hero!(
        Glaive,
        "Glaive",
        (0, 2, 3),
        None,
        [
            (0, "Ability__Glaive__A", Primary(A)),
            (1, "Ability__Glaive__A_KnockbackDefAttack", SubAbility(A)),
            (2, "Ability__Glaive__B", Primary(B)),
            (3, "Ability__Glaive__C", Primary(C)),
            (
                4,
                "Ability__Glaive__A_Talent_AfterburnAnyDistance",
                SubAbility(A)
            ),
        ]
    ),
    custom_hero!(
        Grumpjaw,
        "Grumpjaw",
        (1, 2, 3),
        None,
        [
            (0, "Ability__Grumpjaw__B_FrenzyAttack", SubAbility(B)),
            (1, "Ability__Grumpjaw__A", Primary(A)),
            (2, "Ability__Grumpjaw__B", Primary(B)),
            (3, "Ability__Grumpjaw__C", Primary(C)),
            (4, "Ability__Grumpjaw__Eat", Auxiliary),
            (5, "Ability__Grumpjaw__C_SpitOut", SubAbility(C)),
        ]
    ),
    custom_hero!(
        Gwen,
        "Gwen",
        (1, 2, 3),
        None,
        [
            (0, "Ability__Gwen__PerkAttack", Auxiliary),
            (1, "Ability__Gwen__A", Primary(A)),
            (2, "Ability__Gwen__B", Primary(B)),
            (3, "Ability__Gwen__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Hero057,
        "Hero057",
        (0, 1, 3),
        None,
        [
            (0, "Ability__Hero057__A", Primary(A)),
            (1, "Ability__Hero057__B", Primary(B)),
            (2, "Ability__Hero057__B_LeapOff", SubAbility(B)),
            (3, "Ability__Hero057__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Idris,
        "Idris",
        (0, 3, 4),
        None,
        [
            (0, "Ability__Idris__A", Primary(A)),
            (1, "Ability__Idris__A_Dash", SubAbility(A)),
            (2, "Ability__Idris__A_Blink", SubAbility(A)),
            (3, "Ability__Idris__B", Primary(B)),
            (4, "Ability__Idris__C", Primary(C)),
            (5, "Ability__Idris__C_LeapOff", SubAbility(C)),
        ]
    ),
    custom_hero!(
        Lyra,
        "Lyra",
        (0, 2, 3),
        None,
        [
            (0, "Ability__Lyra__A", Primary(A)),
            (1, "Ability__Lyra__A_Detonate", SubAbility(A)),
            (2, "Ability__Lyra__B", Primary(B)),
            (3, "Ability__Lyra__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Malene,
        "Malene",
        (0, 2, 4),
        Some([1, 3, 5]),
        [
            (0, "Ability__Malene__A1", Primary(A)),
            (1, "Ability__Malene__A2", DualForm(A)),
            (2, "Ability__Malene__B1", Primary(B)),
            (3, "Ability__Malene__B2", DualForm(B)),
            (4, "Ability__Malene__C1", Primary(C)),
            (5, "Ability__Malene__C2", DualForm(C)),
        ]
    ),
    custom_hero!(
        Ozo,
        "Ozo",
        (0, 3, 8),
        None,
        [
            (0, "Ability__Ozo__A1", Primary(A)),
            (1, "Ability__Ozo__A2", SubAbility(A)),
            (2, "Ability__Ozo__A3", SubAbility(A)),
            (3, "Ability__Ozo__B", Primary(B)),
            (4, "Ability__Ozo__B_QueueBounce", SubAbility(B)),
            (5, "Ability__Ozo__B_IntermediateBounce", SubAbility(B)),
            (6, "Ability__Ozo__B_FinalBounce", SubAbility(B)),
            (7, "Ability__Ozo__B_EndingAttack", SubAbility(B)),
            (8, "Ability__Ozo__C", Primary(C)),
            (9, "Ability__Ozo__C_Finisher", SubAbility(C)),
        ]
    ),
    custom_hero!(
        Samuel,
        "Samuel",
        (1, 3, 4),
        None,
        [
            (0, "Ability__Samuel__EmpoweredAttack", Auxiliary),
            (1, "Ability__Samuel__A", Primary(A)),
            (2, "Ability__Samuel__A_Empowered", SubAbility(A)),
            (3, "Ability__Samuel__B", Primary(B)),
            (4, "Ability__Samuel__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Saw,
        "SAW",
        (1, 2, 3),
        None,
        [
            (0, "Ability__SAW__Shank", Auxiliary),
            (1, "Ability__SAW__A", Primary(A)),
            (2, "Ability__SAW__B", Primary(B)),
            (3, "Ability__SAW__C", Primary(C)),
            (4, "Ability__SAW__B_Cancel", SubAbility(B)),
        ]
    ),
    custom_hero!(
        Silvernail,
        "Silvernail",
        (2, 3, 4),
        None,
        [
            (0, "Ability__Silvernail__DefaultFastAttack", Auxiliary),
            (1, "Ability__Silvernail__CritFastAttack", Auxiliary),
            (2, "Ability__Silvernail__A", Primary(A)),
            (3, "Ability__Silvernail__B", Primary(B)),
            (4, "Ability__Silvernail__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Skye,
        "Skye",
        (0, 2, 4),
        None,
        [
            (0, "Ability__Skye__A", Primary(A)),
            (1, "Ability__Skye__A_Cancel", SubAbility(A)),
            (2, "Ability__Skye__B", Primary(B)),
            (3, "Ability__AlwaysDisabled", Auxiliary),
            (4, "Ability__Skye__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Tony,
        "Tony",
        (0, 4, 5),
        None,
        [
            (0, "Ability__Tony__A", Primary(A)),
            (1, "Ability__Tony__A1", SubAbility(A)),
            (2, "Ability__Tony__A2", SubAbility(A)),
            (3, "Ability__Tony__A3", SubAbility(A)),
            (4, "Ability__Tony__B", Primary(B)),
            (5, "Ability__Tony__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Varya,
        "Varya",
        (0, 4, 8),
        None,
        [
            (0, "Ability__Varya__A", Primary(A)),
            (1, "Ability__Varya__A_Default", SubAbility(A)),
            (2, "Ability__Varya__A_Quick", SubAbility(A)),
            (3, "Ability__Varya__A_Queue", SubAbility(A)),
            (4, "Ability__Varya__B", Primary(B)),
            (5, "Ability__Varya__B_Move", SubAbility(B)),
            (6, "Ability__Varya__B_Move_Default", SubAbility(B)),
            (7, "Ability__Varya__B_Move_Charged", SubAbility(B)),
            (8, "Ability__Varya__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Viola,
        "Viola",
        (0, 1, 3),
        None,
        [
            (0, "Ability__Viola__A", Primary(A)),
            (1, "Ability__Viola__B", Primary(B)),
            (2, "Ability__Viola__C0", SubAbility(C)),
            (3, "Ability__Viola__C", Primary(C)),
            (4, "Ability__Viola__C2", SubAbility(C)),
            (5, "Ability__Viola__C3", SubAbility(C)),
            (6, "Ability__Viola__C4", SubAbility(C)),
        ]
    ),
    custom_hero!(
        Yates,
        "Yates",
        (0, 2, 3),
        None,
        [
            (0, "Ability__Yates__A", Primary(A)),
            (1, "Ability__Yates__A_Recoil", SubAbility(A)),
            (2, "Ability__Yates__B", Primary(B)),
            (3, "Ability__Yates__C", Primary(C)),
        ]
    ),
    custom_hero!(
        Ylva,
        "Ylva",
        (0, 2, 3),
        None,
        [
            (0, "Ability__Ylva__A", Primary(A)),
            (1, "Ability__Ylva__A_Cancel", SubAbility(A)),
            (2, "Ability__Ylva__B", Primary(B)),
            (3, "Ability__Ylva__C", Primary(C)),
            (4, "Ability__Ylva__Talent_ShatteredFang", Auxiliary),
        ]
    ),
];

/// Returns all 61 hero CFF0 `AbilitySet` slot layouts.
pub const fn all_hero_ability_slots() -> &'static [HeroAbilitySlots] {
    &HERO_ABILITY_SLOTS
}

/// Looks up the CFF0 `AbilitySet` slot table for a hero [`EntityTemplate`].
/// Returns `None` for non-hero templates (minions, structures, jungle camps).
pub const fn lookup_hero_ability_slots(template: EntityTemplate) -> Option<HeroAbilitySlots> {
    let idx = match template {
        EntityTemplate::Adagio => 0,
        EntityTemplate::Alpha => 1,
        EntityTemplate::Amael => 2,
        EntityTemplate::Ardan => 3,
        EntityTemplate::Baptiste => 4,
        EntityTemplate::Blackfeather => 5,
        EntityTemplate::Caine => 6,
        EntityTemplate::Celeste => 7,
        EntityTemplate::Churnwalker => 8,
        EntityTemplate::Flicker => 9,
        EntityTemplate::Fortress => 10,
        EntityTemplate::Grace => 11,
        EntityTemplate::Hero009 => 12,
        EntityTemplate::Skaarf => 13,
        EntityTemplate::Rona => 14,
        EntityTemplate::Hero062 => 15,
        EntityTemplate::Hero999 => 16,
        EntityTemplate::Inara => 17,
        EntityTemplate::Ishtar => 18,
        EntityTemplate::Joule => 19,
        EntityTemplate::Karas => 20,
        EntityTemplate::Kensei => 21,
        EntityTemplate::Kestrel => 22,
        EntityTemplate::Kinetic => 23,
        EntityTemplate::Koshka => 24,
        EntityTemplate::Lance => 25,
        EntityTemplate::LanceBallLance => 26,
        EntityTemplate::Leo => 27,
        EntityTemplate::Lorelai => 28,
        EntityTemplate::Magnus => 29,
        EntityTemplate::Miho => 30,
        EntityTemplate::Petal => 31,
        EntityTemplate::Phinn => 32,
        EntityTemplate::Reim => 33,
        EntityTemplate::Reza => 34,
        EntityTemplate::Ringo => 35,
        EntityTemplate::Sanfeng => 36,
        EntityTemplate::Sayoc => 37,
        EntityTemplate::Shin => 38,
        EntityTemplate::Vox => 39,
        EntityTemplate::Warhawk => 40,
        EntityTemplate::Anka => 41,
        EntityTemplate::Baron => 42,
        EntityTemplate::Catherine => 43,
        EntityTemplate::Glaive => 44,
        EntityTemplate::Grumpjaw => 45,
        EntityTemplate::Gwen => 46,
        EntityTemplate::Hero057 => 47,
        EntityTemplate::Idris => 48,
        EntityTemplate::Lyra => 49,
        EntityTemplate::Malene => 50,
        EntityTemplate::Ozo => 51,
        EntityTemplate::Samuel => 52,
        EntityTemplate::Saw => 53,
        EntityTemplate::Silvernail => 54,
        EntityTemplate::Skye => 55,
        EntityTemplate::Tony => 56,
        EntityTemplate::Varya => 57,
        EntityTemplate::Viola => 58,
        EntityTemplate::Yates => 59,
        EntityTemplate::Ylva => 60,
        _ => return None,
    };
    Some(HERO_ABILITY_SLOTS[idx])
}

/// Looks up the CFF0 `AbilitySet` slot table for a raw numeric entity template ID.
pub fn lookup_hero_ability_slots_by_raw(type_id: u16) -> Option<HeroAbilitySlots> {
    EntityTemplate::from_raw(type_id).and_then(lookup_hero_ability_slots)
}

/// Looks up the CFF0 `AbilitySet` slot table by exact manifest / `abilities.json` `hero_id`.
pub fn lookup_hero_ability_slots_by_id(hero_id: &str) -> Option<HeroAbilitySlots> {
    EntityTemplate::from_hero_id(hero_id).and_then(lookup_hero_ability_slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn covers_exact_61_heroes_with_41_standard_and_20_non_standard() {
        let all = all_hero_ability_slots();
        assert_eq!(all.len(), 61);
        assert_eq!(all.iter().filter(|h| h.is_standard()).count(), 41);
        assert_eq!(all.iter().filter(|h| !h.is_standard()).count(), 20);

        let mut templates = BTreeSet::new();
        let mut hero_ids = BTreeSet::new();
        for slots in all {
            assert!(templates.insert(slots.template.as_raw()));
            assert!(hero_ids.insert(slots.hero_id));
            assert_eq!(
                lookup_hero_ability_slots(slots.template),
                Some(*slots),
                "{}",
                slots.hero_id
            );
            assert_eq!(
                lookup_hero_ability_slots_by_id(slots.hero_id),
                Some(*slots),
                "{}",
                slots.hero_id
            );
            assert_eq!(slots.template.hero_id(), Some(slots.hero_id));
            assert_eq!(
                EntityTemplate::from_hero_id(slots.hero_id),
                Some(slots.template)
            );
            for (i, entry) in slots.entries.iter().enumerate() {
                assert_eq!(usize::from(entry.index), i, "{}", slots.hero_id);
            }
            for slot in AbilitySlot::ALL {
                let idx = slots.index(slot);
                let entry = slots.entry(idx).expect("primary slot index in bounds");
                assert_eq!(entry.kind, AbilityEntryKind::Primary(slot));
                assert_eq!(slots.primary_slot_for_index(idx), Some(slot));
                assert_eq!(
                    slots.slot_for_index(idx),
                    Some((slot, AbilityForm::Primary))
                );
                assert_eq!(slots.index_of(entry.ability_id), Some(idx));
            }
            if let Some(alt) = slots.dual_form {
                for (i, slot) in AbilitySlot::ALL.into_iter().enumerate() {
                    let idx = alt[i];
                    let entry = slots.entry(idx).expect("dual-form index in bounds");
                    assert_eq!(entry.kind, AbilityEntryKind::DualForm(slot));
                    assert_eq!(
                        slots.slot_for_index(idx),
                        Some((slot, AbilityForm::Alternate))
                    );
                }
            }
        }
        assert!(lookup_hero_ability_slots(EntityTemplate::Turret).is_none());
        assert!(lookup_hero_ability_slots(EntityTemplate::MinionMelee5v5).is_none());
    }

    #[test]
    fn non_standard_hero_indices_match_documented_cff0_table() {
        let check = |t, a, b, c| {
            let s = lookup_hero_ability_slots(t).unwrap();
            assert!(!s.is_standard());
            assert_eq!((s.a, s.b, s.c), (a, b, c), "{t:?}");
        };
        check(EntityTemplate::Anka, 0, 2, 3);
        check(EntityTemplate::Baron, 1, 2, 3);
        check(EntityTemplate::Catherine, 1, 2, 3);
        check(EntityTemplate::Glaive, 0, 2, 3);
        check(EntityTemplate::Grumpjaw, 1, 2, 3);
        check(EntityTemplate::Gwen, 1, 2, 3);
        check(EntityTemplate::Hero057, 0, 1, 3);
        check(EntityTemplate::Idris, 0, 3, 4);
        check(EntityTemplate::Lyra, 0, 2, 3);
        check(EntityTemplate::Ozo, 0, 3, 8);
        check(EntityTemplate::Samuel, 1, 3, 4);
        check(EntityTemplate::Saw, 1, 2, 3);
        check(EntityTemplate::Silvernail, 2, 3, 4);
        check(EntityTemplate::Skye, 0, 2, 4);
        check(EntityTemplate::Tony, 0, 4, 5);
        check(EntityTemplate::Varya, 0, 4, 8);
        check(EntityTemplate::Viola, 0, 1, 3);
        check(EntityTemplate::Yates, 0, 2, 3);
        check(EntityTemplate::Ylva, 0, 2, 3);

        let malene = lookup_hero_ability_slots(EntityTemplate::Malene).unwrap();
        assert!(malene.is_dual_form());
        assert_eq!((malene.a, malene.b, malene.c), (0, 2, 4));
        assert_eq!(malene.dual_form, Some([1, 3, 5]));
        assert_eq!(
            malene.ability_id_for_form(AbilitySlot::A, AbilityForm::Primary),
            Some("Ability__Malene__A1")
        );
        assert_eq!(
            malene.ability_id_for_form(AbilitySlot::A, AbilityForm::Alternate),
            Some("Ability__Malene__A2")
        );
        assert_eq!(
            malene.ability_id_for_form(AbilitySlot::C, AbilityForm::Alternate),
            Some("Ability__Malene__C2")
        );
    }

    #[test]
    #[ignore = "requires the delivery and server-data supplement"]
    fn ability_slot_table_matches_supplement_abilities_json_for_all_61_heroes() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::env::var_os("HALCYON_DATA_EXTRA").unwrap();
        let data = crate::GameData::load_with_extra(base, Some(Path::new(&extra))).unwrap();
        assert_eq!(data.combat_content().heroes().len(), 61);
        for slots in all_hero_ability_slots() {
            assert_eq!(
                data.type_id(slots.hero_id),
                Some(slots.template.as_raw()),
                "{}",
                slots.hero_id
            );
            let hero = data
                .hero_abilities_for(slots.template)
                .unwrap_or_else(|| panic!("missing hero_abilities for {}", slots.hero_id));
            for entry in slots.entries {
                assert_eq!(
                    hero.abilities[usize::from(entry.index)].ability_id,
                    entry.ability_id,
                    "{} slot {}",
                    slots.hero_id,
                    entry.index
                );
            }
            for slot in AbilitySlot::ALL {
                let ability = data
                    .ability_for_slot(slots.template, slot)
                    .unwrap_or_else(|| panic!("missing slot {slot:?} for {}", slots.hero_id));
                assert_eq!(ability.ability_id, slots.ability_id(slot));
            }
        }
    }
}
