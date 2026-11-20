//! Fire and detonate action layouts. A projectile reference has a distinct
//! lifetime from the actors occupying the scene's registration slots.
use crate::{Message, Opcode, fixed_body, invalid};
use binrw::{BinRead, BinWrite};
use std::io;
use zerocopy::{
    FromZeros, IntoBytes,
    byteorder::big_endian::{F32, U16, U32},
};

#[derive(Debug, PartialEq)]
pub struct ProjectileFire<'a, const N: usize, const S: usize> {
    pub projectile: u32,
    pub profile_word: u32,
    pub values: [f32; N],
    pub operand: u16,
    pub slots: [u8; S],
    pub padding: &'a [u8],
}

#[derive(Debug, PartialEq)]
pub enum ProjectileAction<'a> {
    FireTracked(ProjectileFire<'a, 1, 3>),
    FirePoint(ProjectileFire<'a, 4, 2>),
    FireDirected(ProjectileFire<'a, 8, 2>),
    Detonate {
        projectile: u32,
        position: [f32; 3],
        flag: u8,
        padding: &'a [u8],
    },
}

fn valid_len(raw: usize, size: usize) -> bool {
    raw == size || raw == (size + 2).next_multiple_of(8) - 2
}
// All fields have alignment one and store their native bytes in network order.
// binrw reads/writes the single contiguous prefix; no per-field writer dispatch.
#[derive(zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable, zerocopy::Unaligned)]
#[repr(C)]
struct FireWire<const N: usize, const S: usize> {
    projectile: U32,
    profile_word: U32,
    values: [F32; N],
    operand: U16,
    slots: [u8; S],
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct FireBody<const N: usize, const S: usize> {
    #[br(parse_with=read_fire_wire)]
    #[bw(write_with=write_fire_wire)]
    wire: FireWire<N, S>,
}

#[binrw::parser(reader)]
fn read_fire_wire<const N: usize, const S: usize>() -> binrw::BinResult<FireWire<N, S>> {
    let mut wire = FireWire::<N, S>::new_zeroed();
    reader.read_exact(wire.as_mut_bytes())?;
    Ok(wire)
}
#[binrw::writer(writer)]
fn write_fire_wire<const N: usize, const S: usize>(wire: &FireWire<N, S>) -> binrw::BinResult<()> {
    writer.write_all(wire.as_bytes())?;
    Ok(())
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DetonationBody {
    projectile: u32,
    position: [f32; 3],
    flag: u8,
}

impl<'a, const N: usize, const S: usize> ProjectileFire<'a, N, S> {
    const SIZE: usize = 10 + 4 * N + S;
    fn read(body: &'a [u8]) -> Option<Self> {
        if !valid_len(body.len(), Self::SIZE) {
            return None;
        }
        let FireBody { wire }: FireBody<N, S> = fixed_body::read(body).ok()?;
        Some(Self {
            projectile: wire.projectile.get(),
            profile_word: wire.profile_word.get(),
            values: wire.values.map(F32::get),
            operand: wire.operand.get(),
            slots: wire.slots,
            padding: &body[Self::SIZE..],
        })
    }
    #[inline]
    fn wire(&self) -> FireWire<N, S> {
        FireWire {
            projectile: U32::new(self.projectile),
            profile_word: U32::new(self.profile_word),
            values: self.values.map(F32::new),
            operand: U16::new(self.operand),
            slots: self.slots,
        }
    }
    #[inline(always)]
    fn write_body(&self, dst: &mut [u8]) -> Option<usize> {
        write_parts(self.wire().as_bytes(), self.padding, dst)
    }
    #[inline]
    fn message(&self, opcode: Opcode) -> Option<Message> {
        owned_parts(opcode, self.wire().as_bytes(), self.padding)
    }
}

impl<'a> ProjectileAction<'a> {
    pub fn read_body(op: Opcode, body: &'a [u8]) -> Option<Self> {
        match op {
            Opcode::FireProjectile => ProjectileFire::read(body).map(Self::FireTracked),
            Opcode::FirePointProjectile => ProjectileFire::read(body).map(Self::FirePoint),
            Opcode::FireDirectedProjectile => ProjectileFire::read(body).map(Self::FireDirected),
            Opcode::DetonateProjectile if valid_len(body.len(), 17) => {
                let DetonationBody {
                    projectile,
                    position,
                    flag,
                } = fixed_body::read(body).ok()?;
                Some(Self::Detonate {
                    projectile,
                    position,
                    flag,
                    padding: &body[17..],
                })
            }
            _ => None,
        }
    }
    /// Writes the complete native or aligned body, preserving any suffix.
    #[inline(always)]
    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::FireTracked(f) => f.write_body(dst),
            Self::FirePoint(f) => f.write_body(dst),
            Self::FireDirected(f) => f.write_body(dst),
            Self::Detonate {
                projectile,
                position,
                flag,
                padding,
            } => write_parts(
                &detonation_bytes(*projectile, *position, *flag),
                padding,
                dst,
            ),
        }
        .ok_or_else(bad_output)
    }
    #[inline]
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::FireTracked(f) => f.message(Opcode::FireProjectile),
            Self::FirePoint(f) => f.message(Opcode::FirePointProjectile),
            Self::FireDirected(f) => f.message(Opcode::FireDirectedProjectile),
            Self::Detonate {
                projectile,
                position,
                flag,
                padding,
            } => owned_parts(
                Opcode::DetonateProjectile,
                &detonation_bytes(*projectile, *position, *flag),
                padding,
            ),
        }
    }
}

fn detonation_bytes(projectile: u32, position: [f32; 3], flag: u8) -> [u8; 17] {
    let mut bytes = [0; 17];
    fixed_body::write(
        &DetonationBody {
            projectile,
            position,
            flag,
        },
        &mut bytes,
        17,
    )
    .expect("fixed-size detonation output");
    bytes
}
#[inline]
fn total_len(prefix: usize, padding: usize) -> Option<usize> {
    (padding == 0 || padding == (prefix + 2).next_multiple_of(8) - 2 - prefix)
        .then(|| prefix + padding)
}
#[cold]
fn bad_output() -> io::Error {
    invalid("unsupported projectile suffix or output too small")
}
#[inline(always)]
fn write_parts(prefix: &[u8], padding: &[u8], dst: &mut [u8]) -> Option<usize> {
    let len = total_len(prefix.len(), padding.len())?;
    let dst = dst.get_mut(..len)?;
    let (before, after) = dst.split_at_mut(prefix.len());
    before.copy_from_slice(prefix);
    if !padding.is_empty() {
        after.copy_from_slice(padding);
    }
    Some(len)
}
#[inline]
fn owned_parts(opcode: Opcode, prefix: &[u8], padding: &[u8]) -> Option<Message> {
    let len = total_len(prefix.len(), padding.len())?;
    let mut body = Vec::with_capacity(len);
    body.extend_from_slice(prefix);
    body.extend_from_slice(padding);
    Some(Message::known(opcode, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fields;
    #[test]
    fn derived_wire_writer_and_slice_writer_preserve_identical_ieee_words() {
        fn check<const N: usize, const S: usize>(fire: &ProjectileFire<'_, N, S>) {
            let mut standard = Vec::new();
            FireBody { wire: fire.wire() }
                .write_be(&mut std::io::Cursor::new(&mut standard))
                .unwrap();
            let mut dst = [0xae; 48];
            let n = fire.write_body(&mut dst).unwrap();
            assert_eq!(&dst[..n], standard);
            assert_eq!(
                &standard[..8],
                &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
            );
            assert_eq!(&standard[8..12], &[0x7f, 0x80, 0x00, 0x01]);
        }
        fn fire<const N: usize, const S: usize>() -> ProjectileFire<'static, N, S> {
            let mut values = [-0.; N];
            values[0] = f32::from_bits(0x7f800001);
            ProjectileFire {
                projectile: 0x01234567,
                profile_word: 0x89abcdef,
                values,
                operand: 0xaabb,
                slots: [0xc1; S],
                padding: &[],
            }
        }
        check(&fire::<1, 3>());
        check(&fire::<4, 2>());
        check(&fire::<8, 2>());
    }

    #[test]
    fn native_and_padded_forms_preserve_all_operands() {
        for (op, size) in [
            (crate::wire_opcode::FireProjectile, 17usize),
            (crate::wire_opcode::FirePointProjectile, 28),
            (crate::wire_opcode::FireDirectedProjectile, 44),
            (crate::wire_opcode::DetonateProjectile, 17),
        ] {
            let padded = (size + 2).next_multiple_of(8) - 2;
            for len in [size, padded] {
                let mut b: Vec<u8> = (0..len as u8).collect();
                b[..4].copy_from_slice(&5042_u32.to_be_bytes());
                b[8..12].copy_from_slice(&0x7fc00013u32.to_be_bytes());
                let m = Message::new(op, b);
                let Fields::Projectile(action) = m.fields().unwrap() else {
                    panic!()
                };
                assert_eq!(action.message(), Some(m));
            }
            assert!(
                ProjectileAction::read_body(Opcode::try_from(op).unwrap(), &vec![0; size - 1])
                    .is_none()
            );
        }
    }
}
