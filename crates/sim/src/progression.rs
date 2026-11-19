use super::*;
impl World {
    pub(super) fn award_xp(&mut self, i: usize, xp: f32, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.kind != Kind::Hero || e.level >= 12 {
            return;
        }
        e.xp += xp;
        while e.level < 12 {
            let need = 90. + 8. * f32::from(e.level - 1);
            if e.xp < need {
                break;
            }
            e.xp -= need;
            e.level += 1;
            e.skill_points += 1;
            let g = e.stats.growth;
            e.max_hp += g.hp;
            e.hp = (e.hp + g.hp).min(e.max_hp);
            e.max_energy += g.energy;
            e.energy += g.energy;
            e.stats.weapon += g.weapon;
            e.stats.armor += g.armor;
            e.stats.shield += g.shield;
            events.push(Event::Level {
                id: e.id,
                level: e.level,
            });
        }
        if e.level == 12 {
            e.xp = 0.
        }
    }
    pub(super) fn heal(&mut self, src: u32, i: usize, n: f32, events: &mut Vec<Event>) {
        let e = &mut self.entities[i];
        if e.hp <= 0. {
            return;
        }
        let was = e.hp;
        e.hp = (was + n).min(e.max_hp);
        if e.hp > was {
            events.push(Event::Heal {
                source: src,
                target: e.id,
                amount: e.hp - was,
                hp: e.hp,
            })
        }
    }
    pub(super) fn hurt(&mut self, src: u32, team: u8, i: usize, n: f32, events: &mut Vec<Event>) {
        let t = &mut self.entities[i];
        if t.hp <= 0. || n <= 0. {
            return;
        }
        let amount = n.min(t.hp);
        t.hp -= amount;
        let id = t.id;
        events.push(Event::Damage {
            source: src,
            target: id,
            amount,
            hp: t.hp,
        });
        if t.hp > 0. {
            return;
        }
        let pos = t.position;
        let kind = t.kind;
        let (bounty, xp) = match kind {
            Kind::Hero => (250 + if self.first_blood { 0 } else { 125 }, 90.),
            Kind::Tower => (300, 60.),
            Kind::Crystal => (0, 0.),
            Kind::Minion => match t.type_id {
                335 => (25, 6.),
                337 => (60, 12.),
                338 => (100, 12.),
                _ => (35, 6.),
            },
        };
        t.order = Command::Stop;
        t.winding = None;
        t.cast = None;
        t.status = Default::default();
        t.last_seen = None;
        t.respawn = self.tick + RESPAWN + (self.tick / 60).min(900);
        events.push(Event::Death { id, killer: src });
        if kind == Kind::Crystal {
            self.winner = Some(team);
            events.push(Event::End { winner: team });
            return;
        }
        if kind == Kind::Hero {
            self.first_blood = true;
        }
        // Ten stable hero seats. A death awards its killer even if the projectile
        // travelled beyond the project assist radius after release.
        let hero_kill = self
            .index(src)
            .is_some_and(|j| self.entities[j].kind == Kind::Hero);
        let mut allies = [0usize; 10];
        let mut count = 0;
        for (j, a) in self.entities.iter_mut().take(10).enumerate() {
            if a.team != team || a.kind != Kind::Hero {
                continue;
            }
            let nearby = a.hp > 0. && (a.position.dist(pos) <= 10. || a.id == src);
            let amount = if a.id == src {
                bounty
            } else if nearby && kind == Kind::Minion {
                if hero_kill {
                    bounty / 2
                } else {
                    bounty * 3 / 10
                }
            } else {
                0
            };
            if amount > 0 {
                a.gold += amount;
                events.push(Event::GoldAward { id: a.id, amount });
            }
            if !nearby {
                continue;
            }
            allies[count] = j;
            count += 1;
        }
        if count > 0 {
            let shares = if count == 1 {
                xp
            } else {
                xp * 1.2 / count as f32
            };
            for j in &allies[..count] {
                self.award_xp(*j, shares, events)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rank_growth_and_base_shop_are_authoritative() {
        let mut w = World::new(1, 1);
        let mut events = Vec::new();
        w.award_xp(0, 90., &mut events);
        assert_eq!(w.entities[0].level, 2);
        assert_eq!(w.entities[0].stats.weapon, 80.);
        assert!((w.entities[0].max_hp - 830.64).abs() < 0.01);
        w.command(1, Command::Learn(AbilitySlot::A)).unwrap();
        assert!(w.command(1, Command::Learn(AbilitySlot::A)).is_err());
        w.command(1, Command::Learn(AbilitySlot::B)).unwrap();
        w.entities[0].gold = 300;
        w.entities[0].position = Position { x: 10., y: 0. };
        assert!(w.command(1, Command::Buy(BasicItem::WeaponBlade)).is_err());
        w.entities[0].position = base(0);
        w.command(1, Command::Buy(BasicItem::WeaponBlade)).unwrap();
        assert_eq!(w.entities[0].gold, 0);
        assert_eq!(w.entities[0].stats.weapon, 90.);
        assert!(w.command(1, Command::Buy(BasicItem::WeaponBlade)).is_err());
    }
    #[test]
    fn roster_assignment_fills_teams_and_rejects_irreversible_partial_assignment() {
        let mut w = World::new(32, 6);
        assert!(w.assign_teams(&[1; 6]).is_err());
        assert_eq!(w.entities[0].team, 0);
        w.assign_teams(&[1, 1, 1, 1, 1, 0]).unwrap();
        assert_eq!(w.entities[..10].iter().filter(|h| h.team == 1).count(), 5);
        for h in &w.entities[..10] {
            assert_eq!(h.position.x, base(h.team).x)
        }
        w.step();
        assert!(w.assign_teams(&[]).is_err());
    }
}
