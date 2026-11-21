use halcyon_protocol::{MAX_PAYLOAD, MatchCipher, Message, read_frame};

#[test]
fn appending_frames_preserves_prefix_padding_and_caller_storage() {
    let cipher = MatchCipher::new("test-room");
    let mut buffer = Vec::with_capacity(2048);
    let buffer_ptr = buffer.as_ptr();
    buffer.extend_from_slice(b"head");
    let frames = [Message::movement(1.25, -8.5), Message::clock(3.5)];
    for frame in &frames {
        cipher.encode_into(frame, &mut buffer).unwrap();
    }
    let before_error = buffer.clone();
    assert!(
        cipher
            .encode_into(
                &Message::known(halcyon_protocol::Opcode::ClockSync, vec![0; MAX_PAYLOAD]),
                &mut buffer
            )
            .is_err()
    );
    assert_eq!(buffer, before_error);
    assert_eq!(buffer.as_ptr(), buffer_ptr);
    assert_eq!(&buffer[..4], b"head");
    let mut reader = &buffer[4..];
    for original in &frames {
        let received = read_frame(&mut reader).unwrap().unwrap();
        let storage = received.as_ptr();
        let decoded = cipher.decode_owned(received).unwrap();
        assert_eq!(decoded.body.as_ptr(), storage);
        assert_eq!(decoded.opcode, original.opcode);
        assert_eq!(&decoded.body[..original.body.len()], original.body);
        assert!(
            decoded.body[original.body.len()..]
                .iter()
                .all(|byte| *byte == 0)
        );
    }
    assert!(reader.is_empty());
}
