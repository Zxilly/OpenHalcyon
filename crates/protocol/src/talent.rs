use super::{Message, Opcode, fixed_body::fixed_packet};
use binrw::{BinRead, BinWrite};

/// Hero-relative indices observed in ordinary ARAL equip actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum TalentSelection {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    NoTalent = 255,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct RequestEquipTalentWire {
    selection: TalentSelection,
    reserved: [u8; 3],
}
fixed_packet!(RequestEquipTalentWire, Opcode::RequestEquipTalent, 4);

impl TalentSelection {
    pub(super) fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Slot0),
            1 => Some(Self::Slot1),
            2 => Some(Self::Slot2),
            255 => Some(Self::NoTalent),
            _ => None,
        }
    }

    /// The request carries no actor identifier or level.
    pub fn request(self) -> Message {
        RequestEquipTalentWire {
            selection: self,
            reserved: [0; 3],
        }
        .encode()
    }
}

/// Authoritative equip notification. The caller supplies the authenticated
/// actor and level from its own game state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct EquippedTalent {
    pub actor: u32,
    pub selection: TalentSelection,
    pub level: u8,
}
fixed_packet!(EquippedTalent, Opcode::EquipTalent, 6);

impl EquippedTalent {
    pub fn message(self) -> std::io::Result<Message> {
        if self.selection == TalentSelection::NoTalent && self.level != 0 {
            return Err(super::invalid("NoTalent requires level zero"));
        }
        Ok(self.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields, MatchCipher};

    #[test]
    fn requests_do_not_supply_the_level_and_equips_round_trip() {
        let cipher = MatchCipher::new("synthetic");
        for (selection, level) in [
            (TalentSelection::Slot0, 5),
            (TalentSelection::Slot0, 20),
            (TalentSelection::Slot1, 10),
            (TalentSelection::Slot2, 1),
            (TalentSelection::NoTalent, 0),
        ] {
            let equip = EquippedTalent {
                actor: 1503,
                selection,
                level,
            };
            for (m, expected) in [
                (selection.request(), LiveFields::TalentSelection(selection)),
                (equip.message().unwrap(), LiveFields::TalentEquipped(equip)),
            ] {
                let frame = cipher.encode(&m).unwrap();
                let decoded = cipher.decode(&frame[2..]).unwrap();
                assert_eq!(decoded.fields().unwrap(), Fields::Live(expected));
                assert_eq!(cipher.encode(&decoded).unwrap(), frame);
            }
        }
        assert!(
            EquippedTalent {
                actor: 1503,
                selection: TalentSelection::NoTalent,
                level: 1
            }
            .message()
            .is_err()
        );
    }
}
