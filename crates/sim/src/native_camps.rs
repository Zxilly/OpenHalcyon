use super::*;
use halcyon_data::NativeMaps;

pub const NEUTRAL_TEAM: u8 = 2;
const FIRST_APPEARANCE: u64 = (393 * TICK_HZ) / 10;

#[derive(Serialize)]
pub(super) struct NativeCampUnit {
    stats: Stats,
    position: Position,
}

fn lineup(id: &str) -> &'static [(EntityTemplate, &'static str)] {
    use EntityTemplate::*;
    match id {
        "NeutralCampA" | "NeutralCampC" => &[
            (HealingTreant, "5v5_HealingTreant"),
            (SmallBear, "5v5_SmallBear"),
        ],
        "NeutralCampB" => &[(BigBear, "5v5_BigBear"), (BigBear, "5v5_BigBear")],
        "NeutralCampD" => &[(GoldTreant, "5v5_GoldTreant")],
        "BuffCampA" => &[(WeaponTreant, "5v5_WeaponTreant")],
        "BuffCampB" => &[(CrystalTreant, "5v5_CrystalTreant")],
        _ => &[],
    }
}

pub(super) fn load(maps: &NativeMaps, data: &GameData) -> Result<Vec<NativeCampUnit>, String> {
    let level = maps.solo().ok_or("Solo map missing")?;
    let mut units = Vec::new();
    for camp in maps.placed_camps(level) {
        for (index, &(template, name)) in lineup(&camp.definition.id).iter().enumerate() {
            let label = if index == 0 { "Spawn1" } else { "Spawn2" };
            let marker = camp
                .definition
                .local_spawns
                .iter()
                .find(|s| s.name == label)
                .ok_or_else(|| format!("missing {label} for {}", camp.placement.name))?;
            let [x, _, y] = camp
                .spawn_position(marker)
                .ok_or("unsupported camp placement transform")?;
            let base = data
                .unit(name)
                .ok_or_else(|| format!("missing unit base {name}"))?;
            let stats = Stats::load(template.as_raw(), base, data);
            if stats.physiology.is_none() {
                return Err(format!("missing unit definition {name}"));
            }
            units.push(NativeCampUnit {
                stats,
                position: Position { x, y },
            });
        }
    }
    if units.len() != 18 {
        return Err("Solo first-generation camp roster is incomplete".into());
    }
    Ok(units)
}

impl Room<'_> {
    pub fn install_native_camps(&mut self, data: &GameData) -> Result<(), String> {
        if self.tick != 0 || !matches!(self.layout, Layout::SoloMid { .. }) {
            return Err("camp placement requires a fresh Solo world".into());
        }
        if let Some(maps) = data.native_maps() {
            self.native_camps = Some(load(maps, data)?);
        }
        Ok(())
    }
    pub(super) fn spawn_native_camps(&mut self, events: &mut Vec<Event>) {
        if self.tick != FIRST_APPEARANCE || !matches!(self.layout, Layout::SoloMid { .. }) {
            return;
        }
        let Some(camps) = self.native_camps.take() else {
            return;
        };
        for camp in camps {
            let id = self.spawn_with_stats(
                NEUTRAL_TEAM,
                Kind::Neutral,
                camp.stats,
                camp.position,
                false,
            );
            events.push(Event::Spawn { id });
        }
    }

    /// A close neutral interaction uses the walk mesh, independently of brushes.
    pub(super) fn combat_clear(&self, source: &Entity, target: &Entity) -> bool {
        if (source.kind == Kind::Neutral || target.kind == Kind::Neutral)
            && let Some(map) = &self.native_navigation
        {
            return map.ground_segment(source.position, target.position);
        }
        self.layout.clear(source.position, target.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the local base delivery and supplemental map assets"]
    fn solo_camps_match_transformed_placements_and_award_once() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let mut world = World::from_data(0, 2, &data).unwrap();
        world.use_solo_mid_lane().unwrap();
        world
            .install_native_navigation(data.native_maps().unwrap())
            .unwrap();
        world.install_native_camps(&data).unwrap();
        let mut events = Vec::new();
        world.tick = FIRST_APPEARANCE - 1;
        world.spawn_native_camps(&mut events);
        assert!(events.is_empty());
        world.tick = FIRST_APPEARANCE;
        world.spawn_native_camps(&mut events);
        world.spawn_native_camps(&mut events);
        assert_eq!(events.len(), 18);
        let units = world
            .entities
            .iter()
            .filter(|e| e.kind == Kind::Neutral)
            .collect::<Vec<_>>();
        assert_eq!(units.len(), 18);
        assert_eq!(
            units
                .iter()
                .filter(|e| e.type_id == EntityTemplate::BigBear.as_raw())
                .count(),
            4
        );
        assert!(
            units
                .iter()
                .all(|e| e.team == NEUTRAL_TEAM && e.hp == e.max_hp)
        );
        let gold = units
            .iter()
            .find(|e| e.type_id == EntityTemplate::GoldTreant.as_raw() && e.position.x < 0.)
            .unwrap();
        let (id, position) = (gold.id, gold.position);
        assert!((position.x + 34.21).abs() < 0.001 && (position.y - 14.71).abs() < 0.001);
        let target = world.index(id).unwrap();
        world.entities[0].position = position;
        assert!(world.visible(0, &world.entities[target]));
        world.command(1, Command::Attack(id)).unwrap();
        let player_gold = world.entities[0].gold;
        events.clear();
        world.hurt(1, 0, target, 1., DamageSchool::True, &mut events);
        assert_eq!(world.entities[target].order, Command::Attack(1));
        assert!(
            (0..40).flat_map(|_| world.step()).any(|event| {
                matches!(event, Event::Damage { source, target: 1, .. } if source == id)
            }),
            "a nearby attacker receives the neutral's normal retaliation"
        );
        let xp = world.entities[0].xp;
        world.hurt(1, 0, target, 2000., DamageSchool::True, &mut events);
        world.hurt(1, 0, target, 2000., DamageSchool::True, &mut events);
        assert_eq!(world.entities[0].gold - player_gold, 220);
        assert_eq!(world.entities[0].xp, xp + 40.);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e,Event::Death{id: dead,..} if *dead==id))
                .count(),
            1
        );
        assert_eq!(world.scores[0].lane_last_hits, 0);
    }
}
