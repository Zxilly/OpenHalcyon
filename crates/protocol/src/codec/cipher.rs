//! Blowfish-LE match cipher and plaintext client-hello handshake framing.

use crate::codec::{MAX_PAYLOAD, invalid};
use crate::{FixedAscii, Message, Opcode};
use binrw::{BinRead, BinWrite};
use blowfish::{
    BlowfishLE,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit},
};
use md5::{Digest, Md5};
use std::io::{self, Cursor};

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct HelloFrameWire {
    frame_len: u16,
    opcode: u16,
    address: FixedAscii<128>,
    port_word: u32,
}

#[derive(BinRead)]
#[br(big)]
struct HelloPayloadWire {
    opcode: u16,
    address: FixedAscii<128>,
    port_word: u32,
}

pub fn hello(address: &str) -> io::Result<Vec<u8>> {
    hello_with_port(address, 0)
}

pub fn hello_with_port(address: &str, port: u16) -> io::Result<Vec<u8>> {
    if address.len() > 128 || !address.is_ascii() || address.as_bytes().contains(&0) {
        return Err(invalid("invalid hello address"));
    }
    let wire = HelloFrameWire {
        frame_len: 134,
        opcode: Opcode::ClientHello.raw(),
        address: FixedAscii::from_str_trunc(address),
        port_word: u32::from(port),
    };
    let mut frame = vec![0; 136];
    wire.write_be(&mut Cursor::new(&mut frame[..]))
        .map_err(|_| invalid("failed to encode hello frame"))?;
    Ok(frame)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloEndpoint<'a> {
    pub address: &'a str,
    pub port_word: u32,
}

impl HelloEndpoint<'_> {
    pub fn port(self) -> Option<u16> {
        self.port_word.try_into().ok()
    }
}

pub fn parse_hello(payload: &[u8]) -> io::Result<&str> {
    Ok(parse_hello_endpoint(payload)?.address)
}

pub fn parse_hello_endpoint(payload: &[u8]) -> io::Result<HelloEndpoint<'_>> {
    if payload.len() != 134 {
        return Err(invalid("invalid hello frame"));
    }
    let wire = HelloPayloadWire::read_be(&mut Cursor::new(payload))
        .map_err(|_| invalid("invalid hello frame"))?;
    if wire.opcode != Opcode::ClientHello.raw() {
        return Err(invalid("invalid hello frame"));
    }
    let raw_addr = wire.address.as_bytes();
    let len = raw_addr.iter().position(|&x| x == 0).unwrap_or(128);
    let addr_bytes = &raw_addr[..len];
    if !addr_bytes.is_ascii() || addr_bytes.contains(&0) || raw_addr[len..].iter().any(|&v| v != 0)
    {
        return Err(invalid("invalid hello address field"));
    }
    Ok(HelloEndpoint {
        address: std::str::from_utf8(&payload[2..2 + len]).unwrap(),
        port_word: wire.port_word,
    })
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

    /// Decrypt a single frame payload (excluding its two-byte length) in place.
    /// The raw opcode is preserved, and the borrowed body includes alignment bytes.
    /// Invalid lengths return an error before modifying the input.
    pub fn decrypt_in_place<'a>(&self, plain: &'a mut [u8]) -> io::Result<(u16, &'a [u8])> {
        if plain.is_empty() || plain.len() > MAX_PAYLOAD || !plain.len().is_multiple_of(8) {
            return Err(invalid("invalid encrypted frame length"));
        }
        for block in plain.as_chunks_mut::<8>().0 {
            self.0.decrypt_block(block.into());
        }
        Ok((u16::from_be_bytes([plain[0], plain[1]]), &plain[2..]))
    }

    /// Reuse the transport's received allocation for the decrypted body.
    pub fn decode_owned(&self, mut plain: Vec<u8>) -> io::Result<Message> {
        let (opcode, _) = self.decrypt_in_place(&mut plain)?;
        plain.copy_within(2.., 0);
        plain.truncate(plain.len() - 2);
        Ok(Message::new(opcode, plain))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ACK_FRAME, Fields, read_frame};

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
}
