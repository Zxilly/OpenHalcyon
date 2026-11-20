use super::{ModeSet, Opening, Preset::*, Row, row};
use halcyon_data::EntityTemplate;

const TAUNT: Row = row("Ability__Emote_Taunt", EmoteFive);
const DANCE: Row = row("Ability__Emote_Dance", EmoteHalf);
const WITHDRAW: Row = row("Ability__Withdraw", EmoteHalf);
// Appears in a 3-entry static table at 0x1ad11d4 in libGameKindred.so (0x377db359, 0xd60c580b, 0xd1f4faf2); string name remains unresolved in OBB CFF0.
const SHARED: u32 = 0xd60c580b;
const HALF: Row = Row::raw(SHARED, SharedHalf);
const SIX_TENTHS: Row = Row::raw(SHARED, SharedSixTenths);
const SEVEN_TENTHS: Row = Row::raw(SHARED, SharedSevenTenths);
const EIGHT_TENTHS: Row = Row::raw(SHARED, SharedEightTenths);

macro_rules! regular {
    ($hero:literal, $taunt:expr, $withdraw:expr, $shared:expr) => {
        &[
            $taunt,
            DANCE,
            $withdraw,
            row(concat!("Ability__", $hero, "__C"), Ultimate),
            row(concat!("Ability__", $hero, "__B"), Ability),
            row(concat!("Ability__", $hero, "__A"), Ability),
            $shared,
        ]
    };
}

macro_rules! openings {
    ($($template:ident, $modes:ident => $rows:expr,)+) => {
        pub(super) const ALL: &[Opening] = &[$(
            Opening { template: EntityTemplate::$template, modes: ModeSet::$modes, rows: $rows },
        )+];
    };
}

openings! {
    Ringo, Aral => regular!("Ringo", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Petal, Aral => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Petal_C_RecastDelay", Internal),
        row("Ability__Petal__C", Ultimate),
        row("Ability__Petal__B", Ability),
        row("Ability__Petal__A", Ability),
        SEVEN_TENTHS,
    ],
    Glaive, Aral => &[
        TAUNT,
        DANCE,
        row("Ability__Glaive__Withdraw", EmoteHalf),
        row("Ability__Glaive__A_Talent_AfterburnAnyDistance", Ability),
        row("Ability__Glaive__C", Ultimate),
        row("Ability__Glaive__B", Ability),
        row("Ability__Glaive__A", Ability),
        SEVEN_TENTHS,
    ],
    Saw, Aral => &[
        TAUNT,
        DANCE,
        row("Ability__SAW__Withdraw", EmoteHalf),
        row("Ability__SAW__C", Ultimate),
        row("Ability__SAW__B", Ability),
        row("Ability__SAW__A", Ability),
        row("Ability__SAW__Shank", AlternateOne),
        SEVEN_TENTHS,
    ],
    Hero009, Aral => regular!("Krul", TAUNT, row("Ability__Krul__Withdraw", EmoteHalf), SIX_TENTHS),
    Skaarf, Aral => &[
        row("Ability__Skaarf__Emote_Taunt", EmoteFive),
        DANCE,
        WITHDRAW,
        row("Ability__Skaarf__Talent_FlashFire", Ultimate),
        row("Ability__Skaarf__C", Ultimate),
        row("Ability__Skaarf__B", Ability),
        row("Ability__Skaarf__A", Ability),
        SEVEN_TENTHS,
    ],
    Ardan, Both => regular!("Ardan", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Vox, Aral => regular!("Vox", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Rona, Aral => &[
        TAUNT,
        DANCE,
        row("Ability__Rona__Withdraw", EmoteHalf),
        row("CancelWhirlwind", Internal),
        row("Ability__Rona__C", Ultimate),
        row("Ability__Rona__B", Ability),
        row("Ability__Rona__A", Ability),
        HALF,
    ],
    Baron, Solo => &[
        TAUNT,
        DANCE,
        row("Ability__Baron__Withdraw", EmoteHalf),
        row("Baron_C_RecastDelay", Internal),
        row("Ability__Baron__C", Ultimate),
        row("Ability__Baron__B", Ability),
        row("Ability__Baron__A", Ability),
        EIGHT_TENTHS,
    ],
    Skye, Both => &[
        TAUNT,
        DANCE,
        row("Ability__Skye__Withdraw", EmoteHalf),
        row("Skye_C_RecastDelay", Internal),
        row("Ability__Skye__C", Ultimate),
        row("Ability__Skye__B", Ability),
        row("Ability__Skye__A", Ability),
        SEVEN_TENTHS,
    ],
    Reim, Aral => regular!("Reim", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Alpha, Both => &[
        TAUNT,
        DANCE,
        row("Ability__Alpha__Withdraw", EmoteHalf),
        row("Ability__Alpha__SelfDestruct", Ultimate),
        row("Reboot", Internal),
        row("Ability__Alpha__C", Ultimate),
        row("Ability__Alpha__B", Ability),
        row("Ability__Alpha__A", Ability),
        SEVEN_TENTHS,
    ],
    Lyra, Aral => regular!("Lyra", TAUNT, WITHDRAW, SIX_TENTHS),
    Idris, Both => regular!("Idris", TAUNT, row("Ability__Idris__Withdraw", EmoteHalf), SEVEN_TENTHS),
    Lance, Aral => regular!("Lance", TAUNT, row("Ability__Lance__Withdraw", EmoteHalf), EIGHT_TENTHS),
    Blackfeather, Aral => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Blackfeather_C_RecastDelay", Internal),
        row("Ability__Blackfeather__C", Ultimate),
        row("Ability__Blackfeather__B", Ability),
        row("Ability__Blackfeather__A", Ability),
        SEVEN_TENTHS,
    ],
    Celeste, Aral => regular!("Celeste", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Gwen, Aral => regular!("Gwen", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Tony, Solo => regular!("Tony", TAUNT, row("Ability__Tony__Withdraw", EmoteHalf), SEVEN_TENTHS),
    Baptiste, Aral => regular!("Baptiste", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Lorelai, Aral => &[
        row("Ability__Lorelai__Emote_Taunt", EmoteFive),
        DANCE,
        WITHDRAW,
        row("Ability__Lorelai__C", Ultimate),
        row("Lorelai_B_RecastDelay", AlternateZero),
        row("Ability__Lorelai__B", Ability),
        row("Ability__Lorelai__A", Ability),
        SEVEN_TENTHS,
    ],
    Varya, Both => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Ability__Varya__C", Ultimate),
        row("Ability__Varya__B_Move_Charged", MovementVariant),
        row("Ability__Varya__B_Move_Default", MovementVariant),
        row("Ability__Varya__B_Move", Ability),
        row("Ability__Varya__B", Ability),
        row("Ability__Varya__A_Queue", Ability),
        row("Ability__Varya__A", Ability),
        SIX_TENTHS,
    ],
    Magnus, Aral => regular!("Magnus", TAUNT, WITHDRAW, EIGHT_TENTHS),
    Kinetic, Aral => regular!("Kinetic", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Ylva, Aral => regular!("Ylva", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Yates, Aral => regular!("Yates", TAUNT, row("Ability__Yates__Withdraw", EmoteHalf), EIGHT_TENTHS),
    Inara, Both => regular!("Inara", TAUNT, WITHDRAW, SEVEN_TENTHS),
    Caine, Aral => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Ability__Caine__NoAmmo", AlternateOne),
        row("Ability__Caine__C", Ultimate),
        row("Ability__Caine__B", Ability),
        row("Ability__Caine__A", Ability),
        SEVEN_TENTHS,
    ],
    Amael, Both => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Ability__Amael__A_Cancel", Ability),
        row("Ability__Amael__C", Ultimate),
        row("Ability__Amael__B", Ability),
        row("Ability__Amael__A", Ability),
        SEVEN_TENTHS,
    ],
    Karas, Aral => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Ability__Karas__A_Cancel", Ability),
        row("Ability__Karas__C", Ultimate),
        row("Ability__Karas__B", Ability),
        row("Ability__Karas__A", Ability),
        SEVEN_TENTHS,
    ],
    Shin, Aral => &[
        TAUNT,
        DANCE,
        WITHDRAW,
        row("Ability__Shin__C1", Ultimate),
        row("Shin_C", Ultimate),
        row("Shin_B", Ability),
        row("Shin_A", Ability),
        SEVEN_TENTHS,
    ],
}
