//! Opening 5v5 Solo compact structure declarations from the Android 147219 replay.
use crate::Message;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoloStructure {
    slot: u8,
    template: u16,
    team: u8,
    x: f32,
    z: f32,
}

impl SoloStructure {
    /// Native slot, with the hero registration block at 14–23 excluded.
    pub fn at_slot(slot: u8) -> Option<Self> {
        let (template, team, x, z) = match slot {
            0 => (297, 1, -70.00, -11.00),
            1 => (297, 1, -63.00, 0.00),
            2 => (297, 1, -70.00, 11.00),
            3 => (297, 2, 70.00, -11.00),
            4 => (297, 2, 63.00, 0.00),
            5 => (297, 2, 70.00, 11.00),
            6 => (295, 1, -75.00, 0.00),
            7 => (296, 2, 75.00, 0.00),
            8 => (340, 1, -21.30, -57.50),
            9 => (340, 1, -17.50, 0.00),
            10 => (340, 1, -21.30, 57.50),
            11 => (340, 2, 21.30, -57.50),
            12 => (340, 2, 17.50, 0.00),
            13 => (340, 2, 21.30, 57.50),
            24 => (339, 1, -49.24, -37.84),
            25 => (341, 1, -63.00, -22.41),
            26 => (339, 1, -34.50, 0.00),
            27 => (341, 1, -53.04, 0.00),
            28 => (339, 1, -49.24, 37.84),
            29 => (341, 1, -63.00, 22.41),
            30 => (339, 2, 49.24, -37.84),
            31 => (341, 2, 63.00, -22.41),
            32 => (339, 2, 34.50, 0.00),
            33 => (341, 2, 53.04, 0.00),
            34 => (339, 2, 49.24, 37.84),
            35 => (341, 2, 63.00, 22.41),
            _ => return None,
        };
        Some(Self {
            slot,
            template,
            team,
            x,
            z,
        })
    }

    pub fn template(self) -> u16 {
        self.template
    }
    pub fn slot(self) -> u8 {
        self.slot
    }

    /// Encodes a pristine opening board using the authoritative server actor ID.
    pub fn message(self, actor: u32) -> Message {
        let mut body = [0; 122];
        let mut word = |offset: usize, value: u32| {
            body[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        };
        word(0, u32::from(self.template));
        word(
            4,
            if self.template == 297 {
                0x4a2e_ed42
            } else {
                0xc10b_41da
            },
        );
        word(8, actor);
        word(12, self.x.to_bits());
        word(16, if self.slot == 8 { 0 } else { 0x3d75_be29 });
        word(20, self.z.to_bits());
        for (i, component) in self.heading_bits().into_iter().enumerate() {
            word(24 + i * 4, component);
        }
        let hp: f32 = match self.template {
            295 | 296 => 20_000.,
            297 => 4_350.,
            339 => 4_000.,
            340 => 3_000.,
            341 => 5_000.,
            _ => unreachable!("validated by at_slot"),
        };
        word(36, hp.to_bits());
        word(40, hp.to_bits());
        if matches!(self.template, 339..=341) {
            word(64, 1f32.to_bits());
        }
        word(84, 1f32.to_bits());
        word(112, u32::MAX);
        body[88..104].fill(1);
        body[96 + usize::from(self.team)] = 15;
        body[116] = self.slot;
        body[119..122].copy_from_slice(&[1, 255, self.team]);
        Message::known(crate::Opcode::Snapshot, body)
    }

    // Native headings retain signed zero and slightly asymmetric rotation rounding.
    fn heading_bits(self) -> [u32; 3] {
        match self.slot {
            0 => [0x3f12_d5e8, 0, 0xbf51_b3f3],
            1 => [1f32.to_bits(), 0, 0xb33b_bd2e],
            2 => [0x3f12_d5e8, (-0.0f32).to_bits(), 0x3f51_b3f3],
            3 => [0xbf12_d5e8, 0, 0xbf51_b3f3],
            4 => [(-1f32).to_bits(), 0, 0xb33b_bd2e],
            5 => [0xbf12_d5e8, 0, 0x3f51_b3f3],
            6 | 8 | 9 | 10 | 26 | 27 => [1f32.to_bits(), (-0.0f32).to_bits(), 0],
            7 | 11 | 12 | 13 | 32 | 33 => [(-1f32).to_bits(), 0, 0],
            24 | 25 => [0x3f35_04f4, 0, 0xbf35_04f3],
            28 | 29 => [0x3f35_04f4, (-0.0f32).to_bits(), 0x3f35_04f2],
            30 | 31 => [0xbf35_04f2, 0, 0xbf35_04f5],
            34 | 35 => [0xbf35_04f4, 0, 0x3f35_04f2],
            _ => unreachable!("validated by at_slot"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md5::{Digest, Md5};

    #[test]
    fn all_opening_structures_preserve_native_fields_in_slot_order() {
        let mut digest = Md5::new();
        let mut count = 0;
        for slot in 0..=u8::MAX {
            if let Some(profile) = SoloStructure::at_slot(slot) {
                let frame = profile.message(0);
                assert_eq!(frame.opcode, crate::wire_opcode::Snapshot);
                digest.update(&frame.body);
                count += 1;
            }
        }
        assert_eq!(count, 26);
        // Reference bodies use zero in the four-byte runtime actor field.
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "e0fb95a82162bfccbe25d9797530425e"
        );
    }
}
