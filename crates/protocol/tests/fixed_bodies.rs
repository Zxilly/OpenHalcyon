use halcyon_protocol::{
    DeathBody, Fields, ImpactBody, Message, MoveCmd, Opcode, PositionBody, ProjectileAction,
    ProjectileFire,
};

#[test]
fn fixed_prefixes_preserve_signaling_nans_actor_width_and_the_callers_suffix() {
    let bytes = [0x7f, 0x80, 0x00, 0x51, 0x80, 0x00, 0x00, 0x00, 0x45, 0xff];
    let m = MoveCmd::read_body(&bytes).unwrap();
    assert_eq!(m.x.to_bits(), 0x7f800051);
    assert_eq!(m.y.to_bits(), 0x80000000);
    let mut out = [0x3c; 10];
    assert_eq!(m.write_body(&mut out).unwrap(), 8);
    assert_eq!(&out[..8], &bytes[..8]);
    assert_eq!(out[8..], [0x3c; 2]);
    let mut short = [0xaa; 7];
    assert!(m.write_body(&mut short).is_err());
    assert_eq!(short, [0xaa; 7]);
    assert!(MoveCmd::read_body(&short).is_err());

    let pos = [0x12, 0x34, 0x56, 0x78, 0xff, 0x80, 0x00, 0x01, 0, 0, 0, 1];
    let p = PositionBody::read_body(&pos).unwrap();
    assert_eq!(p.actor, 0x12345678);
    let mut buf = [0; 16];
    assert_eq!(p.write_body(&mut buf).unwrap(), 12);
    assert_eq!(&buf[..12], &pos);
    assert!(matches!(
        Message::known(Opcode::Position, pos).fields(),
        Ok(Fields::Position { entity: 0x5678, .. })
    ));
    let hit = [
        0, 0, 1, 2, 0, 0, 3, 4, 0xff, 0xc0, 0, 7, 0x81, 2, 0xfa, 0xfb,
    ];
    let h = ImpactBody::read_body(&hit).unwrap();
    assert_eq!(
        (h.target, h.source, h.word, h.byte, h.flag),
        (258, 772, 0x8102, 0xfa, 0xfb)
    );
    h.write_body(&mut buf).unwrap();
    assert_eq!(buf, hit);
    let d = DeathBody::read_body(&hit[..8]).unwrap();
    assert_eq!((d.victim, d.source), (258, 772));
}

#[test]
fn tracked_fire_reproduces_independent_native_and_padded_bytes() {
    let raw = [
        0, 0, 0x40, 1, 0xfe, 0x11, 0x90, 0x40, 0x7f, 0x80, 0, 2, 0, 0x71, 0x1e, 0x1e, 0x17, 1, 0,
        0xe0, 0xde, 0xff,
    ];
    for n in [17, 22] {
        let m = Message::known(Opcode::FireProjectile, &raw[..n]);
        let Fields::Projectile(shot) = m.fields().unwrap() else {
            panic!()
        };
        let ProjectileAction::FireTracked(ref f) = shot else {
            panic!()
        };
        assert_eq!(
            (
                f.projectile,
                f.profile_word,
                f.values[0].to_bits(),
                f.operand,
                f.slots
            ),
            (0x4001, 0xfe119040, 0x7f800002, 113, [30, 30, 23])
        );
        let mut out = [0xed; 24];
        assert!(shot.write_body(&mut out[..n - 1]).is_err());
        assert_eq!(out, [0xed; 24]);
        assert_eq!(shot.write_body(&mut out).unwrap(), n);
        assert_eq!(&out[..n], &raw[..n]);
        assert!(out[n..].iter().all(|b| *b == 0xed));
    }
    let invalid = ProjectileAction::FireTracked(ProjectileFire {
        projectile: 1,
        profile_word: 2,
        values: [-0.],
        operand: 3,
        slots: [1, 2, 3],
        padding: &[0xff],
    });
    let mut untouched = [0x71; 22];
    assert!(invalid.write_body(&mut untouched).is_err());
    assert_eq!(untouched, [0x71; 22]);
}
