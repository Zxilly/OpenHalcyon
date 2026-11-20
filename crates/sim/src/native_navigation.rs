use super::*;
use halcyon_data::{NativeMaps, NavMesh, RouteSearch};
use std::{collections::VecDeque, sync::Arc};

const PROJECT_RADIUS: f32 = 8.;
const REPLAN_DISTANCE: f32 = 1.25;
type Triangle = [[f32; 2]; 3];

struct Brush {
    bounds: [f32; 4],
    triangles: Box<[Triangle]>,
}

impl Brush {
    fn new(points: &[[f32; 2]]) -> Self {
        let bounds = points.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |[low_x, low_y, high_x, high_y], &[x, y]| {
                [low_x.min(x), low_y.min(y), high_x.max(x), high_y.max(y)]
            },
        );
        Self {
            bounds,
            triangles: triangles(points).collect(),
        }
    }

    fn contains(&self, p: Position) -> bool {
        let [low_x, low_y, high_x, high_y] = self.bounds;
        (low_x..=high_x).contains(&p.x)
            && (low_y..=high_y).contains(&p.y)
            && contains(&self.triangles, p)
    }
}

#[derive(Default, Serialize)]
struct Route {
    goal: Option<Position>,
    steps: VecDeque<Position>,
    retry_at: u64,
    expected: Option<Position>,
}

pub(super) struct NativeNavigation {
    walk: Arc<NavMesh>,
    search: RouteSearch,
    routes: [Route; 10],
    brushes: Vec<Brush>,
    fountains: [Vec<Triangle>; 2],
    shops: [Option<Position>; 2],
}

fn point([x, _, z]: [f32; 3]) -> Position {
    Position { x, y: z }
}

fn horizontal(p: Position) -> [f32; 2] {
    [p.x, p.y]
}

fn triangles(points: &[[f32; 2]]) -> impl Iterator<Item = [[f32; 2]; 3]> + '_ {
    points.as_chunks::<3>().0.iter().copied()
}

fn contains(area: &[[[f32; 2]; 3]], p: Position) -> bool {
    area.iter().any(|t| {
        let cross =
            |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p.y - a[1]) - (b[1] - a[1]) * (p.x - a[0]);
        let [a, b, c] = *t;
        if ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() <= f32::EPSILON {
            return false;
        }
        let sides = [cross(a, b), cross(b, c), cross(c, a)];
        sides.iter().all(|s| *s >= 0.) || sides.iter().all(|s| *s <= 0.)
    })
}

impl NativeNavigation {
    fn build(walk: Arc<NavMesh>, level: &halcyon_data::LevelLayout, aral: bool) -> Self {
        Self {
            walk,
            search: RouteSearch::default(),
            routes: std::array::from_fn(|_| Route::default()),
            brushes: level
                .brushes
                .iter()
                .map(|b| Brush::new(&b.polygon_xz))
                .collect(),
            fountains: std::array::from_fn(|team| {
                level
                    .spawn_stages
                    .iter()
                    .filter(|area| area.team == Some(team as u8 + 1))
                    .flat_map(|area| triangles(&area.polygon_xz))
                    .collect()
            }),
            shops: std::array::from_fn(|team| {
                let (names, placements) = if aral {
                    (["ShopHome", "ShopAway"], &level.turrets_and_structures)
                } else {
                    (["LBaseShop", "RBaseShop"], &level.static_entities)
                };
                placements
                    .iter()
                    .find(|p| p.name == names[team])
                    .map(|p| point(p.pos))
            }),
        }
    }

    pub(super) fn digest(&self, hash: &mut impl std::io::Write) {
        serde_json::to_writer(hash, &self.routes).expect("finite native navigation");
    }

    pub(super) fn can_see(&self, viewer: Position, target: Position) -> bool {
        self.brushes
            .iter()
            .find(|area| area.contains(target))
            .is_none_or(|area| area.contains(viewer))
    }

    pub(super) fn at_fountain(&self, actor: &Entity) -> bool {
        contains(&self.fountains[usize::from(actor.team)], actor.position)
    }

    pub(super) fn at_shop(&self, actor: &Entity) -> bool {
        self.at_fountain(actor)
            || self.shops[usize::from(actor.team)].is_some_and(|p| actor.position.dist(p) <= 3.)
    }

    pub(super) fn can_project(&self, destination: Position) -> bool {
        self.project(destination).is_some()
    }

    pub(super) fn project(&self, destination: Position) -> Option<Position> {
        self.walk
            .nearest_ground(horizontal(destination), PROJECT_RADIUS)
            .map(point)
    }

    pub(super) fn ground_segment(&self, start: Position, end: Position) -> bool {
        self.walk.ground_segment(horizontal(start), horizontal(end))
    }

    pub(super) fn forget_route(&mut self, actor: u32) {
        if let Some(route) = self.routes.get_mut(actor.saturating_sub(1) as usize) {
            route.goal = None;
            route.steps.clear();
            route.retry_at = 0;
            route.expected = None;
        }
    }

    pub(super) fn moved(&mut self, actor: u32, position: Position) {
        if let Some(route) = self.routes.get_mut(actor.saturating_sub(1) as usize) {
            route.expected = Some(position);
        }
    }

    pub(super) fn next(
        &mut self,
        actor: &Entity,
        tick: u64,
        goal: Position,
        budget: &mut u8,
    ) -> Option<Position> {
        let cache = self.routes.get_mut(actor.id.checked_sub(1)? as usize)?;
        if cache.expected.is_some_and(|pos| pos != actor.position) {
            cache.steps.clear();
            cache.goal = None;
        }
        cache.expected = Some(actor.position);
        let changed = cache
            .goal
            .is_none_or(|old| old.dist(goal) > REPLAN_DISTANCE);
        if changed || (cache.steps.is_empty() && cache.goal != Some(goal) && tick >= cache.retry_at)
        {
            if *budget == 0 {
                return cache.steps.front().copied();
            }
            *budget -= 1;
            cache.goal = Some(goal);
            cache.retry_at = tick + 10;
            cache.steps = self
                .walk
                .route_with(
                    horizontal(actor.position),
                    horizontal(goal),
                    PROJECT_RADIUS,
                    &mut self.search,
                )
                .into_iter()
                .flatten()
                .map(|[x, y]| Position { x, y })
                .collect();
        }
        while cache.steps.front().is_some_and(|p| *p == actor.position) {
            cache.steps.pop_front();
        }
        cache.steps.front().copied()
    }
}

impl World {
    /// Install the configured geometry and placements for the selected native layout.
    pub fn install_map_data(&mut self, data: &GameData) -> Result<(), String> {
        if let Some(maps) = data.native_maps() {
            self.install_native_navigation(maps)?;
        }
        if matches!(self.layout, Layout::SoloMid { .. }) {
            self.install_native_camps(data)?;
        }
        Ok(())
    }
    /// Install the operator-supplied map on a newly constructed native room.
    pub fn install_native_navigation(&mut self, maps: &NativeMaps) -> Result<(), String> {
        if self.tick != 0 {
            return Err("native map needs an unstarted simulation".into());
        }
        if self.native_navigation.is_some() {
            return Ok(());
        }
        let (level, is_aral) = match self.layout {
            Layout::Aral { .. } => (maps.aral(), true),
            Layout::SoloMid { .. } => (maps.solo(), false),
            Layout::Validation => {
                return Err("native map requires a native simulation layout".into());
            }
        };
        let level = level.ok_or("native level metadata missing")?;
        let walk = maps
            .mesh(&level.navmesh)
            .map_err(|e| e.to_string())?
            .ok_or("native walk mesh missing")?;
        let mut replacements = Vec::new();
        if is_aral {
            let mut pads = [[Position::default(); 3]; 2];
            for (team, list) in pads.iter_mut().enumerate() {
                for (ordinal, pad) in list.iter_mut().enumerate() {
                    let name = format!(
                        "Spawn_Team{}_Player{}",
                        if team == 0 { "A" } else { "B" },
                        ordinal + 1
                    );
                    let p = level
                        .spawn_points
                        .iter()
                        .find(|p| p.name == name && p.team == Some(team as u8 + 1))
                        .ok_or("ARAL hero pad missing")?;
                    *pad = point(p.pos);
                    if walk.height_at(pad.x, pad.y).is_none() {
                        return Err("ARAL hero pad outside walk mesh".into());
                    }
                }
            }
            let mut ordinal = [0; 2];
            let mut starts = [Position::default(); 6];
            for e in &self.entities[..6] {
                let team = usize::from(e.team);
                starts[(e.id - 1) as usize] = pads[team][ordinal[team]];
                ordinal[team] += 1;
            }
            let names = [
                (1, "VainCrystal"),
                (3, "VainTurret"),
                (4, "VainTurret"),
                (5, "BaseTurret"),
                (6, "InnerTurret"),
                (7, "OuterTurret"),
            ];
            for e in &self.entities {
                let Some(slot) = self.aral_structure_slot(e.id) else {
                    continue;
                };
                let (_, base) = names
                    .iter()
                    .find(|(i, _)| *i == slot % 8)
                    .ok_or("ARAL structure slot missing")?;
                let side = if e.team == 0 { "Home" } else { "Away" };
                let suffix = match slot % 8 {
                    3 => "1",
                    4 => "2",
                    _ => "",
                };
                let name = format!("{base}{side}{suffix}");
                let p = level
                    .turrets_and_structures
                    .iter()
                    .find(|p| p.name == name)
                    .ok_or("ARAL structure placement missing")?;
                replacements.push((e.id, point(p.pos)));
            }
            self.layout = Layout::Aral {
                hero_starts: starts,
            };
            for e in &mut self.entities[..6] {
                e.position = self.layout.home(e);
            }
        } else {
            // This asset contains only canonical turret/crystal positions. The live capture
            // remains the source for pads and the additional armories.
            for e in self
                .entities
                .iter()
                .filter(|e| matches!(e.kind, Kind::Tower | Kind::Crystal))
            {
                if let Some(p) = level
                    .turrets_and_structures
                    .iter()
                    .find(|p| point(p.pos).dist(e.position) < 0.1)
                {
                    replacements.push((e.id, point(p.pos)));
                }
            }
        }
        for (id, position) in replacements {
            let i = self.index(id).expect("known static entity");
            self.entities[i].position = position;
        }
        self.native_navigation = Some(Box::new(NativeNavigation::build(walk, level, is_aral)));
        Ok(())
    }

    pub(super) fn at_item_shop(&self, actor: &Entity) -> bool {
        self.layout.at_base(actor, true)
            || self
                .native_navigation
                .as_ref()
                .is_some_and(|map| map.at_shop(actor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the local base delivery and server-data supplement"]
    fn supplemental_native_hero_uses_connected_ground_in_both_modes() {
        let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
        let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
        let data = GameData::load_with_extra(base, Some(&extra)).unwrap();
        let maps = data.native_maps().unwrap();
        for aral in [false, true] {
            let mut world = World::from_data(0, if aral { 6 } else { 1 }, &data).unwrap();
            if aral {
                world.use_six_human_roster().unwrap();
                world.use_aral_lane().unwrap();
            } else {
                world.use_solo_mid_lane().unwrap();
            }
            world.install_native_navigation(maps).unwrap();
            assert!(
                world
                    .native_navigation
                    .as_ref()
                    .unwrap()
                    .at_fountain(&world.entities[0])
            );
            let start = world.entities[0].position;
            // Move out of the home platform; direct movement crosses the edge of the mesh.
            let goal = if aral {
                Position { x: -70., y: 14. }
            } else {
                Position { x: -82., y: 6. }
            };
            world.command(1, Command::MoveTo(goal)).unwrap();
            for _ in 0..150 {
                world.step();
                let p = world.entities[0].position;
                assert!(
                    world
                        .native_navigation
                        .as_ref()
                        .unwrap()
                        .walk
                        .height_at(p.x, p.y)
                        .is_some(),
                    "{aral}: {p:?}"
                );
            }
            let end = world.entities[0].position;
            assert!(
                start.dist(end) > 4. && end.dist(goal) < 1.5,
                "{aral}: {start:?} {end:?}"
            );
        }
    }
}
