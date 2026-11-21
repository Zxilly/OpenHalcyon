//! Synchronous framing, Blowfish match cipher, wire text, and replay codecs.

pub mod async_frame;
pub(crate) mod cipher;
pub(crate) mod fixed_body;
pub(crate) mod message;
pub(crate) mod replay;
pub mod wire_text;

use std::io::{self, Read};

pub const MAX_PAYLOAD: usize = 0x2800 - 2;
pub const ACK_FRAME: [u8; 5] = [0, 3, 0, 6, 0];

pub(crate) fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

// Read one byte separately to distinguish record-boundary EOF from truncation.
pub(crate) fn header<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<bool> {
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

pub(crate) fn payload<R: Read>(reader: &mut R, len: usize, limit: usize) -> io::Result<Vec<u8>> {
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
