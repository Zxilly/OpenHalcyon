use halcyon_protocol::{
    AndroidLayout, CampVisibilityBody, CampVisibilityState, Fields, MatchCipher, Message, Opcode,
    ReplayReader, ReplayWriter,
};

#[test]
fn camp_visibility_decodes_encodes_and_roundtrips_blowfish_without_padding() {
    let cipher = MatchCipher::new("solo-camp-visibility-roundtrip");
    let cases: [(u32, u8, CampVisibilityState, u8); 5] = [
        (6580, 0, CampVisibilityState::EmptyHidden, 0),
        (6583, 1, CampVisibilityState::EmptyVisible, 1),
        (6586, 2, CampVisibilityState::PopulatedHidden, 2),
        (6589, 0, CampVisibilityState::PopulatedVisible, 3),
        (6591, 7, CampVisibilityState::Unknown(0xfe), 0xfe),
    ];

    for (actor, index, state, raw_state) in cases {
        assert_eq!(CampVisibilityState::from_wire(raw_state), state);
        assert_eq!(state.raw(), raw_state);

        let mut expected_body = [0u8; 6];
        expected_body[..4].copy_from_slice(&actor.to_be_bytes());
        expected_body[4] = index;
        expected_body[5] = raw_state;

        let msg = Message::camp_visibility(actor, index, state);
        assert_eq!(msg.opcode, Opcode::CampVisibility.raw());
        assert_eq!(msg.body, expected_body);
        assert_eq!(
            AndroidLayout::for_opcode(msg.opcode)
                .split_body(&msg.body)
                .unwrap(),
            (&expected_body[..], &[][..])
        );

        let body = CampVisibilityBody::read_body(&expected_body).unwrap();
        assert_eq!(
            body,
            CampVisibilityBody {
                actor,
                index,
                state,
            }
        );

        let mut out = [0xaa; 8];
        assert_eq!(body.write_body(&mut out).unwrap(), 6);
        assert_eq!(&out[..6], &expected_body);
        assert_eq!(&out[6..], [0xaa, 0xaa]);

        assert_eq!(
            msg.fields().unwrap(),
            Fields::CampVisibility {
                actor,
                index,
                state,
            }
        );

        // 2-byte opcode + 6-byte body = 8 bytes (exact single Blowfish block, no zero pad).
        let frame = cipher.encode(&msg).unwrap();
        assert_eq!(frame.len(), 10);
        assert_eq!(u16::from_be_bytes([frame[0], frame[1]]), 8);
        let decoded = cipher.decode(&frame[2..]).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(decoded.fields().unwrap(), msg.fields().unwrap());
    }
}

#[test]
fn camp_visibility_rejects_truncated_and_wrong_length_bodies() {
    let sample = [0x00, 0x00, 0x19, 0xb4, 0x01, 0x02, 0x99, 0x88];
    for len in 0..6 {
        assert!(CampVisibilityBody::read_body(&sample[..len]).is_err());
        let mut short_dst = [0x5a; 5];
        let codec = CampVisibilityBody {
            actor: 6580,
            index: 1,
            state: CampVisibilityState::PopulatedHidden,
        };
        assert!(codec.write_body(&mut short_dst[..len]).is_err());
        assert_eq!(short_dst, [0x5a; 5]);
    }

    for len in [0, 4, 5, 7, 8, 14] {
        let raw = &sample[..len.min(sample.len())];
        let mut body = raw.to_vec();
        body.resize(len, 0x11);
        let msg = Message::known(Opcode::CampVisibility, body.clone());
        assert_eq!(msg.fields().unwrap(), Fields::Opaque(&body));
    }
}

#[test]
fn observed_solo_camp_transitions_roundtrip_through_replay_and_cipher() {
    use CampVisibilityState::*;

    // Anonymized sequence derived from Solo VGR chunks (camp markers 6583, 6587, 6589):
    // 1. Countdown init at ~24.597s (indices 0..=7 -> EmptyHidden)
    // 2. Pre-spawn fog toggle at ~24.733s..25.152s (index 1: EmptyHidden -> EmptyVisible -> EmptyHidden)
    // 3. Spawn activation at ~44.590s (index 0 -> PopulatedVisible, indices 1..=7 -> PopulatedHidden)
    // 4. Camp entry fog gain at ~47.864s (index 1: PopulatedHidden -> PopulatedVisible)
    // 5. Last monster death at ~65.027s (indices 0, 1: PopulatedVisible -> EmptyVisible)
    // 6. Departure fog loss at ~68.677s (index 1: EmptyVisible -> EmptyHidden)
    let transitions: &[(f32, u32, u8, CampVisibilityState, [u8; 6])] = &[
        (24.597, 6583, 0, EmptyHidden, [0, 0, 0x19, 0xb7, 0, 0]),
        (24.597, 6583, 1, EmptyHidden, [0, 0, 0x19, 0xb7, 1, 0]),
        (24.733, 6583, 1, EmptyVisible, [0, 0, 0x19, 0xb7, 1, 1]),
        (25.152, 6583, 1, EmptyHidden, [0, 0, 0x19, 0xb7, 1, 0]),
        (44.590, 6587, 0, PopulatedVisible, [0, 0, 0x19, 0xbb, 0, 3]),
        (44.590, 6587, 1, PopulatedHidden, [0, 0, 0x19, 0xbb, 1, 2]),
        (47.864, 6587, 1, PopulatedVisible, [0, 0, 0x19, 0xbb, 1, 3]),
        (65.027, 6587, 0, EmptyVisible, [0, 0, 0x19, 0xbb, 0, 1]),
        (65.027, 6587, 1, EmptyVisible, [0, 0, 0x19, 0xbb, 1, 1]),
        (68.677, 6587, 1, EmptyHidden, [0, 0, 0x19, 0xbb, 1, 0]),
        (68.156, 6589, 1, PopulatedVisible, [0, 0, 0x19, 0xbd, 1, 3]),
        (81.138, 6589, 0, EmptyVisible, [0, 0, 0x19, 0xbd, 0, 1]),
    ];

    let mut replay_bytes = Vec::new();
    let mut writer = ReplayWriter::new(&mut replay_bytes);
    let cipher = MatchCipher::new("solo-camp-transitions");

    for &(time, actor, index, state, wire_body) in transitions {
        let msg = Message::camp_visibility(actor, index, state);
        assert_eq!(msg.body, wire_body);
        writer.write_record(time, &msg).unwrap();

        let frame = cipher.encode(&msg).unwrap();
        let decrypted = cipher.decode(&frame[2..]).unwrap();
        assert_eq!(decrypted, msg);
    }

    let mut reader = ReplayReader::new(replay_bytes.as_slice());
    for &(time, actor, index, state, _) in transitions {
        let record = reader.next_record().unwrap().unwrap();
        assert_eq!(record.time, time);
        assert_eq!(
            record.message.fields().unwrap(),
            Fields::CampVisibility {
                actor,
                index,
                state,
            }
        );
    }
    assert!(reader.next_record().unwrap().is_none());
}
