//! Vainglory CE game transport. Decrypted messages preserve every byte,
//! including ambiguous padding, so unknown opcodes can be re-encoded exactly.
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit},
};
use md5::{Digest, Md5};
use std::io::{self, Read};

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
        Ok(match self.opcode {
            0 => Fields::Clock(float(0)?),
            6 => Fields::Heartbeat,
            0x455 => Fields::HeroLock,
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
pub enum Fields<'a> {
    Clock(f32),
    Heartbeat,
    HeroLock,
    Movement { x: f32, y: f32 },
    Position { entity: u16, x: f32, y: f32 },
    EntityStat { entity: u16, value: f32, stat: u8 },
    PlayerUuid(&'a str),
    HeroCatalog(&'a str),
    GameMode(&'a str),
    Opaque(&'a [u8]),
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
        if msg.body.len() > MAX_PAYLOAD - 2 {
            return Err(invalid("oversized message"));
        }
        let len = (2 + msg.body.len()).next_multiple_of(8);
        if len > MAX_PAYLOAD {
            return Err(invalid("oversized encrypted frame"));
        }
        let mut frame = Vec::with_capacity(len + 2);
        frame.extend((len as u16).to_be_bytes());
        frame.extend(msg.opcode.to_be_bytes());
        frame.extend(&msg.body);
        frame.resize(len + 2, 0);
        for block in frame[2..].chunks_exact_mut(8) {
            self.0.encrypt_block(block.into());
        }
        Ok(frame)
    }
    pub fn decode(&self, payload: &[u8]) -> io::Result<Message> {
        if payload.is_empty() || payload.len() > MAX_PAYLOAD || !payload.len().is_multiple_of(8) {
            return Err(invalid("invalid encrypted frame length"));
        }
        let mut plain = payload.to_vec();
        for block in plain.chunks_exact_mut(8) {
            self.0.decrypt_block(block.into());
        }
        Message::from_plain(&plain)
    }
}

#[derive(Debug, PartialEq)]
pub struct ReplayRecord {
    pub time: f32,
    pub message: Message,
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
