use super::*;
use halcyon_data::NativeMaps;

pub const NEUTRAL_TEAM: u8 = 2;
const FIRST_APPEARANCE: u64 = (393 * TICK_HZ) / 10;

#[derive(Serialize)]
pub(super) struct NativeCampUnit {
    stats: Stats,
    position: Position,
}

fn lineup(id: &str) -> &'static [(EntityTemplate, &str)] {
    use EntityTemplate::*;
    match id {
        "NeutralCampA" | "NeutralCampC" => &[(HealingTreant,"5v5_HealingTreant"),(SmallBear,"5v5_SmallBear")],
        "NeutralCampB" => &[(BigBear,"5v5_BigBear"),(BigBear,"5v5_BigBear")],
        "NeutralCampD" => &[(GoldTreant,"5v5_GoldTreant")],
        "BuffCampA" => &[(WeaponTreant,"5v5_WeaponTreant")],
        "BuffCampB" => &[(CrystalTreant,"5v5_CrystalTreant")],
        _ => &[],
    }
}

pub(super) fn load(maps: &NativeMaps, data:&GameData) -> Result<Vec<NativeCampUnit>,String> {
    let level = maps.solo().ok_or("Solo map missing")?;
    let mut units = Vec::new();
    for camp in maps.placed_camps(level) {
        for (index, &(template, name)) in lineup(&camp.definition.id).iter().enumerate() {
            let label = if index == 0 {"Spawn1"} else {"Spawn2"};
            let marker=camp.definition.local_spawns.iter().find(|s|s.name==label)
                .ok_or_else(||format!("missing {label} for {}",camp.placement.name))?;
            let [x,_,y]=camp.spawn_position(marker).ok_or("unsupported camp placement transform")?;
            let full=data.full_unit(name).ok_or_else(||format!("missing unit definition {name}"))?;
            let base=data.unit(name).ok_or_else(||format!("missing unit base {name}"))?;
            let mut stats=Stats::load(template,base);
            stats.with_full(full);
            units.push(NativeCampUnit { stats,position: Position{x,y} });
        }
    }
    if units.len()!=18 {return Err("Solo first-generation camp roster is incomplete".into());}
    Ok(units)
}

impl World {
    pub(super) fn spawn_native_camps(&mut self, events: &mut Vec<Event>) {
        if self.tick != FIRST_APPEARANCE || !matches!(self.layout,Layout::SoloMid{..}) {return;}
        let Some(camps)=self.native_camps.take() else {return;};
        for camp in camps {
            let id=self.spawn_with_stats(NEUTRAL_TEAM,Kind::Neutral,camp.stats,camp.position,false);
            events.push(Event::Spawn{id});
        }
    }

    /// A close neutral interaction uses the walk mesh, independently of brushes.
    pub(super) fn combat_clear(&self, source:&Entity, target:&Entity) -> bool {
        if (source.kind==Kind::Neutral || target.kind==Kind::Neutral)
            && let Some(map)= &self.native_navigation {
            return map.ground_segment(source.position,target.position);
        }
        self.layout.clear(source.position,target.position)
    }
}
