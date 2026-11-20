//! Big-endian fixed prefixes. A caller may retain trailing native fields or
//! block padding in its own buffer; writing a prefix never changes that suffix.
use std::io::{self, Cursor};

use crate::{Message, Opcode, invalid};
use binrw::{BinRead, BinWrite};

#[inline]
pub(crate) fn read<T>(body: &[u8]) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    T::read_be(&mut Cursor::new(body)).map_err(|_| invalid("truncated fixed wire body"))
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

/// The first eight body bytes of a native move command. X and Y are IEEE-754
/// words transmitted without coordinate normalization.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct MoveCmd {
    pub x: f32,
    pub y: f32,
}
#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct PositionBody {
    pub actor: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct DeathBody {
    pub victim: u32,
    pub source: u32,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct ImpactBody {
    pub target: u32,
    pub source: u32,
    pub operand: f32,
    pub word: u16,
    pub byte: u8,
    pub flag: u8,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct VisibilityBody {
    pub actor: u32,
    pub index: u8,
    pub state: u8,
    pub masks: [u8; 2],
}

macro_rules! fixed_prefix {
    ($ty:ty, $len:literal) => {
        impl $ty {
            pub const BODY_LEN: usize = $len;
            pub(crate) fn message(&self, opcode: Opcode, body_len: usize) -> Message {
                let mut body = vec![0; body_len];
                self.write_body(&mut body)
                    .expect("fixed prefix fits the owned message body");
                Message::known(opcode, body)
            }
            /// Reads the fixed prefix. Callers retain any remaining wire bytes.
            pub fn read_body(body: &[u8]) -> io::Result<Self> {
                read(body)
            }
            /// Writes the fixed prefix, leaving trailing destination bytes intact.
            pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
                write(self, dst, Self::BODY_LEN)
            }
        }
    };
}
fixed_prefix!(MoveCmd, 8);
fixed_prefix!(PositionBody, 12);
fixed_prefix!(DeathBody, 8);
fixed_prefix!(ImpactBody, 16);

fixed_prefix!(VisibilityBody, 8);
