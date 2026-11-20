use super::Message;

const KEYS: [u32; 6] = [
    0xf53c_c2dd,
    0x4ccc_cfe4,
    0x4eb4_dfc1,
    0x224f_f1a2,
    0x7c9d_df52,
    0xe674_f03d,
];
const MODE_KEY: u32 = 0x224f_f1a2;

pub(crate) enum Opening {
    Solo,
    Aral,
}

impl Opening {
    pub(crate) fn messages(self) -> impl Iterator<Item = Message> {
        let mode_word: u32 = match self {
            Self::Solo => 1,
            Self::Aral => 5,
        };
        KEYS.into_iter().map(move |key| {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&key.to_be_bytes());
            if key == MODE_KEY {
                body[4..8].copy_from_slice(&mode_word.to_be_bytes());
            }
            Message::new(0x041f, body)
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
