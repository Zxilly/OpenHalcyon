//! Fixed layouts from Android 147219 dispatch conversions and live or replay body sizes.
use crate::codec::fixed_body;
use crate::packets::combat::RequestEquipTalentWire;
use crate::{
    ActionWords, ActorBounceBody, AralRerollStateBody, AralSurrenderBoard, EquippedTalent, Fields,
    Flyout, HudQuickMessage, ItemAction, KickPlayerBody, MapPing, MovePointRequestBody,
    RecommendedBuildPath, RequestModifyBasicAttackTargetBody, RequestPointAbilityBody,
    RequestRecommendedBuildPathBody, RequestTargetAbilityBody, RequestUpgradeAbilityBody,
    SurrenderRequestBody, TalentSelection, TeamSurrenderBoard,
};

#[derive(Debug, PartialEq)]
pub enum LiveFields<'a> {
    Item(ItemAction<'a>),
    SurrenderRequest(u8),
    AralSurrenderBoard(AralSurrenderBoard),
    TeamSurrenderBoard(TeamSurrenderBoard),
    AbilityTarget {
        target: u32,
        selector: u8,
    },
    AbilityPoint {
        vector: [f32; 3],
        selector: u8,
    },
    AttackTarget {
        target: u32,
        flag: u8,
    },
    LearnSelection(u8),
    MovePoint {
        x: f32,
        y: f32,
    },
    TalentSelection(TalentSelection),
    RecommendedBuildPath(RecommendedBuildPath),
    TalentEquipped(EquippedTalent),
    Motion {
        reference: u32,
        values: [f32; 2],
        enabled: bool,
    },
    WordHalfWord {
        first: u32,
        half: u16,
        last: u32,
    },
    MapPing(MapPing),
    HudQuickMessage(HudQuickMessage),
    TwoTables {
        first: u32,
        left: &'a [[u8; 4]; 16],
        right: &'a [[u8; 4]; 16],
    },
    KickPlayer {
        player_uuid: &'a [u8; 64],
        disconnect_code: u16,
    },
    /// `0x0480` (`AralRerollState`) ARAL hero-reroll state decoded at `0x82d890`
    /// and consumed by the ARAL draft UI at `0xad4250`.
    AralRerollState {
        /// `+0x00` (BE32): reroll cost (`w1` to `0xb32be4`).
        cost: u32,
        /// `+0x04` (BE32): reroll cap compared against `used`.
        limit: u32,
        /// `+0x08` (BE32): rerolls already consumed.
        used: u32,
        /// `+0x4c` (`76`): nonzero permits rerolling even when `used >= limit`.
        allow_override: u8,
        /// `+0x4d` (`77`, unaligned BE32): available reroll currency/token balance (`>= cost` required; displayed via `0xb32c00`).
        balance: u32,
        /// `+0x51` (`81`): nonzero locks/disables the reroll button (`eor w2, w8, #1`).
        locked: u8,
    },
}

pub(crate) fn fields(op: u16, b: &[u8]) -> Option<Fields<'_>> {
    if let Some(item) = ItemAction::decode(op, b) {
        return Some(Fields::Live(LiveFields::Item(item)));
    }
    let expected = |n: usize| b.len() == n || b.len() == (n + 2).next_multiple_of(8) - 2;
    match op {
        // These directions are ignored by the 147219 dispatcher. Its outbound packers
        // give the meaningful body length independently of the transport padding.
        crate::wire_opcode::RequestTargetAbility => {
            let (body, _) = RequestTargetAbilityBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::AbilityTarget {
                target: body.target,
                selector: body.selector,
            }));
        }
        crate::wire_opcode::RequestPointAbility => {
            let (body, _) = RequestPointAbilityBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::AbilityPoint {
                vector: body.coordinates,
                selector: body.selector,
            }));
        }
        crate::wire_opcode::RequestModifyBasicAttackTarget => {
            let (body, _) = RequestModifyBasicAttackTargetBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::AttackTarget {
                target: body.target,
                flag: body.flag,
            }));
        }
        crate::wire_opcode::RequestUpgradeAbility => {
            let (body, _) = RequestUpgradeAbilityBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::LearnSelection(body.ability_slot)));
        }
        crate::wire_opcode::RequestMovePoint => {
            let (body, _) = MovePointRequestBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::MovePoint {
                x: body.x,
                y: body.y,
            }));
        }
        crate::wire_opcode::EquipTalent => {
            let (equipped, _) = EquippedTalent::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::TalentEquipped(equipped)));
        }
        crate::wire_opcode::RequestEquipTalent => {
            if let Ok((wire, tail)) = RequestEquipTalentWire::read_with_padding(b)
                && wire.reserved == [0; 3]
                && tail.iter().all(|&v| v == 0)
            {
                return Some(Fields::Live(LiveFields::TalentSelection(wire.selection)));
            }
        }
        crate::wire_opcode::SetSurrenderStateRequest => {
            let (body, _) = SurrenderRequestBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::SurrenderRequest(body.vote)));
        }
        crate::wire_opcode::RequestRecommendedBuildPath
            if let Ok((body, tail)) = RequestRecommendedBuildPathBody::read_with_padding(b)
                && tail.iter().all(|&byte| byte == 0) =>
        {
            return Some(Fields::Live(LiveFields::RecommendedBuildPath(body.path)));
        }
        crate::wire_opcode::DraftRerollHero
        | crate::wire_opcode::DraftSelectPosition
        | crate::wire_opcode::DraftSelectBuildPath
        | crate::wire_opcode::DraftReadyControl
        | crate::wire_opcode::RequestRecommendedBuildPath
            if expected(1) =>
        {
            return Some(Fields::ControlByte {
                value: b[0],
                remaining: &b[1..],
            });
        }

        crate::wire_opcode::TransportAck
        | crate::wire_opcode::CrystalShattered
        | crate::wire_opcode::RejectBuyItem
            if expected(0) && b.iter().all(|&x| x == 0) =>
        {
            return Some(Fields::HeaderOnly { remaining: b });
        }
        crate::wire_opcode::DraftTransitionToLoading if expected(1) => {
            return Some(Fields::ControlByte {
                value: b[0],
                remaining: &b[1..],
            });
        }
        crate::wire_opcode::AutoActorBounce => {
            let (body, _) = ActorBounceBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::Motion {
                reference: body.actor,
                values: [body.height, body.duration],
                enabled: body.enabled != 0,
            }));
        }
        crate::wire_opcode::ShowFlyoutText => {
            let flyout = Flyout::decode(b)?;
            return Some(Fields::Live(LiveFields::WordHalfWord {
                first: flyout.actor,
                half: flyout.selector,
                last: flyout.parameter,
            }));
        }
        crate::wire_opcode::Ping => {
            return MapPing::decode(b).map(|v| Fields::Live(LiveFields::MapPing(v)));
        }
        crate::wire_opcode::HUDQuickMessage => {
            return HudQuickMessage::decode(b)
                .map(|v| Fields::Live(LiveFields::HudQuickMessage(v)));
        }
        crate::wire_opcode::TeamSurrenderStateChanged if expected(151) || b.len() == 155 => {
            if let Some(board) = AralSurrenderBoard::read(b) {
                return Some(Fields::Live(LiveFields::AralSurrenderBoard(board)));
            }
            if let Some(board) = TeamSurrenderBoard::read(b) {
                return Some(Fields::Live(LiveFields::TeamSurrenderBoard(board)));
            }
            return Some(Fields::Live(LiveFields::TwoTables {
                first: fixed_body::read::<u32>(b).ok()?,
                left: b[4..68].as_chunks::<4>().0.try_into().unwrap(),
                right: b[84..148].as_chunks::<4>().0.try_into().unwrap(),
            }));
        }
        crate::wire_opcode::KickPlayer => {
            let (body, _) = KickPlayerBody::read_with_padding(b).ok()?;
            return Some(Fields::Live(LiveFields::KickPlayer {
                player_uuid: b[..64].try_into().unwrap(),
                disconnect_code: body.disconnect_code,
            }));
        }
        crate::wire_opcode::AralRerollState => {
            let body = AralRerollStateBody::read_body(b).ok()?;
            return Some(Fields::Live(LiveFields::AralRerollState {
                cost: body.cost,
                limit: body.limit,
                used: body.used,
                allow_override: body.allow_override,
                balance: body.balance,
                locked: body.locked,
            }));
        }
        _ => {}
    }
    let (words, bytes) = match op {
        crate::wire_opcode::PlayerAfk
        | crate::wire_opcode::PlayerReconnected
        | crate::wire_opcode::DestroyZoneOfControl
        | crate::wire_opcode::ModifyBasicAttackTarget
        | crate::wire_opcode::SyncWorldClock => (1, 0),
        crate::wire_opcode::RequestCancelAbility => (0, 2),
        crate::wire_opcode::AutoMoveTo => (5, 1),
        crate::wire_opcode::AutoOrbit => (8, 0),
        crate::wire_opcode::CancelAutoOrbit => (4, 0),
        crate::wire_opcode::TeleportTo | crate::wire_opcode::PlayPointAbility => (4, 1),
        crate::wire_opcode::PlayPfxAtLocation | crate::wire_opcode::MakeAnnouncement => (6, 0),
        crate::wire_opcode::CreateZoneOfControl => (6, 1),
        crate::wire_opcode::ModifyGameModeVar | crate::wire_opcode::TimeoutBuff => (2, 0),
        crate::wire_opcode::ModifyBuffStack | crate::wire_opcode::DecrementBuffStack => (3, 0),
        crate::wire_opcode::SetMinimapSpawnPhase => (2, 1),
        crate::wire_opcode::SetRecommendedBuildPath => (1, 1),
        _ => return None,
    };
    let word_end = 4 * words;
    let end = word_end + bytes;
    if !expected(end) {
        return None;
    }
    Some(Fields::ActionWords(ActionWords {
        words: b[..word_end].as_chunks::<4>().0,
        halfword: None,
        bytes: &b[word_end..end],
        remaining: &b[end..],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_requests_preserve_target_vectors_selectors_and_padding() {
        let mut b = [0u8; 14];
        b[0..4].copy_from_slice(&1.25f32.to_be_bytes());
        b[4..8].copy_from_slice(&(-6.5f32).to_be_bytes());
        b[8..12].copy_from_slice(&31.75f32.to_be_bytes());
        b[12] = 3;
        assert!(matches!(
            fields(crate::wire_opcode::RequestPointAbility, &b),
            Some(Fields::Live(LiveFields::AbilityPoint {
                vector: [1.25, -6.5, 31.75],
                selector: 3
            }))
        ));
        assert!(matches!(
            fields(crate::wire_opcode::RequestMovePoint, &b),
            Some(Fields::Live(LiveFields::MovePoint { x: 1.25, y: -6.5 }))
        ));
        assert!(matches!(
            fields(
                crate::wire_opcode::RequestTargetAbility,
                &[0, 0, 5, 220, 2, 0]
            ),
            Some(Fields::Live(LiveFields::AbilityTarget {
                target: 1500,
                selector: 2
            }))
        ));
        assert!(matches!(
            fields(
                crate::wire_opcode::RequestModifyBasicAttackTarget,
                &[0, 0, 5, 220, 0, 0]
            ),
            Some(Fields::Live(LiveFields::AttackTarget {
                target: 1500,
                flag: 0
            }))
        ));
        assert!(fields(crate::wire_opcode::RequestPointAbility, &b[..12]).is_none());
        assert!(matches!(
            fields(
                crate::wire_opcode::RequestUpgradeAbility,
                &[2, 0, 0, 0, 0, 0]
            ),
            Some(Fields::Live(LiveFields::LearnSelection(2)))
        ));
    }

    #[test]
    fn mixed_width_and_strided_live_fields_use_native_offsets() {
        let b: Vec<_> = (0..158).map(|v| v as u8).collect();
        assert!(matches!(
            fields(crate::wire_opcode::ShowFlyoutText, &b[..14]),
            Some(Fields::Live(LiveFields::WordHalfWord {
                first: 0x10203,
                half: crate::wire_opcode::AutoMoveToLocation,
                last: 0x6070809
            }))
        ));
        let Some(Fields::Live(LiveFields::TwoTables { left, right, .. })) =
            fields(crate::wire_opcode::TeamSurrenderStateChanged, &b)
        else {
            panic!()
        };
        assert_eq!(left[15], [64, 65, 66, 67]);
        assert_eq!(right[15], [144, 145, 146, 147]);
        assert!(matches!(
            fields(crate::wire_opcode::AralRerollState, &b[..86]),
            Some(Fields::Live(LiveFields::AralRerollState {
                cost: 0x0001_0203,
                limit: 0x0405_0607,
                used: 0x0809_0a0b,
                allow_override: 76,
                balance: 0x4d4e_4f50,
                locked: 81,
            }))
        ));
        let mut observed = [0u8; 86];
        observed[..4].copy_from_slice(&1u32.to_be_bytes());
        observed[4..8].copy_from_slice(&5u32.to_be_bytes());
        observed[76] = 1;
        observed[77..81].copy_from_slice(&3u32.to_be_bytes());
        assert!(matches!(
            fields(crate::wire_opcode::AralRerollState, &observed),
            Some(Fields::Live(LiveFields::AralRerollState {
                cost: 1,
                limit: 5,
                used: 0,
                allow_override: 1,
                balance: 3,
                locked: 0,
            }))
        ));
        assert!(fields(crate::wire_opcode::TeamSurrenderStateChanged, &b[..150]).is_none());
    }
}
