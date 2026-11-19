use crate::Message;

impl Message {
    /// Observed 0x043A actor and learned ability operand.
    pub fn ability_learned(actor: u32, operand: u32) -> Self {
        Self::new(
            0x043a,
            [actor.to_be_bytes(), operand.to_be_bytes()].concat(),
        )
    }

    /// Observed 0x0415 caster, target actor and ability selector.
    pub fn ability_target_used(actor: u32, target: u32, selector: u8) -> Self {
        let mut body = [0; 9];
        body[..4].copy_from_slice(&actor.to_be_bytes());
        body[4..8].copy_from_slice(&target.to_be_bytes());
        body[8] = selector;
        Self::new(0x0415, body)
    }

    /// Observed 0x0416 actor, three world coordinates and ability selector.
    pub fn ability_point_used(actor: u32, coordinates: [f32; 3], selector: u8) -> Self {
        let mut body = [0; 17];
        body[..4].copy_from_slice(&actor.to_be_bytes());
        for (word, coordinate) in body[4..16]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(coordinates)
        {
            word.copy_from_slice(&coordinate.to_be_bytes());
        }
        body[16] = selector;
        Self::new(0x0416, body)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Fields, LiveFields, MatchCipher, Message};

    #[test]
    fn observed_idris_points_round_trip_and_preserve_signed_zero() {
        let cipher = MatchCipher::new("synthetic-round-trip");
        for coordinates in [
            [-40.8939_f32, 0.0, 1.4944],
            [-58.2402, -0.0, 14.9417],
            [-44.1787, -0.0, 3.5794],
            [-34.1510, 0.0, -0.7185],
        ] {
            let mut body = coordinates
                .into_iter()
                .flat_map(f32::to_be_bytes)
                .collect::<Vec<_>>();
            body.push(3);
            let request = Message::new(0x0412, &body[..]);
            let wire = cipher.encode(&request).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.body[13..], [0]);
            assert_eq!(
                decoded.fields().unwrap(),
                Fields::Live(LiveFields::AbilityPoint {
                    vector: coordinates,
                    selector: 3
                })
            );
            let response = Message::ability_point_used(1505, coordinates, 3);
            let wire = cipher.encode(&response).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.opcode, 0x0416);
            assert_eq!(&decoded.body[..4], &1505u32.to_be_bytes());
            assert_eq!(&decoded.body[4..17], &body);
            assert_eq!(decoded.body[17..], [0; 5]);
        }
    }
}
