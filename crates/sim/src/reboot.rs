use super::*;

const DURATION: u64 = 7 * TICK_HZ / 2;

// Alpha's native perk parameters are separate from her A/B/C ability tables.
fn cooldown(actor: &Entity) -> f32 {
    match actor.ability_ranks[AbilitySlot::C.idx()] {
        0 | 1 => 85.,
        2 => 75.,
        _ => 65.,
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) enum Phase {
    Rebooting { until: u64, instance: u32 },
    Recharging { until: u64 },
}

impl Entity {
    pub(super) fn is_rebooting(&self) -> bool {
        matches!(self.reboot, Some(Phase::Rebooting { .. }))
    }
}

pub(super) fn on_lethal(
    actor: &mut Entity,
    source: u32,
    tick: u64,
    next_id: &mut u32,
    events: &mut Vec<Event>,
) -> bool {
    if actor.type_id != EntityTemplate::Alpha.as_raw() || actor.kind != Kind::Hero {
        return false;
    }
    let cooldown_seconds = cooldown(actor);
    match actor.reboot {
        Some(Phase::Rebooting { .. }) => {
            actor.reboot = Some(Phase::Recharging {
                until: tick + cooldown_seconds as u64 * TICK_HZ,
            });
            return false;
        }
        Some(Phase::Recharging { until }) if tick < until => return false,
        _ => {}
    }
    let level = f32::from(actor.level.clamp(1, 12) - 1);
    let base_energy = actor.stats.energy + level * actor.stats.growth.energy;
    actor.hp = 600. + 2000. * level / 11. + 0.65 * (actor.max_energy - base_energy).max(0.);
    actor.cast = None;
    actor.winding = None;
    actor.order = Command::Stop;
    actor.status = Default::default();
    let instance = *next_id;
    *next_id += 1;
    actor.reboot = Some(Phase::Rebooting {
        until: tick + DURATION,
        instance,
    });
    events.push(Event::RebootStarted {
        actor: actor.id,
        source,
        instance,
        cooldown_seconds,
    });
    true
}

pub(super) fn advance(actor: &mut Entity, tick: u64, events: &mut Vec<Event>) -> bool {
    match actor.reboot {
        Some(Phase::Rebooting { until, instance }) if tick >= until => {
            let cooldown_seconds = cooldown(actor);
            actor.reboot = Some(Phase::Recharging {
                until: tick + cooldown_seconds as u64 * TICK_HZ,
            });
            actor.cooldown[AbilitySlot::A.idx()] = tick;
            actor.cooldown[AbilitySlot::B.idx()] = tick;
            actor.hp = actor.max_hp * 0.4;
            events.push(Event::RebootRecovered {
                actor: actor.id,
                instance,
                cooldown_seconds,
                hp: actor.hp,
            });
            true
        }
        Some(Phase::Rebooting { .. }) => true,
        Some(Phase::Recharging { until }) if tick >= until => {
            actor.reboot = None;
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lethal_hit_opens_a_targetable_reboot_then_recovers_or_dies_once() {
        for destroyed in [false, true] {
            let mut world = World::new(42, 6);
            world.use_six_human_roster().unwrap();
            world.use_aral_lane().unwrap();
            let alpha = &mut world.entities[0];
            alpha.type_id = EntityTemplate::Alpha.as_raw();
            alpha.level = 12;
            alpha.max_energy = alpha.stats.energy + 11. * alpha.stats.growth.energy + 100.;
            alpha.hp = 1.;
            alpha.ability_ranks[2] = 2;
            let hp = alpha.max_hp;
            let mut events = Vec::new();
            world.hurt(4, 1, 0, 20., DamageSchool::True, &mut events);
            assert!(matches!(
                events.last(),
                Some(Event::RebootStarted {
                    actor: 1,
                    cooldown_seconds: 75.,
                    ..
                })
            ));
            assert_eq!(world.entities[0].hp, 2665.);
            assert!(!events.iter().any(|e| matches!(e, Event::Death { .. })));
            assert!(world.command(1, Command::Attack(4)).is_err());
            if destroyed {
                world.hurt(4, 1, 0, 3000., DamageSchool::True, &mut events);
                assert_eq!(world.entities[0].hp, 0.);
                assert_eq!(
                    events
                        .iter()
                        .filter(|e| matches!(e, Event::Death { id: 1, .. }))
                        .count(),
                    1
                );
                assert!(!advance(&mut world.entities[0], DURATION, &mut events));
            } else {
                assert!(advance(&mut world.entities[0], DURATION - 1, &mut events));
                assert!(world.entities[0].is_rebooting());
                assert!(advance(&mut world.entities[0], DURATION, &mut events));
                assert!(matches!(
                    events.last(),
                    Some(Event::RebootRecovered {
                        actor: 1,
                        cooldown_seconds: 75.,
                        ..
                    })
                ));
                assert_eq!(world.entities[0].hp, hp * 0.4);
                world.tick = DURATION + 1;
                world.hurt(4, 1, 0, hp, DamageSchool::True, &mut events);
                assert_eq!(world.entities[0].hp, 0.);
            }
        }
    }
}
