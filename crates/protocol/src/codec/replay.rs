//! Big-endian VGR replay framing (`ReplayRecord`, `ReplayWriter`, `ReplayReader`).

use crate::Message;
use crate::codec::{header, invalid, payload};
use std::io::{self, Read, Write};

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
    use crate::READ_SIZES;

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
