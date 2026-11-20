//! A persistent replication projection. Only the simulation owns gameplay IDs and geometry.
use super::{Matrix, Site, direct};
use crate::{Kind, Position, World as Simulation};
use bevy_ecs::prelude::*;
use std::collections::{HashMap, hash_map::Entry};

#[derive(Resource)]
struct Source(Simulation);
#[derive(Resource, Default)]
struct Handles(HashMap<u32, (Entity, u64)>);
#[derive(Resource, Default)]
struct Output(Vec<(u32, u8)>);
#[derive(Component)]
struct Viewer(Site);
#[derive(Component)]
struct Target {
    id: u32,
    row: usize,
    position: Position,
    kind: Kind,
    direct: [Option<bool>; 2],
}
#[derive(Component)]
struct West;
#[derive(Component)]
struct East;

fn synchronize(
    source: Res<Source>,
    mut handles: ResMut<Handles>,
    mut commands: Commands,
    mut existing: Query<(&mut Target, &mut Viewer)>,
    mut output: ResMut<Output>,
) {
    let sim = &source.0;
    output.0.clear();
    for e in sim.entities.iter().filter(|e| e.hp > 0.) {
        let target = Target {
            id: e.id,
            row: output.0.len(),
            position: e.position,
            kind: e.kind,
            direct: [direct(sim, 0, e), direct(sim, 1, e)],
        };
        output.0.push((e.id, 0));
        let viewer = Viewer(Site::from(e));
        match handles.0.entry(e.id) {
            Entry::Occupied(mut entry) => {
                let (handle, seen) = entry.get_mut();
                *seen = sim.tick;
                let (mut old_target, mut old_viewer) =
                    existing.get_mut(*handle).expect("registered actor");
                if old_viewer.0.team != viewer.0.team {
                    let mut actor = commands.entity(*handle);
                    actor.remove::<(West, East)>();
                    match viewer.0.team {
                        0 => {
                            actor.insert(West);
                        }
                        1 => {
                            actor.insert(East);
                        }
                        _ => {}
                    }
                }
                *old_target = target;
                *old_viewer = viewer;
            }
            Entry::Vacant(entry) => {
                let team = viewer.0.team;
                let mut actor = commands.spawn((target, viewer));
                match team {
                    0 => {
                        actor.insert(West);
                    }
                    1 => {
                        actor.insert(East);
                    }
                    _ => {}
                }
                entry.insert((actor.id(), sim.tick));
            }
        }
    }
    handles.0.retain(|_, (actor, seen)| {
        if *seen == sim.tick {
            true
        } else {
            commands.entity(*actor).despawn();
            false
        }
    });
}
fn sees(s: &Site, w: &Simulation, t: &Target) -> bool {
    if (s.position.x - t.position.x).abs() > s.sight || s.position.dist(t.position) > s.sight {
        return false;
    }
    let clear = if (s.kind == Kind::Neutral || t.kind == Kind::Neutral)
        && let Some(map) = &w.native_navigation
    {
        map.ground_segment(s.position, t.position)
    } else {
        w.layout.clear(s.position, t.position)
    };
    clear
        && w.native_navigation
            .as_ref()
            .is_none_or(|map| map.can_see(s.position, t.position))
}
fn visibility(
    source: Res<Source>,
    targets: Query<&Target>,
    west: Query<&Viewer, With<West>>,
    east: Query<&Viewer, With<East>>,
    mut output: ResMut<Output>,
) {
    for t in &targets {
        let a = t.direct[0].unwrap_or_else(|| west.iter().any(|s| sees(&s.0, &source.0, t)));
        let b = t.direct[1].unwrap_or_else(|| east.iter().any(|s| sees(&s.0, &source.0, t)));
        output.0[t.row] = (t.id, u8::from(a) | (u8::from(b) << 1));
    }
}

pub struct Projection {
    world: World,
    schedule: Schedule,
}
impl Projection {
    pub fn new(sim: Simulation) -> Self {
        let mut world = World::new();
        world.insert_resource(Source(sim));
        world.init_resource::<Handles>();
        world.insert_resource(Output(Vec::with_capacity(256)));
        let mut schedule = Schedule::default();
        schedule.add_systems((synchronize, visibility).chain());
        Self { world, schedule }
    }
    pub fn step(&mut self) {
        std::hint::black_box(self.world.resource_mut::<Source>().0.step());
    }
    pub fn direct(&self, engine: &mut Matrix, mode: u8, out: &mut Vec<(u32, u8)>) {
        engine.run(mode, &self.world.resource::<Source>().0, out);
    }
    pub fn run(&mut self) {
        self.schedule.run(&mut self.world);
    }
    pub fn masks(&self) -> &[(u32, u8)] {
        &self.world.resource::<Output>().0
    }
    pub fn digest(&self) -> String {
        self.world.resource::<Source>().0.state_digest()
    }
}
