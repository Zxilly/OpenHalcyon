//! Draft rosters, ARAL reroll state, and scene readiness row packets.

use super::{encode_bin, read_bin, read_bin_variable, write_bin};
use crate::codec::invalid;
use crate::{FixedAscii, Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io::{self, Read, Seek, SeekFrom, Write};

/// `0x0480` (`Opcode::AralRerollState`, 82B native / 86B cipher-padded) ARAL reroll state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct AralRerollStateBody {
    pub cost: u32,
    pub limit: u32,
    pub used: u32,
    pub reserved_12: [u8; 64],
    pub allow_override: u8,
    pub balance: u32,
    pub locked: u8,
    pub reserved_tail: [u8; 4],
}

impl Default for AralRerollStateBody {
    fn default() -> Self {
        Self {
            cost: 0,
            limit: 0,
            used: 0,
            reserved_12: [0; 64],
            allow_override: 0,
            balance: 0,
            locked: 0,
            reserved_tail: [0; 4],
        }
    }
}

impl AralRerollStateBody {
    pub const NATIVE_LEN: usize = 82;
    pub const BODY_LEN: usize = 86;

    pub fn new(
        cost: u32,
        limit: u32,
        used: u32,
        allow_override: u8,
        balance: u32,
        locked: u8,
    ) -> Self {
        Self {
            cost,
            limit,
            used,
            reserved_12: [0; 64],
            allow_override,
            balance,
            locked,
            reserved_tail: [0; 4],
        }
    }

    /// Reads either the 82-byte unpadded Android native body or the 86-byte padded body.
    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        #[derive(BinRead)]
        #[br(big)]
        struct NativePrefix {
            cost: u32,
            limit: u32,
            used: u32,
            reserved_12: [u8; 64],
            allow_override: u8,
            balance: u32,
            locked: u8,
        }

        if body.len() == Self::NATIVE_LEN {
            let prefix: NativePrefix = read_bin(
                body,
                Self::NATIVE_LEN,
                "invalid native AralRerollState body",
            )?;
            return Ok(Self {
                cost: prefix.cost,
                limit: prefix.limit,
                used: prefix.used,
                reserved_12: prefix.reserved_12,
                allow_override: prefix.allow_override,
                balance: prefix.balance,
                locked: prefix.locked,
                reserved_tail: [0; 4],
            });
        }
        read_bin(body, Self::BODY_LEN, "invalid AralRerollState body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::AralRerollState, self, Self::BODY_LEN)
    }
}

/// Single 6-byte entry inside `0x045C` (`Opcode::SceneReadinessRows`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SceneReadinessRow {
    pub reference: u32,
    pub gate: u8,
    pub flag: u8,
}

impl SceneReadinessRow {
    pub const BODY_LEN: usize = 6;
}

/// `0x045C` (`Opcode::SceneReadinessRows`, 96B) 16-row scene readiness table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct SceneReadinessRowsBody {
    pub rows: [SceneReadinessRow; 16],
}

impl SceneReadinessRowsBody {
    pub const ROW_COUNT: usize = 16;
    pub const BODY_LEN: usize = 96;

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid SceneReadinessRows body")
    }

    /// Reads the 96-byte readiness table from any `96+`-byte `SceneReadinessRows` body.
    pub fn read_prefix(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "truncated control entries")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }

    pub fn encode(&self) -> Message {
        encode_bin(Opcode::SceneReadinessRows, self, Self::BODY_LEN)
    }
}

/// Three-byte draft preference block at offsets `12..15` inside each `DraftSlotBody`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftPreferenceFlags {
    pub reserved_12: u8,
    pub position: u8,
    pub build_path: u8,
}

impl Default for DraftPreferenceFlags {
    fn default() -> Self {
        Self {
            reserved_12: u8::MAX,
            position: u8::MAX,
            build_path: u8::MAX,
        }
    }
}

impl DraftPreferenceFlags {
    pub const BODY_LEN: usize = 3;

    pub fn new(position: Option<u8>, build_path: Option<u8>) -> Self {
        Self {
            reserved_12: u8::MAX,
            position: position.unwrap_or(u8::MAX),
            build_path: build_path.unwrap_or(u8::MAX),
        }
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftPreferenceFlags body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }
}

/// 161-byte seat slot row inside `0x0459` (`DraftRosterFull`) and `0x045A` (`DraftRosterAral`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct DraftSlotBody {
    /// Bytes `0..8`. On slot `0`, bytes `0..8` carry the draft `remaining_seconds` and
    /// `allotted_seconds` floats on the wire.
    pub prefix_0: [u8; 8],
    pub occupied: u8,
    pub slot: u8,
    pub selected: u8,
    pub locked: u8,
    pub reserved_12: u8,
    pub position: u8,
    pub build_path: u8,
    pub reserved_15: u8,
    pub team: u8,
    pub template: u16,
    pub entity: u16,
    pub skin: u32,
    pub display_name: FixedAscii<32>,
    pub reserved_57: [u8; 48],
    pub identity: FixedAscii<36>,
    pub reserved_141: [u8; 20],
}

impl DraftSlotBody {
    pub const BODY_LEN: usize = 161;
    pub const ARAL_EMPTY_SKIN: u32 = 0x10c2_bad9;

    /// Builds an unoccupied draft slot with `slot = index`, unset `0xff` preferences/IDs,
    /// and the caller-supplied default `skin` hash (`0x10c2bad9` for ARAL, `0` for 5v5/3v3).
    pub fn empty_with_skin(index: u8, skin: u32) -> Self {
        Self {
            prefix_0: [0; 8],
            occupied: 0,
            slot: index,
            selected: 0,
            locked: 0,
            reserved_12: u8::MAX,
            position: u8::MAX,
            build_path: u8::MAX,
            reserved_15: 0,
            team: 0,
            template: u16::MAX,
            entity: u16::MAX,
            skin,
            display_name: FixedAscii::default(),
            reserved_57: [0; 48],
            identity: FixedAscii::default(),
            reserved_141: [0; 20],
        }
    }

    pub fn empty(index: u8) -> Self {
        Self::empty_with_skin(index, 0)
    }

    pub const fn preferences(&self) -> DraftPreferenceFlags {
        DraftPreferenceFlags {
            reserved_12: self.reserved_12,
            position: self.position,
            build_path: self.build_path,
        }
    }

    pub fn set_preferences(&mut self, prefs: DraftPreferenceFlags) {
        self.reserved_12 = prefs.reserved_12;
        self.position = prefs.position;
        self.build_path = prefs.build_path;
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin(body, Self::BODY_LEN, "invalid DraftSlotBody")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, Self::BODY_LEN)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct DraftRosterTimers {
    remaining: f32,
    allotted: f32,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
struct DraftRosterWire {
    slots: [DraftSlotBody; 16],
    trailer: [u8; 8],
}

/// Wire payload for `0x045A` (`DraftRosterAral`, 2584B) and `0x0459` (`DraftRosterFull`, 2590B).
///
/// On the wire, the 8-byte draft timer header `(remaining_seconds, allotted_seconds)` overlays
/// bytes `0..8` of `slots[0]` (whose semantic fields begin at offset `8`), followed by `slots[1..16]`
/// through offset `2576`, an 8-byte structural `trailer` (`2576..2584`), and optional 6-byte
/// transport `padding` (`2584..2590`).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftRosterBody {
    pub remaining_seconds: f32,
    pub allotted_seconds: f32,
    pub slots: [DraftSlotBody; 16],
    pub trailer: [u8; 8],
    pub padding: Option<[u8; 6]>,
}

impl BinRead for DraftRosterBody {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let wire = DraftRosterWire::read_options(reader, endian, ())?;
        let pos = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(pos))?;
        let padding = match end - pos {
            0 => None,
            6 => Some(<[u8; 6]>::read_options(reader, endian, ())?),
            _ => {
                return Err(binrw::Error::AssertFail {
                    pos,
                    message: "invalid DraftRoster length (expected 2584 or 2590 bytes)".into(),
                });
            }
        };
        let (remaining_seconds, allotted_seconds) = Self::read_timers(&wire.slots[0].prefix_0)
            .map_err(|err| binrw::Error::AssertFail {
                pos: 0,
                message: err.to_string(),
            })?;
        Ok(Self {
            remaining_seconds,
            allotted_seconds,
            slots: wire.slots,
            trailer: wire.trailer,
            padding,
        })
    }
}

impl BinWrite for DraftRosterBody {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let mut slots = self.slots;
        Self::write_timers_to(
            self.remaining_seconds,
            self.allotted_seconds,
            &mut slots[0].prefix_0,
        );
        DraftRosterWire {
            slots,
            trailer: self.trailer,
        }
        .write_options(writer, endian, ())?;
        if let Some(pad) = self.padding {
            pad.write_options(writer, endian, ())?;
        }
        Ok(())
    }
}

impl DraftRosterBody {
    pub const SLOT_COUNT: usize = 16;
    pub const SLOT_BYTES: usize = DraftSlotBody::BODY_LEN;
    pub const BODY_LEN: usize = 2584;
    pub const PADDED_LEN: usize = 2590;

    /// Builds a 2584-byte `DraftRosterAral` body (`padding = None`).
    pub fn aral(remaining_seconds: f32, allotted_seconds: f32, slots: [DraftSlotBody; 16]) -> Self {
        let mut out = Self {
            remaining_seconds,
            allotted_seconds,
            slots,
            trailer: [0; 8],
            padding: None,
        };
        out.sync_slot0_timer_prefix();
        out
    }

    /// Builds a 2590-byte `DraftRosterFull` body (`padding = Some([0; 6])`).
    pub fn full(remaining_seconds: f32, allotted_seconds: f32, slots: [DraftSlotBody; 16]) -> Self {
        let mut out = Self {
            remaining_seconds,
            allotted_seconds,
            slots,
            trailer: [0; 8],
            padding: Some([0; 6]),
        };
        out.sync_slot0_timer_prefix();
        out
    }

    fn write_timers_to(remaining: f32, allotted: f32, dst: &mut [u8; 8]) {
        write_bin(
            &DraftRosterTimers {
                remaining,
                allotted,
            },
            dst,
            8,
        )
        .expect("8-byte timer header fits prefix_0");
    }

    fn sync_slot0_timer_prefix(&mut self) {
        Self::write_timers_to(
            self.remaining_seconds,
            self.allotted_seconds,
            &mut self.slots[0].prefix_0,
        );
    }

    /// Clears admission UUID identities for the first `count` slots (observer snapshots).
    pub fn redact_identities(&mut self, count: usize) {
        for slot in self.slots.iter_mut().take(count) {
            slot.identity = FixedAscii::default();
        }
    }

    /// Clears display names and UUID identities on all non-bot slots (replay recording sanitization).
    pub fn redact_non_bots(&mut self) {
        for slot in &mut self.slots {
            if !slot
                .identity
                .as_bytes()
                .starts_with(b"__Kindred_Player_Bot__")
            {
                slot.display_name = FixedAscii::default();
                slot.identity = FixedAscii::default();
            }
        }
    }

    pub const fn wire_len(&self) -> usize {
        if self.padding.is_some() {
            Self::PADDED_LEN
        } else {
            Self::BODY_LEN
        }
    }

    pub fn read_timers(body: &[u8]) -> io::Result<(f32, f32)> {
        let timers: DraftRosterTimers = read_bin(body, 8, "invalid DraftRoster timer header")?;
        Ok((timers.remaining, timers.allotted))
    }

    /// Reads the 8-byte draft timer header from any `8+`-byte body.
    pub fn read_timers_prefix(body: &[u8]) -> io::Result<(f32, f32)> {
        let timers: DraftRosterTimers =
            read_bin_variable(body, "invalid DraftRoster timer header")?;
        Ok((timers.remaining, timers.allotted))
    }

    pub fn read_body(body: &[u8]) -> io::Result<Self> {
        read_bin_variable(body, "invalid DraftRoster body")
    }

    pub fn write_body(&self, dst: &mut [u8]) -> io::Result<usize> {
        write_bin(self, dst, self.wire_len())
    }

    pub fn encode_aral(&self) -> Message {
        if self.padding.is_none() {
            encode_bin(Opcode::DraftRosterAral, self, Self::BODY_LEN)
        } else {
            let mut unpadded = self.clone();
            unpadded.padding = None;
            encode_bin(Opcode::DraftRosterAral, &unpadded, Self::BODY_LEN)
        }
    }

    pub fn encode_full(&self) -> Message {
        if self.padding.is_some() {
            encode_bin(Opcode::DraftRosterFull, self, Self::PADDED_LEN)
        } else {
            let mut padded = self.clone();
            padded.padding = Some([0; 6]);
            encode_bin(Opcode::DraftRosterFull, &padded, Self::PADDED_LEN)
        }
    }
}

/// Five-versus-five selection roster with fixed-stride slots; coordinates
/// here are relative to the decrypted message body (the opcode is excluded).
#[derive(Debug, PartialEq)]
pub struct DraftRoster<'a>(&'a [u8]);

#[derive(Debug, PartialEq)]
pub struct DraftSlot<'a> {
    pub occupied: bool,
    pub slot: u8,
    pub selected: bool,
    pub locked: bool,
    pub team: u8,
    pub template: u16,
    pub entity: u16,
    pub skin: u32,
    pub display_name: &'a [u8],
}

fn decode_slot_view<'a>(wire: &'a [u8], index: usize) -> Option<DraftSlot<'a>> {
    let offset = index * DraftSlotBody::BODY_LEN;
    let raw = wire.get(offset..offset + DraftSlotBody::BODY_LEN)?;
    let slot = DraftSlotBody::read_body(raw).ok()?;
    let name_len = slot.display_name.trimmed_bytes().len();
    Some(DraftSlot {
        occupied: slot.occupied != 0,
        slot: slot.slot,
        selected: slot.selected != 0,
        locked: slot.locked != 0,
        team: slot.team,
        template: slot.template,
        entity: slot.entity,
        skin: slot.skin,
        display_name: &wire[offset + 25..offset + 25 + name_len],
    })
}

impl<'a> DraftRoster<'a> {
    pub fn new(bytes: &'a [u8]) -> io::Result<Self> {
        if bytes.len() < DraftRosterBody::BODY_LEN {
            return Err(invalid("truncated draft roster"));
        }
        Ok(Self(bytes))
    }

    pub fn remaining_seconds(&self) -> f32 {
        DraftRosterBody::read_timers_prefix(self.0)
            .expect("validated draft roster header")
            .0
    }

    pub fn allotted_seconds(&self) -> f32 {
        DraftRosterBody::read_timers_prefix(self.0)
            .expect("validated draft roster header")
            .1
    }

    pub fn slot(&self, i: usize) -> Option<DraftSlot<'a>> {
        if i >= DraftRosterBody::SLOT_COUNT {
            return None;
        }
        decode_slot_view(self.0, i)
    }
}

/// Full 0x045A roster observed on the live ARAL control stream.
/// Coordinates exclude the opcode and transport length.
#[derive(Debug, PartialEq)]
pub struct AralRoster<'a> {
    wire: &'a [u8],
}

impl<'a> AralRoster<'a> {
    pub const SLOT_COUNT: usize = DraftRosterBody::SLOT_COUNT;
    pub const SLOT_BYTES: usize = DraftRosterBody::SLOT_BYTES;
    pub const BODY_BYTES: usize = DraftRosterBody::BODY_LEN;
    pub const PADDED_BYTES: usize = DraftRosterBody::PADDED_LEN;

    pub fn new(wire: &'a [u8]) -> io::Result<Self> {
        if !matches!(wire.len(), Self::BODY_BYTES | Self::PADDED_BYTES) {
            return Err(invalid("invalid full ARAL roster length"));
        }
        // Validate the structural roster via binrw while retaining the compact borrowed slice.
        let _ = DraftRosterBody::read_body(wire)?;
        Ok(Self { wire })
    }

    pub fn decode_body(&self) -> io::Result<DraftRosterBody> {
        DraftRosterBody::read_body(self.wire)
    }

    pub fn remaining_seconds(&self) -> f32 {
        DraftRosterBody::read_timers(&self.wire[..8])
            .expect("validated ARAL roster header")
            .0
    }

    pub fn allotted_seconds(&self) -> f32 {
        DraftRosterBody::read_timers(&self.wire[..8])
            .expect("validated ARAL roster header")
            .1
    }

    pub fn slot(&self, index: usize) -> Option<DraftSlot<'a>> {
        if index >= Self::SLOT_COUNT {
            return None;
        }
        decode_slot_view(self.wire, index)
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
        let raw = self.raw_slot(index)?;
        let slot = DraftSlotBody::read_body(raw).ok()?;
        let offset = index * Self::SLOT_BYTES;
        let len = slot.identity.trimmed_bytes().len();
        Some(&self.wire[offset + 105..offset + 105 + len])
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
    use crate::{Fields, LiveFields};

    #[test]
    fn aral_reroll_state_and_readiness_rows_roundtrip() {
        let reroll = AralRerollStateBody::new(1, 5, 0, 1, 3, 0);
        let msg = reroll.encode();
        assert_eq!(msg.opcode, Opcode::AralRerollState.raw());
        assert_eq!(msg.body.len(), 86);
        let mut expected = [0u8; 86];
        for (i, v) in [(3, 1), (7, 5), (76, 1), (80, 3)] {
            expected[i] = v;
        }
        assert_eq!(msg.body, expected);
        assert_eq!(AralRerollStateBody::read_body(&msg.body).unwrap(), reroll);
        assert_eq!(
            AralRerollStateBody::read_body(&expected[..82]).unwrap(),
            reroll
        );
        let Fields::Live(LiveFields::AralRerollState {
            cost,
            limit,
            used,
            allow_override,
            balance,
            locked,
        }) = msg.fields().unwrap()
        else {
            panic!("expected AralRerollState")
        };
        assert_eq!(
            (cost, limit, used, allow_override, balance, locked),
            (1, 5, 0, 1, 3, 0)
        );

        let mut table = SceneReadinessRowsBody::default();
        table.rows[0] = SceneReadinessRow {
            reference: 1500,
            gate: 1,
            flag: 0,
        };
        table.rows[15] = SceneReadinessRow {
            reference: 1515,
            gate: 1,
            flag: 1,
        };
        let table_msg = table.encode();
        assert_eq!(table_msg.opcode, Opcode::SceneReadinessRows.raw());
        assert_eq!(table_msg.body.len(), 96);
        assert_eq!(&table_msg.body[..6], &[0, 0, 5, 220, 1, 0]);
        assert_eq!(
            SceneReadinessRowsBody::read_body(&table_msg.body).unwrap(),
            table
        );
    }

    #[test]
    fn draft_roster_matches_aral_and_full_wire_layouts_losslessly() {
        let mut slots = std::array::from_fn(|i| {
            DraftSlotBody::empty_with_skin(i as u8, DraftSlotBody::ARAL_EMPTY_SKIN)
        });
        slots[0].occupied = 1;
        slots[0].set_preferences(DraftPreferenceFlags::new(Some(5), Some(2)));
        slots[0].selected = 1;
        slots[0].locked = 0;
        slots[0].team = 1;
        slots[0].template = 243;
        slots[0].entity = 1500;
        slots[0].skin = 0xf240_ebfe;
        slots[0].display_name = FixedAscii::from_str_trunc("Guest");
        slots[0].identity = FixedAscii::from_str_trunc("11111111-2222-4333-8444-555555555555");
        slots[0].reserved_141[19] = 0x55;

        let mut aral = DraftRosterBody::aral(12.5, 20.0, slots);
        aral.trailer[7] = 0x77;
        let aral_msg = aral.encode_aral();
        assert_eq!(aral_msg.opcode, Opcode::DraftRosterAral.raw());
        assert_eq!(aral_msg.body.len(), 2584);
        assert_eq!(&aral_msg.body[..4], &12.5f32.to_be_bytes());
        assert_eq!(&aral_msg.body[4..8], &20.0f32.to_be_bytes());
        assert_eq!(aral_msg.body[8], 1);
        assert_eq!(aral_msg.body[9], 0);
        assert_eq!(&aral_msg.body[12..15], &[0xff, 5, 2]);
        assert_eq!(&aral_msg.body[17..19], &243u16.to_be_bytes());
        assert_eq!(&aral_msg.body[19..21], &1500u16.to_be_bytes());
        assert_eq!(&aral_msg.body[21..25], &0xf240_ebfeu32.to_be_bytes());
        assert_eq!(&aral_msg.body[25..31], b"Guest\0");
        assert_eq!(
            &aral_msg.body[105..141],
            b"11111111-2222-4333-8444-555555555555"
        );
        assert_eq!(aral_msg.body[160], 0x55);
        assert_eq!(aral_msg.body[161 + 9], 1);
        assert_eq!(
            &aral_msg.body[161 + 21..161 + 25],
            &DraftSlotBody::ARAL_EMPTY_SKIN.to_be_bytes()
        );
        assert_eq!(aral_msg.body[2583], 0x77);
        assert_eq!(DraftRosterBody::read_body(&aral_msg.body).unwrap(), aral);

        let aral_view = AralRoster::new(&aral_msg.body).unwrap();
        assert_eq!(aral_view.remaining_seconds(), 12.5);
        assert_eq!(aral_view.allotted_seconds(), 20.0);
        assert_eq!(aral_view.slot(0).unwrap().entity, 1500);
        assert_eq!(aral_view.trailer()[7], 0x77);

        let mut full = DraftRosterBody::full(300.0, 300.0, slots);
        full.padding = Some([1, 2, 3, 4, 5, 6]);
        let full_msg = full.encode_full();
        assert_eq!(full_msg.opcode, Opcode::DraftRosterFull.raw());
        assert_eq!(full_msg.body.len(), 2590);
        assert_eq!(&full_msg.body[2584..2590], &[1, 2, 3, 4, 5, 6]);
        assert_eq!(DraftRosterBody::read_body(&full_msg.body).unwrap(), full);

        let full_view = DraftRoster::new(&full_msg.body).unwrap();
        assert_eq!(full_view.remaining_seconds(), 300.0);
        assert_eq!(full_view.slot(0).unwrap().display_name, b"Guest");
        assert_eq!(aral_view.decode_body().unwrap(), aral);
        assert!(DraftRosterBody::read_body(&full_msg.body[..2589]).is_err());

        // `encode_aral` always emits the unpadded 2584B ARAL roster even if padding was set.
        let mut aral_with_pad = aral.clone();
        aral_with_pad.padding = Some([1, 2, 3, 4, 5, 6]);
        let forced_aral = aral_with_pad.encode_aral();
        assert_eq!(forced_aral.opcode, Opcode::DraftRosterAral.raw());
        assert_eq!(forced_aral.body.len(), DraftRosterBody::BODY_LEN);
        assert_eq!(forced_aral.body, aral_msg.body);

        // Timer bit patterns (including -0.0 and signaling NaN) must round-trip losslessly.
        let mut raw_nan = vec![0u8; DraftRosterBody::BODY_LEN];
        raw_nan[..4].copy_from_slice(&0x7f80_0051u32.to_be_bytes());
        raw_nan[4..8].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let decoded_nan = DraftRosterBody::read_body(&raw_nan).unwrap();
        assert_eq!(decoded_nan.encode_aral().body, raw_nan);
    }

    #[test]
    fn roster_decoders_reject_short_and_oversized_slices() {
        super::super::assert_rejects(
            SceneReadinessRowsBody::BODY_LEN,
            SceneReadinessRowsBody::read_body,
        );
        super::super::assert_rejects(
            DraftPreferenceFlags::BODY_LEN,
            DraftPreferenceFlags::read_body,
        );
        super::super::assert_rejects(DraftSlotBody::BODY_LEN, DraftSlotBody::read_body);
        super::super::assert_rejects(8, DraftRosterBody::read_timers);

        for bad_len in [81, 83, 85, 87] {
            assert!(AralRerollStateBody::read_body(&vec![0u8; bad_len]).is_err());
        }
        for bad_len in [2583, 2585, 2589, 2591] {
            assert!(DraftRosterBody::read_body(&vec![0u8; bad_len]).is_err());
        }
    }

    #[test]
    fn roster_redaction_clears_human_identities_and_preserves_bot_sentinel() {
        let mut slots = std::array::from_fn(|i| DraftSlotBody::empty(i as u8));
        slots[0].occupied = 1;
        slots[0].display_name = FixedAscii::from_str_trunc("Guest");
        slots[0].identity = FixedAscii::from_str_trunc("11111111-2222-4333-8444-555555555555");
        slots[1].occupied = 1;
        slots[1].display_name = FixedAscii::from_str_trunc("Ringo Bot");
        slots[1].identity = FixedAscii::from_str_trunc("__Kindred_Player_Bot__");

        let mut observer = DraftRosterBody::full(120.0, 300.0, slots);
        observer.redact_identities(1);
        assert_eq!(observer.slots[0].display_name.trimmed_bytes(), b"Guest");
        assert!(observer.slots[0].identity.trimmed_bytes().is_empty());
        assert_eq!(
            observer.slots[1].identity.trimmed_bytes(),
            b"__Kindred_Player_Bot__"
        );

        let mut recorded = DraftRosterBody::full(120.0, 300.0, slots);
        recorded.redact_non_bots();
        assert!(recorded.slots[0].display_name.trimmed_bytes().is_empty());
        assert!(recorded.slots[0].identity.trimmed_bytes().is_empty());
        assert_eq!(recorded.slots[1].display_name.trimmed_bytes(), b"Ringo Bot");
        assert_eq!(
            recorded.slots[1].identity.trimmed_bytes(),
            b"__Kindred_Player_Bot__"
        );
    }

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
        let message = Message::known(Opcode::DraftRosterAral, body.clone());
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
