//! Asynchronous CE TCP framing. Frame lengths count payload bytes, excluding the prefix.
use crate::MAX_PAYLOAD;
use bytes::{Buf, Bytes, BytesMut};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

/// Keep one reader per connection, including across plaintext handshakes and socket splits.
/// A cancelled read retains both its partial frame and any bytes already read from the next frame.
pub struct FrameReader {
    codec: LengthDelimitedCodec,
    buffer: BytesMut,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameReader {
    pub fn new() -> Self {
        Self {
            // Retain the prefix until the whole payload arrives. With the codec's default
            // skip, EOF immediately after a complete prefix could leave an empty buffer.
            codec: LengthDelimitedCodec::builder()
                .big_endian()
                .length_field_type::<u16>()
                .max_frame_length(MAX_PAYLOAD)
                .length_adjustment(2)
                .num_skip(0)
                .new_codec(),
            buffer: BytesMut::with_capacity(2048),
        }
    }

    /// Returns None only at EOF between frames. Partial prefixes and bodies return UnexpectedEof.
    pub async fn next<R: AsyncRead + Unpin>(
        &mut self,
        input: &mut R,
    ) -> io::Result<Option<BytesMut>> {
        loop {
            if matches!(self.buffer.get(..2), Some([0, 0 | 1])) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE payload needs an opcode",
                ));
            }
            if let Some(mut frame) = self.codec.decode(&mut self.buffer)? {
                frame.advance(2);
                return Ok(Some(frame));
            }
            if input.read_buf(&mut self.buffer).await? == 0 {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete CE frame",
                ));
            }
        }
    }

    /// Use during handshakes and streams which require another complete frame.
    pub async fn required<R: AsyncRead + Unpin>(&mut self, input: &mut R) -> io::Result<BytesMut> {
        self.next(input)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "CE connection closed"))
    }
}

/// Write individual plaintext or ciphertext payloads. Already-framed encrypted batches can
/// be passed directly to the transport in one write without copying them into this buffer.
pub struct FrameWriter {
    codec: LengthDelimitedCodec,
    buffer: BytesMut,
}

impl Default for FrameWriter {
    fn default() -> Self {
        Self {
            codec: LengthDelimitedCodec::builder()
                .big_endian()
                .length_field_type::<u16>()
                .max_frame_length(MAX_PAYLOAD)
                .new_codec(),
            buffer: BytesMut::new(),
        }
    }
}

impl FrameWriter {
    /// Writes one complete frame. On a cancelled or failed write the caller must close the stream.
    pub async fn write<W: AsyncWrite + Unpin>(
        &mut self,
        output: &mut W,
        payload: Bytes,
    ) -> io::Result<()> {
        if !(2..=MAX_PAYLOAD).contains(&payload.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid CE payload length",
            ));
        }
        self.buffer.clear();
        self.codec.encode(payload, &mut self.buffer)?;
        output.write_all(&self.buffer).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original plaintext ACK and an independent minimal word frame, each with a BE16 prefix.
    const ACK: &[u8] = &[0, 3, 0, 6, 0];
    const MINIMUM: &[u8] = &[0, 2, 0xab, 0xcd];

    #[tokio::test]
    async fn fragmented_and_coalesced_frames_preserve_boundaries() {
        let (mut sender, mut socket) = tokio::io::duplex(1);
        let source = tokio::spawn(async move {
            for byte in ACK.iter().chain(MINIMUM) {
                sender.write_all(&[*byte]).await.unwrap();
            }
        });
        let mut reader = FrameReader::new();
        assert_eq!(&reader.required(&mut socket).await.unwrap()[..], &[0, 6, 0]);
        assert_eq!(
            &reader.required(&mut socket).await.unwrap()[..],
            &[0xab, 0xcd]
        );
        source.await.unwrap();
        assert!(reader.next(&mut socket).await.unwrap().is_none());

        let both = [MINIMUM, ACK].concat();
        let mut stream = both.as_slice();
        let mut reader = FrameReader::new();
        assert_eq!(
            &reader.required(&mut stream).await.unwrap()[..],
            &[0xab, 0xcd]
        );
        // ACK was prefetched in the first read. Preserve the decoder when switching readers.
        assert!(stream.is_empty());
        let mut after_split = tokio::io::empty();
        assert_eq!(
            &reader.required(&mut after_split).await.unwrap()[..],
            &[0, 6, 0]
        );
        assert!(reader.next(&mut after_split).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelled_read_can_resume_after_the_length_prefix() {
        let (mut sender, mut socket) = tokio::io::duplex(16);
        let mut reader = FrameReader::new();
        sender.write_all(&ACK[..2]).await.unwrap();
        tokio::select! {
            biased;
            _ = reader.next(&mut socket) => panic!("prefix alone cannot complete this frame"),
            _ = std::future::ready(()) => {}
        }
        sender.write_all(&ACK[2..]).await.unwrap();
        assert_eq!(
            reader.required(&mut socket).await.unwrap().as_ref(),
            &ACK[2..]
        );
    }

    #[tokio::test]
    async fn clean_eof_truncation_and_invalid_lengths_are_distinct() {
        let mut reader = FrameReader::new();
        assert!(reader.next(&mut &[][..]).await.unwrap().is_none());
        for prefix in [&[0][..], &[0, 3], &[0, 3, 0], &[0, 3, 0, 6]] {
            let error = FrameReader::new().next(&mut &prefix[..]).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        }
        for prefix in [[0, 0], [0, 1], [0x27, 0xff], [0xff, 0xff]] {
            let error = FrameReader::new().next(&mut &prefix[..]).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }

        let mut maximum = vec![0x5a; 0x2800];
        maximum[..2].copy_from_slice(&[0x27, 0xfe]);
        let body = FrameReader::new()
            .required(&mut &maximum[..])
            .await
            .unwrap();
        assert_eq!(body.as_ref(), &maximum[2..]);
    }

    #[tokio::test]
    async fn writer_matches_independent_wire_bytes_and_rejects_invalid_sizes() {
        let mut output = Vec::new();
        let mut writer = FrameWriter::default();
        writer
            .write(&mut output, Bytes::from_static(&[0, 6, 0]))
            .await
            .unwrap();
        writer
            .write(&mut output, Bytes::from_static(&[0xab, 0xcd]))
            .await
            .unwrap();
        assert_eq!(output, [ACK, MINIMUM].concat());
        output.clear();
        for len in [0, 1, 0x27ff, 0xffff] {
            let err = writer
                .write(&mut output, vec![0; len].into())
                .await
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
            assert!(output.is_empty());
        }
        writer
            .write(&mut output, vec![0xa5; 0x27fe].into())
            .await
            .unwrap();
        assert_eq!(&output[..2], &[0x27, 0xfe]);
        assert_eq!(output.len(), 0x2800);
        assert!(output[2..].iter().all(|&byte| byte == 0xa5));
    }
}
