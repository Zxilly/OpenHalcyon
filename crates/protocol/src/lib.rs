//! Vainglory CE game transport. Decrypted messages preserve every byte,
//! including ambiguous padding, so unknown opcodes can be re-encoded exactly.
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit},
};
use md5::{Digest, Md5};
use std::io::{self, Read, Write};

mod metadata;
mod opcode;
mod skin;
pub use metadata::READ_SIZES;
pub use opcode::Opcode;
pub use skin::skin_hash;
pub const MAX_PAYLOAD: usize = 0x2800 - 2;
pub const ACK_FRAME: [u8; 5] = [0, 3, 0, 6, 0];

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

// Read one byte separately to distinguish record-boundary EOF from truncation.
fn header<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    loop {
        match reader.read(&mut buf[..1]) {
            Ok(0) => return Ok(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
            Ok(_) => break,
        }
    }
    reader.read_exact(&mut buf[1..])?;
    Ok(true)
}
fn payload<R: Read>(reader: &mut R, len: usize, limit: usize) -> io::Result<Vec<u8>> {
    if !(2..=limit).contains(&len) {
        return Err(invalid("invalid payload length"));
    }
    let mut data = vec![0; len];
    reader.read_exact(&mut data)?;
    Ok(data)
}
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0; 2];
    if !header(reader, &mut len)? {
        return Ok(None);
    }
    payload(reader, u16::from_be_bytes(len) as usize, MAX_PAYLOAD).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub opcode: u16,
    pub body: Vec<u8>,
}
impl Message {
    pub fn new(opcode: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode,
            body: body.into(),
        }
    }
    pub fn from_plain(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 2 {
            return Err(invalid("missing opcode"));
        }
        Ok(Self::new(
            u16::from_be_bytes([bytes[0], bytes[1]]),
            &bytes[2..],
        ))
    }
    pub fn clock(time: f32) -> Self {
        Self::new(0, time.to_be_bytes())
    }
    pub fn movement(x: f32, y: f32) -> Self {
        Self::new(0x3f4, [x.to_be_bytes(), y.to_be_bytes()].concat())
    }
    pub fn position(entity: u16, x: f32, y: f32) -> Self {
        let mut b = vec![0, 0];
        b.extend(entity.to_be_bytes());
        b.extend(x.to_be_bytes());
        b.extend(y.to_be_bytes());
        Self::new(0x42e, b)
    }
    pub fn entity_stat(entity: u16, value: f32, stat: u8) -> Self {
        let mut b = vec![0, 0];
        b.extend(entity.to_be_bytes());
        b.extend(value.to_be_bytes());
        b.push(stat);
        Self::new(0x41d, b)
    }
    pub fn actor_death(victim: u32, source: u32) -> Self {
        let mut body = vec![0; 14];
        body[..4].copy_from_slice(&victim.to_be_bytes());
        body[4..8].copy_from_slice(&source.to_be_bytes());
        Self::new(0x430, body)
    }
    pub fn health_impact(
        target: u32,
        source: u32,
        operand: f32,
        word: u16,
        byte: u8,
        flag: u8,
    ) -> Self {
        let mut body = vec![0; 22];
        body[..4].copy_from_slice(&target.to_be_bytes());
        body[4..8].copy_from_slice(&source.to_be_bytes());
        body[8..12].copy_from_slice(&operand.to_be_bytes());
        body[12..14].copy_from_slice(&word.to_be_bytes());
        body[14] = byte;
        body[15] = flag;
        Self::new(0x41e, body)
    }
    pub fn fields(&self) -> io::Result<Fields<'_>> {
        let b = &self.body;
        let float = |i: usize| -> io::Result<f32> {
            Ok(f32::from_be_bytes(
                b.get(i..i + 4)
                    .ok_or(invalid("truncated float"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        let eid = || -> io::Result<u16> {
            Ok(u16::from_be_bytes(
                b.get(2..4)
                    .ok_or(invalid("truncated entity"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        let number = |i: usize| -> io::Result<u32> {
            Ok(u32::from_be_bytes(
                b.get(i..i + 4)
                    .ok_or(invalid("missing u32 field"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        let word = |i: usize| -> io::Result<u16> {
            Ok(u16::from_be_bytes(
                b.get(i..i + 2)
                    .ok_or(invalid("missing u16 field"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        Ok(match self.opcode {
            0 => Fields::Clock(float(0)?),
            0x430 => Fields::ActorDeath {
                victim: number(0)?,
                source: number(4)?,
                remaining: &b[8..],
            },
            0x41e => Fields::HealthImpact {
                target: number(0)?,
                source: number(4)?,
                operand: float(8)?,
                word: word(12)?,
                byte: *b.get(14).ok_or(invalid("missing impact byte"))?,
                flag: *b.get(15).ok_or(invalid("missing impact flag"))?,
                remaining: &b[16..],
            },
            0x3e9 => Fields::GameSetup {
                first_word: number(0)?,
                mode: b
                    .get(32..96)
                    .ok_or(invalid("truncated game setup"))?
                    .split(|&v| v == 0)
                    .next()
                    .unwrap_or_default(),
                remaining: &b[96..],
            },
            0x3f8 if matches!(b.len(), 9 | 14) => Fields::MoveAction {
                selector: b[0],
                x: float(1)?,
                y: float(5)?,
                remaining: &b[9..],
            },
            0x42b | 0x43e | 0x43f => Fields::EntityPrefix {
                kind: Opcode::try_from(self.opcode).expect("matched known opcode"),
                entity: number(0)?,
                remaining: &b[4..],
            },
            0x3ed => Fields::HandleRecord {
                text: b
                    .get(..64)
                    .ok_or(invalid("truncated name field"))?
                    .split(|&v| v == 0)
                    .next()
                    .unwrap_or_default(),
                field64: number(64)?,
                lookup: number(68)?,
                remaining: &b[72..],
            },
            0x46f => Fields::NameField {
                kind: Opcode::ModeName,
                text: b
                    .get(..64)
                    .ok_or(invalid("truncated name field"))?
                    .split(|&v| v == 0)
                    .next()
                    .unwrap_or_default(),
                remaining: &b[64..],
            },
            0x45c => {
                let raw = b.get(..96).ok_or(invalid("truncated control entries"))?;
                let entries = std::array::from_fn(|i| {
                    let slot = &raw[i * 6..][..6];
                    ControlEntry {
                        reference: u32::from_be_bytes(slot[..4].try_into().unwrap()),
                        gate: slot[4],
                        flag: slot[5],
                    }
                });
                Fields::ControlTable {
                    entries,
                    remaining: &b[96..],
                }
            }
            0x471 => Fields::ReferencedFlag {
                reference: number(0)?,
                flag: *b.get(4).ok_or(invalid("truncated control flag"))?,
                remaining: &b[5..],
            },
            0x458 | 0x46d | 0x46e | 0x470 | 0x47c => {
                let (flag, remaining) = b.split_first().ok_or(invalid("truncated control byte"))?;
                Fields::ControlByte {
                    value: *flag,
                    remaining,
                }
            }
            0x3f2 if matches!(b.len(), 122 | 126) => Fields::CompactSpawn {
                template: number(0)?,
                skin: number(4)?,
                entity: number(8)?,
                remaining: &b[12..],
            },
            0x45a => Fields::AralRosterHeader {
                remaining_seconds: float(0)?,
                allotted_seconds: float(4)?,
                remaining: &b[8..],
            },
            0x459 => Fields::DraftRoster(DraftRoster::new(b)?),
            0x41c => Fields::StatModifier {
                actor: number(0)?,
                source: number(4)?,
                value: float(8)?,
                stat: *b.get(12).ok_or(invalid("missing modifier stat"))?,
                flags: *b.get(13).ok_or(invalid("missing modifier flags"))?,
            },
            0x44b => Fields::ItemRemoval {
                actor: number(0)?,
                instance: number(4)?,
                flags: word(8)?,
            },
            0x43d => Fields::ItemGrant {
                actor: number(0)?,
                item: number(4)?,
                instance: number(8)?,
            },
            0x45f => Fields::SkinSelection(number(0)?),
            0x439 => Fields::ShopRequest {
                actor: number(0)?,
                item: number(4)?,
            },
            0x45e => Fields::HeroSelection {
                template: number(0)?,
                skin: number(4)?,
            },
            0x3f3 => Fields::Spawn {
                template: number(0)?,
                skin: number(4)?,
                entity: number(8)?,
                team: number(12)?,
                x: float(18)?,
                height: float(22)?,
                z: float(26)?,
                hp: float(42)?,
                max_hp: float(46)?,
            },
            0x3ee => Fields::PlayerInfo {
                entity: number(160)?,
                template: number(164)?,
                skin: number(168)?,
            },

            6 => Fields::Heartbeat,
            0x455 | 0x463 => Fields::HeroLock,
            0x3f4 => Fields::Movement {
                x: float(0)?,
                y: float(4)?,
            },
            0x42e => Fields::Position {
                entity: eid()?,
                x: float(4)?,
                y: float(8)?,
            },
            0x41d => Fields::EntityStat {
                entity: eid()?,
                value: float(4)?,
                stat: *b.get(8).ok_or(invalid("missing stat type"))?,
            },
            0x3e8 | 0x453 | 0x454 => {
                let offset = if self.opcode == 0x454 { 4 } else { 0 };
                let raw = b.get(offset..).ok_or(invalid("truncated string header"))?;
                let name = std::str::from_utf8(raw.split(|v| *v == 0).next().unwrap())
                    .map_err(|_| invalid("invalid text"))?;
                match self.opcode {
                    0x3e8 => Fields::PlayerUuid(name),
                    0x453 => Fields::HeroCatalog(name),
                    _ => Fields::GameMode(name),
                }
            }
            _ => Fields::Opaque(b),
        })
    }
}
#[derive(Debug, PartialEq)]
pub struct ControlEntry {
    pub reference: u32,
    pub gate: u8,
    pub flag: u8,
}

#[derive(Debug, PartialEq)]
pub enum Fields<'a> {
    ActorDeath {
        victim: u32,
        source: u32,
        remaining: &'a [u8],
    },
    HealthImpact {
        target: u32,
        source: u32,
        operand: f32,
        word: u16,
        byte: u8,
        flag: u8,
        remaining: &'a [u8],
    },
    GameSetup {
        first_word: u32,
        mode: &'a [u8],
        remaining: &'a [u8],
    },
    MoveAction {
        selector: u8,
        x: f32,
        y: f32,
        remaining: &'a [u8],
    },
    HandleRecord {
        text: &'a [u8],
        field64: u32,
        lookup: u32,
        remaining: &'a [u8],
    },
    ControlTable {
        entries: [ControlEntry; 16],
        remaining: &'a [u8],
    },
    ReferencedFlag {
        reference: u32,
        flag: u8,
        remaining: &'a [u8],
    },
    ControlByte {
        value: u8,
        remaining: &'a [u8],
    },
    EntityPrefix {
        kind: Opcode,
        entity: u32,
        remaining: &'a [u8],
    },
    NameField {
        kind: Opcode,
        text: &'a [u8],
        remaining: &'a [u8],
    },
    CompactSpawn {
        template: u32,
        skin: u32,
        entity: u32,
        remaining: &'a [u8],
    },
    AralRosterHeader {
        remaining_seconds: f32,
        allotted_seconds: f32,
        remaining: &'a [u8],
    },
    DraftRoster(DraftRoster<'a>),
    StatModifier {
        actor: u32,
        source: u32,
        value: f32,
        stat: u8,
        flags: u8,
    },
    ItemRemoval {
        actor: u32,
        instance: u32,
        flags: u16,
    },
    ItemGrant {
        actor: u32,
        item: u32,
        instance: u32,
    },
    SkinSelection(u32),
    ShopRequest {
        actor: u32,
        item: u32,
    },
    HeroSelection {
        template: u32,
        skin: u32,
    },
    Spawn {
        template: u32,
        skin: u32,
        entity: u32,
        team: u32,
        x: f32,
        height: f32,
        z: f32,
        hp: f32,
        max_hp: f32,
    },
    PlayerInfo {
        entity: u32,
        template: u32,
        skin: u32,
    },
    Clock(f32),
    Heartbeat,
    HeroLock,
    Movement {
        x: f32,
        y: f32,
    },
    Position {
        entity: u16,
        x: f32,
        y: f32,
    },
    EntityStat {
        entity: u16,
        value: f32,
        stat: u8,
    },
    PlayerUuid(&'a str),
    HeroCatalog(&'a str),
    GameMode(&'a str),
    Opaque(&'a [u8]),
}

/// Five-versus-five selection roster with fixed-stride slots; coordinates
/// here are relative to the decrypted message body (the opcode is excluded).
#[derive(Debug, PartialEq)]
pub struct DraftRoster<'a>(&'a [u8]);
#[derive(Debug, PartialEq)]
pub struct DraftSlot<'a> {
    pub occupied: bool,
    pub slot: u8,
    pub selected: bool,
    pub locked: bool,
    pub team: u8,
    pub template: u16,
    pub entity: u16,
    pub skin: u32,
    pub display_name: &'a [u8],
}
impl<'a> DraftRoster<'a> {
    pub fn new(bytes: &'a [u8]) -> io::Result<Self> {
        if bytes.len() < 16 * 161 + 8 {
            return Err(invalid("truncated draft roster"));
        }
        Ok(Self(bytes))
    }
    pub fn remaining_seconds(&self) -> f32 {
        f32::from_be_bytes(self.0[..4].try_into().unwrap())
    }
    pub fn allotted_seconds(&self) -> f32 {
        f32::from_be_bytes(self.0[4..8].try_into().unwrap())
    }
    pub fn slot(&self, i: usize) -> Option<DraftSlot<'a>> {
        if i >= 16 {
            return None;
        }
        let b = &self.0[i * 161..][..161];
        Some(DraftSlot {
            occupied: b[8] != 0,
            slot: b[9],
            selected: b[10] != 0,
            locked: b[11] != 0,
            team: b[16],
            template: u16::from_be_bytes(b[17..19].try_into().unwrap()),
            entity: u16::from_be_bytes(b[19..21].try_into().unwrap()),
            skin: u32::from_be_bytes(b[21..25].try_into().unwrap()),
            display_name: b[25..57].split(|v| *v == 0).next().unwrap(),
        })
    }
}

pub fn hello(address: &str) -> io::Result<Vec<u8>> {
    hello_with_port(address, 0)
}
pub fn hello_with_port(address: &str, port: u16) -> io::Result<Vec<u8>> {
    if address.len() > 128 || !address.is_ascii() || address.as_bytes().contains(&0) {
        return Err(invalid("invalid hello address"));
    }
    let mut frame = vec![0; 136];
    frame[..4].copy_from_slice(&[0, 134, 0, 5]);
    frame[4..4 + address.len()].copy_from_slice(address.as_bytes());
    frame[132..].copy_from_slice(&u32::from(port).to_be_bytes());
    Ok(frame)
}
pub fn parse_hello(payload: &[u8]) -> io::Result<&str> {
    if payload.len() != 134 || payload[..2] != [0, 5] {
        return Err(invalid("invalid hello frame"));
    }
    let len = payload[2..130].iter().position(|&x| x == 0).unwrap_or(128);
    let address = &payload[2..2 + len];
    if !address.is_ascii() || address.contains(&0) || payload[2 + len..130].iter().any(|v| *v != 0)
    {
        return Err(invalid("invalid hello address field"));
    }
    Ok(std::str::from_utf8(address).unwrap())
}

pub struct MatchCipher(BlowfishLE);
impl MatchCipher {
    pub fn new(match_id: &str) -> Self {
        let salt = [
            0x467c46341a2f5f1eu64,
            0xa778c8d74b1ca88b,
            0x459d33ab9685e0e3,
            0xf378e7b493322ceb,
            0x4036be8b31396d33,
            0x30ddaa6d7031415e,
            0xfe903f60be8834c5,
            0x3299e3e8877c3a26,
        ];
        let mut hash = Md5::new();
        for word in salt {
            hash.update(word.to_be_bytes());
        }
        hash.update(match_id.as_bytes());
        Self(BlowfishLE::new_from_slice(&hash.finalize()).expect("MD5 digest fits Blowfish"))
    }
    pub fn encode(&self, msg: &Message) -> io::Result<Vec<u8>> {
        let mut frame = Vec::new();
        self.encode_into(msg, &mut frame)?;
        Ok(frame)
    }

    /// Append a length-prefixed encrypted frame to an existing send buffer.
    /// An invalid frame leaves the caller's buffer intact.
    pub fn encode_into(&self, msg: &Message, frame: &mut Vec<u8>) -> io::Result<()> {
        if msg.body.len() > MAX_PAYLOAD - 2 {
            return Err(invalid("oversized message"));
        }
        let len = (2 + msg.body.len()).next_multiple_of(8);
        if len > MAX_PAYLOAD {
            return Err(invalid("oversized encrypted frame"));
        }
        let start = frame.len();
        frame.reserve(len + 2);
        frame.extend((len as u16).to_be_bytes());
        frame.extend(msg.opcode.to_be_bytes());
        frame.extend(&msg.body);
        frame.resize(start + len + 2, 0);
        for block in frame[start + 2..].as_chunks_mut::<8>().0 {
            self.0.encrypt_block(block.into());
        }
        Ok(())
    }
    pub fn decode(&self, payload: &[u8]) -> io::Result<Message> {
        if payload.len() > MAX_PAYLOAD {
            return Err(invalid("invalid encrypted frame length"));
        }
        self.decode_owned(payload.to_vec())
    }

    /// Reuse the transport's received allocation for the decrypted body.
    pub fn decode_owned(&self, mut plain: Vec<u8>) -> io::Result<Message> {
        if plain.is_empty() || plain.len() > MAX_PAYLOAD || !plain.len().is_multiple_of(8) {
            return Err(invalid("invalid encrypted frame length"));
        }
        for block in plain.as_chunks_mut::<8>().0 {
            self.0.decrypt_block(block.into());
        }
        let opcode = u16::from_be_bytes([plain[0], plain[1]]);
        plain.copy_within(2.., 0);
        plain.truncate(plain.len() - 2);
        Ok(Message::new(opcode, plain))
    }
}

#[derive(Debug, PartialEq)]
pub struct ReplayRecord {
    pub time: f32,
    pub message: Message,
}
/// Big-endian VGR timestamp, opcode-inclusive payload length, opcode, and body.
pub struct ReplayWriter<W>(W);
impl<W: Write> ReplayWriter<W> {
    pub fn new(writer: W) -> Self {
        Self(writer)
    }
    pub fn write_record(&mut self, time: f32, message: &Message) -> io::Result<()> {
        let length = message
            .body
            .len()
            .checked_add(2)
            .filter(|n| *n <= 1024 * 1024)
            .ok_or_else(|| invalid("replay record too large"))?;
        self.0.write_all(&time.to_be_bytes())?;
        self.0.write_all(&(length as u32).to_be_bytes())?;
        self.0.write_all(&message.opcode.to_be_bytes())?;
        self.0.write_all(&message.body)
    }
    pub fn into_inner(self) -> W {
        self.0
    }
}
pub struct ReplayReader<R>(R);
impl<R: Read> ReplayReader<R> {
    pub fn new(reader: R) -> Self {
        Self(reader)
    }
    pub fn next_record(&mut self) -> io::Result<Option<ReplayRecord>> {
        let mut h = [0; 8];
        if !header(&mut self.0, &mut h)? {
            return Ok(None);
        }
        let time = f32::from_be_bytes(h[..4].try_into().unwrap());
        let len = u32::from_be_bytes(h[4..].try_into().unwrap()) as usize;
        let data = payload(&mut self.0, len, 1024 * 1024)?;
        Ok(Some(ReplayRecord {
            time,
            message: Message::from_plain(&data)?,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn health_and_death_keep_raw_presentation_fields_and_padding() {
        let data = [0, 1, 0, 2, 0, 3, 0, 4, 0xc0, 0x60, 0, 0, 0, 74, 2, 1, 0xf1];
        let msg = Message::new(0x41e, data);
        assert_eq!(
            msg.fields().unwrap(),
            Fields::HealthImpact {
                target: 65538,
                source: 196612,
                operand: -3.5,
                word: 74,
                byte: 2,
                flag: 1,
                remaining: &[0xf1]
            }
        );
        assert!(Message::new(0x41e, &data[..15]).fields().is_err());
        assert_eq!(
            Message::new(0x430, &data[..9]).fields().unwrap(),
            Fields::ActorDeath {
                victim: 65538,
                source: 196612,
                remaining: &[0xc0]
            }
        );
        assert!(Message::new(0x430, &data[..7]).fields().is_err());
    }
    #[test]
    fn read_only_prefixes_and_compact_spawn_variants() {
        for (code, kind) in [
            (0x42b, Opcode::EntityState),
            (0x43e, Opcode::EntityProp),
            (0x43f, Opcode::EntityData),
        ] {
            assert_eq!(
                Message::new(code, [0x12, 0x34, 0x56, 0x78, 0xab])
                    .fields()
                    .unwrap(),
                Fields::EntityPrefix {
                    kind,
                    entity: 0x12345678,
                    remaining: &[0xab]
                }
            );
            assert!(Message::new(code, [0, 1, 2]).fields().is_err());
        }
        for size in [122, 126] {
            let mut body = vec![0xfe; size];
            body[..12].copy_from_slice(&[0, 1, 0, 5, 0, 2, 0, 6, 0, 3, 0, 7]);
            assert_eq!(
                Message::new(0x3f2, body.clone()).fields().unwrap(),
                Fields::CompactSpawn {
                    template: 65541,
                    skin: 131078,
                    entity: 196615,
                    remaining: &body[12..]
                }
            );
        }
        assert!(matches!(
            Message::new(0x3f2, [0; 8]).fields().unwrap(),
            Fields::Opaque(_)
        ));
        let mut body = vec![0; 74];
        body[..5].copy_from_slice(b"Guest");
        body[64] = 10;
        assert_eq!(
            Message::new(0x3ed, body.clone()).fields().unwrap(),
            Fields::HandleRecord {
                text: b"Guest",
                field64: 10 << 24,
                lookup: 0,
                remaining: &body[72..]
            }
        );
        assert_eq!(
            Message::new(0x46f, body.clone()).fields().unwrap(),
            Fields::NameField {
                kind: Opcode::ModeName,
                text: b"Guest",
                remaining: &body[64..]
            }
        );
        let mut table = vec![0; 102];
        table[90..96].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        let msg = Message::new(0x45c, table);
        let Fields::ControlTable { entries, remaining } = msg.fields().unwrap() else {
            panic!()
        };
        assert_eq!(
            entries[15],
            ControlEntry {
                reference: 0x01020304,
                gate: 5,
                flag: 6
            }
        );
        assert_eq!(remaining, [0; 6]);
        assert!(Message::new(0x45c, [0; 95]).fields().is_err());
        assert_eq!(
            Message::new(0x471, [0, 0, 0, 7, 1, 3]).fields().unwrap(),
            Fields::ReferencedFlag {
                reference: 7,
                flag: 1,
                remaining: &[3]
            }
        );
        let action = Message::new(0x3f8, [5, 0x3f, 0x80, 0, 0, 0xc0, 0, 0, 0]);
        assert_eq!(
            action.fields().unwrap(),
            Fields::MoveAction {
                selector: 5,
                x: 1.,
                y: -2.,
                remaining: &[]
            }
        );
        let roster = Message::new(0x45a, [0x41, 0xa0, 0, 0, 0x41, 0xf0, 0, 0, 0x7f]);
        assert_eq!(
            roster.fields().unwrap(),
            Fields::AralRosterHeader {
                remaining_seconds: 20.,
                allotted_seconds: 30.,
                remaining: &[0x7f]
            }
        );
        assert!(Message::new(0x45a, [0; 7]).fields().is_err());
    }
    #[test]
    fn inventory_event_fields_preserve_source_and_flags() {
        let modifier = [0, 0, 0, 37, 0, 0, 0, 23, 0xc1, 0x48, 0, 0, 7, 2];
        let removal = [0, 0, 0, 37, 0, 0, 0x11, 0x22, 0, 1];
        let decoded = Message::new(0x41c, modifier);
        assert_eq!(
            decoded.fields().unwrap(),
            Fields::StatModifier {
                actor: 37,
                source: 23,
                value: -12.5,
                stat: 7,
                flags: 2
            }
        );
        assert_eq!(
            Message::new(0x44b, removal).fields().unwrap(),
            Fields::ItemRemoval {
                actor: 37,
                instance: 0x1122,
                flags: 1
            }
        );
        assert!(Message::new(0x41c, &modifier[..13]).fields().is_err());
        assert!(Message::new(0x44b, &removal[..9]).fields().is_err());
    }

    #[test]
    fn independent_crypto_vector() {
        // Computed using PyCryptodome standard Blowfish with LE word conversion.
        let cipher = MatchCipher::new("local-test-match");
        let wire = [
            0, 16, 0x9d, 0x7a, 0xf6, 0x30, 0xab, 0xbf, 0xfb, 0x6d, 0xcd, 0x97, 0x44, 0xe0, 0x77,
            0xf2, 0xd7, 0xfd,
        ];
        assert_eq!(cipher.encode(&Message::movement(1.5, -2.5)).unwrap(), wire);
        let decoded = cipher.decode(&wire[2..]).unwrap();
        assert_eq!(
            decoded.fields().unwrap(),
            Fields::Movement { x: 1.5, y: -2.5 }
        );
        assert_eq!(cipher.encode(&decoded).unwrap(), wire);
    }
    #[test]
    fn frames_and_hello() {
        for addr in ["", "127.0.0.1"] {
            let bytes = [hello(addr).unwrap(), ACK_FRAME.to_vec()].concat();
            let mut r = bytes.as_slice();
            assert_eq!(
                parse_hello(&read_frame(&mut r).unwrap().unwrap()).unwrap(),
                addr
            );
            assert_eq!(read_frame(&mut r).unwrap().unwrap(), [0, 6, 0]);
            assert!(read_frame(&mut r).unwrap().is_none());
        }
        for mut bytes in [&[0u8][..], &[0, 3, 1][..], &[255, 255][..], &[0, 1, 0][..]] {
            assert!(read_frame(&mut bytes).is_err());
        }
    }
    #[test]
    fn replay_boundaries() {
        let b = [0x40, 0, 0, 0, 0, 0, 0, 3, 0xff, 0x11, 0];
        let mut writer = ReplayWriter::new(Vec::new());
        writer.write_record(2., &Message::new(0xff11, [0])).unwrap();
        assert_eq!(writer.into_inner(), b);
        let mut reader = ReplayReader::new(b.as_slice());
        assert_eq!(
            reader.next_record().unwrap(),
            Some(ReplayRecord {
                time: 2.,
                message: Message::new(0xff11, [0])
            })
        );
        assert!(reader.next_record().unwrap().is_none());
        for cut in 1..b.len() {
            assert!(ReplayReader::new(&b[..cut]).next_record().is_err());
        }
        assert!(ReplayReader::new(&[255; 8][..]).next_record().is_err());
        assert_eq!(READ_SIZES.len(), 171);
    }
}
