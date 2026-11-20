//! ARAL Ringo's Double Down basic-attack override.
use super::{Batch, Message, messages::wire_id};
use halcyon_sim::Event;

pub(super) fn append(out: &mut Batch, event: &Event, visible: impl Fn(u32) -> bool) -> bool {
    let Event::DoubleDown {
        source,
        attack_target,
    } = *event
    else {
        return false;
    };
    if !visible(source) {
        return true;
    }
    let actor = u32::from(wire_id(source));
    if let Some(target) = attack_target {
        if visible(target) && wire_id(target) != u16::MAX {
            out.push(Message::ability_target_used(
                actor,
                u32::from(wire_id(target)),
                3,
            ));
        }
        for key in [3u32, 8, 9, 10] {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&actor.to_be_bytes());
            body[4..8].copy_from_slice(&key.to_be_bytes());
            out.push(Message::new(0x41b, body));
        }
    } else {
        for key in [8u32, 9, 10] {
            let mut body = [0; 14];
            body[..4].copy_from_slice(&actor.to_be_bytes());
            body[4..8].copy_from_slice(&key.to_be_bytes());
            body[8..12].copy_from_slice(&3u32.to_be_bytes());
            out.push(Message::new(0x41a, body));
        }
    }
    true
}

pub(super) fn references(m: &Message) -> Option<(u32, Option<u32>)> {
    let body = &m.body;
    let target = match m.opcode {
        0x415 if body.len() == 9 && body[8] == 3 => {
            let target = u32::from_be_bytes(body[4..8].try_into().ok()?);
            if !(1500..65535).contains(&target) {
                return None;
            }
            Some(target - 1499)
        }
        0x41a
            if body.len() == 14
                && body[4..7] == [0; 3]
                && matches!(body[7], 8..=10)
                && body[8..] == [0, 0, 0, 3, 0, 0] =>
        {
            None
        }
        0x41b
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_registered_visibles_are_referenced_in_release_order() {
        let mut out = Batch::new();
        let grant = Event::DoubleDown {
            source: 1,
            attack_target: None,
        };
        append(&mut out, &grant, |_| false);
        assert!(out.is_empty());
        append(&mut out, &grant, |_| true);
        assert_eq!(
            out.iter()
                .map(|m| (m.opcode, m.body[7]))
                .collect::<Vec<_>>(),
            [(0x41a, 8), (0x41a, 9), (0x41a, 10)]
        );
        let use_perk = Event::DoubleDown {
            source: 1,
            attack_target: Some(6),
        };
        out.clear();
        append(&mut out, &use_perk, |_| true);
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [0x415, 0x41b, 0x41b, 0x41b, 0x41b]
        );
        assert_eq!(references(&out[0]), Some((1, Some(6))));
        out.clear();
        append(&mut out, &use_perk, |id| id == 1);
        assert_eq!(
            out.iter().map(|m| m.body[7]).collect::<Vec<_>>(),
            [3, 8, 9, 10]
        );
        assert!(out.iter().all(|m| references(m) == Some((1, None))));
    }
}
