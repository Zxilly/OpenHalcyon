use halcyon_protocol::{
    AndroidLayout, Fields, MatchCipher, Message, STATIC_ACTIONS, StaticActionLayout, StaticValue,
};

#[test]
fn every_static_entry_keeps_payload_and_padding_losslessly() {
    let cipher = MatchCipher::new("static action fixtures");
    assert!(STATIC_ACTIONS.windows(2).all(|w| w[0].opcode < w[1].opcode));
    let mut added = 0;
    for layout in STATIC_ACTIONS {
        let AndroidLayout::Fixed { frame_bytes } = AndroidLayout::for_opcode(layout.opcode) else {
            panic!("non-fixed opcode {:#06x}", layout.opcode);
        };
        let len = frame_bytes - 4;
        let padded = (len + 2).next_multiple_of(8) - 2;
        let body: Vec<_> = (0..padded).map(|offset| offset as u8 ^ 0xa5).collect();
        for (sample, size) in [len, padded].into_iter().enumerate() {
            let msg = Message::new(layout.opcode, &body[..size]);
            let view = layout.read(&msg.body).unwrap();
            assert_eq!(view.payload(), &body[..len]);
            assert_eq!(view.padding(), &body[len..size]);
            assert_eq!(view.to_message(), msg);
            for (offset, value) in view.primitives() {
                let bytes = match value {
                    StaticValue::Byte(n) => vec![n],
                    StaticValue::Be16(n) => n.to_be_bytes().to_vec(),
                    StaticValue::Be32(n) | StaticValue::BeFloatBits(n) => n.to_be_bytes().to_vec(),
                };
                assert_eq!(&view.payload()[offset..offset + bytes.len()], bytes);
            }
            let encrypted = cipher.encode(&msg).unwrap();
            let decoded = cipher.decode(&encrypted[2..]).unwrap();
            assert_eq!(
                cipher
                    .encode(&layout.read(&decoded.body).unwrap().to_message())
                    .unwrap(),
                encrypted
            );
            match msg.fields().unwrap() {
                Fields::StaticAction(actual) => {
                    assert_eq!(actual, view);
                    added += usize::from(sample == 0);
                }
                Fields::Heartbeat => {
                    assert_eq!(layout.opcode, halcyon_protocol::wire_opcode::ServerHelloAck)
                }
                Fields::Zone(zone) => {
                    assert_eq!(
                        layout.opcode,
                        halcyon_protocol::wire_opcode::CreateRectZoneOfControl
                    );
                    assert_eq!(zone.to_message().unwrap().body, body[..size]);
                }
                Fields::AbilityCharges { .. } => assert_eq!(
                    layout.opcode,
                    halcyon_protocol::wire_opcode::ModifyAbilityCharges
                ),
                Fields::ControlByte { .. } => {
                    assert_eq!(layout.opcode, halcyon_protocol::wire_opcode::Unresolved047C)
                }
                Fields::Opaque(raw) => {
                    assert!(
                        !layout.has_fields(),
                        "missing static view for {:#06x}",
                        layout.opcode
                    );
                    assert_eq!(raw, &body[..size]);
                }
                other => panic!("unexpected static opcode {:#06x}: {other:?}", layout.opcode),
            }
        }
        assert!(layout.read(&vec![0; padded + 1]).is_none());
        if len != 0 {
            assert!(layout.read(&body[..len - 1]).is_none());
        }
    }
    assert_eq!(added, 31);
}

#[test]
fn packed_words_keep_their_types_and_bit_patterns() {
    use StaticValue::{Be16, Be32, BeFloatBits, Byte};
    let mut face = [0u8; 16];
    face[..4].copy_from_slice(&0x0000_05dcu32.to_be_bytes());
    face[4..8].copy_from_slice(&0x7fc0_0123u32.to_be_bytes());
    face[8..12].copy_from_slice(&(-0.0f32).to_be_bytes());
    face[12..16].copy_from_slice(&(1.25f32).to_be_bytes());
    let face = StaticActionLayout::for_opcode(halcyon_protocol::wire_opcode::FaceDir)
        .unwrap()
        .read(&face)
        .unwrap();
    assert_eq!(face.layout.action_name, Some("ActionFaceDir"));
    assert_eq!(
        face.primitives().collect::<Vec<_>>(),
        [
            (0, Be32(1500)),
            (4, BeFloatBits(0x7fc0_0123)),
            (8, BeFloatBits(0x8000_0000)),
            (12, BeFloatBits(1.25f32.to_bits())),
        ]
    );

    let mut roster = [0u8; 2134];
    roster[273..275].copy_from_slice(&0x0515u16.to_be_bytes());
    roster[1769..1771].copy_from_slice(&0x1234u16.to_be_bytes());
    let roster = StaticActionLayout::for_opcode(halcyon_protocol::wire_opcode::DraftRosterRanked)
        .unwrap()
        .read(&roster)
        .unwrap();
    let values: std::collections::BTreeMap<_, _> = roster.primitives().collect();
    assert_eq!(values.len(), 86);
    assert_eq!(values[&273], Be16(0x0515));
    assert_eq!(values[&1769], Be16(0x1234));

    let path = StaticActionLayout::for_opcode(halcyon_protocol::wire_opcode::SetBuildPath).unwrap();
    let mut body = [0u8; 99];
    body[96] = 2;
    body[4..8].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    let decode = |body: &[u8]| {
        let mut fields: Vec<_> = path.read(body).unwrap().primitives().collect();
        fields.sort_unstable_by_key(|&(offset, _)| offset);
        fields
    };
    assert_eq!(
        decode(&body),
        [
            (0, Be32(0)),
            (4, Be32(0x1234_5678)),
            (8, Be32(0)),
            (96, Byte(2))
        ]
    );
    body[96] = 255;
    assert_eq!(decode(&body), [(0, Be32(0)), (96, Byte(255))]);
}
