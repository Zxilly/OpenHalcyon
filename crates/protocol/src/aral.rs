use crate::{DraftRoster, DraftSlot, invalid};
use std::io;

/// Full 0x045A roster observed on the live ARAL control stream.
/// Coordinates exclude the opcode and transport length.
#[derive(Debug, PartialEq)]
pub struct AralRoster<'a> {
    wire: &'a [u8],
    roster: DraftRoster<'a>,
}

impl<'a> AralRoster<'a> {
    pub const SLOT_COUNT: usize = 16;
    pub const SLOT_BYTES: usize = 161;
    pub const BODY_BYTES: usize = 2584;
    pub const PADDED_BYTES: usize = 2590;

    pub fn new(wire: &'a [u8]) -> io::Result<Self> {
        if !matches!(wire.len(), Self::BODY_BYTES | Self::PADDED_BYTES) {
            return Err(invalid("invalid full ARAL roster length"));
        }
        Ok(Self {
            wire,
            roster: DraftRoster::new(wire)?,
        })
    }

    pub fn remaining_seconds(&self) -> f32 {
        self.roster.remaining_seconds()
    }

    pub fn allotted_seconds(&self) -> f32 {
        self.roster.allotted_seconds()
    }

    pub fn slot(&self, index: usize) -> Option<DraftSlot<'a>> {
        self.roster.slot(index)
    }

    /// Preserve undocumented fields, including the timer bytes over slot zero.
    pub fn raw_slot(&self, index: usize) -> Option<&'a [u8]> {
        if index >= Self::SLOT_COUNT {
            return None;
        }
        let offset = index * Self::SLOT_BYTES;
        Some(&self.wire[offset..offset + Self::SLOT_BYTES])
    }

    /// The raw 36-byte identity field. Unoccupied rows carry an empty identity.
    pub fn identity(&self, index: usize) -> Option<&'a [u8]> {
        self.raw_slot(index)?[105..141].split(|&v| v == 0).next()
    }

    /// Eight original bytes at the end of the structural roster.
    pub fn trailer(&self) -> &'a [u8] {
        &self.wire[Self::SLOT_COUNT * Self::SLOT_BYTES..Self::BODY_BYTES]
    }

    /// Any transport padding retained by the decoder.
    pub fn padding(&self) -> &'a [u8] {
        &self.wire[Self::BODY_BYTES..]
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.wire
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, Message};

    #[test]
    fn full_aral_preserves_slots_and_uninterpreted_bytes() {
        let mut body = vec![0; AralRoster::PADDED_BYTES];
        body[..4].copy_from_slice(&5f32.to_be_bytes());
        body[4..8].copy_from_slice(&20f32.to_be_bytes());
        for index in 0..AralRoster::SLOT_COUNT {
            let start = index * AralRoster::SLOT_BYTES;
            let slot = &mut body[start..start + AralRoster::SLOT_BYTES];
            slot[9] = index as u8;
            if index < 6 {
                slot[8] = 1;
                slot[10] = 1;
                slot[11] = 1;
                slot[16] = (index % 2 + 1) as u8;
                slot[17..19].copy_from_slice(&243u16.to_be_bytes());
                slot[19..21].copy_from_slice(&(1500 + index as u16).to_be_bytes());
                slot[21..25].copy_from_slice(&0xf240ebfeu32.to_be_bytes());
                slot[25..29].copy_from_slice(b"Test");
                slot[105..141].copy_from_slice(b"11111111-2222-4333-8444-555555555555");
            }
        }
        body[160] = 8;
        body[2583] = 5;
        body[2589] = 7;
        let message = Message::known(crate::Opcode::SnapshotAral, body.clone());
        let Fields::AralRoster(view) = message.fields().unwrap() else {
            panic!("full ARAL view")
        };
        assert_eq!(
            (view.remaining_seconds(), view.allotted_seconds()),
            (5., 20.)
        );
        assert_eq!(view.slot(5).unwrap().entity, 1505);
        assert_eq!(view.slot(0).unwrap().display_name, b"Test");
        assert!(!view.slot(6).unwrap().occupied);
        assert_eq!(
            view.identity(1).unwrap(),
            b"11111111-2222-4333-8444-555555555555"
        );
        assert_eq!(view.identity(7).unwrap(), b"");
        assert_eq!(view.raw_slot(0).unwrap()[160], 8);
        assert_eq!(view.trailer()[7], 5);
        assert_eq!(view.padding()[5], 7);
        assert_eq!(view.as_bytes(), body);
        assert!(AralRoster::new(&body[..2584]).unwrap().padding().is_empty());
        assert!(AralRoster::new(&body[..2589]).is_err());
    }
}
