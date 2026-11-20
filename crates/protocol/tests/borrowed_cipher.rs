use halcyon_protocol::{MAX_PAYLOAD, MatchCipher, Opcode};

#[test]
fn borrowed_plaintext_stays_in_its_frame_and_keeps_padding() {
    // PyCryptodome Blowfish ECB, swapping each u32 before and after encryption.
    let cipher = MatchCipher::new("local-test-match");
    let mut buffer = [
        0x9d, 0x7a, 0xf6, 0x30, 0xab, 0xbf, 0xfb, 0x6d, 0xcd, 0x97, 0x44, 0xe0, 0x77, 0xf2, 0xd7,
        0xfd, 0x0e, 0x5a, 0xcf, 0x33, 0xb4, 0x72, 0x51, 0x69,
    ];
    let (movement, extension) = buffer.split_at_mut(16);
    let movement_body_ptr = movement[2..].as_ptr();
    let extension_body_ptr = extension[2..].as_ptr();
    let (first_op, first) = cipher.decrypt_in_place(movement).unwrap();
    assert_eq!(first_op, Opcode::MoveCmd.raw());
    assert_eq!(extension, &[0x0e, 0x5a, 0xcf, 0x33, 0xb4, 0x72, 0x51, 0x69]);
    let owned = cipher.decode_owned(extension.to_vec()).unwrap();
    let (second_op, second) = cipher.decrypt_in_place(extension).unwrap();
    assert_eq!(first.as_ptr(), movement_body_ptr);
    assert_eq!(second.as_ptr(), extension_body_ptr);
    assert_eq!(
        first,
        &[0x3f, 0xc0, 0, 0, 0xc0, 0x20, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(second_op, 0xface);
    assert_eq!(second, &[1, 0x80, 0xaa, 0, 0xff, 0x31]);
    assert_eq!((owned.opcode, owned.body.as_slice()), (second_op, second));
}

#[test]
fn invalid_boundaries_leave_the_ciphertext_untouched() {
    let cipher = MatchCipher::new("local-test-match");
    for length in [0, 1, 7, 9, MAX_PAYLOAD, MAX_PAYLOAD + 2] {
        let mut invalid = vec![0xc5; length];
        assert_eq!(
            cipher.decrypt_in_place(&mut invalid).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(invalid.iter().all(|&b| b == 0xc5));
    }
}
