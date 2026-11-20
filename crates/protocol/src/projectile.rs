//! Fire and detonate action layouts. A projectile reference has a distinct
//! lifetime from the actors occupying the scene's registration slots.
use crate::Message;

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
fn word(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}

impl<'a, const N: usize, const S: usize> ProjectileFire<'a, N, S> {
    fn read(b: &'a [u8]) -> Option<Self> {
        let half = 8 + 4 * N;
        let size = half + 2 + S;
        if !valid_len(b.len(), size) {
            return None;
        }
        Some(Self {
            projectile: word(b, 0),
            profile_word: word(b, 4),
            values: std::array::from_fn(|i| f32::from_bits(word(b, 8 + 4 * i))),
            operand: u16::from_be_bytes(b[half..half + 2].try_into().unwrap()),
            slots: b[half + 2..size].try_into().unwrap(),
            padding: &b[size..],
        })
    }
    fn message(&self, op: u16) -> Option<Message> {
        let size = 10 + 4 * N + S;
        if !valid_len(size + self.padding.len(), size) {
            return None;
        }
        let mut b = Vec::with_capacity(size + self.padding.len());
        b.extend(self.projectile.to_be_bytes());
        b.extend(self.profile_word.to_be_bytes());
        for v in self.values {
            b.extend(v.to_be_bytes());
        }
        b.extend(self.operand.to_be_bytes());
        b.extend(self.slots);
        b.extend(self.padding);
        Some(Message::new(op, b))
    }
}

impl<'a> ProjectileAction<'a> {
    pub(super) fn decode(op: u16, b: &'a [u8]) -> Option<Self> {
        match op {
            crate::wire_opcode::FireProjectile => ProjectileFire::read(b).map(Self::FireTracked),
            crate::wire_opcode::FirePointProjectile => ProjectileFire::read(b).map(Self::FirePoint),
            crate::wire_opcode::FireDirectedProjectile => {
                ProjectileFire::read(b).map(Self::FireDirected)
            }
            crate::wire_opcode::DetonateProjectile if valid_len(b.len(), 17) => {
                Some(Self::Detonate {
                    projectile: word(b, 0),
                    position: std::array::from_fn(|i| f32::from_bits(word(b, 4 + 4 * i))),
                    flag: b[16],
                    padding: &b[17..],
                })
            }
            _ => None,
        }
    }
    pub fn message(&self) -> Option<Message> {
        match self {
            Self::FireTracked(f) => f.message(crate::wire_opcode::FireProjectile),
            Self::FirePoint(f) => f.message(crate::wire_opcode::FirePointProjectile),
            Self::FireDirected(f) => f.message(crate::wire_opcode::FireDirectedProjectile),
            Self::Detonate {
                projectile,
                position,
                flag,
                padding,
            } if valid_len(17 + padding.len(), 17) => {
                let mut b = Vec::with_capacity(17 + padding.len());
                b.extend(projectile.to_be_bytes());
                for v in position {
                    b.extend(v.to_be_bytes());
                }
                b.push(*flag);
                b.extend(*padding);
                Some(Message::known(crate::Opcode::DetonateProjectile, b))
            }
            _ => None,
        }
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
            assert!(ProjectileAction::decode(op, &vec![0; size - 1]).is_none());
        }
    }
}
