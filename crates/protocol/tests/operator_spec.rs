use halcyon_protocol::{ActorFlagChange, Fields, LiveFields, MatchCipher, Message, Opcode};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

#[derive(Deserialize)]
struct Manifest {
    total_fixtures: usize,
    files: BTreeMap<String, Group>,
}

#[derive(Deserialize)]
struct Group {
    count: usize,
    opcodes: Vec<String>,
}

#[derive(Deserialize)]
struct Fixture {
    id: String,
    opcode: String,
    payload_hex: String,
    wire_len_bytes: usize,
    expected_decoded: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct Expected {
    actor_id: Option<u32>,
    owner_actor_id: Option<u32>,
    cooldown_key: Option<u32>,
    value_sec: Option<f32>,
    mode: Option<u8>,
    disconnect_code: Option<u16>,
    player_uuid: Option<String>,
}

fn verify_layout(msg: &Message, expected: &serde_json::Map<String, serde_json::Value>, id: &str) {
    use halcyon_protocol::spec::Value;
    let view = msg
        .spec_view()
        .unwrap_or_else(|| panic!("layout mismatch: {id}"));
    for (field, value) in view.fields() {
        let e = &expected[field.name];
        let matches = match value {
            Value::Unsigned(n) => Some(u64::from(n)) == e.as_u64(),
            Value::Signed(n) => Some(i64::from(n)) == e.as_i64(),
            Value::Float16Bits(bits) => e["raw_bits"].as_str().is_some_and(|s| {
                s.strip_prefix("0x")
                    .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                    == Some(bits)
            }),
            Value::Float32Bits(bits) => {
                let actual = f64::from(f32::from_bits(bits));
                // Fixture JSON rounds finite binary32 values to six decimal places.
                e.as_f64()
                    .is_some_and(|n| (actual - n).abs() <= 0.000_000_51)
                    || (actual.is_nan() && e.as_str() == Some("nan"))
            }
            Value::Text(raw) => e.as_str().is_some_and(|v| raw == v.as_bytes()),
            Value::Bytes(raw) => e.as_str().is_some_and(|v| {
                v.len() == raw.len() * 2
                    && raw
                        .iter()
                        .zip(v.as_bytes().as_chunks::<2>().0)
                        .all(|(byte, hex)| {
                            std::str::from_utf8(hex)
                                .ok()
                                .and_then(|h| u8::from_str_radix(h, 16).ok())
                                == Some(*byte)
                        })
            }),
        };
        assert!(matches, "{id}: {}", field.name);
    }
}

#[test]
#[ignore = "requires HALCYON_PROTOCOL_SPEC pointing to the 147219 protocol_spec directory"]
fn decode_operator_protocol_fixtures() {
    let dir = PathBuf::from(std::env::var_os("HALCYON_PROTOCOL_SPEC").unwrap()).join("fixtures");
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let cipher = MatchCipher::new("fixture round trip");
    let mut seen = BTreeSet::new();
    for (file, group) in manifest.files {
        let fixtures: Vec<Fixture> =
            serde_json::from_slice(&fs::read(dir.join(file)).unwrap()).unwrap();
        assert_eq!(fixtures.len(), group.count);
        let mut opcodes = BTreeSet::new();
        for f in fixtures {
            assert!(seen.insert(f.id.clone()), "duplicate fixture {}", f.id);
            let raw: Vec<_> = f
                .payload_hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(2 * raw.len(), f.payload_hex.len(), "{}", f.id);
            assert_eq!(raw.len(), f.wire_len_bytes, "{}", f.id);
            let msg = Message::from_plain(&raw).unwrap();
            let opcode = msg
                .known_opcode()
                .unwrap_or_else(|| panic!("unregistered {}", f.id));
            assert_eq!(
                opcode.raw(),
                u16::from_str_radix(&f.opcode[2..], 16).unwrap()
            );
            opcodes.insert(f.opcode);
            let fields = msg.fields().unwrap_or_else(|e| panic!("{}: {e}", f.id));
            verify_layout(&msg, &f.expected_decoded, &f.id);
            let e: Expected =
                serde_json::value::from_value(serde_json::Value::Object(f.expected_decoded))
                    .unwrap();
            match opcode {
                Opcode::PlayerAfk | Opcode::PlayerDeserter | Opcode::PlayerReconnected => {
                    let action = msg.actor_flags().unwrap();
                    assert_eq!(Some(action.actor), e.actor_id);
                    assert_eq!(
                        action.change,
                        match opcode {
                            Opcode::PlayerAfk => ActorFlagChange::Afk,
                            Opcode::PlayerDeserter => ActorFlagChange::Deserter,
                            _ => ActorFlagChange::Reconnected,
                        }
                    );
                    assert_eq!(action.to_message(), msg);
                }
                Opcode::CooldownValue => {
                    let Fields::CooldownValue {
                        owner,
                        key,
                        value,
                        mode,
                        ..
                    } = fields
                    else {
                        panic!("{}", f.id)
                    };
                    assert_eq!(
                        (Some(owner), Some(key), Some(mode)),
                        (e.owner_actor_id, e.cooldown_key, e.mode)
                    );
                    assert_eq!(value.to_bits(), e.value_sec.unwrap().to_bits());
                }
                Opcode::KickPlayer => {
                    let Fields::Live(LiveFields::KickPlayer {
                        player_uuid,
                        disconnect_code,
                    }) = fields
                    else {
                        panic!("{}", f.id)
                    };
                    let end = player_uuid
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(player_uuid.len());
                    assert_eq!(
                        &player_uuid[..end],
                        e.player_uuid.as_deref().unwrap().as_bytes()
                    );
                    assert_eq!(Some(disconnect_code), e.disconnect_code);
                }
                _ => {}
            }
            if !matches!(opcode, Opcode::Hello | Opcode::Heartbeat) {
                let frame = cipher.encode(&msg).unwrap();
                let decoded = cipher.decode(&frame[2..]).unwrap();
                assert_eq!(decoded.opcode, msg.opcode);
                assert!(decoded.body.starts_with(&msg.body), "{}", f.id);
                assert_eq!(cipher.encode(&decoded).unwrap(), frame);
            }
        }
        assert_eq!(opcodes, group.opcodes.into_iter().collect());
    }
    assert_eq!(seen.len(), manifest.total_fixtures);
}
