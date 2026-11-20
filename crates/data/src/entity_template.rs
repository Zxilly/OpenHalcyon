//! Named entity IDs used by the native scenes and authoritative simulation.
//! The manifest and entity storage accept arbitrary template IDs.
macro_rules! entity_templates {
    ($($name:ident = $id:literal),+ $(,)?) => {
        #[repr(u16)]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum EntityTemplate {
            $($name = $id),+
        }

        impl EntityTemplate {
            pub const fn from_raw(id: u16) -> Option<Self> {
                match id {
                    $($id => Some(Self::$name),)+
                    _ => None,
                }
            }

            pub const fn as_raw(self) -> u16 {
                self as u16
            }
        }
    };
}

entity_templates! {
    Ringo = 243,
    Adagio = 244,
    Saw = 250,
    Hero009 = 254,
    Reim = 266,
    Lyra = 269,
    Idris = 273,
    RangedMinion = 287,
    LeadMinion = 288,
    TankMinion = 289,
    VainCrystalHome3v3 = 292,
    VainCrystalAway3v3 = 293,
    VainCrystalHome5v5 = 295,
    VainCrystalAway5v5 = 296,
    VainNode = 297,
    VisionBeacon = 298,
    Turret = 300,
    VainTurret = 301,
    NeutralJungleMinionStore = 314,
    HealingTreant = 323,
    GoldTreant = 324,
    WeaponTreant = 325,
    CrystalTreant = 326,
    BigBear = 327,
    SmallBear = 328,
    MinionRange5v5 = 335,
    MinionMelee5v5 = 336,
    MinionSiege5v5 = 337,
    MinionCaptain5v5 = 338,
    Turret5v5 = 339,
    OuterTurret5v5 = 340,
    ArmoryTurret5v5 = 341,
    Lorelai = 409,
    Yates = 439,
    Karas = 926,
    Shin = 927,
}

impl EntityTemplate {
    pub const fn is_solo_camp_unit(self) -> bool {
        matches!(
            self,
            Self::HealingTreant
                | Self::GoldTreant
                | Self::WeaponTreant
                | Self::CrystalTreant
                | Self::BigBear
                | Self::SmallBear
        )
    }
    pub const fn is_aral_lane_minion(self) -> bool {
        matches!(
            self,
            Self::RangedMinion | Self::LeadMinion | Self::TankMinion
        )
    }

    pub const fn is_solo_lane_minion(self) -> bool {
        matches!(
            self,
            Self::MinionRange5v5
                | Self::MinionMelee5v5
                | Self::MinionSiege5v5
                | Self::MinionCaptain5v5
        )
    }
}

impl TryFrom<u16> for EntityTemplate {
    type Error = u16;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::from_raw(value).ok_or(value)
    }
}

#[cfg(test)]
mod tests {
    use super::EntityTemplate;

    #[test]
    fn known_and_custom_ids_preserve_the_original_word() {
        for id in u16::MIN..=u16::MAX {
            match EntityTemplate::try_from(id) {
                Ok(template) => assert_eq!(template.as_raw(), id),
                Err(raw) => assert_eq!(raw, id),
            }
        }
        assert_eq!(EntityTemplate::from_raw(243), Some(EntityTemplate::Ringo));
        assert_eq!(EntityTemplate::from_raw(65535), None);
    }
}
