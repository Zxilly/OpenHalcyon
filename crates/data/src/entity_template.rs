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
    Catherine = 242,
    Ringo = 243,
    Adagio = 244,
    Koshka = 245,
    Petal = 246,
    Glaive = 249,
    Saw = 250,
    Joule = 253,
    Hero009 = 254,
    Skaarf = 255,
    // Internal manifest and CFF0 ID for Taka.
    Sayoc = 256,
    Ardan = 257,
    Vox = 258,
    Fortress = 259,
    Rona = 260,
    Baron = 261,
    Skye = 265,
    Reim = 266,
    Kestrel = 267,
    Alpha = 268,
    Lyra = 269,
    Idris = 273,
    Ozo = 274,
    Lance = 275,
    Samuel = 276,
    Phinn = 279,
    Blackfeather = 280,
    Malene = 281,
    Celeste = 285,
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
    Flicker = 393,
    Gwen = 395,
    Grumpjaw = 396,
    Tony = 397,
    Baptiste = 399,
    Reza = 403,
    Grace = 406,
    Churnwalker = 408,
    Lorelai = 409,
    Kensei = 412,
    Varya = 413,
    Magnus = 418,
    Kinetic = 420,
    Anka = 429,
    Silvernail = 432,
    Hero057 = 435,
    Ylva = 436,
    Yates = 439,
    Inara = 440,
    LanceBallLance = 443,
    Sanfeng = 446,
    Hero062 = 912,
    Leo = 913,
    Hero999 = 914,
    Caine = 915,
    Warhawk = 916,
    Miho = 919,
    Ishtar = 922,
    Viola = 924,
    Amael = 925,
    Karas = 926,
    Shin = 927,
}

impl EntityTemplate {
    /// Display alias for [`Self::Hero009`].
    pub const KRUL: Self = Self::Hero009;
    /// Display alias for [`Self::Sayoc`].
    pub const TAKA: Self = Self::Sayoc;

    /// Returns the exact manifest / `abilities.json` `hero_id` for all 61 hero templates.
    pub const fn hero_id(self) -> Option<&'static str> {
        Some(match self {
            Self::Catherine => "Catherine",
            Self::Ringo => "Ringo",
            Self::Adagio => "Adagio",
            Self::Koshka => "Koshka",
            Self::Petal => "Petal",
            Self::Glaive => "Glaive",
            Self::Saw => "SAW",
            Self::Joule => "Joule",
            Self::Hero009 => "Hero009",
            Self::Skaarf => "Hero010",
            Self::Sayoc => "Sayoc",
            Self::Ardan => "Ardan",
            Self::Vox => "Vox",
            Self::Fortress => "Fortress",
            Self::Rona => "Hero016",
            Self::Baron => "Baron",
            Self::Skye => "Skye",
            Self::Reim => "Reim",
            Self::Kestrel => "Kestrel",
            Self::Alpha => "Alpha",
            Self::Lyra => "Lyra",
            Self::Idris => "Idris",
            Self::Ozo => "Ozo",
            Self::Lance => "Lance",
            Self::Samuel => "Samuel",
            Self::Phinn => "Phinn",
            Self::Blackfeather => "Blackfeather",
            Self::Malene => "Malene",
            Self::Celeste => "Celeste",
            Self::Flicker => "Flicker",
            Self::Gwen => "Gwen",
            Self::Grumpjaw => "Grumpjaw",
            Self::Tony => "Tony",
            Self::Baptiste => "Baptiste",
            Self::Reza => "Reza",
            Self::Grace => "Grace",
            Self::Churnwalker => "Churnwalker",
            Self::Lorelai => "Lorelai",
            Self::Kensei => "Kensei",
            Self::Varya => "Varya",
            Self::Magnus => "Magnus",
            Self::Kinetic => "Kinetic",
            Self::Anka => "Anka",
            Self::Silvernail => "Silvernail",
            Self::Hero057 => "Hero057",
            Self::Ylva => "Ylva",
            Self::Yates => "Yates",
            Self::Inara => "Inara",
            Self::LanceBallLance => "LanceBall_Lance",
            Self::Sanfeng => "Sanfeng",
            Self::Hero062 => "Hero062",
            Self::Leo => "Leo",
            Self::Hero999 => "Hero999",
            Self::Caine => "Caine",
            Self::Warhawk => "Warhawk",
            Self::Miho => "Miho",
            Self::Ishtar => "Ishtar",
            Self::Viola => "Viola",
            Self::Amael => "Amael",
            Self::Karas => "Karas",
            Self::Shin => "Shin",
            _ => return None,
        })
    }

    /// Resolves an exact manifest / `abilities.json` `hero_id` (such as `"Hero009"`, `"SAW"`, or
    /// `"Sayoc"`) to its [`EntityTemplate`].
    pub fn from_hero_id(hero_id: &str) -> Option<Self> {
        Some(match hero_id {
            "Catherine" => Self::Catherine,
            "Ringo" => Self::Ringo,
            "Adagio" => Self::Adagio,
            "Koshka" => Self::Koshka,
            "Petal" => Self::Petal,
            "Glaive" => Self::Glaive,
            "SAW" => Self::Saw,
            "Joule" => Self::Joule,
            "Hero009" => Self::Hero009,
            "Hero010" => Self::Skaarf,
            "Sayoc" => Self::Sayoc,
            "Ardan" => Self::Ardan,
            "Vox" => Self::Vox,
            "Fortress" => Self::Fortress,
            "Hero016" => Self::Rona,
            "Baron" => Self::Baron,
            "Skye" => Self::Skye,
            "Reim" => Self::Reim,
            "Kestrel" => Self::Kestrel,
            "Alpha" => Self::Alpha,
            "Lyra" => Self::Lyra,
            "Idris" => Self::Idris,
            "Ozo" => Self::Ozo,
            "Lance" => Self::Lance,
            "Samuel" => Self::Samuel,
            "Phinn" => Self::Phinn,
            "Blackfeather" => Self::Blackfeather,
            "Malene" => Self::Malene,
            "Celeste" => Self::Celeste,
            "Flicker" => Self::Flicker,
            "Gwen" => Self::Gwen,
            "Grumpjaw" => Self::Grumpjaw,
            "Tony" => Self::Tony,
            "Baptiste" => Self::Baptiste,
            "Reza" => Self::Reza,
            "Grace" => Self::Grace,
            "Churnwalker" => Self::Churnwalker,
            "Lorelai" => Self::Lorelai,
            "Kensei" => Self::Kensei,
            "Varya" => Self::Varya,
            "Magnus" => Self::Magnus,
            "Kinetic" => Self::Kinetic,
            "Anka" => Self::Anka,
            "Silvernail" => Self::Silvernail,
            "Hero057" => Self::Hero057,
            "Ylva" => Self::Ylva,
            "Yates" => Self::Yates,
            "Inara" => Self::Inara,
            "LanceBall_Lance" => Self::LanceBallLance,
            "Sanfeng" => Self::Sanfeng,
            "Hero062" => Self::Hero062,
            "Leo" => Self::Leo,
            "Hero999" => Self::Hero999,
            "Caine" => Self::Caine,
            "Warhawk" => Self::Warhawk,
            "Miho" => Self::Miho,
            "Ishtar" => Self::Ishtar,
            "Viola" => Self::Viola,
            "Amael" => Self::Amael,
            "Karas" => Self::Karas,
            "Shin" => Self::Shin,
            _ => return None,
        })
    }

    pub const fn is_hero(self) -> bool {
        self.hero_id().is_some()
    }

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
