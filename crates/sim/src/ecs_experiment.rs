//! Isolated experiment; this module is not part of the project.
use super::*;
#[cfg(feature = "bevy-visibility-bench")]
pub mod bevy;
struct West;
struct East;
#[derive(Clone, Copy)]
struct Site {
    position: Position,
    sight: f32,
    team: u8,
    kind: Kind,
    alive: bool,
}
impl Site {
    fn from(e: &Entity) -> Self {
        Self {
            position: e.position,
            sight: e.stats.sight(),
            team: e.team,
            kind: e.kind,
            alive: e.hp > 0.,
        }
    }
    fn sees(&self, w: &World, team: u8, t: &Entity) -> bool {
        if self.team != team
            || !self.alive
            || (self.position.x - t.position.x).abs() > self.sight
            || self.position.dist(t.position) > self.sight
        {
            return false;
        }
        let clear = if (self.kind == Kind::Neutral || t.kind == Kind::Neutral)
            && let Some(map) = &w.native_navigation
        {
            map.ground_segment(self.position, t.position)
        } else {
            w.layout.clear(self.position, t.position)
        };
        clear
            && w.native_navigation
                .as_ref()
                .is_none_or(|m| m.can_see(self.position, t.position))
    }
}
fn direct(w: &World, team: u8, e: &Entity) -> Option<bool> {
    if e.team == team {
        return Some(true);
    }
    if e.hp <= 0. {
        return Some(false);
    }
    if w.device_sight(team, e.position)
        || w.sigil_sight(team, e.position)
        || e.status.reveal_until > w.tick
    {
        return Some(true);
    }
    None
}
pub struct Matrix {
    handles: std::collections::HashMap<u32, (hecs::Entity, u64)>,
    teams: [Vec<Site>; 2],
    rows: Vec<Site>,
    ecs: hecs::World,
}
impl Default for Matrix {
    fn default() -> Self {
        Self {
            handles: Default::default(),
            teams: Default::default(),
            rows: Vec::new(),
            ecs: hecs::World::new(),
        }
    }
}
impl Matrix {
    pub fn sizes() -> (usize, usize, usize) {
        (
            std::mem::size_of::<Entity>(),
            std::mem::size_of::<Site>(),
            std::mem::size_of::<combat::Status>(),
        )
    }
    pub fn run(&mut self, mode: u8, w: &World, mask: &mut Vec<(u32, u8)>) {
        mask.clear();
        match mode {
            0 => {
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    mask.push((
                        t.id,
                        u8::from(w.visible(0, t)) | u8::from(w.visible(1, t)) << 1,
                    ))
                }
            }
            1 => {
                self.rows.clear();
                self.rows.extend(w.entities.iter().map(Site::from));
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let q = |team| {
                        direct(w, team, t)
                            .unwrap_or_else(|| self.rows.iter().any(|s| s.sees(w, team, t)))
                    };
                    mask.push((t.id, u8::from(q(0)) | u8::from(q(1)) << 1));
                }
            }
            2 => {
                self.ecs.clear();
                self.ecs
                    .spawn_batch(w.entities.iter().map(|e| (Site::from(e),)))
                    .for_each(drop);
                let mut query = self.ecs.query::<&Site>();
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let mut q = |team| {
                        direct(w, team, t)
                            .unwrap_or_else(|| query.iter().any(|s| s.sees(w, team, t)))
                    };
                    mask.push((t.id, u8::from(q(0)) | u8::from(q(1)) << 1));
                }
            }
            3 => {
                self.teams.iter_mut().for_each(Vec::clear);
                for e in w.entities.iter().filter(|e| e.team < 2 && e.hp > 0.) {
                    self.teams[e.team as usize].push(Site::from(e));
                }
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let q = |team| {
                        direct(w, team, t).unwrap_or_else(|| {
                            self.teams[team as usize].iter().any(|s| s.sees(w, team, t))
                        })
                    };
                    mask.push((t.id, u8::from(q(0)) | u8::from(q(1)) << 1));
                }
            }
            4 => {
                self.ecs.clear();
                self.ecs
                    .spawn_batch(
                        w.entities
                            .iter()
                            .filter(|e| e.team == 0 && e.hp > 0.)
                            .map(|e| (Site::from(e), West)),
                    )
                    .for_each(drop);
                self.ecs
                    .spawn_batch(
                        w.entities
                            .iter()
                            .filter(|e| e.team == 1 && e.hp > 0.)
                            .map(|e| (Site::from(e), East)),
                    )
                    .for_each(drop);
                let mut west = self.ecs.query::<hecs::With<&Site, &West>>();
                let mut east = self.ecs.query::<hecs::With<&Site, &East>>();
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let a = direct(w, 0, t).unwrap_or_else(|| west.iter().any(|s| s.sees(w, 0, t)));
                    let b = direct(w, 1, t).unwrap_or_else(|| east.iter().any(|s| s.sees(w, 1, t)));
                    mask.push((t.id, u8::from(a) | u8::from(b) << 1));
                }
            }
            5 => {
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let mut brush = None;
                    let mut q = |team| {
                        direct(w, team, t).unwrap_or_else(|| {
                            w.entities.iter().any(|s| {
                                s.team == team
                                    && s.hp > 0.
                                    && (s.position.x - t.position.x).abs() <= s.stats.sight()
                                    && s.position.dist(t.position) <= s.stats.sight()
                                    && w.combat_clear(s, t)
                                    && w.native_navigation.as_ref().is_none_or(|map| {
                                        brush
                                            .get_or_insert_with(|| map.brush_at(t.position))
                                            .is_none_or(|b| b.contains(s.position))
                                    })
                            })
                        })
                    };
                    mask.push((t.id, u8::from(q(0)) | u8::from(q(1)) << 1));
                }
            }
            6 => {
                for e in &w.entities {
                    match self.handles.entry(e.id) {
                        std::collections::hash_map::Entry::Occupied(mut a) => {
                            let (ent, epoch) = a.get_mut();
                            *epoch = w.tick;
                            *self.ecs.get::<&mut Site>(*ent).unwrap() = Site::from(e);
                        }
                        std::collections::hash_map::Entry::Vacant(a) => {
                            let ent = self.ecs.spawn((e.id, Site::from(e)));
                            a.insert((ent, w.tick));
                        }
                    }
                }
                self.handles.retain(|_, (ent, epoch)| {
                    if *epoch == w.tick {
                        true
                    } else {
                        self.ecs.despawn(*ent).unwrap();
                        false
                    }
                });
                let mut query = self.ecs.query::<&Site>();
                for t in w.entities.iter().filter(|t| t.hp > 0.) {
                    let mut q = |team| {
                        direct(w, team, t)
                            .unwrap_or_else(|| query.iter().any(|s| s.sees(w, team, t)))
                    };
                    mask.push((t.id, u8::from(q(0)) | u8::from(q(1)) << 1));
                }
            }
            _ => unreachable!(),
        }
    }
}
