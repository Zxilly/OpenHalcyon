//! Fire and detonate action layouts. A projectile reference has a distinct
//! lifetime from the actors occupying the scene's registration slots.
use crate::{Message, Opcode, fixed_body, invalid};
use binrw::{BinRead, BinWrite};
use std::io;

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
#[derive(BinRead, BinWrite)]
#[brw(big)]
struct FireBody<const N: usize, const S: usize> {
    projectile: u32,
    profile_word: u32,
    values: [f32; N],
    operand: u16,
    slots: [u8; S],
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
        let FireBody {
            projectile,
            profile_word,
            values,
            operand,
            slots,
        } = fixed_body::read(body).ok()?;
        Some(Self {
            projectile,
            profile_word,
            values,
            operand,
            slots,
            padding: &body[Self::SIZE..],
        })
    }
    fn write_prefix(&self, dst: &mut [u8]) -> io::Result<usize> {
        let prefix = FireBody {
            projectile: self.projectile,
            profile_word: self.profile_word,
            values: self.values,
            operand: self.operand,
            slots: self.slots,
        };
        fixed_body::write(&prefix, dst, Self::SIZE)
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
    fn layout(&self) -> (Opcode, usize, &[u8]) {
        match self {
            Self::FireTracked(f) => (
                Opcode::FireProjectile,
                ProjectileFire::<1, 3>::SIZE,
                f.padding,
            ),
            Self::FirePoint(f) => (
                Opcode::FirePointProjectile,
                ProjectileFire::<4, 2>::SIZE,
                f.padding,
            ),
            Self::FireDirected(f) => (
                Opcode::FireDirectedProjectile,
                ProjectileFire::<8, 2>::SIZE,
                f.padding,
            ),
            Self::Detonate { padding, .. } => (Opcode::DetonateProjectile, 17, padding),
        }
    }

    /// Writes the entire native or padded body. Length and output capacity
    /// are validated before writing. Extra destination bytes remain untouched.
    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        let (_, prefix, padding) = self.layout();
        let len = prefix
            .checked_add(padding.len())
            .filter(|&n| valid_len(n, prefix))
            .ok_or_else(|| invalid("unsupported projectile suffix length"))?;
        let out = dst
            .get_mut(..len)
            .ok_or_else(|| invalid("projectile output is too small"))?;
        match self {
            Self::FireTracked(f) => f.write_prefix(out)?,
            Self::FirePoint(f) => f.write_prefix(out)?,
            Self::FireDirected(f) => f.write_prefix(out)?,
            Self::Detonate {
                projectile,
                position,
                flag,
                ..
            } => fixed_body::write(
                &DetonationBody {
                    projectile: *projectile,
                    position: *position,
                    flag: *flag,
                },
                out,
                prefix,
            )?,
        };
        out[prefix..].copy_from_slice(padding);
        Ok(len)
    }
    pub fn message(&self) -> Option<Message> {
        let (opcode, prefix, padding) = self.layout();
        let len = prefix.checked_add(padding.len())?;
        if !valid_len(len, prefix) {
            return None;
        }
        let mut body = vec![0; len];
        self.write_body(&mut body).ok()?;
        Some(Message::known(opcode, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fields;
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
