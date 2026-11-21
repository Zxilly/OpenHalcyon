use crate::AbilitySlot;
use crate::Event;
use crate::Room;
use crate::TICK_HZ;
use crate::combat::Status;
use crate::combat::native_combat::ranks;
use halcyon_data::AbilityParameter as Parameter;
use halcyon_data::GameData;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Rules {
    speed: [f32; 5],
    duration: [f32; 5],
    immunity: [f32; 5],
    cost: [f32; 5],
    cooldown: [f32; 5],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Active {
    immunity_until: u64,
    speed_until: u64,
    speed: f32,
}

fn ticks(seconds: f32) -> u64 {
    (seconds * TICK_HZ as f32).round() as u64
}

impl Rules {
    pub fn load(data: &GameData) -> Result<Option<Self>, String> {
        let Some(a) = data.ability("Gwen", "Ability__Gwen__B") else {
            return Ok(None);
        };
        Ok(Some(Self {
            speed: ranks(a, Parameter::SkedaddleSpeed)?,
            duration: ranks(a, Parameter::SkedaddleDuration)?,
            immunity: ranks(a, Parameter::SkedaddleImmunityDuration)?,
            cost: ranks(a, Parameter::EnergyCost)?,
            cooldown: ranks(a, Parameter::Cooldown)?,
        }))
    }

    pub fn cast(&self, rank: usize) -> (f32, f32, u64, f32) {
        (0., self.cost[rank], 1, self.cooldown[rank])
    }
}

pub(crate) fn speed(status: &Status, tick: u64) -> f32 {
    status
        .skedaddle
        .filter(|s| tick < s.speed_until)
        .map_or(0., |s| s.speed)
}

impl Status {
    pub(crate) fn blocks_movement_impairment(&self, tick: u64) -> bool {
        self.blocks_debuffs(tick) || self.skedaddle.is_some_and(|s| tick < s.immunity_until)
    }

    fn clear_skedaddle_impairments(&mut self) {
        self.stun_until = 0;
        self.root_until = 0;
        self.slow_until = 0;
        self.bulwark_slow_until = 0;
        self.water_slow_until = 0;
    }
}

impl Room<'_> {
    pub(crate) fn release_skedaddle(&mut self, seat: usize, rank: usize, events: &mut Vec<Event>) {
        let rules = self
            .state
            .abilities
            .skedaddle
            .as_ref()
            .expect("validated Skedaddle");
        let (immunity_seconds, speed_seconds, speed) = (
            rules.immunity[rank],
            rules.duration[rank],
            rules.speed[rank],
        );
        let tick = self.state.tick;
        let actor = &mut self.entities[seat];
        actor.status.clear_skedaddle_impairments();
        actor.status.skedaddle = Some(Active {
            immunity_until: tick + ticks(immunity_seconds),
            speed_until: tick + ticks(speed_seconds),
            speed,
        });
        let talent_reset = actor.speed_loader.is_some();
        if talent_reset {
            actor.cooldown[AbilitySlot::A.idx()] = tick;
        }

        let immunity_instance = self.state.next_id;
        let speed_instance = immunity_instance + 1;
        self.state.next_id = speed_instance + 1;
        events.push(Event::Skedaddle {
            source: actor.id,
            immunity_instance,
            speed_instance,
            immunity_seconds,
            speed_seconds,
            talent_reset,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::BasicItem;
    use crate::CastTarget;
    use crate::Command;
    use crate::World;
    #[test]
    fn overlapping_block_sources_keep_independent_deadlines_and_cleansing() {
        for gwen_tick in [1, 25] {
            let mut w = World::new(7, 6);
            w.use_six_human_roster().unwrap();
            w.use_aral_lane().unwrap();
            std::sync::Arc::make_mut(&mut w.state.abilities).skedaddle = Some(Rules {
                speed: [2.5; 5],
                duration: [2.2; 5],
                immunity: [0.5; 5],
                cost: [60.; 5],
                cooldown: [19.; 5],
            });
            w.entities[0].type_id = halcyon_data::EntityTemplate::Gwen.as_raw();
            let instance = w
                .purchase(1, BasicItem::ReflexBlock)
                .unwrap()
                .granted
                .instance;
            w.entities[0].status.root_until = 80;
            w.command(1, Command::ActivateReflexBlock { instance })
                .unwrap();
            assert_eq!(w.entities[0].status.root_until, 80);
            w.tick = gwen_tick;
            w.release_skedaddle(0, 0, &mut Vec::new());
            assert_eq!(w.entities[0].status.root_until, 0);
            for tick in gwen_tick..=40 {
                crate::heroes::native::reflex::advance(&mut w.entities[0], tick, &mut Vec::new());
                let status = &w.entities[0].status;
                assert_eq!(status.blocks_debuffs(tick), tick < 30);
                assert_eq!(
                    status.blocks_movement_impairment(tick),
                    tick < 30 || tick < gwen_tick + 10
                );
            }
        }
    }

    #[test]
    fn authorized_cast_cleanses_and_only_speed_loader_resets_buckshot() {
        for talent in [None, Some(std::num::NonZeroU8::MIN)] {
            let mut world = World::new(12, 6);
            world.use_six_human_roster().unwrap();
            world.use_aral_lane().unwrap();
            std::sync::Arc::make_mut(&mut world.state.abilities).skedaddle = Some(Rules {
                speed: [2.5; 5],
                duration: [2.2; 5],
                immunity: [0.5; 5],
                cost: [60.; 5],
                cooldown: [19.; 5],
            });
            let e = &mut world.entities[0];
            e.type_id = halcyon_data::EntityTemplate::Gwen.as_raw();
            e.speed_loader = talent;
            e.energy = 300.;
            e.status.root_until = 70;
            e.status.slow_until = 70;
            e.cooldown[AbilitySlot::A.idx()] = 100;
            let cast = Command::Cast {
                slot: AbilitySlot::B,
                target: CastTarget::SelfCast,
            };
            assert!(world.command(1, cast).is_err());
            world.command(1, Command::Learn(AbilitySlot::B)).unwrap();
            world.command(1, cast).unwrap();
            world.tick = 1;
            let mut events = Vec::new();
            world.advance_cast(0, &mut events);
            let e = &world.entities[0];
            assert_eq!(e.energy, 240.);
            assert_eq!(
                e.cooldown[AbilitySlot::A.idx()],
                if talent.is_some() { 1 } else { 100 }
            );
            assert_eq!((e.status.root_until, e.status.slow_until), (0, 0));
            assert!(e.status.blocks_movement_impairment(10));
            assert!(!e.status.blocks_movement_impairment(11));
            assert_eq!(speed(&e.status, 44), 2.5);
            assert_eq!(speed(&e.status, 45), 0.);
            assert_eq!(events.iter().filter(|e| matches!(e, Event::Skedaddle { source: 1, talent_reset, .. } if *talent_reset == talent.is_some())).count(), 1);
            assert!(world.command(1, cast).is_err());
        }
    }
    #[test]
    #[ignore = "requires original delivery and supplemental ability and talent data"]
    fn supplied_assets_identify_the_named_talent_and_rank_table() {
        let data = GameData::load_with_extra(
            std::env::var_os("HALCYON_DELIVERY").unwrap(),
            Some(std::path::Path::new(
                &std::env::var_os("HALCYON_DATA_EXTRA").unwrap(),
            )),
        )
        .unwrap();
        let rules = Rules::load(&data).unwrap().unwrap();
        assert_eq!(rules.cost, [60., 65., 70., 75., 80.]);
        assert_eq!(rules.cooldown, [19., 17., 15., 13., 10.]);
        assert_eq!(rules.speed, [2.5, 2.8, 3.1, 3.4, 4.]);
        assert_eq!(rules.immunity, [0.5; 5]);
        assert_eq!(rules.duration, [2.2; 5]);
        let mut world = World::from_data(42, 6, &data).unwrap();
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        world.select_hero(0, "Gwen", &data).unwrap();
        let named = halcyon_data::ModeledTalent::SpeedLoader.as_str();
        world
            .set_named_talent(1, Some((named, std::num::NonZeroU8::MIN)), &data)
            .unwrap();
        assert!(world.entities[0].speed_loader.is_some());
        world.set_named_talent(1, None, &data).unwrap();
        assert!(world.entities[0].speed_loader.is_none());
    }
}
