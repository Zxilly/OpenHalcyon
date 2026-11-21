use super::shop::{BasicItem::*, Shop, seconds_to_ticks};
use crate::BasicItem;
use crate::Entity;
use crate::Event;
use crate::Room;
use crate::TICK_HZ;
use halcyon_data::items::{FullItem, ItemTuning};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Rule {
    duration: u64,
    ranges: [[f32; 2]; 4],
}

impl Rule {
    pub(crate) const DEFAULTS: [Option<Self>; 2] = [
        Some(Self {
            duration: 150 * TICK_HZ,
            ranges: [[20., 40.], [0.15, 0.35], [10., 20.], [10., 20.]],
        }),
        Some(Self {
            duration: 150 * TICK_HZ,
            ranges: [[30., 60.], [0.075, 0.15], [10., 20.], [10., 20.]],
        }),
    ];

    pub(crate) fn load(row: &FullItem, kind: BasicItem) -> Option<Self> {
        use ItemTuning::*;
        let pairs = match kind {
            WeaponInfusion => [(WeaponMin, WeaponMax), (AttackSpeedMin, AttackSpeedMax)],
            CrystalInfusion => [(CrystalMin, CrystalMax), (CooldownMin, CooldownMax)],
            _ => return None,
        };
        let mut ranges = [[0.; 2]; 4];
        for (range, (lo, hi)) in ranges.iter_mut().zip([
            pairs[0],
            pairs[1],
            (ArmorMin, ArmorMax),
            (ShieldMin, ShieldMax),
        ]) {
            *range = [row.passive_value(lo.key())?, row.passive_value(hi.key())?];
            if !range.iter().all(|v| v.is_finite()) || range[0] < 0. || range[1] < range[0] {
                return None;
            }
        }
        Some(Self {
            duration: seconds_to_ticks(row.passive_value(Duration.key())?)?,
            ranges,
        })
    }
    fn change(self, hero: &mut Entity, kind: BasicItem, level: u8, sign: f32) {
        let fraction = f32::from(level.clamp(1, 12) - 1) / 11.;
        let [power, speed, armor, shield] = self
            .ranges
            .map(|[lo, hi]| (lo + fraction * (hi - lo)) * sign);
        if kind == WeaponInfusion {
            hero.stats.weapon += power;
            hero.item_stats.bonus_weapon += power;
            hero.item_stats.attack_speed += speed;
        } else {
            hero.item_stats.crystal += power;
            hero.item_stats.cooldown += speed;
        }
        hero.stats.armor += armor;
        hero.stats.shield += shield;
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Active {
    kind: BasicItem,
    level: u8,
    until: u64,
    effect: u32,
}
impl Shop {
    fn infusion(&self, kind: BasicItem) -> Option<Rule> {
        self.infusions[match kind {
            WeaponInfusion => 0,
            CrystalInfusion => 1,
            _ => return None,
        }]
    }
}
pub(crate) fn update(hero: &mut Entity, tick: u64, shop: &Shop, events: &mut Vec<Event>) {
    let Some(active) = hero.item_stats.infusion else {
        return;
    };
    if active.until > tick && active.level == hero.level {
        return;
    }
    let rule = shop.infusion(active.kind).expect("active rule");
    rule.change(hero, active.kind, active.level, -1.);
    hero.item_stats.infusion = if active.until > tick {
        rule.change(hero, active.kind, hero.level, 1.);
        events.push(Event::InfusionLevel {
            actor: hero.id,
            effect: active.effect,
            level: hero.level,
        });
        Some(Active {
            level: hero.level,
            ..active
        })
    } else {
        None
    };
}
impl Room<'_> {
    pub(crate) fn consume_infusion(
        &mut self,
        index: usize,
        instance: u32,
    ) -> Result<(), &'static str> {
        let hero = &mut self.entities[index];
        let pos = hero
            .inventory
            .iter()
            .position(|h| h.instance == instance)
            .ok_or("item not held")?;
        let held = hero.inventory[pos];
        let rule = self
            .state
            .shop
            .infusion(held.kind)
            .ok_or("infusion unavailable")?;
        let effect = self.state.next_id;
        let next = effect.checked_add(1).ok_or("effect instance limit")?;
        if let Some(slots) = &mut hero.aral_inventory {
            slots.remove_held(held)?;
        }
        self.state.next_id = next;
        hero.inventory.remove(pos);
        if let Some(old) = hero.item_stats.infusion {
            self.state
                .shop
                .infusion(old.kind)
                .expect("active rule")
                .change(hero, old.kind, old.level, -1.);
        }
        rule.change(hero, held.kind, hero.level, 1.);
        self.state.pending_item_events.push(Event::InfusionApplied {
            actor: hero.id,
            item: instance,
            effect,
            kind: held.kind,
            level: hero.level,
            duration_ticks: rule.duration,
        });
        if let Some(old) = hero
            .item_stats
            .infusion
            .filter(|old| old.until > self.state.tick)
        {
            self.state
                .pending_item_events
                .push(Event::InfusionReplaced {
                    actor: hero.id,
                    effect: old.effect,
                });
        }
        hero.item_stats.infusion = Some(Active {
            effect,
            kind: held.kind,
            level: hero.level,
            until: self.state.tick.saturating_add(rule.duration),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use crate::World;
    #[test]
    fn purchase_consume_refresh_replace_level_and_expire() {
        let mut world = World::new(4, 2);
        world.entities[0].gold = 5000;
        let base = world.entities[0].stats.weapon;
        let armor = world.entities[0].stats.armor;
        let buy = |world: &mut World, item| world.purchase(1, item).unwrap().granted.instance;
        let consume = |instance| Command::ConsumeInfusion { instance };
        let red = buy(&mut world, WeaponInfusion);
        assert_eq!(world.entities[0].stats.weapon, base);
        assert!(world.command(2, consume(red)).is_err());
        world.command(1, consume(red)).unwrap();
        assert!(world.command(1, consume(red)).is_err());
        assert_eq!(world.entities[0].stats.weapon, base + 20.);
        world.tick = 100;
        let red = buy(&mut world, WeaponInfusion);
        world.command(1, consume(red)).unwrap();
        assert_eq!(world.entities[0].stats.weapon, base + 20.);
        assert_eq!(world.entities[0].item_stats.infusion.unwrap().until, 3100);
        let blue = buy(&mut world, CrystalInfusion);
        world.command(1, consume(blue)).unwrap();
        assert_eq!(world.entities[0].stats.weapon, base);
        assert_eq!(world.entities[0].item_stats.crystal, 30.);
        assert_eq!(world.entities[0].stats.armor, armor + 10.);
        let effect = world.entities[0].item_stats.infusion.unwrap().effect;
        world.entities[0].level = 12;
        let events = world.step();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::InfusionApplied { .. }))
                .count(),
            3
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::InfusionReplaced { .. }))
                .count(),
            2
        );
        assert!(events.contains(&Event::InfusionLevel {
            actor: 1,
            effect,
            level: 12
        }));
        assert!(
            !world
                .step()
                .iter()
                .any(|e| matches!(e, Event::InfusionLevel { .. }))
        );
        assert_eq!(world.entities[0].item_stats.crystal, 60.);
        assert_eq!(world.entities[0].stats.armor, armor + 20.);
        world.tick = 3099;
        world.step();
        assert_eq!(world.entities[0].item_stats.crystal, 0.);
        assert_eq!(world.entities[0].stats.armor, armor);
        assert!(world.entities[0].item_stats.infusion.is_none());
    }
}
