//! Abilities, cooldowns, charges, overrides, projectiles, PFX, and talent wire codecs.

use crate::codec::fixed_body::{self, fixed_packet};
use crate::codec::invalid;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io;
use zerocopy::{
    FromZeros, IntoBytes,
    byteorder::big_endian::{F32, U16, U32},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big, repr = u8)]
#[repr(u8)]
pub enum CooldownMode {
    Adjust = 0,
    Set = 1,
}

impl CooldownMode {
    pub const fn from_wire(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Adjust),
            1 => Some(Self::Set),
            _ => None,
        }
    }

    pub const fn raw(self) -> u8 {
        self as u8
    }
}

/// 13-byte body of `0x0428` (`Opcode::ModifyCooldown`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyCooldownBody {
    pub owner: u32,
    pub key: u32,
    pub value: f32,
    pub mode: u8,
}
fixed_packet!(ModifyCooldownBody, Opcode::ModifyCooldown, 13);

impl ModifyCooldownBody {
    pub const fn from_mode(owner: u32, key: u32, value: f32, mode: CooldownMode) -> Self {
        Self {
            owner,
            key,
            value,
            mode: mode.raw(),
        }
    }

    pub const fn cooldown_mode(self) -> Option<CooldownMode> {
        CooldownMode::from_wire(self.mode)
    }
}

/// 8-byte body of `0x043A` (`Opcode::GrantAbility`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct GrantAbilityBody {
    pub actor: u32,
    pub operand: u32,
}
fixed_packet!(GrantAbilityBody, Opcode::GrantAbility, 8);

/// 9-byte body of `0x0415` (`Opcode::PlayTargetAbility`), padded to 14 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayTargetAbilityBody {
    pub actor: u32,
    pub target: u32,
    pub selector: u8,
}
fixed_packet!(PlayTargetAbilityBody, Opcode::PlayTargetAbility, 9);

/// 17-byte body of `0x0416` (`Opcode::PlayPointAbility`), padded to 22 bytes when aligned.
#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct PlayPointAbilityBody {
    pub actor: u32,
    pub coordinates: [f32; 3],
    pub selector: u8,
}
fixed_packet!(PlayPointAbilityBody, Opcode::PlayPointAbility, 17);

/// 18-byte preset payload inside `0x048a InitCooldown` (`key`, `value`, `config_value`, `flags`).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct InitCooldownPayload {
    /// Cooldown slot/definition key.
    pub key: u32,
    /// Initial current remaining cooldown seconds.
    pub value: f32,
    /// Configured base cooldown duration seconds.
    pub config_value: f32,
    /// Trailing preset flag bytes.
    pub flags: [u8; 6],
}

impl InitCooldownPayload {
    /// Fixed byte width of the cooldown preset payload.
    pub const LEN: usize = 18;

    /// Serializes this 18-byte preset payload using big-endian `binrw`.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        fixed_body::write(self, &mut out, Self::LEN)
            .expect("18-byte InitCooldownPayload fits output array");
        out
    }

    /// Reads an 18-byte preset payload using big-endian `binrw`.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::LEN]) -> Self {
        fixed_body::read(bytes).expect("18-byte slice is valid InitCooldownPayload")
    }
}

/// `0x048a InitCooldown`: 22-byte initial ability/item cooldown slot registration.
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct InitCooldownBody {
    /// Wire actor identifier.
    pub owner: u32,
    /// Cooldown slot/definition key.
    pub key: u32,
    /// Initial current remaining cooldown seconds.
    pub value: f32,
    /// Configured base cooldown duration seconds.
    pub config_value: f32,
    /// Trailing preset flag bytes.
    pub flags: [u8; 6],
}
fixed_packet!(InitCooldownBody, Opcode::InitCooldown, 22);

impl InitCooldownBody {
    /// Raw offset where the 18-byte preset payload begins after [`Self::owner`].
    pub const PAYLOAD_OFFSET: usize = 4;

    /// Returns the 18-byte [`InitCooldownPayload`] view of this cooldown entry.
    #[must_use]
    pub const fn payload(&self) -> InitCooldownPayload {
        InitCooldownPayload {
            key: self.key,
            value: self.value,
            config_value: self.config_value,
            flags: self.flags,
        }
    }

    /// Serializes only the 18-byte preset payload `(key, value, config_value, flags)`.
    #[must_use]
    pub fn encode_payload(&self) -> [u8; InitCooldownPayload::LEN] {
        self.payload().to_bytes()
    }

    /// Reconstructs an [`InitCooldownBody`] from an owner wire ID and an 18-byte preset payload.
    #[must_use]
    pub fn from_payload(owner: u32, bytes: [u8; InitCooldownPayload::LEN]) -> Self {
        let payload = InitCooldownPayload::from_bytes(&bytes);
        Self {
            owner,
            key: payload.key,
            value: payload.value,
            config_value: payload.config_value,
            flags: payload.flags,
        }
    }
}

/// `0x0429 ModifyAbilityCharges`: 13-byte ability charge update frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ModifyAbilityChargesBody {
    /// Owning wire actor identifier.
    pub owner: u32,
    /// Target ability slot or key identifier.
    pub ability_slot: u32,
    /// Charge count or delta operand.
    pub operand: u32,
    /// Update mode byte.
    pub mode: u8,
}
fixed_packet!(ModifyAbilityChargesBody, Opcode::ModifyAbilityCharges, 13);

/// `0x041a OverrideAbility`: 13-byte ability slot override frame (often padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct OverrideAbilityBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Ability key/slot identifier being overridden.
    pub key: u32,
    /// Replacement ability key/identifier.
    pub replacement: u32,
    /// Override flag byte.
    pub flag: u8,
}
fixed_packet!(OverrideAbilityBody, Opcode::OverrideAbility, 13);

/// `0x041b ClearAbilityOverride`: 8-byte ability slot override clear frame (often padded to 14B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct ClearAbilityOverrideBody {
    /// Owning wire actor identifier.
    pub actor: u32,
    /// Ability key/slot identifier to clear.
    pub key: u32,
}
fixed_packet!(ClearAbilityOverrideBody, Opcode::ClearAbilityOverride, 8);

/// `0x043c CancelAbility`: 10-byte server ability/item cast cancellation frame (up to 14B padded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct CancelAbilityBody {
    /// Casting wire actor identifier.
    pub actor: u32,
    /// Selector or item instance reference being cancelled.
    pub reference: u32,
    /// Non-zero when cancelling an item activation rather than an ability slot.
    pub is_item: u8,
    /// Non-zero when shared channel state should also be cancelled.
    pub cancel_shared: u8,
}
fixed_packet!(CancelAbilityBody, Opcode::CancelAbility, 10);

/// `0x0414 RequestCancelAbility`: 2-byte client cast-cancel request (padded to 6 bytes on wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestCancelAbilityBody {
    /// Ability slot selector byte.
    pub ability_slot: u8,
    /// Client cancel mode flag byte.
    pub cancel_flag: u8,
}
fixed_packet!(RequestCancelAbilityBody, Opcode::RequestCancelAbility, 2);

/// `0x0436 RequestUpgradeAbility`: 1-byte client ability upgrade request (padded to 6 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestUpgradeAbilityBody {
    /// Ability slot selector byte.
    pub ability_slot: u8,
}
fixed_packet!(RequestUpgradeAbilityBody, Opcode::RequestUpgradeAbility, 1);

/// `0x0412 RequestPointAbility`: 13-byte client point-targeted cast request (padded to 14 bytes).
#[derive(Clone, Copy, Debug, PartialEq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestPointAbilityBody {
    /// World coordinates `[x, y, z]`.
    pub coordinates: [f32; 3],
    /// Ability selector byte.
    pub selector: u8,
}
fixed_packet!(RequestPointAbilityBody, Opcode::RequestPointAbility, 13);

/// `0x0411 RequestTargetAbility`: 5-byte client unit-targeted cast request (padded to 6 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
pub struct RequestTargetAbilityBody {
    /// Target wire actor identifier.
    pub target: u32,
    /// Ability selector byte.
    pub selector: u8,
}
fixed_packet!(RequestTargetAbilityBody, Opcode::RequestTargetAbility, 5);

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

const PFX_PREFIX_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct PlayPfxWire {
    effect_key: u32,
    actor: u32,
    position: [f32; 3],
    unknown_word: u32,
}

/// ActionPlayPfxAtLocation (0419), including the optional decrypted frame padding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayPfxAtLocation {
    pub effect_key: u32,
    pub actor: u32,
    pub position: [f32; 3],
    pub unknown_word: u32,
    pub padding: Option<[u8; 6]>,
}

impl PlayPfxAtLocation {
    pub fn to_message(&self) -> Message {
        let wire = PlayPfxWire {
            effect_key: self.effect_key,
            actor: self.actor,
            position: self.position,
            unknown_word: self.unknown_word,
        };
        let suffix = self.padding.as_ref().map_or(&[][..], |pad| &pad[..]);
        fixed_body::message_with_padding(Opcode::PlayPfxAtLocation, &wire, PFX_PREFIX_LEN, suffix)
            .expect("pfx padding is 0 or 6 bytes")
    }
}

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
pub(crate) struct RequestEquipTalentWire {
    pub(crate) selection: TalentSelection,
    pub(crate) reserved: [u8; 3],
}
fixed_packet!(RequestEquipTalentWire, Opcode::RequestEquipTalent, 4);

impl TalentSelection {
    pub const fn from_wire(value: u8) -> Option<Self> {
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
    pub fn message(self) -> io::Result<Message> {
        if self.selection == TalentSelection::NoTalent && self.level != 0 {
            return Err(invalid("NoTalent requires level zero"));
        }
        Ok(self.encode())
    }
}

impl Message {
    /// The charge operand is an unsigned wire word; its interpretation depends on the mode.
    pub fn modify_ability_charges(owner: u32, ability_slot: u32, operand: u32, mode: u8) -> Self {
        ModifyAbilityChargesBody {
            owner,
            ability_slot,
            operand,
            mode,
        }
        .encode_padded()
    }

    pub fn cooldown_adjustment(owner: u32, key: u32, value: f32, mode: CooldownMode) -> Self {
        ModifyCooldownBody::from_mode(owner, key, value, mode).encode()
    }

    /// Observed 0x043A actor and learned ability operand.
    pub fn ability_learned(actor: u32, operand: u32) -> Self {
        GrantAbilityBody { actor, operand }.encode()
    }

    /// Observed 0x0415 caster, target actor and ability selector.
    pub fn ability_target_used(actor: u32, target: u32, selector: u8) -> Self {
        PlayTargetAbilityBody {
            actor,
            target,
            selector,
        }
        .encode()
    }

    /// Observed 0x0416 actor, three world coordinates and ability selector.
    pub fn ability_point_used(actor: u32, coordinates: [f32; 3], selector: u8) -> Self {
        PlayPointAbilityBody {
            actor,
            coordinates,
            selector,
        }
        .encode()
    }

    pub fn play_pfx_at_location(&self) -> Option<PlayPfxAtLocation> {
        if !self.is(Opcode::PlayPfxAtLocation) {
            return None;
        }
        let (wire, tail): (PlayPfxWire, _) =
            fixed_body::read_with_padding(&self.body, PFX_PREFIX_LEN).ok()?;
        let padding = match tail {
            [] => None,
            tail => Some(tail.try_into().ok()?),
        };
        Some(PlayPfxAtLocation {
            effect_key: wire.effect_key,
            actor: wire.actor,
            position: wire.position,
            unknown_word: wire.unknown_word,
            padding,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fields, LiveFields, MatchCipher};

    #[test]
    fn observed_idris_points_round_trip_and_preserve_signed_zero() {
        let cipher = MatchCipher::new("synthetic-round-trip");
        for coordinates in [
            [-40.8939_f32, 0.0, 1.4944],
            [-58.2402, -0.0, 14.9417],
            [-44.1787, -0.0, 3.5794],
            [-34.1510, 0.0, -0.7185],
        ] {
            let mut body = coordinates
                .into_iter()
                .flat_map(f32::to_be_bytes)
                .collect::<Vec<_>>();
            body.push(3);
            let request = Message::known(Opcode::RequestPointAbility, &body[..]);
            let wire = cipher.encode(&request).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.body[13..], [0]);
            assert_eq!(
                decoded.fields().unwrap(),
                Fields::Live(LiveFields::AbilityPoint {
                    vector: coordinates,
                    selector: 3
                })
            );
            let response = Message::ability_point_used(1505, coordinates, 3);
            let wire = cipher.encode(&response).unwrap();
            let decoded = cipher.decode(&wire[2..]).unwrap();
            assert_eq!(decoded.opcode, crate::wire_opcode::PlayPointAbility);
            assert_eq!(&decoded.body[..4], &1505u32.to_be_bytes());
            assert_eq!(&decoded.body[4..17], &body);
            assert_eq!(decoded.body[17..], [0; 5]);
        }
    }

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

    #[test]
    fn preserves_arbitrary_keys_float_bits_and_padding() {
        let words = [
            0x12345678u32,
            1505,
            0x80000000,
            0x7fc01234,
            0xff800000,
            0xdeadbeef,
        ];
        let body: Vec<_> = words.into_iter().flat_map(u32::to_be_bytes).collect();
        for tail in [&[][..], &[6, 5, 4, 3, 2, 1]] {
            let message =
                Message::known(Opcode::PlayPfxAtLocation, [body.as_slice(), tail].concat());
            let effect = message.play_pfx_at_location().unwrap();
            assert_eq!(effect.position.map(f32::to_bits), words[2..5]);
            assert_eq!(effect.unknown_word, words[5]);
            assert_eq!(effect.to_message(), message);
            assert!(matches!(message.fields(), Ok(Fields::ActionWords(_))));
        }
        for size in [0, 23, 25, 29, 31] {
            assert!(
                Message::known(Opcode::PlayPfxAtLocation, vec![0; size])
                    .play_pfx_at_location()
                    .is_none()
            );
        }
        assert!(
            Message::known(Opcode::OverrideAbility, body)
                .play_pfx_at_location()
                .is_none()
        );
    }

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

    #[test]
    fn cooldowns_charges_overrides_and_ability_requests_roundtrip() {
        let cd = InitCooldownBody {
            owner: 1500,
            key: 77,
            value: 0.0,
            config_value: 14.0,
            flags: [1, 0, 2, 0, 3, 4],
        };
        let cd_msg = cd.encode();
        assert_eq!(cd_msg.body.len(), 22);
        assert_eq!(InitCooldownBody::read_exact(&cd_msg.body).ok(), Some(cd));
        let payload_bytes = cd.encode_payload();
        assert_eq!(
            &payload_bytes[..],
            &cd_msg.body[InitCooldownBody::PAYLOAD_OFFSET..]
        );
        assert_eq!(
            payload_bytes,
            [
                0, 0, 0, 77, // key = 77
                0, 0, 0, 0, // value = 0.0
                0x41, 0x60, 0, 0, // config_value = 14.0
                1, 0, 2, 0, 3, 4, // flags
            ]
        );
        assert_eq!(
            InitCooldownPayload::from_bytes(&payload_bytes),
            cd.payload()
        );
        assert_eq!(InitCooldownBody::from_payload(1500, payload_bytes), cd);

        let charges = ModifyAbilityChargesBody {
            owner: 1500,
            ability_slot: 2,
            operand: 1,
            mode: 3,
        };
        assert_eq!(
            ModifyAbilityChargesBody::read_exact(&charges.encode().body).ok(),
            Some(charges)
        );

        let ov = OverrideAbilityBody {
            actor: 1500,
            key: 8,
            replacement: 3,
            flag: 0,
        };
        let ov_msg = ov.encode_padded();
        assert_eq!(ov_msg.body.len(), 14);
        assert_eq!(
            OverrideAbilityBody::read_padded_zero(&ov_msg.body).ok(),
            Some(ov)
        );

        let clr = ClearAbilityOverrideBody {
            actor: 1500,
            key: 3,
        };
        let clr_msg = clr.encode_padded();
        assert_eq!(clr_msg.body.len(), 14);
        assert_eq!(
            ClearAbilityOverrideBody::read_padded_zero(&clr_msg.body).ok(),
            Some(clr)
        );

        let cancel = CancelAbilityBody {
            actor: 1500,
            reference: 2,
            is_item: 0,
            cancel_shared: 0,
        };
        assert_eq!(cancel.encode().body.len(), 10);
        assert_eq!(
            CancelAbilityBody::read_exact(&cancel.encode().body).ok(),
            Some(cancel)
        );
        assert_eq!(
            CancelAbilityBody::read_padded_zero(&cancel.encode_padded().body).ok(),
            Some(cancel)
        );

        let req_cancel = RequestCancelAbilityBody {
            ability_slot: 1,
            cancel_flag: 0,
        };
        assert_eq!(req_cancel.encode_padded().body, [1, 0, 0, 0, 0, 0]);
        assert_eq!(
            RequestCancelAbilityBody::read_padded_zero(&req_cancel.encode_padded().body).ok(),
            Some(req_cancel)
        );

        let req_up = RequestUpgradeAbilityBody { ability_slot: 2 };
        assert_eq!(req_up.encode_padded().body, [2, 0, 0, 0, 0, 0]);
        assert_eq!(
            RequestUpgradeAbilityBody::read_padded_zero(&req_up.encode_padded().body).ok(),
            Some(req_up)
        );

        let req_pt = RequestPointAbilityBody {
            coordinates: [1.5, 0.0, -2.5],
            selector: 1,
        };
        let req_pt_msg = req_pt.encode_padded();
        assert_eq!(req_pt_msg.body.len(), 14);
        assert_eq!(
            RequestPointAbilityBody::read_padded_zero(&req_pt_msg.body).ok(),
            Some(req_pt)
        );

        let req_tgt = RequestTargetAbilityBody {
            target: 1501,
            selector: 2,
        };
        assert_eq!(req_tgt.encode_padded().body, [0, 0, 5, 221, 2, 0]);
        assert_eq!(
            RequestTargetAbilityBody::read_padded_zero(&req_tgt.encode_padded().body).ok(),
            Some(req_tgt)
        );
    }
}
