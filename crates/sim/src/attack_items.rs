use super::*;
use halcyon_data::items::ItemCatalog;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Rules {
    interval: u64,
    bow_base: f32,
    bow_ratio: f32,
    bow_pierce: f32,
    current: f32,
    book: f32,
    needle: f32,
    mask: f32,
    melee_heal: f32,
    ranged_heal: f32,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            interval: 6 * TICK_HZ,
            bow_base: 100.,
            bow_ratio: 1.,
            bow_pierce: 0.3,
            current: 0.5,
            book: 0.05,
            needle: 0.1,
            mask: 0.15,
            melee_heal: 30.,
            ranged_heal: 10.,
        }
    }
}
impl Rules {
    pub(super) fn load(catalog: Option<&ItemCatalog>) -> Self {
        let Some(catalog) = catalog else {
            return Self::default();
        };
        let value = |item: BasicItem, key: ItemTuning| {
            catalog
                .item(item.definition().name)
                .and_then(|r| r.passive_value(key.key()))
                .filter(|v| v.is_finite() && *v >= 0.)
                .unwrap_or_default()
        };
        use ItemTuning::*;
        Self {
            interval: seconds_to_ticks(value(TensionBow, ProcTime)).unwrap_or(0),
            bow_base: value(TensionBow, Damage),
            bow_ratio: value(TensionBow, BonusWeaponRatio),
            bow_pierce: value(TensionBow, ArmorPen).min(1.),
            current: value(AlternatingCurrent, Ratio),
            book: value(BookOfEulogies, Vamp),
            needle: value(BarbedNeedle, Vamp),
            mask: value(SerpentMask, OmniVamp),
            melee_heal: value(BookOfEulogies, HealMelee),
            ranged_heal: value(BookOfEulogies, HealRanged),
        }
    }
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub(super) struct Equipped {
    bow: bool,
    bow_ready: u64,
    current: bool,
    alternate: bool,
    vamp: f32,
    book: bool,
}

impl ItemStats {
    pub(crate) fn has_attack_passives(&self) -> bool {
        let p = &self.attack_passives;
        p.bow || p.current || p.book
    }
}
pub(super) fn refresh(hero: &mut Entity, shop: &Shop) {
    let rules = &shop.attacks;
    let mut pierce = 0.0_f32;
    let prev = hero.item_stats.attack_passives;
    let mut equipped = Equipped {
        bow_ready: prev.bow_ready,
        ..Default::default()
    };
    for item in &hero.inventory {
        let candidate = if item.kind == TensionBow {
            rules.bow_pierce
        } else {
            shop.offers[item.kind as usize]
                .bonuses
                .iter()
                .filter(|(s, _)| *s == NativeItemStat::ArmorPierce)
                .map(|(_, v)| *v)
                .sum()
        };
        pierce = pierce.max(candidate);
        match item.kind {
            TensionBow => equipped.bow = true,
            AlternatingCurrent => {
                equipped.current = true;
                equipped.alternate = prev.alternate;
            }
            BookOfEulogies | BarbedNeedle | SerpentMask => {
                equipped.book = true;
                equipped.vamp = equipped.vamp.max(match item.kind {
                    BookOfEulogies => rules.book,
                    BarbedNeedle => rules.needle,
                    _ => rules.mask,
                });
            }
            _ => {}
        }
    }
    hero.item_stats.armor_pierce = pierce;
    hero.item_stats.attack_passives = equipped;
}

impl Room<'_> {
    fn retarget_idris_after_last_hit(&mut self, i: usize, victim: usize, events: &mut Vec<Event>) {
        let hero = &self.entities[i];
        if !matches!(self.layout, Layout::Aral { .. })
            || hero.bot
            || hero.type_id != EntityTemplate::Idris.as_raw()
            || hero.attack_target() != Some(self.entities[victim].id)
            || !EntityTemplate::from_raw(self.entities[victim].type_id)
                .is_some_and(EntityTemplate::is_aral_lane_minion)
        {
            return;
        }
        // The original priority between competing targets is unknown.
        let mut candidates = self.entities.iter().filter(|other| {
            other.hp > 0.
                && other.team != hero.team
                && hero.position.dist(other.position) <= hero.stats.range
                && self.visible(hero.team, other)
                && self.combat_clear(hero, other)
        });
        let Some(next) = candidates.next() else {
            return;
        };
        if candidates.next().is_some()
            || !(next.kind == Kind::Hero
                || EntityTemplate::from_raw(next.type_id)
                    .is_some_and(EntityTemplate::is_aral_lane_minion))
        {
            return;
        }
        let (actor, target) = (hero.id, next.id);
        self.entities[i].order = Command::Attack(target);
        self.entities[i].winding = None;
        events.push(Event::BasicTargetChanged { actor, target });
    }

    pub(crate) fn basic_hit(
        &mut self,
        source: u32,
        team: u8,
        target: usize,
        damage: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) {
        let victim = &self.entities[target];
        if victim.hp <= 0. {
            return;
        }
        let hp = victim.hp;
        let kind = victim.kind;
        let owner = self.index(source).filter(|&i| {
            self.entities[i].kind == Kind::Hero
                && self.entities[i].hp > 0.
                && matches!(kind, Kind::Hero | Kind::Neutral | Kind::Minion)
        });
        let Some(i) = owner else {
            self.hurt(source, team, target, damage, school, events);
            return;
        };
        let armor = victim.stats.armor;
        let shield = victim.stats.shield;
        let hero = &mut self.entities[i];
        let r = self.state.shop.attacks;
        let passive = &mut hero.item_stats.attack_passives;
        let bonus_weapon = if passive.bow && r.interval > 0 && self.state.tick >= passive.bow_ready
        {
            passive.bow_ready = self.state.tick + r.interval;
            mitigated(
                r.bow_base + r.bow_ratio * hero.item_stats.bonus_weapon,
                armor,
                hero.item_stats.armor_pierce,
            )
        } else {
            0.
        };
        let crystal = if passive.current {
            passive.alternate = !passive.alternate;
            if passive.alternate {
                0.
            } else {
                mitigated(
                    r.current * hero.item_stats.crystal,
                    shield,
                    hero.item_stats.shield_pierce,
                )
            }
        } else {
            0.
        };
        let vamp = passive.vamp;
        let heal = if passive.book && matches!(kind, Kind::Neutral | Kind::Minion) {
            if hero.stats.range >= 4. {
                r.ranged_heal
            } else {
                r.melee_heal
            }
        } else {
            0.
        };
        self.hurt(source, team, target, damage, school, events);
        let mut weapon_loss = if matches!(school, DamageSchool::Weapon) {
            hp - self.entities[target].hp
        } else {
            0.
        };
        if bonus_weapon > 0. && self.entities[target].hp > 0. {
            let before = self.entities[target].hp;
            self.hurt(
                source,
                team,
                target,
                bonus_weapon,
                DamageSchool::Weapon,
                events,
            );
            weapon_loss += before - self.entities[target].hp;
        }
        if crystal > 0. && self.entities[target].hp > 0. {
            self.hurt(source, team, target, crystal, DamageSchool::Crystal, events);
        }
        if self.entities[target].hp <= 0. && kind == Kind::Minion {
            self.retarget_idris_after_last_hit(i, target, events);
        }
        let restoration = weapon_loss * vamp
            + if self.entities[target].hp <= 0. {
                heal
            } else {
                0.
            };
        if restoration > 0. {
            self.heal(source, i, restoration, events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encounter() -> World {
        let mut w = World::new(3, 2);
        w.use_catalog_store(halcyon_data::items::StoreSettings::Five);
        w.entities[0].gold = 25000;
        w.entities[1].hp = 5000.;
        w.entities[1].max_hp = 5000.;
        w.entities[1].stats.armor = 0.;
        w.entities[1].stats.shield = 0.;
        w
    }
    #[test]
    fn idris_basic_last_hit_transfers_only_an_unambiguous_current_lock() {
        for (extra, current, expected) in [
            (false, true, true),
            (true, true, false),
            (false, false, false),
        ] {
            let mut world = World::new(11, 6);
            world.use_six_human_roster().unwrap();
            world.use_aral_lane().unwrap();
            world.tick = 15 * TICK_HZ;
            world.spawn_aral_wave(&mut Vec::new());
            let victim = world
                .entities
                .iter()
                .position(|e| e.kind == Kind::Minion && e.team == 1)
                .unwrap();
            let id = world.entities[victim].id;
            let point = world.entities[0].position;
            world.entities[0].type_id = EntityTemplate::Idris.as_raw();
            world.entities[victim].position = point;
            world.entities[1].position = point;
            if extra {
                world.entities[3].position = point;
            }
            if current {
                world.command(1, Command::Attack(id)).unwrap();
            }
            let mut events = Vec::new();
            world.basic_hit(1, 0, victim, 100000., DamageSchool::Weapon, &mut events);
            assert!(world.entities[victim].hp <= 0.);
            assert_eq!(
                events.contains(&Event::BasicTargetChanged {
                    actor: 1,
                    target: 2
                }),
                expected
            );
            assert_eq!(world.entities[0].attack_target() == Some(2), expected);
        }
    }

    #[test]
    fn bow_current_and_vamp_follow_landed_damage_and_ownership() {
        let mut w = encounter();
        w.purchase(1, TensionBow).unwrap();
        w.purchase(1, AlternatingCurrent).unwrap();
        w.purchase(1, BarbedNeedle).unwrap();
        w.purchase(1, BookOfEulogies).unwrap();
        w.entities[0].hp = 100.;
        let mut events = vec![];
        w.basic_hit(1, 0, 1, 10., DamageSchool::Weapon, &mut events);
        assert_eq!(w.entities[1].hp, 5000. - 165.);
        assert!((w.entities[0].hp - 116.5).abs() < 0.001);
        assert_eq!(w.entities[0].item_stats.armor_pierce, 0.3);
        w.basic_hit(1, 0, 1, 10., DamageSchool::Weapon, &mut events);
        assert_eq!(w.entities[1].hp, 4835. - 32.5);
        assert!((w.entities[0].hp - 117.5).abs() < 0.001);
        w.tick = 6 * TICK_HZ;
        w.entities[1].status.barrier_until = w.tick + 20;
        w.entities[1].status.barrier = 500.;
        w.basic_hit(1, 0, 1, 10., DamageSchool::Weapon, &mut events);
        assert_eq!(w.entities[1].hp, 4802.5);
        assert_eq!(w.entities[0].hp, 117.5);
    }

    #[test]
    fn sale_removes_bow_and_overkill_restores_only_actual_loss_plus_book_heal() {
        let mut w = encounter();
        let bow = w.purchase(1, TensionBow).unwrap().granted;
        w.purchase(1, BarbedNeedle).unwrap();
        w.sell_item(1, bow.instance).unwrap();
        assert_eq!(w.entities[0].item_stats.armor_pierce, 0.);
        let monster =
            w.spawn_with_stats(1, Kind::Minion, w.stats[4], w.entities[0].position, false);
        let j = w.index(monster).unwrap();
        w.entities[j].hp = 5.;
        w.entities[0].hp = 100.;
        w.entities[0].stats.range = 6.;
        let mut events = vec![];
        w.basic_hit(1, 0, j, 1000., DamageSchool::Weapon, &mut events);
        assert!(events.iter().any(
            |e| matches!(e,Event::Heal{source:1,target:1,amount,..} if (*amount-10.5).abs()<0.001)
        ));
        let count = events.len();
        w.basic_hit(1, 0, j, 1000., DamageSchool::Weapon, &mut events);
        assert_eq!(events.len(), count);
    }

    #[test]
    fn ranged_projectile_applies_equipment_only_on_impact() {
        for bot in [false, true] {
            let mut w = encounter();
            w.purchase(1, AlternatingCurrent).unwrap();
            w.entities[0].bot = bot;
            w.entities[0].stats.range = 6.;
            w.entities[1].position = w.entities[0].position;
            w.entities[1].position.x += 3.;
            w.entities[0].order = Command::Attack(2);
            w.entities[0].winding = Some((0, 2));
            w.entities[0].next_attack = 0;
            w.entities[0].item_stats.attack_passives.alternate = true;
            let mut fired = false;
            let mut proc = false;
            for _ in 0..20 {
                let events = w.step();
                if events.iter().any(|e| {
                    matches!(
                        e,
                        Event::Attack {
                            source: 1,
                            target: 2
                        }
                    )
                }) {
                    fired = true;
                    assert!(!proc);
                }
                proc|=events.iter().any(|e|matches!(e,Event::Damage{source:1,target:2,amount,..} if (*amount-22.5).abs()<0.001));
                if proc {
                    break;
                }
            }
            assert!(fired && proc, "bot={bot}");
        }
    }
}
