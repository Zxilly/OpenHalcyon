use crate::abilities::deliver_registered;
use crate::replication::{Batch, messages::wire_id};
use crate::{Message, Room};
use halcyon_data::EntityTemplate;
use halcyon_protocol::{BuffApplication, BuffPrefix, CooldownMode, Flyout, FlyoutText};
use halcyon_sim::Event;

const BUCKSHOT_BONANZA: u32 = 0x35421f5b;
const SELF_CLEANSE: u16 = 806;
const ACTIVE_SPEED: u16 = 813;

pub(crate) fn publish(room: &mut Room, event: &Event) {
    let Event::Skedaddle {
        source,
        immunity_instance,
        speed_instance,
        immunity_seconds,
        speed_seconds,
        talent_reset,
    } = *event
    else {
        return;
    };
    let Some(hero) = source
        .checked_sub(1)
        .and_then(|i| room.world.entities.get(i as usize))
        .filter(|actor| actor.id == source && actor.type_id == EntityTemplate::Gwen.as_raw())
    else {
        return;
    };
    let enemy = 1 - hero.team;
    let Some(batch) = observed_batch(
        source,
        immunity_instance,
        speed_instance,
        immunity_seconds,
        speed_seconds,
        talent_reset,
    ) else {
        return;
    };
    deliver_registered(room, source, enemy, None, &batch);
    room.publish_observer_actor(source, &batch);
}

fn observed_batch(
    source: u32,
    immunity_instance: u32,
    speed_instance: u32,
    immunity_seconds: f32,
    speed_seconds: f32,
    talent_reset: bool,
) -> Option<Batch> {
    let actor = u32::from(wire_id(source));
    let buff = |instance: u32, definition, duration_bits| {
        BuffApplication {
            prefix: BuffPrefix {
                target: actor,
                source: actor,
                duration_bits,
                instance: instance.checked_add(1499)?,
                definition,
            },
            extras: None,
            suffix: &[0; 6],
        }
        .to_message()
        .ok()
    };
    // ARM fixture lengths and binary16 values are established for the delivered rule table.
    if immunity_seconds != 0.5 || speed_seconds != 2.2 {
        return None;
    }
    let mut batch = vec![
        Message::ability_target_used(actor, u32::MAX, 2),
        buff(immunity_instance, SELF_CLEANSE, 0x3800)?,
        Flyout::new(actor, FlyoutText::Cleansed, u32::MAX).to_message(),
        buff(speed_instance, ACTIVE_SPEED, 0x4066)?,
    ];
    if talent_reset {
        batch.push(Message::cooldown_adjustment(
            actor,
            BUCKSHOT_BONANZA,
            0.,
            CooldownMode::Set,
        ));
    }
    Some(batch)
}

pub(crate) fn recordable(m: &Message) -> bool {
    match m.fields() {
        Ok(halcyon_protocol::Fields::BuffApplication(buff)) => {
            matches!(
                buff.prefix,
                BuffPrefix { target: 1500..=1505, source, duration_bits: 0x3800, instance: 1500.., definition: SELF_CLEANSE }
                | BuffPrefix { target: 1500..=1505, source, duration_bits: 0x4066, instance: 1500.., definition: ACTIVE_SPEED } if source == buff.prefix.target
            ) && buff.extras.is_none()
                && buff.suffix == [0; 6]
        }
        Ok(halcyon_protocol::Fields::CooldownValue { .. }) => {
            matches!(
                halcyon_protocol::ModifyCooldownBody::read_exact(&m.body),
                Ok(body)
                    if (1500..=1505).contains(&body.owner)
                        && body.key == BUCKSHOT_BONANZA
                        && body.value.to_bits() == 0.0f32.to_bits()
                        && body.cooldown_mode() == Some(CooldownMode::Set)
            )
        }
        _ => m.flyout().is_some_and(|flyout| {
            flyout.text() == Some(FlyoutText::Cleansed)
                && (1500..=1505).contains(&flyout.actor)
                && flyout.parameter == u32::MAX
                && flyout.padding.is_none()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skedaddle_opened_cast_has_cleanse_then_speed_and_optional_talent_reset() {
        let messages = observed_batch(1, 700, 701, 0.5, 2.2, true).unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|m| m.known_opcode().unwrap())
                .collect::<Vec<_>>(),
            [
                crate::Opcode::PlayTargetAbility,
                crate::Opcode::ApplyBuffCompact,
                crate::Opcode::ShowFlyoutText,
                crate::Opcode::ApplyBuffCompact,
                crate::Opcode::ModifyCooldown
            ]
        );
        assert_eq!(messages[0], Message::ability_target_used(1500, u32::MAX, 2));
        assert_eq!(
            messages[2].flyout(),
            Some(Flyout::new(1500, FlyoutText::Cleansed, u32::MAX))
        );
        assert_eq!(
            messages[4],
            Message::cooldown_adjustment(1500, BUCKSHOT_BONANZA, 0., CooldownMode::Set)
        );
        assert!(messages[1..].iter().all(recordable));
        let normal = observed_batch(1, 700, 701, 0.5, 2.2, false).unwrap();
        assert_eq!(normal, messages[..4]);
        assert!(observed_batch(1, 700, 701, 0.5, 3., false).is_none());
    }
    #[tokio::test]
    async fn skedaddle_reaches_only_visible_declared_enemy_seats() {
        use crate::Peer;
        use crate::host::manager::Rooms;
        use crate::replication::messages;
        use halcyon_types::{MatchHost, Reservation};
        use tokio::sync::{mpsc, watch};
        use uuid::Uuid;

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: (0..6).map(|_| Uuid::new_v4()).collect(),
                mode: "casual_aral".into(),
                teams: vec![0, 0, 1, 0, 1, 1],
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        room.world.entities[0].type_id = EntityTemplate::Gwen.as_raw();
        let mut caster = messages::spawn(&room.world.entities[0], &room.picks, None);
        caster.body[745] = 16;
        let mut streams = Vec::new();
        for seat in room.seats.iter_mut().take(5) {
            let (tx, rx) = mpsc::channel(8);
            seat.peers[1] = Some(Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
            streams.push(rx);
        }
        for i in [0, 1, 2] {
            room.scene_views[i].declarations(std::slice::from_ref(&caster));
        }
        let enemy = usize::from(room.world.entities[2].team);
        let event = Event::Skedaddle {
            source: 1,
            immunity_instance: 700,
            speed_instance: 701,
            immunity_seconds: 0.5,
            speed_seconds: 2.2,
            talent_reset: true,
        };
        crate::abilities::publish_before_impacts(&mut room, std::slice::from_ref(&event));
        assert!(streams.iter_mut().all(|rx| rx.try_recv().is_err()));
        room.visible[enemy].insert(1);
        crate::abilities::publish_before_impacts(&mut room, &[event]);
        let records = streams[2].try_recv().unwrap().into_records();
        assert_eq!(
            records,
            observed_batch(1, 700, 701, 0.5, 2.2, true).unwrap()
        );
        assert!(streams.iter_mut().all(|rx| rx.try_recv().is_err()));
    }
}
