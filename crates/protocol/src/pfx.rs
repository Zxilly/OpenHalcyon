use crate::{Message, Opcode, fixed_body};
use binrw::{BinRead, BinWrite};

const PFX_PREFIX_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct PlayPfxWire {
    effect_key: u32,
    actor: u32,
    position: [f32; 3],
    unknown_word: u32,
}

/// ActionPlayPfxAtLocation (0419), including the optional decrypted frame padding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayPfxAtLocation {
    pub effect_key: u32,
    pub actor: u32,
    pub position: [f32; 3],
    pub unknown_word: u32,
    pub padding: Option<[u8; 6]>,
}

impl Message {
    pub fn play_pfx_at_location(&self) -> Option<PlayPfxAtLocation> {
        if !self.is(Opcode::PlayPfxAtLocation) {
            return None;
        }
        let (wire, tail): (PlayPfxWire, _) =
            fixed_body::read_with_padding(&self.body, PFX_PREFIX_LEN).ok()?;
        let padding = match tail {
            [] => None,
            tail => Some(tail.try_into().ok()?),
        };
        Some(PlayPfxAtLocation {
            effect_key: wire.effect_key,
            actor: wire.actor,
            position: wire.position,
            unknown_word: wire.unknown_word,
            padding,
        })
    }
}

impl PlayPfxAtLocation {
    pub fn to_message(&self) -> Message {
        let wire = PlayPfxWire {
            effect_key: self.effect_key,
            actor: self.actor,
            position: self.position,
            unknown_word: self.unknown_word,
        };
        let suffix = self.padding.as_ref().map_or(&[][..], |pad| &pad[..]);
        fixed_body::message_with_padding(Opcode::PlayPfxAtLocation, &wire, PFX_PREFIX_LEN, suffix)
            .expect("pfx padding is 0 or 6 bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fields;

    #[test]
    fn preserves_arbitrary_keys_float_bits_and_padding() {
        let words = [
            0x12345678u32,
            1505,
            0x80000000,
            0x7fc01234,
            0xff800000,
            0xdeadbeef,
        ];
        let body: Vec<_> = words.into_iter().flat_map(u32::to_be_bytes).collect();
        for tail in [&[][..], &[6, 5, 4, 3, 2, 1]] {
            let message = Message::known(
                crate::Opcode::PlayPfxAtLocation,
                [body.as_slice(), tail].concat(),
            );
            let effect = message.play_pfx_at_location().unwrap();
            assert_eq!(effect.position.map(f32::to_bits), words[2..5]);
            assert_eq!(effect.unknown_word, words[5]);
            assert_eq!(effect.to_message(), message);
            assert!(matches!(message.fields(), Ok(Fields::ActionWords(_))));
        }
        for size in [0, 23, 25, 29, 31] {
            assert!(
                Message::known(crate::Opcode::PlayPfxAtLocation, vec![0; size])
                    .play_pfx_at_location()
                    .is_none()
            );
        }
        assert!(
            Message::known(crate::Opcode::OverrideAbility, body)
                .play_pfx_at_location()
                .is_none()
        );
    }
}
