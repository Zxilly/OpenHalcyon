use halcyon_protocol::{
    AndroidLayout, Fields, MatchCipher, Message, OUTBOUND_LAYOUTS, OutboundLayout,
    OutboundOperands, READ_SIZES,
};

#[test]
fn writer_evidence_covers_receiver_ignore_list_and_keeps_every_byte() {
    let expected: Vec<_> = READ_SIZES
        .iter()
        .filter_map(|(opcode, _)| {
            (AndroidLayout::for_opcode(*opcode) == AndroidLayout::IgnoredByReceiver)
                .then_some(*opcode)
        })
        .collect();
    let found: Vec<_> = OUTBOUND_LAYOUTS
        .iter()
        .map(|entry| entry.opcode())
        .collect();
    assert_eq!(found, expected);
    assert_eq!(found.len(), 48);
    let cipher = MatchCipher::new("outbound fixtures");
    let mut windows = 0;
    let mut android = 0;
    for &entry in OUTBOUND_LAYOUTS {
        windows += usize::from(entry.windows_width);
        let Some(len) = entry.body_bytes() else {
            assert!(entry.read(&[]).is_none());
            continue;
        };
        android += 1;
        let padded = (len + 2).next_multiple_of(8) - 2;
        let body: Vec<_> = (0..padded).map(|i| 0x80 ^ i as u8).collect();
        for size in [len, padded] {
            let input = Message::new(entry.opcode(), &body[..size]);
            let action = entry.read(&input.body).unwrap();
            assert_eq!(action.payload(), &body[..len]);
            assert_eq!(action.padding(), &body[len..size]);
            assert_eq!(action.to_message(), input);
            let wire = cipher.encode(&input).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            let action = entry.read(&decoded.body).unwrap();
            assert_eq!(cipher.encode(&action.to_message()).unwrap(), wire);
        }
        assert!(entry.read(&body[..len - 1]).is_none());
        assert!(entry.read(&[0; 64]).is_none());
    }
    assert_eq!((android, windows), (43, 36));
}

#[test]
fn alternate_move_and_ability_words_preserve_variant_and_bits() {
    let bits = [0x8000_0000u32, 0x7fc0_0123];
    let motion = Message::known(
        halcyon_protocol::Opcode::Unseen03F6,
        bits.map(u32::to_be_bytes).concat(),
    );
    let Fields::Outbound(view) = motion.fields().unwrap() else {
        panic!("missing specialized outbound movement view");
    };
    assert_eq!(view.layout.action_name, Some("ActionRequestMoveTo_Client"));
    assert_eq!(
        view.operands,
        OutboundOperands::MoveAlternate {
            coordinate_bits: bits
        }
    );
    assert_eq!(view.to_message(), motion);

    let ability_words = [1500u32, 0x3f80_0000, 0, 0xbf80_0000];
    let mut body = ability_words.map(u32::to_be_bytes).concat();
    body.push(3);
    let ability = Message::known(halcyon_protocol::Opcode::RequestActivateAbilityUnseen, body);
    let Fields::Outbound(view) = ability.fields().unwrap() else {
        panic!("missing specialized outbound ability view");
    };
    assert_eq!(
        view.operands,
        OutboundOperands::AbilityAlternate {
            words: ability_words,
            byte: 3
        }
    );
    assert_eq!(view.to_message(), ability);

    let pair = OutboundLayout::for_opcode(halcyon_protocol::wire_opcode::DraftSwapRequest).unwrap();
    assert_eq!(
        pair.read(&[0x80, 3]).unwrap().operands,
        OutboundOperands::Bytes([0x80, 3])
    );
    let unknown = OutboundLayout::for_opcode(halcyon_protocol::wire_opcode::DraftSkipBan).unwrap();
    assert_eq!(
        unknown.read(&[1, 2, 3, 4]).unwrap().operands,
        OutboundOperands::Opaque
    );
}
