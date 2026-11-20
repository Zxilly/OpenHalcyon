//! Compare the CE receive paths on identical fragmented and coalesced streams.
use halcyon_protocol::{MAX_PAYLOAD, MatchCipher, Message, Opcode, async_frame::FrameReader};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::{
    alloc::System,
    hint::black_box,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FRAME_COUNT: usize = 65_536;

#[derive(Clone, Copy, Debug)]
enum Reader {
    Exact,
    BufferedOwned,
    BufferedBorrowed,
}

struct Source<'a> {
    bytes: &'a [u8],
    chunk: usize,
    reads: usize,
}

impl AsyncRead for Source<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        dst: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let size = self.chunk.min(dst.remaining()).min(self.bytes.len());
        dst.put_slice(&self.bytes[..size]);
        self.bytes = &self.bytes[size..];
        self.reads += 1;
        Poll::Ready(Ok(()))
    }
}

// Mirrors the pre-LengthDelimitedCodec game transport.
async fn exact(source: &mut Source<'_>) -> io::Result<Vec<u8>> {
    let size = usize::from(source.read_u16().await?);
    if !(2..=MAX_PAYLOAD).contains(&size) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; size];
    source.read_exact(&mut bytes).await?;
    Ok(bytes)
}

fn signature(opcode: u16, body: &[u8]) -> usize {
    usize::from(opcode) + body.len() + usize::from(body[0])
}

async fn run(
    input: &[u8],
    chunk: usize,
    method: Reader,
    cipher: &MatchCipher,
    expected: usize,
    print: bool,
) -> io::Result<()> {
    let mut source = Source {
        bytes: input,
        chunk,
        reads: 0,
    };
    let region = Region::new(GLOBAL);
    let start = Instant::now();
    let mut buffered = None;
    let mut total = 0;
    for _ in 0..FRAME_COUNT {
        total += match method {
            Reader::Exact => {
                let frame = cipher.decode_owned(exact(&mut source).await?)?;
                signature(frame.opcode, black_box(&frame.body))
            }
            Reader::BufferedOwned | Reader::BufferedBorrowed => {
                let reader = buffered.get_or_insert_with(FrameReader::new);
                let mut raw = reader.required(&mut source).await?;
                if matches!(method, Reader::BufferedOwned) {
                    let frame = cipher.decode_owned(raw.into())?;
                    signature(frame.opcode, black_box(&frame.body))
                } else {
                    let (opcode, body) = cipher.decrypt_in_place(&mut raw)?;
                    signature(opcode, black_box(body))
                }
            }
        };
    }
    let elapsed = start.elapsed().as_secs_f64();
    let stats = region.change();
    assert!(source.bytes.is_empty());
    assert_eq!(total, expected);
    if print {
        println!(
            "{method:?},{chunk},{FRAME_COUNT},{elapsed:.9},{},{},{},{}",
            source.reads, stats.allocations, stats.reallocations, stats.bytes_allocated
        );
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let cipher = MatchCipher::new("frame read benchmark");
    let messages = [
        Message::clock(2.),
        Message::position(1524, -5.25, 3.),
        Message::health_impact(1524, 1520, -10., 1, 0, 0),
        Message::known(Opcode::FireDirectedProjectile, [0x42; 44]),
    ];
    let mut input = Vec::new();
    let mut expected = 0;
    for index in 0..FRAME_COUNT {
        let message = &messages[index % messages.len()];
        let start = input.len();
        cipher.encode_into(message, &mut input)?;
        // Cipher payload includes the opcode, native body, and final block padding.
        expected += signature(message.opcode, &message.body)
            + (input.len() - start - 4 - message.body.len());
    }
    println!("reader,chunk,frames,seconds,source_reads,allocations,reallocations,bytes_allocated");
    for round in 0..=6 {
        for chunk in [1, 16, 256, 8192] {
            let mut methods = [
                Reader::Exact,
                Reader::BufferedOwned,
                Reader::BufferedBorrowed,
            ];
            methods.rotate_left(round % 3);
            for method in methods {
                run(&input, chunk, method, &cipher, expected, round != 0).await?;
            }
        }
    }
    Ok(())
}
