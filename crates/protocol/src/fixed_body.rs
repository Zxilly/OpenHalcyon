//! Big-endian fixed prefixes. A caller may retain trailing native fields or
//! block padding in its own buffer; writing a prefix never changes that suffix.
use std::io::{self, Cursor};

use crate::{Message, Opcode, invalid};
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

/// The first eight body bytes of a native move command. X and Y are IEEE-754
/// words transmitted without coordinate normalization.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MoveCmd {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct PositionBody {
    pub actor: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DeathBody {
    pub victim: u32,
    pub source: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ImpactBody {
    pub target: u32,
    pub source: u32,
    pub operand: f32,
    pub word: u16,
    pub byte: u8,
    pub flag: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct VisibilityBody {
    pub actor: u32,
    pub index: u8,
    pub state: u8,
    pub masks: [u8; 2],
}

/// Observed `0x042C` camp minimap state byte (`(populated << 1) | visible`).
///
/// Android 147219 (`0x82aee0`) extracts only the low 2 bits via
/// `bfxil w3, w21, #0, #2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampVisibilityState {
    /// Camp has no living monsters (countdown or cleared) and is hidden from this index (`0`).
    EmptyHidden,
    /// Camp has no living monsters (countdown or cleared) and is visible to this index (`1`).
    EmptyVisible,
    /// Camp has living monsters and is hidden in fog from this index (`2`).
    PopulatedHidden,
    /// Camp has living monsters and is visible to this index (`3`).
    PopulatedVisible,
    /// Unobserved state byte preserved losslessly.
    Unknown(u8),
}

impl CampVisibilityState {
    pub const fn from_wire(raw: u8) -> Self {
        match raw {
            0 => Self::EmptyHidden,
            1 => Self::EmptyVisible,
            2 => Self::PopulatedHidden,
            3 => Self::PopulatedVisible,
            other => Self::Unknown(other),
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::EmptyHidden => 0,
            Self::EmptyVisible => 1,
            Self::PopulatedHidden => 2,
            Self::PopulatedVisible => 3,
            Self::Unknown(other) => other,
        }
    }
}

impl From<u8> for CampVisibilityState {
    fn from(raw: u8) -> Self {
        Self::from_wire(raw)
    }
}

impl From<CampVisibilityState> for u8 {
    fn from(state: CampVisibilityState) -> Self {
        state.raw()
    }
}

/// Six-byte body of `0x042C` (`Opcode::CampVisibility`).
///
/// Android 147219 (`vt[3]` at `0x8844dc`) filters strictly on
/// `index == GetLocalTeamId()` (`0xbedf10`, `[local_player + 0xc2] & 0xf`) before
/// storing `state` into `[minimap_entry + 0x6c]` via `0x820a7c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CampVisibilityBody {
    pub actor: u32,
    pub index: u8,
    #[br(map = CampVisibilityState::from_wire)]
    #[bw(map = |state: &CampVisibilityState| state.raw())]
    pub state: CampVisibilityState,
}

macro_rules! fixed_prefix {
    ($ty:ty, $len:literal) => {
        impl $ty {
            pub const BODY_LEN: usize = $len;
            pub(crate) fn message(&self, opcode: crate::Opcode, body_len: usize) -> crate::Message {
                crate::fixed_body::message(opcode, self, Self::BODY_LEN, body_len)
            }
            /// Reads the fixed prefix. Callers retain any remaining wire bytes.
            pub fn read_body(body: &[u8]) -> std::io::Result<Self> {
                crate::fixed_body::read(body)
            }
            /// Writes the fixed prefix, leaving trailing destination bytes intact.
            pub fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize> {
                crate::fixed_body::write(self, dst, Self::BODY_LEN)
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
            pub const PADDED_LEN: usize = crate::fixed_body::padded_len($len);

            /// Reads the fixed prefix. Callers retain any remaining wire bytes.
            pub fn read_body(body: &[u8]) -> std::io::Result<Self> {
                crate::fixed_body::read(body)
            }

            /// Writes the fixed prefix, leaving trailing destination bytes intact.
            pub fn write_body(&self, dst: &mut [u8]) -> std::io::Result<usize> {
                crate::fixed_body::write(self, dst, Self::BODY_LEN)
            }

            /// Reads an exact native-length body (`BODY_LEN` bytes).
            pub fn read_exact(body: &[u8]) -> std::io::Result<Self> {
                crate::fixed_body::read_exact(body, Self::BODY_LEN)
            }

            /// Reads either the native `BODY_LEN` or Blowfish-aligned `PADDED_LEN` body.
            pub fn read_exact_or_padded(body: &[u8]) -> std::io::Result<Self> {
                crate::fixed_body::read_with_padding(body, Self::BODY_LEN).map(|(value, _)| value)
            }

            /// Reads a Blowfish-aligned `PADDED_LEN` body whose padding bytes are all zero.
            pub fn read_padded_zero(body: &[u8]) -> std::io::Result<Self> {
                let (value, tail) = crate::fixed_body::read_with_padding(body, Self::BODY_LEN)?;
                if body.len() != Self::PADDED_LEN || tail.iter().any(|&b| b != 0) {
                    return Err(crate::invalid("expected zero-padded fixed wire body"));
                }
                Ok(value)
            }

            /// Reads the fixed prefix together with any native (`[]`) or Blowfish-aligned tail.
            pub fn read_with_padding(body: &[u8]) -> std::io::Result<(Self, &[u8])> {
                crate::fixed_body::read_with_padding(body, Self::BODY_LEN)
            }

            /// Encodes the native `BODY_LEN`-byte frame.
            pub fn encode(&self) -> crate::Message {
                crate::fixed_body::message(Self::OPCODE, self, Self::BODY_LEN, Self::BODY_LEN)
            }

            /// Encodes the Blowfish-aligned `PADDED_LEN`-byte frame with zero padding.
            pub fn encode_padded(&self) -> crate::Message {
                crate::fixed_body::message(Self::OPCODE, self, Self::BODY_LEN, Self::PADDED_LEN)
            }

            /// Encodes the body while preserving an empty or Blowfish-aligned padding suffix.
            pub fn encode_with_padding(&self, suffix: &[u8]) -> std::io::Result<crate::Message> {
                crate::fixed_body::message_with_padding(Self::OPCODE, self, Self::BODY_LEN, suffix)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use fixed_packet;

fixed_prefix!(MoveCmd, 8);
fixed_prefix!(PositionBody, 12);
fixed_prefix!(DeathBody, 8);
fixed_prefix!(ImpactBody, 16);

fixed_prefix!(VisibilityBody, 8);
fixed_prefix!(CampVisibilityBody, 6);
