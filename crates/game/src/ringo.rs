//! ARAL Ringo's Double Down basic-attack override.
use super::{Batch, Message, messages::wire_id};
use halcyon_sim::Event;

pub(super) fn append(out: &mut Batch, event: &Event, visible: impl Fn(u32) -> bool) -> bool {
    let Event::DoubleDown {
        source,
        instance,
        attack_target,
    } = *event
    else {
        return false;
    };
    if !visible(source) {
        return true;
    }
    let actor = u32::from(wire_id(source));
    let Some(instance) = instance.checked_add(1499) else {
        return true;
    };
    if let Some(target) = attack_target {
        if visible(target) && wire_id(target) != u16::MAX {
            out.push(Message::ability_target_used(
                actor,
                u32::from(wire_id(target)),
                3,
            ));
        }
        out.push(
            halcyon_protocol::BuffMutation {
                target: actor,
                instance,
                change: halcyon_protocol::BuffChange::Cancel,
                padding: &[0; 6],
            }
            .to_message()
            .expect("fixed Double Down cancel"),
        );
        for key in [3u32, 8, 9, 10] {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&actor.to_be_bytes());
            body[4..8].copy_from_slice(&key.to_be_bytes());
            out.push(Message::known(crate::Opcode::ClearAbilityOverride, body));
        }
    } else {
        out.push(
            halcyon_protocol::BuffApplication {
                prefix: halcyon_protocol::BuffPrefix {
                    target: actor,
                    source: actor,
                    instance,
                    definition: 379,
                    duration_bits: 0xbc00,
                },
                extras: None,
                suffix: &[0; 6],
            }
            .to_message()
            .expect("fixed Double Down application"),
        );
        for key in [8u32, 9, 10] {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&actor.to_be_bytes());
            body[4..8].copy_from_slice(&key.to_be_bytes());
            body[8..12].copy_from_slice(&3u32.to_be_bytes());
            out.push(Message::known(crate::Opcode::OverrideAbility, body));
        }
    }
    true
}

pub(super) fn references(m: &Message) -> Option<(u32, Option<u32>)> {
    let body = &m.body;
    let target = match m.known_opcode() {
        Some(crate::Opcode::EntityProp) if matches!(m.fields(), Ok(halcyon_protocol::Fields::BuffApplication(v)) if v.prefix.source==v.prefix.target && v.prefix.definition==379 && v.prefix.duration_bits==0xbc00 && v.prefix.instance>1505 && v.suffix==[0;6]) => {
            None
        }
        Some(crate::Opcode::CancelBuff)
            if body.len() == 14
                && u32::from_be_bytes(body[4..8].try_into().ok()?) > 1505
                && body[8..] == [0; 6] =>
        {
            None
        }
        Some(crate::Opcode::PlayAbility) if body.len() == 9 && body[8] == 3 => {
            let target = u32::from_be_bytes(body[4..8].try_into().ok()?);
            if !(1500..65535).contains(&target) {
                return None;
            }
            Some(target - 1499)
        }
        Some(crate::Opcode::OverrideAbility)
            if body.len() == 14
                && body[4..7] == [0; 3]
                && matches!(body[7], 8..=10)
                && body[8..] == [0, 0, 0, 3, 0, 0] =>
        {
            None
        }
        Some(crate::Opcode::ClearAbilityOverride)
            if body.len() == 14
                && body[4..7] == [0; 3]
                && matches!(body[7], 3 | 8..=10)
                && body[8..] == [0; 6] =>
        {
            None
        }
        _ => return None,
    };
    let wire = u32::from_be_bytes(body[..4].try_into().ok()?);
    (1500..=1505).contains(&wire).then(|| (wire - 1499, target))
}

/// A cancellation references the application delivered to this scene generation.
pub(super) fn track(view: &mut super::tower_aggro::View, message: &Message) -> bool {
    if !matches!(
        message.opcode,
        crate::wire_opcode::EntityProp | crate::wire_opcode::CancelBuff
    ) {
        return true;
    }
    let Some((actor, _)) = references(message) else {
        return true;
    };
    let active = &mut view.ringo_effects[(actor - 1) as usize];
    if message.is(crate::Opcode::EntityProp) {
        *active = Some(u32::from_be_bytes(message.body[10..14].try_into().unwrap()));
        true
    } else {
        let instance = u32::from_be_bytes(message.body[4..8].try_into().unwrap());
        if *active != Some(instance) {
            return false;
        }
        *active = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_registered_visibles_are_referenced_in_release_order() {
        let mut out = Batch::new();
        let grant = Event::DoubleDown {
            source: 1,
            instance: 990,
            attack_target: None,
        };
        append(&mut out, &grant, |_| false);
        assert!(out.is_empty());
        append(&mut out, &grant, |_| true);
        assert_eq!(
            out.iter()
                .skip(1)
                .map(|m| (m.opcode, m.body[7]))
                .collect::<Vec<_>>(),
            [
                (crate::wire_opcode::OverrideAbility, 8),
                (crate::wire_opcode::OverrideAbility, 9),
                (crate::wire_opcode::OverrideAbility, 10)
            ]
        );
        let Ok(halcyon_protocol::Fields::BuffApplication(effect)) = out[0].fields() else {
            panic!("expected grant")
        };
        let instance = effect.prefix.instance;
        let mut view = super::super::tower_aggro::View::default();
        assert!(track(&mut view, &out[0]));
        let use_perk = Event::DoubleDown {
            source: 1,
            instance: 990,
            attack_target: Some(6),
        };
        out.clear();
        append(&mut out, &use_perk, |_| true);
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::PlayAbility,
                crate::wire_opcode::CancelBuff,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride,
                crate::wire_opcode::ClearAbilityOverride
            ]
        );
        assert!(!track(
            &mut super::super::tower_aggro::View::default(),
            &out[1]
        ));
        assert!(track(&mut view, &out[1]));
        assert!(!track(&mut view, &out[1]));
        assert_eq!(out[1].buff_mutation().unwrap().instance, instance);
        assert_eq!(references(&out[0]), Some((1, Some(6))));
        out.clear();
        append(&mut out, &use_perk, |id| id == 1);
        assert_eq!(
            out.iter().skip(1).map(|m| m.body[7]).collect::<Vec<_>>(),
            [3, 8, 9, 10]
        );
        assert!(out.iter().all(|m| references(m) == Some((1, None))));
    }
}
