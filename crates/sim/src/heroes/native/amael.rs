use crate::AbilitySlot;
use crate::CastTarget;
use crate::Command;
use crate::Entity;
use crate::Event;
use crate::ImpactEffect;
use crate::Kind;
use crate::Position;
use crate::Room;
use crate::TICK_HZ;
use crate::combat::native_combat::{Damage, ranks};
use crate::ecs::score::DamageSchool;
use halcyon_data::GameData;
use halcyon_data::{AbilityParameter as P, DamageOperand};
use serde::Deserialize;
use serde::Serialize;

/// Unverified Elbow Drop AoE radius; neither Ability__Amael__C variables nor captured matches specify it.
pub(crate) const ELBOW_DROP_RADIUS: f32 = 4.5;
/// Unverified Elbow Drop knock-up stun duration (1.0 s at 20 Hz); no captured Elbow Drop wire timing exists.
pub(crate) const ELBOW_DROP_KNOCKUP_TICKS: u64 = TICK_HZ;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KickKind {
    #[default]
    SuperKick,
    ElbowDrop,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Rules {
    pub damage: Damage<3>,
    pub aoe_damage: Damage<3>,
    range: [f32; 3],
    cost: [f32; 3],
    cooldown: [f32; 3],
}
impl Rules {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Amael", "Ability__Amael__C") else {
            return Ok(None);
        };
        Ok(Some(Self {
            damage: Damage::named(a, DamageOperand::UppercaseDamage)?,
            aoe_damage: Damage::named(a, DamageOperand::AoEDamage)?,
            range: ranks(a, P::Range)?,
            cost: ranks(a, P::UppercaseEnergyCost)?,
            cooldown: ranks(a, P::Cooldown)?,
        }))
    }
    pub fn cast(&self, r: usize) -> (f32, f32, u64, f32) {
        (self.range[r], self.cost[r], 1, self.cooldown[r])
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Flight {
    pub target: u32,
    pub goal: Position,
    pub range: f32,
    pub step: f32,
    pub elapsed: u8,
    pub kind: KickKind,
}

pub(crate) fn is_elbow_drop_victim(team: u8, center: Position, e: &Entity) -> bool {
    e.team != team
        && e.hp > 0.
        && matches!(e.kind, Kind::Hero | Kind::Minion)
        && center.dist(e.position) <= ELBOW_DROP_RADIUS
}

#[derive(Clone, Copy)]
struct Impact {
    source: u32,
    team: u8,
    raw: f32,
    pierce: f32,
    kind: KickKind,
}

fn impact_victim(w: &mut Room<'_>, impact: Impact, j: usize, events: &mut Vec<Event>) {
    let damage = crate::items::shop::mitigated(impact.raw, w.entities[j].armor(), impact.pierce);
    let tick = w.tick;
    let reveal_until = tick + 30;
    let knockup_until = tick + ELBOW_DROP_KNOCKUP_TICKS;
    let victim = &mut w.entities[j];
    let target = victim.id;
    victim.status.reveal_until = victim.status.reveal_until.max(reveal_until);
    if impact.kind == KickKind::SuperKick || !victim.status.blocks_movement_impairment(tick) {
        if impact.kind == KickKind::ElbowDrop {
            victim.status.stun_until = victim.status.stun_until.max(knockup_until);
            victim.status.channel = 0;
            victim.status.channel_until = 0;
        }
        victim.cast = None;
        victim.winding = None;
        victim.order = Command::Stop;
    }
    let instance = w.next_id;
    w.next_id += 1;
    events.push(Event::AmaelKick {
        source: impact.source,
        target,
        instance,
        kind: impact.kind,
    });
    w.hurt_with_effect(
        impact.source,
        impact.team,
        j,
        damage,
        (DamageSchool::Weapon, ImpactEffect::AmaelKick),
        events,
    );
}

impl Room<'_> {
    pub(crate) fn amael_kick_order(&self, caster: usize, fallback: &Entity) -> Option<Command> {
        let hero = &self.entities[caster];
        let mut best: Option<(usize, f32, u32)> = None;
        self.entities.for_each_unordered(|t| {
            if t.kind != Kind::Hero
                || t.team == hero.team
                || t.hp <= 0.
                || (t.status.stun_until <= self.tick && t.status.root_until <= self.tick)
                || !self.can_cast(caster, AbilitySlot::C, CastTarget::Entity(t.id))
            {
                return;
            }
            let mut nearby = 0usize;
            self.entities.for_each_unordered(|n| {
                if is_elbow_drop_victim(hero.team, t.position, n) {
                    nearby += 1;
                }
            });
            let dist = hero.position.dist(t.position);
            let better = best.is_none_or(|(best_nearby, best_dist, best_id)| {
                nearby
                    .cmp(&best_nearby)
                    .reverse()
                    .then_with(|| dist.total_cmp(&best_dist))
                    .then_with(|| t.id.cmp(&best_id))
                    == std::cmp::Ordering::Less
            });
            if better {
                best = Some((nearby, dist, t.id));
            }
        });
        if let Some((_, _, target)) = best {
            return Some(Command::Cast {
                slot: AbilitySlot::C,
                target: CastTarget::Entity(target),
            });
        }
        let target = CastTarget::Entity(fallback.id);
        self.can_cast(caster, AbilitySlot::C, target)
            .then_some(Command::Cast {
                slot: AbilitySlot::C,
                target,
            })
    }
}

impl Flight {
    pub fn advance(
        &mut self,
        w: &mut Room<'_>,
        source: u32,
        team: u8,
        raw: f32,
        events: &mut Vec<Event>,
    ) -> bool {
        let (Some(i), Some(j)) = (w.index(source), w.index(self.target)) else {
            return false;
        };
        let (caster, target) = (&w.entities[i], &w.entities[j]);
        if caster.hp <= 0.
            || caster.status.stun_until > w.tick
            || caster.status.root_until > w.tick
            || target.hp <= 0.
        {
            return false;
        }
        let start = caster.position;
        let next = start.advance(self.goal, self.step);
        if !w.spell_motion_clear(start, next) || w.bulwark_blocks_leap(i, next) {
            return false;
        }
        self.elapsed += 1;
        if next != start {
            w.place_spell_actor(i, next, events);
        }
        if self.elapsed == 2 {
            let target = &w.entities[j];
            if !w.visible(team, target)
                || next.dist(target.position) > self.range
                || !w.spell_motion_clear(next, target.position)
                || (self.kind == KickKind::SuperKick
                    && (target.status.stun_until > w.tick
                        || target.status.root_until > w.tick
                        || target.status.blocks_movement_impairment(w.tick)))
            {
                return false;
            }
            let impact = Impact {
                source,
                team,
                raw,
                pierce: w.entities[i].item_stats.armor_pierce,
                kind: self.kind,
            };
            match self.kind {
                KickKind::SuperKick => impact_victim(w, impact, j, events),
                KickKind::ElbowDrop => {
                    let center = target.position;
                    let mut victims = Vec::new();
                    w.entities.for_each_unordered(|e| {
                        if is_elbow_drop_victim(team, center, e) {
                            victims.push(e.id);
                        }
                    });
                    victims.sort_unstable();
                    for id in victims {
                        let Some(idx) = w.index(id) else {
                            continue;
                        };
                        if w.entities[idx].hp <= 0. {
                            continue;
                        }
                        impact_victim(w, impact, idx, events);
                    }
                }
            }
        }
        self.elapsed < 6
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BasicItem;
    use crate::World;
    use halcyon_data::EntityTemplate;
    use halcyon_data::combat_content::{Ability, AbilityVariable, DamageVariable};
    use std::path::Path;

    fn synthetic_rules() -> Rules {
        let var = |name: &str, ranks: &[f32]| AbilityVariable {
            name: name.into(),
            ranks: Some(ranks.to_vec()),
            ..Default::default()
        };
        let ability = Ability {
            ability_id: "Ability__Amael__C".into(),
            damage_variables: vec![
                DamageVariable {
                    header_u32: 3_302_314_624,
                    cp_ratio: 1.8,
                    wp_ratio: 1.5,
                    ranks: vec![300., 400., 500.],
                    ..Default::default()
                },
                DamageVariable {
                    header_u32: 2_853_935_461,
                    cp_ratio: 1.8,
                    wp_ratio: 1.5,
                    ranks: vec![400., 525., 650.],
                    ..Default::default()
                },
            ],
            variables: vec![
                var("Range", &[7., 7., 7.]),
                var("ENERGY COST", &[80., 95., 110.]),
                var("Cooldown", &[80., 65., 50.]),
            ],
            ..Default::default()
        };
        Rules {
            damage: Damage::named(&ability, DamageOperand::UppercaseDamage).unwrap(),
            aoe_damage: Damage::named(&ability, DamageOperand::AoEDamage).unwrap(),
            range: ranks(&ability, P::Range).unwrap(),
            cost: ranks(&ability, P::UppercaseEnergyCost).unwrap(),
            cooldown: ranks(&ability, P::Cooldown).unwrap(),
        }
    }

    fn setup_amael_aral_world(seed: u64, data: Option<&GameData>) -> World {
        let mut w = match data {
            Some(d) => World::from_data(seed, 6, d).unwrap(),
            None => {
                let mut w = World::new(seed, 6);
                std::sync::Arc::make_mut(&mut w.abilities).super_kick = Some(synthetic_rules());
                w
            }
        };
        w.use_six_human_roster().unwrap();
        if let Some(d) = data {
            w.select_hero(0, "Amael", d).unwrap();
        } else {
            w.entities[0].type_id = EntityTemplate::Amael.as_raw();
        }
        w.use_aral_lane().unwrap();
        w.entities[0].level = 12;
        w.entities[0].skill_points = 3;
        w.entities[0].max_energy = 500.;
        w.entities[0].energy = 500.;
        w.entities[0].stats.weapon = 100.;
        w.entities[0].item_stats.crystal = 50.;
        w.entities[0].position = Position { x: 20., y: 0. };
        for j in 0..w.entities.len() {
            let hero = &mut w.entities[j];
            if hero.kind == Kind::Hero && hero.team == 1 {
                hero.hp = 2000.;
                hero.max_hp = 2000.;
                hero.stats.armor = 0.;
                hero.position = Position { x: 24., y: 0. };
            }
        }
        w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
        w
    }

    fn cast_c(target: u32) -> Command {
        Command::Cast {
            slot: AbilitySlot::C,
            target: CastTarget::Entity(target),
        }
    }

    fn run_kick_cycle(w: &mut World, target: u32) -> Vec<Event> {
        w.command(1, cast_c(target)).unwrap();
        let mut ev = vec![];
        w.tick += 1;
        w.advance_cast(0, &mut ev);
        for _ in 0..6 {
            w.tick += 1;
            w.advance_spell_family::<crate::combat::SpellEffect>(&mut ev);
        }
        ev
    }

    fn buy_reflex_block(w: &mut World, actor_id: u32) -> u32 {
        let idx = w.index(actor_id).unwrap();
        let saved = w.entities[idx].position;
        w.entities[idx].position = w.layout.home(&w.entities[idx]);
        w.entities[idx].gold = 2000;
        let reflex = w
            .purchase(actor_id, BasicItem::ReflexBlock)
            .unwrap()
            .granted;
        w.entities[idx].position = saved;
        reflex.instance
    }

    #[test]
    fn cast_authorization_allows_stunned_or_rooted_and_rejects_cc_immune_ordinary_and_invalid_targets()
     {
        let mut w = setup_amael_aral_world(11, None);
        // Non-CC'd enemy hero with Reflex Block active is rejected for ordinary Super Kick.
        let reflex = buy_reflex_block(&mut w, 2);
        w.entities[1].max_hp = 2000.;
        w.entities[1].hp = 2000.;
        w.command(2, Command::ActivateReflexBlock { instance: reflex })
            .unwrap();
        assert_eq!(w.command(1, cast_c(2)), Err("unsupported kick target"));

        // Stunned or rooted enemy hero is accepted (including if CC immunity is also set).
        w.entities[1].status.stun_until = w.tick + 5;
        assert!(w.command(1, cast_c(2)).is_ok());
        w.cancel_cast(1, AbilitySlot::C, CastTarget::Entity(2));
        w.entities[1].status.stun_until = 0;
        w.entities[1].status.reflex = None;
        w.entities[1].status.root_until = w.tick + 5;
        assert!(w.command(1, cast_c(2)).is_ok());
        w.cancel_cast(1, AbilitySlot::C, CastTarget::Entity(2));
        w.entities[1].status.root_until = 0;

        // Self and ally rejected.
        assert!(w.command(1, cast_c(1)).is_err());
        assert!(w.command(1, cast_c(3)).is_err());

        // Non-hero enemy minion rejected as primary cast target.
        let minion_id = w.spawn(1, Kind::Minion, 4, Position { x: 23., y: 0. }, false);
        assert_eq!(
            w.command(1, cast_c(minion_id)),
            Err("unsupported kick target")
        );

        // Out of range and hidden rejected.
        w.entities[1].position = Position { x: 27.1, y: 0. };
        assert_eq!(w.command(1, cast_c(2)), Err("cast out of range"));
        w.entities[1].position = Position { x: 40., y: 0. };
        assert_eq!(w.command(1, cast_c(2)), Err("invalid cast target"));

        // Dead rejected.
        w.entities[1].position = Position { x: 24., y: 0. };
        w.entities[1].hp = 0.;
        assert_eq!(w.command(1, cast_c(2)), Err("invalid cast target"));
        w.entities[1].hp = 2000.;

        // Obstructed path rejected.
        w.entities[1].position = Position { x: 20., y: 5. };
        let wall = Position { x: 20., y: 200. };
        w.entities[1].position = wall;
        assert!(w.command(1, cast_c(2)).is_err());
        w.entities[1].position = Position { x: 24., y: 0. };

        // Silenced and stunned caster rejected.
        w.entities[0].status.silence_until = w.tick + 5;
        assert_eq!(w.command(1, cast_c(2)), Err("silenced"));
        w.entities[0].status.silence_until = 0;
        w.entities[0].status.stun_until = w.tick + 5;
        assert_eq!(w.command(1, cast_c(2)), Err("stunned"));
        w.entities[0].status.stun_until = 0;

        // Insufficient energy and cooldown rejected.
        w.entities[0].energy = 79.;
        assert_eq!(w.command(1, cast_c(2)), Err("ability not ready"));
        w.entities[0].energy = 500.;
        w.entities[0].cooldown[2] = w.tick + 10;
        assert_eq!(w.command(1, cast_c(2)), Err("ability not ready"));
    }

    #[test]
    fn elbow_drop_aoe_math_cc_immunity_and_deterministic_event_order() {
        // Verify all 3 ranks of AoEDamage: [400, 525, 650] + 1.8*50 + 1.5*100 = 640 / 765 / 890.
        for (rank, expected_raw) in [(1usize, 640.), (2, 765.), (3, 890.)] {
            let mut w = setup_amael_aral_world(17, None);
            for _ in 1..rank {
                w.command(1, Command::Learn(AbilitySlot::C)).unwrap();
            }
            w.entities[1].status.root_until = w.tick + 10;
            w.entities[3].position = Position { x: 24., y: 2. };
            w.entities[5].position = Position { x: 40., y: 0. };
            let ev = run_kick_cycle(&mut w, 2);
            let dmg: Vec<_> = ev
                .iter()
                .filter_map(|e| match *e {
                    Event::Damage {
                        effect: ImpactEffect::AmaelKick,
                        target,
                        amount,
                        ..
                    } => Some((target, amount)),
                    _ => None,
                })
                .collect();
            assert_eq!(dmg.len(), 2);
            assert!((dmg[0].1 - expected_raw).abs() < 0.001);
            assert!((dmg[1].1 - expected_raw).abs() < 0.001);
        }

        // Verify hero + minion AoE, armor mitigation, CC-immune secondary damage-without-stun,
        // out-of-radius exclusion, and deterministic same-seed Event ordering by entity.id
        // even after Bevy archetype reordering.
        #[derive(bevy_ecs::component::Component)]
        struct TestArchetypeTag;

        let run_multi = |permute_ecs: bool| -> Vec<Event> {
            let mut w = setup_amael_aral_world(42, None);
            w.entities[0].item_stats.armor_pierce = 0.2;
            // Primary rooted hero id=2
            w.entities[1].position = Position { x: 24., y: 0. };
            w.entities[1].status.root_until = w.tick + 10;
            w.entities[1].stats.armor = 100.;
            // Secondary normal enemy hero id=4 inside ELBOW_DROP_RADIUS
            w.entities[3].position = Position {
                x: 24.,
                y: ELBOW_DROP_RADIUS - 0.1,
            };
            w.entities[3].order = Command::MoveTo(Position { x: 30., y: 0. });
            // Secondary CC-immune enemy hero id=6 inside ELBOW_DROP_RADIUS (ReflexBlock grants barrier)
            w.entities[5].position = Position { x: 25., y: 1. };
            w.entities[5].order = Command::MoveTo(Position { x: 30., y: 1. });
            let reflex = buy_reflex_block(&mut w, 6);
            w.command(6, Command::ActivateReflexBlock { instance: reflex })
                .unwrap();

            // Enemy minion inside radius (given enough HP to observe stun survival) & enemy minion outside radius
            let minion_in = w.spawn(1, Kind::Minion, 4, Position { x: 22.5, y: -1. }, false);
            let m_in_idx = w.index(minion_in).unwrap();
            w.entities[m_in_idx].max_hp = 1000.;
            w.entities[m_in_idx].hp = 1000.;
            let minion_out = w.spawn(
                1,
                Kind::Minion,
                4,
                Position {
                    x: 24. + ELBOW_DROP_RADIUS + 0.2,
                    y: 0.,
                },
                false,
            );
            if permute_ecs {
                let handle = w
                    .entities
                    .ecs
                    .query::<(bevy_ecs::entity::Entity, &Entity)>()
                    .iter(&w.entities.ecs)
                    .find(|(_, e)| e.id == 2)
                    .unwrap()
                    .0;
                w.entities.ecs.entity_mut(handle).insert(TestArchetypeTag);
            }
            let ev = run_kick_cycle(&mut w, 2);

            // Check effects on each entity
            let expected_primary = crate::items::shop::mitigated(640., 100., 0.2);
            assert!((2000. - w.entities[1].hp - expected_primary).abs() < 0.01);
            assert_eq!(
                w.entities[1].status.stun_until,
                3 + ELBOW_DROP_KNOCKUP_TICKS
            );
            assert_eq!(w.entities[1].status.reveal_until, 33);

            assert!((2000. - w.entities[3].hp - 640.).abs() < 0.01);
            assert_eq!(
                w.entities[3].status.stun_until,
                3 + ELBOW_DROP_KNOCKUP_TICKS
            );
            assert_eq!(w.entities[3].order, Command::Stop);
            assert_eq!(w.entities[3].status.reveal_until, 33);

            // CC-immune secondary hero takes damage minus ARAL level-4 Reflex Block barrier (236.36), NO stun, NO interrupt
            let expected_barrier = 100. + f32::from(w.entities[5].level - 1) * (500. / 11.);
            assert!(
                (w.entities[5].max_hp - w.entities[5].hp - (640. - expected_barrier)).abs() < 0.01
            );
            assert_eq!(w.entities[5].status.stun_until, 0);
            assert_eq!(
                w.entities[5].order,
                Command::MoveTo(Position { x: 30., y: 1. })
            );
            assert_eq!(w.entities[5].status.reveal_until, 33);

            let m_in = &w.entities[w.index(minion_in).unwrap()];
            assert!(m_in.hp < m_in.max_hp);
            assert_eq!(m_in.status.stun_until, 3 + ELBOW_DROP_KNOCKUP_TICKS);

            let m_out = &w.entities[w.index(minion_out).unwrap()];
            assert_eq!(m_out.hp, m_out.max_hp);
            assert_eq!(m_out.status.stun_until, 0);

            let kicks: Vec<(u32, KickKind)> = ev
                .iter()
                .filter_map(|e| match *e {
                    Event::AmaelKick { target, kind, .. } => Some((target, kind)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                kicks,
                vec![
                    (2, KickKind::ElbowDrop),
                    (4, KickKind::ElbowDrop),
                    (6, KickKind::ElbowDrop),
                    (minion_in, KickKind::ElbowDrop),
                ]
            );
            ev
        };
        let a = run_multi(false);
        let b = run_multi(true);
        assert_eq!(a, b);
    }

    #[test]
    fn bot_prefers_stunned_hero_maximizing_nearby_enemies_and_breaks_ties_deterministically() {
        // Closer non-CC'd enemy hero (id=2 at dist 2) vs stunned hero (id=4 at dist 5.5)
        let mut w = setup_amael_aral_world(19, None);
        w.entities[1].position = Position { x: 18., y: 0. };
        w.entities[3].position = Position { x: 25.5, y: 0. };
        w.entities[3].status.stun_until = w.tick + 10;
        w.entities[5].position = Position { x: 40., y: 0. };
        assert_eq!(w.decide(0), cast_c(4));

        // Two stunned/rooted heroes: id=2 (dist 2, 0 other nearby) vs id=4 (dist 5.5, +1 minion nearby)
        w.entities[1].status.root_until = w.tick + 10;
        let minion_id = w.spawn(1, Kind::Minion, 4, Position { x: 26., y: 0. }, false);
        assert_eq!(w.decide(0), cast_c(4));

        // Tie on nearby count (both have 1), pick shortest distance: id=6 (dist 4) beats id=4 (dist 5.5)
        w.entities[1].position = Position { x: 40., y: 0. };
        w.entities[5].position = Position { x: 24., y: 0. };
        w.entities[5].status.stun_until = w.tick + 10;
        let minion_idx = w.index(minion_id).unwrap();
        w.entities[minion_idx].hp = 0.;
        assert_eq!(w.decide(0), cast_c(6));

        // Tie on nearby count AND distance -> lowest entity.id: id=2 vs id=4 both at dist 4.0
        w.entities[1].position = Position { x: 24., y: 0. };
        w.entities[3].position = Position { x: 16., y: 0. };
        w.entities[5].position = Position { x: 40., y: 0. };
        assert_eq!(w.decide(0), cast_c(2));

        // No stunned/rooted heroes -> fall back to hero_combat_order target (closest hero id=4 at dist 3)
        w.entities[1].status.root_until = 0;
        w.entities[3].status.stun_until = 0;
        w.entities[3].position = Position { x: 23., y: 0. };
        assert_eq!(w.decide(0), cast_c(4));
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn ordinary_kick_learns_costs_lunges_and_hits_only_the_visible_enemy() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = World::from_data(9, 6, &data).unwrap();
        w.use_six_human_roster().unwrap();
        w.select_hero(0, "Amael", &data).unwrap();
        w.use_aral_lane().unwrap();
        let start = Position {
            x: 20.01865,
            y: 0.72592,
        };
        let victim = Position { x: 23.5, y: 5.5 };
        w.entities[0].position = start;
        w.entities[1].position = victim;
        w.entities[0].level = 6;
        w.entities[0].skill_points = 1;
        w.entities[0].max_energy = 400.;
        w.entities[0].energy = 400.;
        w.entities[1].stats.armor = 0.;
        w.entities[0].stats.weapon = 100.;
        w.entities[0].item_stats.crystal = 50.;
        w.entities[1].hp = 2000.;
        w.entities[1].max_hp = 2000.;
        let skill = AbilitySlot::C;
        w.command(1, Command::Learn(skill)).unwrap();
        let cast = |target| Command::Cast {
            slot: skill,
            target: CastTarget::Entity(target),
        };
        assert!(w.command(1, cast(1)).is_err());
        assert!(w.command(1, cast(3)).is_err());
        let reflex = buy_reflex_block(&mut w, 2);
        w.entities[1].max_hp = 2000.;
        w.entities[1].hp = 2000.;
        w.command(2, Command::ActivateReflexBlock { instance: reflex })
            .unwrap();
        assert_eq!(w.command(1, cast(2)), Err("unsupported kick target"));
        w.tick = 40;
        w.entities[1].position.x = 40.;
        assert!(w.command(1, cast(2)).is_err());
        w.entities[1].position = victim;
        w.entities[0].energy = 79.;
        assert_eq!(w.command(1, cast(2)), Err("ability not ready"));
        w.entities[0].energy = 400.;
        assert_eq!(w.decide(0), cast(2));
        w.command(1, cast(2)).unwrap();
        let mut ev = vec![];
        w.tick = 41;
        w.advance_cast(0, &mut ev);
        assert_eq!(w.entities[0].energy, 320.);
        assert_eq!(w.entities[0].cooldown[2], 1641);
        for t in 42..=47 {
            w.tick = t;
            w.advance_spell_family::<crate::combat::SpellEffect>(&mut ev);
            if t == 42 {
                assert_eq!(w.entities[1].hp, 2000.);
            }
        }
        let i = ev
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::AmaelKick {
                        source: 1,
                        target: 2,
                        kind: KickKind::SuperKick,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(
            matches!(ev[i+1],Event::Damage{effect:ImpactEffect::AmaelKick,source:1,target:2,amount,..} if (amount-540.).abs()<0.001)
        );
        assert_eq!(w.entities[1].hp, 1460.);
        assert_eq!(w.entities[1].status.reveal_until, 73);
        assert_eq!(w.entities[1].status.stun_until, 0);
        assert!((w.entities[0].position.dist(victim) - 1.8).abs() < 0.001);
        assert!(w.command(1, cast(2)).is_err());
    }

    #[test]
    #[ignore = "requires original delivery and supplemental combat data"]
    fn elbow_drop_with_delivered_data_loads_aoe_damage_and_prefers_stunned_cluster() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(Path::new(&std::env::var_os("HALCYON_DATA_EXTRA").unwrap())),
        )
        .unwrap();
        let mut w = setup_amael_aral_world(31, Some(&data));
        w.entities[1].position = Position { x: 22., y: 0. };
        w.entities[3].position = Position { x: 25., y: 0. };
        w.entities[3].status.stun_until = w.tick + 10;
        w.entities[5].position = Position { x: 26., y: 1. };
        let minion = w.spawn(1, Kind::Minion, 4, Position { x: 25.5, y: -1. }, false);
        assert_eq!(w.decide(0), cast_c(4));
        let ev = run_kick_cycle(&mut w, 4);
        let hits: Vec<_> = ev
            .iter()
            .filter_map(|e| match *e {
                Event::AmaelKick {
                    source: 1,
                    target,
                    kind: KickKind::ElbowDrop,
                    ..
                } => Some(target),
                _ => None,
            })
            .collect();
        assert_eq!(hits, vec![2, 4, 6, minion]);
        assert!((2000. - w.entities[3].hp - 640.).abs() < 0.001);
    }
}
