//! Big-endian fixed prefixes. A caller may retain trailing native fields or
//! block padding in its own buffer; writing a prefix never changes that suffix.
use std::io::{self, Cursor};

use crate::codec::invalid;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};

#[inline]
pub(crate) const fn padded_len(prefix_len: usize) -> usize {
    (prefix_len + 2).next_multiple_of(8) - 2
}

#[inline]
pub(crate) fn read<T>(body: &[u8]) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    T::read_be(&mut Cursor::new(body)).map_err(|_| invalid("truncated fixed wire body"))
}

#[inline]
pub(crate) fn read_exact<T>(body: &[u8], len: usize) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    if body.len() != len {
        return Err(invalid("unexpected fixed wire body length"));
    }
    read(body)
}

#[inline]
pub(crate) fn read_with_padding<T>(body: &[u8], prefix_len: usize) -> io::Result<(T, &[u8])>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    let padded = padded_len(prefix_len);
    if body.len() != prefix_len && body.len() != padded {
        return Err(invalid("unexpected fixed wire body length"));
    }
    let (head, tail) = body.split_at(prefix_len);
    Ok((read(head)?, tail))
}

#[inline]
pub(crate) fn write<T>(value: &T, dst: &mut [u8], len: usize) -> io::Result<usize>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let out = dst
        .get_mut(..len)
        .ok_or_else(|| invalid("fixed wire output is too small"))?;
    let mut writer = Cursor::new(out);
    value
        .write_be(&mut writer)
        .map_err(|_| invalid("invalid fixed wire body size"))?;
    debug_assert_eq!(writer.position(), len as u64);
    Ok(len)
}

#[inline]
pub(crate) fn write_with_padding<T>(
    value: &T,
    prefix_len: usize,
    suffix: &[u8],
    dst: &mut [u8],
) -> io::Result<usize>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let total = prefix_len
        .checked_add(suffix.len())
        .filter(|&n| n == prefix_len || n == padded_len(prefix_len))
        .ok_or_else(|| invalid("unsupported fixed wire suffix length"))?;
    let out = dst
        .get_mut(..total)
        .ok_or_else(|| invalid("fixed wire output is too small"))?;
    write(value, out, prefix_len)?;
    if !suffix.is_empty() {
        out[prefix_len..].copy_from_slice(suffix);
    }
    Ok(total)
}

#[inline]
pub(crate) fn message<T>(opcode: Opcode, value: &T, prefix_len: usize, body_len: usize) -> Message
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut body = vec![0; body_len];
    write(value, &mut body, prefix_len).expect("fixed prefix fits the owned message body");
    Message::known(opcode, body)
}

#[inline]
pub(crate) fn message_with_padding<T>(
    opcode: Opcode,
    value: &T,
    prefix_len: usize,
    suffix: &[u8],
) -> io::Result<Message>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let total = prefix_len
        .checked_add(suffix.len())
        .filter(|&n| n == prefix_len || n == padded_len(prefix_len))
        .ok_or_else(|| invalid("unsupported fixed wire suffix length"))?;
    let mut body = vec![0; total];
    write(value, &mut body, prefix_len)?;
    if !suffix.is_empty() {
        body[prefix_len..].copy_from_slice(suffix);
    }
    Ok(Message::known(opcode, body))
}

macro_rules! fixed_prefix {
    ($ty:ty, $len:literal) => {
        impl $ty {
            pub const BODY_LEN: usize = $len;
            pub(crate) fn message(&self, opcode: crate::Opcode, body_len: usize) -> crate::Message {
                crate::codec::fixed_body::message(opcode, self, Self::BODY_LEN, body_len)
            }
            /// Reads the fixed prefix. Callers retain any remaining wire bytes.
            pub fn read_body(body: &[u8]) -> std::io::Result<Self> {
                crate::codec::fixed_body::read(body)
            }
            /// Writes the fixed prefix, leaving trailing destination bytes intact.
            pub fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize> {
                crate::codec::fixed_body::write(self, dst, Self::BODY_LEN)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use fixed_prefix;

macro_rules! fixed_packet {
    ($ty:ty, $opcode:expr, $len:literal) => {
        #[allow(dead_code)]
        impl $ty {
            pub const BODY_LEN: usize = $len;
            pub const OPCODE: crate::Opcode = $opcode;
            pub const PADDED_LEN: usize = crate::codec::fixed_body::padded_len($len);

            /// Reads the fixed prefix. Callers retain any remaining wire bytes.
            pub fn read_body(body: &[u8]) -> std::io::Result<Self> {
                crate::codec::fixed_body::read(body)
            }

            /// Writes the fixed prefix, leaving trailing destination bytes intact.
            pub fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize> {
                crate::codec::fixed_body::write(self, dst, Self::BODY_LEN)
            }

            /// Reads an exact native-length body (`BODY_LEN` bytes).
            pub fn read_exact(body: &[u8]) -> std::io::Result<Self> {
                crate::codec::fixed_body::read_exact(body, Self::BODY_LEN)
            }

            /// Reads either the native `BODY_LEN` or Blowfish-aligned `PADDED_LEN` body.
            pub fn read_exact_or_padded(body: &[u8]) -> std::io::Result<Self> {
                crate::codec::fixed_body::read_with_padding(body, Self::BODY_LEN)
                    .map(|(value, _)| value)
            }

            /// Reads a Blowfish-aligned `PADDED_LEN` body whose padding bytes are all zero.
            pub fn read_padded_zero(body: &[u8]) -> std::io::Result<Self> {
                let (value, tail) =
                    crate::codec::fixed_body::read_with_padding(body, Self::BODY_LEN)?;
                if body.len() != Self::PADDED_LEN || tail.iter().any(|&b| b != 0) {
                    return Err(crate::codec::invalid(
                        "expected zero-padded fixed wire body",
                    ));
                }
                Ok(value)
            }

            /// Reads the fixed prefix together with any native (`[]`) or Blowfish-aligned tail.
            pub fn read_with_padding(body: &[u8]) -> std::io::Result<(Self, &[u8])> {
                crate::codec::fixed_body::read_with_padding(body, Self::BODY_LEN)
            }

            /// Encodes the native `BODY_LEN`-byte frame.
            pub fn encode(&self) -> crate::Message {
                crate::codec::fixed_body::message(
                    Self::OPCODE,
                    self,
                    Self::BODY_LEN,
                    Self::BODY_LEN,
                )
            }

            /// Encodes the Blowfish-aligned `PADDED_LEN`-byte frame with zero padding.
            pub fn encode_padded(&self) -> crate::Message {
                crate::codec::fixed_body::message(
                    Self::OPCODE,
                    self,
                    Self::BODY_LEN,
                    Self::PADDED_LEN,
                )
            }

            /// Encodes the body while preserving an empty or Blowfish-aligned padding suffix.
            pub fn encode_with_padding(&self, suffix: &[u8]) -> std::io::Result<crate::Message> {
                crate::codec::fixed_body::message_with_padding(
                    Self::OPCODE,
                    self,
                    Self::BODY_LEN,
                    suffix,
                )
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use fixed_packet;
