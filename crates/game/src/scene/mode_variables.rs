use super::Message;

/// Client-registered `ModifyGameModeVar` (`0x041F`) keys from `libGameKindred.so` (`0xc701b0`,
/// `0x7d1944`), hashed with `Jenkins::lookup2(..., 0x12345678)`.
pub(crate) const COUNTDOWN_START_KEY: u32 = 0xf53c_c2dd; // "__GAME_MODE_COUNTDOWN_START__"
pub(crate) const COUNTDOWN_DURATION_KEY: u32 = 0x4ccc_cfe4; // "__GAME_MODE_COUNTDOWN_DURATION__"
pub(crate) const COUNTDOWN_PAUSED_KEY: u32 = 0x4eb4_dfc1; // "__GAME_MODE_COUNTDOWN_PAUSED__"
pub(crate) const COUNTDOWN_DO_ROUNDING_KEY: u32 = 0x1f55_e987; // "__GAME_MODE_COUNTDOWN_DO_ROUNDING__"
pub(crate) const BOT_DIFFICULTY_KEY: u32 = 0x224f_f1a2; // "__GAME_MODE_BOT_DIFFICULTY__"
pub(crate) const TEAM_1_SCORE_KEY: u32 = 0x7c9d_df52; // "Team1Score"
pub(crate) const TEAM_2_SCORE_KEY: u32 = 0xe674_f03d; // "Team2Score"

/// Complete set of 2-team `GameModeVar` keys registered in the client's `[0x302edf0]` table.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const CLIENT_REGISTERED_KEYS: [u32; 7] = [
    COUNTDOWN_START_KEY,
    COUNTDOWN_DURATION_KEY,
    COUNTDOWN_PAUSED_KEY,
    COUNTDOWN_DO_ROUNDING_KEY,
    BOT_DIFFICULTY_KEY,
    TEAM_1_SCORE_KEY,
    TEAM_2_SCORE_KEY,
];

const KEYS: [u32; 6] = [
    COUNTDOWN_START_KEY,
    COUNTDOWN_DURATION_KEY,
    COUNTDOWN_PAUSED_KEY,
    BOT_DIFFICULTY_KEY,
    TEAM_1_SCORE_KEY,
    TEAM_2_SCORE_KEY,
];
const MODE_KEY: u32 = BOT_DIFFICULTY_KEY;

pub(crate) enum Opening {
    Solo,
    HalcyonFold,
    Blitz,
    Aral,
}

impl Opening {
    pub(crate) fn messages(self) -> impl Iterator<Item = Message> {
        let mode_word: u32 = match self {
            Self::Solo => 1,
            Self::HalcyonFold => 2,
            Self::Blitz => 3,
            Self::Aral => 5,
        };
        KEYS.into_iter().map(move |key| {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&key.to_be_bytes());
            if key == MODE_KEY {
                body[4..8].copy_from_slice(&mode_word.to_be_bytes());
            }
            Message::known(crate::Opcode::ModifyGameModeVar, body)
        })
    }
}

pub(crate) fn recordable(body: &[u8]) -> bool {
    let Ok(bytes) = <&[u8; 14]>::try_from(body) else {
        return false;
    };
    let key = u32::from_be_bytes(bytes[..4].try_into().unwrap());
    let value = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    bytes[8..] == [0; 6]
        && KEYS.contains(&key)
        && if key == MODE_KEY {
            matches!(value, 1 | 5)
        } else {
            value == 0
        }
}
