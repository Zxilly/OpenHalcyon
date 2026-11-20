use halcyon_protocol::{Message, Opcode, spec::CATALOG};
use std::collections::BTreeSet;

#[test]
fn all_names_and_values_round_trip_in_text_and_binary_formats() {
    assert_eq!(Opcode::ALL.len(), CATALOG.len());
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for (&opcode, catalog) in Opcode::ALL.iter().zip(CATALOG) {
        let name = opcode.as_str();
        assert!(names.insert(name));
        assert!(ids.insert(opcode.raw()));
        assert_eq!(catalog.opcode, opcode);
        assert_eq!(opcode.to_string(), name);
        assert_eq!(name.parse::<Opcode>(), Ok(opcode));
        let json = serde_json::to_string(&opcode).unwrap();
        assert_eq!(serde_json::from_str::<String>(&json).unwrap(), name);
        assert_eq!(serde_json::from_str::<Opcode>(&json).unwrap(), opcode);
        assert_eq!(
            serde_json::from_str::<Opcode>(&opcode.raw().to_string()).unwrap(),
            opcode
        );

        let mut bytes = Vec::new();
        ciborium::into_writer(&opcode, &mut bytes).unwrap();
        assert_eq!(
            ciborium::from_reader::<u16, _>(&bytes[..]).unwrap(),
            opcode.raw()
        );
        assert_eq!(
            ciborium::from_reader::<Opcode, _>(&bytes[..]).unwrap(),
            opcode
        );
        assert_eq!(
            Message::from_plain(&opcode.raw().to_be_bytes())
                .unwrap()
                .known_opcode(),
            Some(opcode)
        );
    }
    assert_eq!(
        Opcode::from_name("ActionModifyCooldown"),
        Some(Opcode::CooldownValue)
    );
    assert_eq!(Opcode::CooldownValue.raw(), 0x0428);
    for input in ["65536", "-1", "65535", "\"ActionNotRegistered\""] {
        assert!(serde_json::from_str::<Opcode>(input).is_err());
    }
    let extension = Message::from_plain(&[0xff, 0xff, 0x42]).unwrap();
    assert_eq!(extension.known_opcode(), None);
    assert_eq!((extension.opcode, extension.body), (u16::MAX, vec![0x42]));
}
