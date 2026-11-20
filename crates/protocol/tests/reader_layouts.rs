use halcyon_protocol::{AndroidLayout, Fields, Message, READ_SIZES};

#[test]
fn all_android_prefixes_preserve_surplus_bytes_and_reject_short_bodies() {
    assert!(READ_SIZES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    let mut fixed = 0;
    let mut ignored = 0;
    for &(opcode, frame_bytes) in READ_SIZES {
        let layout = AndroidLayout::for_opcode(opcode);
        let expected = match frame_bytes {
            Some(bytes) => {
                fixed += 1;
                bytes - 4
            }
            None if opcode == 0 => {
                assert_eq!(layout, AndroidLayout::Clock);
                4
            }
            None => {
                ignored += 1;
                assert_eq!(layout, AndroidLayout::IgnoredByReceiver);
                0
            }
        };
        let body = vec![0xa5; expected + 3];
        assert_eq!(layout.split_body(&body).unwrap(), body.split_at(expected));
        if expected > 0 {
            assert!(layout.split_body(&body[..expected - 1]).is_err());
        }
    }
    assert_eq!((fixed, ignored), (122, 48));
    let unknown = AndroidLayout::for_opcode(0xffff);
    assert_eq!(unknown, AndroidLayout::Unlisted);
    assert_eq!(unknown.split_body(b"xyz").unwrap(), (&b""[..], &b"xyz"[..]));
}

#[test]
fn removing_cipher_padding_preserves_the_android_action_decoder() {
    for &(opcode, frame_bytes) in READ_SIZES {
        let Some(frame_bytes) = frame_bytes else {
            continue;
        };
        let body_bytes = frame_bytes - 4;
        let padded = (body_bytes + 2).next_multiple_of(8) - 2;
        if padded == body_bytes {
            continue;
        }
        let compact = Message::new(opcode, vec![0; body_bytes]);
        let encrypted_body = Message::new(opcode, vec![0; padded]);
        let Ok(wire) = encrypted_body.fields() else {
            continue;
        };
        if matches!(wire, Fields::Opaque(_)) {
            continue;
        }
        let native = compact
            .fields()
            .unwrap_or_else(|err| panic!("{opcode:#x}: {err}"));
        assert_eq!(
            std::mem::discriminant(&native),
            std::mem::discriminant(&wire),
            "{opcode:#x}: {body_bytes} native bytes and {padded} padded bytes"
        );
    }
}
