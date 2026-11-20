use super::*;

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Active {
    pub until: u64,
    pub barrier: f32,
    effect: u32,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
pub(super) struct Rule {
    low: f32,
    high: f32,
    duration: u64,
}

impl Rule {
    pub const NATIVE: Self = Self {
        low: 100.,
        high: 600.,
        duration: 3 * TICK_HZ / 2,
    };
    pub fn is_native(value: &Option<Self>) -> bool {
        *value == Some(Self::NATIVE)
    }
    pub fn load(item: &halcyon_data::items::FullItem) -> Option<Self> {
        use halcyon_data::items::ItemTuning;
        let low = item.passive_value(ItemTuning::BarrierMin.key())?;
        let high = item.passive_value(ItemTuning::BarrierMax.key())?;
        let seconds = item.passive_value(ItemTuning::ProcTime.key())?;
        // The native effect prefix is verified only for this duration.
        (low.is_finite() && low > 0. && high.is_finite() && high >= low && seconds == 1.5)
            .then_some(Self {
                low,
                high,
                duration: Self::NATIVE.duration,
            })
    }
}

impl Status {
    pub(super) fn blocks_debuffs(&self, tick: u64) -> bool {
        self.reflex.is_some_and(|block| tick < block.until)
    }
}

impl Room<'_> {
    pub(super) fn activate_reflex(&mut self, i: usize, instance: u32) -> Result<(), &'static str> {
        let h = &self.entities[i];
        if h.hp <= 0.
            || h.status.silence_until > self.tick
            || h.kind != Kind::Hero
            || h.item_stats.reflex_ready.is_some_and(|end| self.tick < end)
            || !h
                .inventory
                .iter()
                .any(|item| item.kind == BasicItem::ReflexBlock && item.instance == instance)
        {
            return Err("Reflex Block unavailable");
        }
        let rule = self.shop.reflex.ok_or("Reflex Block tuning unavailable")?;
        let barrier =
            rule.low + f32::from(h.level.clamp(1, 12) - 1) * ((rule.high - rule.low) / 11.);
        let first_effect = self.next_id;
        self.next_id = first_effect.checked_add(3).ok_or("effect instance limit")?;
        let tick = self.tick;
        let h = &mut self.entities[i];
        h.item_stats.reflex_ready = Some(tick + 90 * TICK_HZ);
        h.status.reflex = Some(Active {
            until: tick + rule.duration,
            barrier,
            effect: first_effect + 1,
        });
        self.state.pending_item_events.push(Event::ReflexActivated {
            actor: h.id,
            item: instance,
            first_effect,
            barrier,
        });
        Ok(())
    }
}

pub(super) fn advance(hero: &mut Entity, tick: u64, events: &mut Vec<Event>) {
    if let Some(block) = hero.status.reflex
        && (hero.hp <= 0. || tick >= block.until)
    {
        hero.status.reflex = None;
        if hero.hp > 0. {
            events.push(Event::ReflexExpired {
                actor: hero.id,
                effect: block.effect,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_prevents_new_crowd_control_without_cleansing_or_cancelling_damage() {
        fn achilles(block: bool) -> (f32, u64) {
            let mut w = World::new(7, 6);
            w.use_six_human_roster().unwrap();
            w.use_aral_lane().unwrap();
            let item = w.purchase(1, BasicItem::ReflexBlock).unwrap().granted;
            if block {
                w.command(
                    1,
                    Command::ActivateReflexBlock {
                        instance: item.instance,
                    },
                )
                .unwrap();
            }
            w.entities[0].status.slow_until = 2;
            let hp = w.entities[0].hp;
            let point = w.entities[0].position;
            w.entities[1].position = point;
            w.entities[1].type_id = EntityTemplate::Ringo.as_raw();
            w.entities[1].ability_ranks = [1, 0, 0];
            w.command(
                2,
                Command::Cast {
                    slot: AbilitySlot::A,
                    target: CastTarget::Entity(1),
                },
            )
            .unwrap();
            let mut landed = false;
            for _ in 0..20 {
                let events = w.step();
                if events
                    .iter()
                    .any(|e| matches!(e, Event::Cast { source: 2, .. }))
                {
                    landed = true;
                    break;
                }
            }
            assert!(landed);
            (hp - w.entities[0].hp, w.entities[0].status.slow_until)
        }
        let (damage, until) = achilles(false);
        assert!(damage > 0. && until > 2);
        assert_eq!(achilles(true), (0., 2));
    }

    #[test]
    fn purchased_instance_blocks_damage_and_new_debuffs_until_expiry() {
        let mut world = World::new(23, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let bought = world.purchase(1, BasicItem::ReflexBlock).unwrap().granted;
        let activate = Command::ActivateReflexBlock {
            instance: bought.instance,
        };
        assert!(world.command(2, activate.clone()).is_err());
        let rank = world.entities[0].level;
        world.command(1, activate.clone()).unwrap();
        assert!(world.command(1, activate.clone()).is_err());
        let hp = world.entities[0].hp;
        let mut events = Vec::new();
        world.hurt(2, 1, 0, 20., DamageSchool::True, &mut events);
        assert_eq!(world.entities[0].hp, hp);
        assert!(world.entities[0].status.blocks_debuffs(29));
        assert!(!world.entities[0].status.blocks_debuffs(30));
        let (mut starts, mut ends) = (0, Vec::new());
        for _ in 0..31 {
            for e in world.step() {
                match e {
                    Event::ReflexActivated {
                        first_effect,
                        barrier,
                        ..
                    } => {
                        assert_eq!(barrier, 100. + f32::from(rank - 1) * (500. / 11.));
                        starts = first_effect + 1;
                    }
                    Event::ReflexExpired { effect, .. } => ends.push(effect),
                    _ => {}
                }
            }
        }
        assert_ne!(starts, 0);
        assert_eq!(ends, [starts]);
        assert!(world.command(1, activate.clone()).is_err());
        world.tick = 90 * TICK_HZ;
        world.command(1, activate).unwrap();
    }
}
