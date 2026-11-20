use crate::Message;

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
        if !self.is(crate::Opcode::PlayPfxAtLocation) {
            return None;
        }
        let (fields, tail) = self.body.split_at_checked(24)?;
        let padding = match tail {
            [] => None,
            tail => Some(tail.try_into().ok()?),
        };
        let words = fields.as_chunks::<4>().0;
        Some(PlayPfxAtLocation {
            effect_key: u32::from_be_bytes(words[0]),
            actor: u32::from_be_bytes(words[1]),
            position: std::array::from_fn(|i| f32::from_be_bytes(words[i + 2])),
            unknown_word: u32::from_be_bytes(words[5]),
            padding,
        })
    }
}

impl PlayPfxAtLocation {
    pub fn to_message(&self) -> Message {
        let mut body = Vec::with_capacity(30);
        body.extend(self.effect_key.to_be_bytes());
        body.extend(self.actor.to_be_bytes());
        for coordinate in self.position {
            body.extend(coordinate.to_be_bytes());
        }
        body.extend(self.unknown_word.to_be_bytes());
        if let Some(padding) = self.padding {
            body.extend(padding);
        }
        Message::known(crate::Opcode::PlayPfxAtLocation, body)
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
