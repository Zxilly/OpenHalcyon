//! Fixed layouts from Android 147219 dispatch conversions and live or replay body sizes.
use super::{ActionWords, EquippedTalent, Fields, TalentSelection};

#[derive(Debug, PartialEq)]
pub enum LiveFields<'a> {
    Item(super::ItemAction<'a>),
    SurrenderRequest(u8),
    AralSurrenderBoard(super::AralSurrenderBoard),
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
    RecommendedBuildPath(super::RecommendedBuildPath),
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
    MapPing(super::MapPing),
    HudQuickMessage(super::HudQuickMessage),
    TwoTables {
        first: u32,
        left: &'a [[u8; 4]; 16],
        right: &'a [[u8; 4]; 16],
    },
    KickPlayer {
        player_uuid: &'a [u8; 64],
        disconnect_code: u16,
    },
    UnalignedWord {
        prefix: [u32; 3],
        word77: u32,
    },
}
pub(super) fn fields(op: u16, b: &[u8]) -> Option<Fields<'_>> {
    if let Some(item) = super::ItemAction::decode(op, b) {
        return Some(Fields::Live(LiveFields::Item(item)));
    }
    let expected = |n: usize| b.len() == n || b.len() == (n + 2).next_multiple_of(8) - 2;
    let word = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    let half = |i: usize| u16::from_be_bytes(b[i..i + 2].try_into().unwrap());
    match op {
        // These directions are ignored by the 147219 dispatcher. Its outbound packers
        // give the meaningful body length independently of the transport padding.
        crate::wire_opcode::RequestTargetAbility if expected(5) => {
            return Some(Fields::Live(LiveFields::AbilityTarget {
                target: word(0),
                selector: b[4],
            }));
        }
        crate::wire_opcode::RequestPointAbility if expected(13) => {
            return Some(Fields::Live(LiveFields::AbilityPoint {
                vector: std::array::from_fn(|i| f32::from_bits(word(4 * i))),
                selector: b[12],
            }));
        }
        crate::wire_opcode::RequestModifyBasicAttackTarget if expected(5) => {
            return Some(Fields::Live(LiveFields::AttackTarget {
                target: word(0),
                flag: b[4],
            }));
        }
        crate::wire_opcode::RequestUpgradeAbility if expected(1) => {
            return Some(Fields::Live(LiveFields::LearnSelection(b[0])));
        }
        crate::wire_opcode::RequestMovePoint if expected(8) => {
            return Some(Fields::Live(LiveFields::MovePoint {
                x: f32::from_bits(word(0)),
                y: f32::from_bits(word(4)),
            }));
        }
        crate::wire_opcode::EquipTalent if expected(6) => {
            return Some(Fields::Live(LiveFields::TalentEquipped(EquippedTalent {
                actor: word(0),
                selection: TalentSelection::from_wire(b[4])?,
                level: b[5],
            })));
        }
        crate::wire_opcode::RequestEquipTalent if expected(4) && b[1..].iter().all(|&v| v == 0) => {
            return Some(Fields::Live(LiveFields::TalentSelection(
                TalentSelection::from_wire(b[0])?,
            )));
        }
        crate::wire_opcode::SetSurrenderStateRequest if expected(1) => {
            return Some(Fields::Live(LiveFields::SurrenderRequest(b[0])));
        }
        crate::wire_opcode::RequestRecommendedBuildPath
            if expected(1)
                && b[1..].iter().all(|&byte| byte == 0)
                && let Some(path) = super::RecommendedBuildPath::from_repr(b[0]) =>
        {
            return Some(Fields::Live(LiveFields::RecommendedBuildPath(path)));
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
        crate::wire_opcode::AutoActorBounce if expected(16) => {
            return Some(Fields::Live(LiveFields::Motion {
                reference: word(0),
                values: [f32::from_bits(word(4)), f32::from_bits(word(8))],
                enabled: b[12..16].iter().any(|&v| v != 0),
            }));
        }
        crate::wire_opcode::ShowFlyoutText => {
            let flyout = super::Flyout::decode(b)?;
            return Some(Fields::Live(LiveFields::WordHalfWord {
                first: flyout.actor,
                half: flyout.selector,
                last: flyout.parameter,
            }));
        }
        crate::wire_opcode::Ping => {
            return super::MapPing::decode(b).map(|v| Fields::Live(LiveFields::MapPing(v)));
        }
        crate::wire_opcode::HUDQuickMessage => {
            return super::HudQuickMessage::decode(b)
                .map(|v| Fields::Live(LiveFields::HudQuickMessage(v)));
        }
        crate::wire_opcode::TeamSurrenderStateChanged if expected(151) || b.len() == 155 => {
            if let Some(board) = super::AralSurrenderBoard::read(b) {
                return Some(Fields::Live(LiveFields::AralSurrenderBoard(board)));
            }
            return Some(Fields::Live(LiveFields::TwoTables {
                first: word(0),
                left: b[4..68].as_chunks::<4>().0.try_into().unwrap(),
                right: b[84..148].as_chunks::<4>().0.try_into().unwrap(),
            }));
        }
        crate::wire_opcode::KickPlayer if expected(66) => {
            return Some(Fields::Live(LiveFields::KickPlayer {
                player_uuid: b[..64].try_into().unwrap(),
                disconnect_code: half(64),
            }));
        }
        crate::wire_opcode::AralDraftMarker if expected(82) => {
            return Some(Fields::Live(LiveFields::UnalignedWord {
                prefix: [word(0), word(4), word(8)],
                word77: word(77),
            }));
        }
        _ => {}
    }
    let (words, has_halfword, bytes) = match op {
        crate::wire_opcode::PlayerAfk
        | crate::wire_opcode::PlayerReconnected
        | crate::wire_opcode::DestroyZoneOfControl
        | crate::wire_opcode::ModifyBasicAttackTarget
        | crate::wire_opcode::SyncWorldClock => (1, false, 0),
        crate::wire_opcode::RequestCancelAbility => (0, false, 2),
        crate::wire_opcode::AutoMoveTo => (5, false, 1),
        crate::wire_opcode::AutoOrbit => (8, false, 0),
        crate::wire_opcode::CancelAutoOrbit => (4, false, 0),
        crate::wire_opcode::TeleportTo | crate::wire_opcode::PlayPointAbility => (4, false, 1),
        crate::wire_opcode::PlayPfxAtLocation | crate::wire_opcode::MakeAnnouncement => {
            (6, false, 0)
        }
        crate::wire_opcode::CreateZoneOfControl => (6, false, 1),
        crate::wire_opcode::ModifyGameModeVar | crate::wire_opcode::TimeoutBuff => (2, false, 0),
        crate::wire_opcode::ModifyBuffStack | crate::wire_opcode::DecrementBuffStack => {
            (3, false, 0)
        }
        crate::wire_opcode::SetMinimapSpawnPhase => (2, false, 1),
        crate::wire_opcode::SetRecommendedBuildPath => (1, false, 1),
        _ => return None,
    };
    let word_end = 4 * words;
    let byte_start = word_end + 2 * usize::from(has_halfword);
    let end = byte_start + bytes;
    if !expected(end) {
        return None;
    }
    Some(Fields::ActionWords(ActionWords {
        words: b[..word_end].as_chunks::<4>().0,
        halfword: has_halfword.then(|| half(word_end)),
        bytes: &b[byte_start..end],
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
            fields(crate::wire_opcode::AralDraftMarker, &b[..86]),
            Some(Fields::Live(LiveFields::UnalignedWord {
                word77: 0x4d4e4f50,
                ..
            }))
        ));
        assert!(fields(crate::wire_opcode::TeamSurrenderStateChanged, &b[..150]).is_none());
    }
}
