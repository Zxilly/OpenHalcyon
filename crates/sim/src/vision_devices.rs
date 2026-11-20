use super::*;
use bevy_ecs::prelude::{Commands, Component, Entity as Handle, In, Query};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Rules {
    life: u64,
    stealth: u64,
    arm: u64,
    detonate: u64,
    reveal: u64,
    cooldown: u64,
    trigger: f32,
    blast: f32,
    sight: f32,
    reach: f32,
    charges: u8,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            life: 3000,
            stealth: 40,
            arm: 40,
            detonate: 26,
            reveal: 100,
            cooldown: 80,
            trigger: 1.9,
            blast: 3.2,
            sight: 6.,
            reach: 7.5,
            charges: 2,
        }
    }
}
impl Rules {
    pub(super) fn load(catalog: Option<&halcyon_data::items::ItemCatalog>) -> Result<Self, String> {
        use halcyon_data::items::{ItemActiveParameter, ItemTuning as T};
        let mut r = Self::default();
        let Some(c) = catalog else { return Ok(r) };
        let bounded = |v: Option<f32>| v.filter(|v| v.is_finite() && *v > 0. && *v <= 86400.);
        let time = |v: Option<f32>| {
            bounded(v)
                .map(|v| (v * TICK_HZ as f32).round().max(1.) as u64)
                .ok_or("invalid vision device duration")
        };
        let radius = |v: Option<f32>| {
            bounded(v)
                .filter(|v| *v <= 256.)
                .ok_or("invalid vision device radius")
        };
        if let Some(cam) = c.item("Item_VisionTotem") {
            r.life = time(cam.passive_value(T::Lifespan.key()))?;
            r.stealth = time(cam.passive_value(T::InvisibleDelay.key()))?;
            r.reach = radius(cam.fixed_active(ItemActiveParameter::Range))?;
            r.charges = cam
                .fixed_active(ItemActiveParameter::Charges)
                .filter(|v| v.fract() == 0. && (1.0..=8.0).contains(v))
                .ok_or("invalid scout cam charges")? as u8;
        }
        if let Some(trap) = c.item("Item_ScoutTrap") {
            r.cooldown = time(trap.fixed_active_seconds(ItemActiveParameter::Cooldown.key()))?;
            r.arm = time(trap.passive_value(T::TrapArmTime.key()))?;
            r.detonate = time(trap.passive_value(T::DetonateDelay.key()))?;
            r.reveal = time(trap.passive_value(T::RevealDuration.key()))?;
            r.trigger = radius(trap.passive_value(T::TriggerRadius.key()))?;
            r.blast = radius(trap.passive_value(T::DetonateRadius.key()))?;
            r.sight = radius(trap.passive_value(T::VisionRadius.key()))?;
        }
        Ok(r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisionDeviceKind {
    ScoutCam,
    ScoutTrap,
    Flare,
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisionDevice {
    pub id: u32,
    pub owner: u32,
    pub team: u8,
    pub kind: VisionDeviceKind,
    pub point: Position,
    born: u64,
    hits: u8,
    detonate: Option<u64>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    extra_hits: u8,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    expires_at: u64,
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub revealed: bool,
}
impl VisionDevice {
    pub(super) fn new(
        id: u32,
        owner: u32,
        team: u8,
        kind: VisionDeviceKind,
        point: Position,
        born: u64,
    ) -> Self {
        Self {
            id,
            owner,
            team,
            kind,
            point,
            born,
            hits: 0,
            detonate: None,
            extra_hits: 0,
            expires_at: 0,
            revealed: false,
        }
    }
    pub(super) fn with_extras(mut self, extra_hits: u8, expires_at: u64) -> Self {
        self.extra_hits = extra_hits;
        self.expires_at = expires_at;
        self
    }
}
#[derive(Default, Clone, Copy, Serialize)]
pub(super) struct Charges {
    spent: u8,
    next: u64,
    trap_ready: u64,
}
impl Charges {
    fn recharge_with_period(&mut self, tick: u64, period: u64) {
        let step = period.max(TICK_HZ);
        while self.spent > 0 && self.next <= tick {
            self.spent -= 1;
            self.next += step;
        }
    }
}
impl Room<'_> {
    pub fn visible_enemy_cams(&self, actor: u32) -> Vec<u32> {
        let Some(i) = self.index(actor) else {
            return Vec::new();
        };
        let e = &self.entities[i];
        self.vision_devices()
            .filter(|v| {
                v.kind == VisionDeviceKind::ScoutCam
                    && v.team != e.team
                    && (self.tick < v.born + self.shop.vision.stealth
                        || v.revealed
                        || self.sees_stealth(e.team, v.point))
                    && self.sees_position(e.team, v.point)
            })
            .map(|v| v.id)
            .collect()
    }
    /// True if an active allied Flare grants true sight over `p`.
    pub fn sees_stealth(&self, team: u8, p: Position) -> bool {
        let radius = self.shop.effect_rules.active.flare_sight_radius;
        self.vision_devices().any(|v| {
            v.kind == VisionDeviceKind::Flare && v.team == team && v.point.dist(p) <= radius
        })
    }
    pub(super) fn reveal_in_flare(&mut self, team: u8, center: Position, radius: f32, until: u64) {
        let ids: Vec<Handle> = self.vision_device_ids.clone();
        for handle in ids {
            if let Some(mut v) = self.entities.ecs.get_mut::<VisionDevice>(handle)
                && v.team != team
                && v.point.dist(center) <= radius
                && matches!(
                    v.kind,
                    VisionDeviceKind::ScoutTrap | VisionDeviceKind::ScoutCam
                )
            {
                v.revealed = true;
            }
        }
        for idx in 0..self.entities.len() {
            let e = &mut self.entities[idx];
            if e.team != team && e.hp > 0. && e.position.dist(center) <= radius {
                e.status.reveal_until = e.status.reveal_until.max(until);
            }
        }
    }
    /// Live devices in deployment order. Gameplay and native IDs are independent of ECS handles.
    pub fn vision_devices(&self) -> impl ExactSizeIterator<Item = &VisionDevice> {
        self.vision_device_ids.iter().map(|id| {
            self.entities
                .ecs
                .get::<VisionDevice>(*id)
                .expect("live registered vision device")
        })
    }
    pub(super) fn device_sight(&self, team: u8, p: Position) -> bool {
        let flare_radius = self.shop.effect_rules.active.flare_sight_radius;
        self.vision_devices().any(|v| {
            v.team == team
                && match v.kind {
                    VisionDeviceKind::Flare => v.point.dist(p) <= flare_radius,
                    VisionDeviceKind::ScoutCam => {
                        v.point.dist(p) <= 10.
                            && self.layout.clear(v.point, p)
                            && self
                                .native_navigation
                                .as_ref()
                                .is_none_or(|nav| nav.can_see(v.point, p))
                    }
                    VisionDeviceKind::ScoutTrap => {
                        v.point.dist(p) <= self.shop.vision.sight
                            && self.layout.clear(v.point, p)
                            && self
                                .native_navigation
                                .as_ref()
                                .is_none_or(|nav| nav.can_see(v.point, p))
                    }
                }
        })
    }
    pub(super) fn deploy_device(
        &mut self,
        i: usize,
        instance: u32,
        point: Option<Position>,
    ) -> Result<(), &'static str> {
        if self.vision_device_ids.len() >= 256
            || self.entities[i].kind != Kind::Hero
            || self.entities[i].hp <= 0.
        {
            return Err("vision deployment unavailable");
        }
        let hero = &self.entities[i];
        let active_rules = self.shop.effect_rules.active;
        let (kind, point) = if let Some(p) = point {
            if !matches!(self.layout, Layout::Aral { .. }) || i >= 6 {
                return Err("vision deployment unavailable");
            }
            let slots = hero
                .aral_inventory
                .as_ref()
                .ok_or("ARAL inventory unavailable")?;
            if !slots.slots().iter().flatten().any(|s| {
                s.instance == instance && s.template == AralStarter::VisionTotem.grant().template
            }) {
                return Err("vision totem not held");
            }
            if !self.layout.walkable(p)
                || hero.position.dist(p) > self.shop.vision.reach
                || self
                    .native_navigation
                    .as_ref()
                    .is_some_and(|nav| !nav.can_see(hero.position, p))
            {
                return Err("outside placement range");
            }
            (VisionDeviceKind::ScoutCam, p)
        } else {
            (VisionDeviceKind::ScoutTrap, hero.position)
        };
        let bonus_charges = if hero.has_item(BasicItem::SuperScout2000) {
            active_rules.super_scout_bonus_charges
        } else if hero.has_item(BasicItem::ScoutPak) {
            active_rules.scout_pak_bonus_charges
        } else {
            0
        };
        let cd_reduction = if hero.has_item(BasicItem::SuperScout2000) {
            active_rules.super_scout_cd_reduction
        } else if hero.has_item(BasicItem::ScoutPak) {
            active_rules.scout_pak_cd_reduction
        } else {
            0
        };
        let extra_hits = if hero.has_item(BasicItem::SuperScout2000) {
            active_rules.super_scout_bonus_hits
        } else if hero.has_item(BasicItem::ScoutTuff) {
            active_rules.scout_tuff_bonus_hits
        } else {
            0
        };
        let bonus_lifespan = if hero.has_item(BasicItem::SuperScout2000) {
            active_rules.super_scout_bonus_lifespan
        } else if hero.has_item(BasicItem::ScoutTuff) {
            active_rules.scout_tuff_bonus_lifespan
        } else {
            0
        };
        let id = self.next_id;
        let next = id.checked_add(1).ok_or("entity id exhausted")?;
        let tick = self.state.tick;
        let vision_rules = self.shop.vision;
        let mut device_expires = 0;
        match kind {
            VisionDeviceKind::ScoutCam => {
                let period = (180 * TICK_HZ).saturating_sub(cd_reduction);
                let max_charges = vision_rules.charges.saturating_add(bonus_charges);
                let c = &mut self.state.vision_charges[i];
                c.recharge_with_period(tick, period);
                if c.spent >= max_charges {
                    return Err("no scout cam charge");
                }
                if c.spent == 0 {
                    c.next = tick + period;
                }
                c.spent += 1;
                self.state.scores[i].scout_cam_uses += 1;
                device_expires = tick + vision_rules.life + bonus_lifespan;
            }
            VisionDeviceKind::ScoutTrap => {
                let on_cd = if let Some(c) = self.state.vision_charges.get(i) {
                    c.trap_ready > tick
                } else {
                    self.entities[i].rt().is_some_and(|rt| rt.trap_ready > tick)
                };
                if on_cd {
                    return Err("trap cooldown");
                }
                let hero = &mut self.entities[i];
                let index = hero
                    .inventory
                    .iter()
                    .position(|item| item.instance == instance && item.kind == BasicItem::ScoutTrap)
                    .ok_or("scout trap not held")?;
                let sold = hero.inventory[index];
                if let Some(aral) = hero.aral_inventory.as_mut() {
                    aral.remove_held(sold)?;
                }
                hero.inventory.remove(index);
                if let Some(c) = self.state.vision_charges.get_mut(i) {
                    c.trap_ready = tick + vision_rules.cooldown;
                } else {
                    hero.rt_mut().trap_ready = tick + vision_rules.cooldown;
                }
            }
            VisionDeviceKind::Flare => unreachable!(),
        }
        let hero = &self.entities[i];
        self.state.next_id = next;
        let device = VisionDevice::new(id, hero.id, hero.team, kind, point, tick).with_extras(
            if kind == VisionDeviceKind::ScoutCam {
                extra_hits
            } else {
                0
            },
            device_expires,
        );
        let handle = self.entities.ecs.spawn(device).id();
        self.vision_device_ids.push(handle);
        self.pending_item_events
            .push(Event::VisionDeployed { device, instance });
        Ok(())
    }
    pub(super) fn hit_device(&mut self, i: usize, id: u32) -> Result<(), &'static str> {
        let e = &self.entities[i];
        let Some((j, v)) = self
            .vision_devices()
            .copied()
            .enumerate()
            .find(|(_, v)| v.id == id)
        else {
            return Err("unknown device");
        };
        let revealed = self.tick < v.born + self.shop.vision.stealth
            || v.revealed
            || self.sees_stealth(e.team, v.point);
        if v.kind != VisionDeviceKind::ScoutCam
            || v.team == e.team
            || !revealed
            || e.position.dist(v.point) > e.stats.range
            || !self.sees_position(e.team, v.point)
            || e.status.stun_until > self.tick
            || e.winding.is_some()
            || e.next_attack > self.tick
        {
            return Err("cannot hit scout cam");
        }
        let interval = self.basic_attack_interval(e);
        self.entities[i].next_attack = self.tick + interval;
        let handle = self.vision_device_ids[j];
        let max_hits = 5u8.saturating_add(v.extra_hits);
        if v.hits + 1 >= max_hits {
            self.entities.ecs.despawn(handle);
            self.vision_device_ids.remove(j);
        } else {
            self.entities
                .ecs
                .get_mut::<VisionDevice>(handle)
                .expect("registered scout cam")
                .hits += 1;
        }
        Ok(())
    }
    pub(super) fn advance_devices(&mut self) {
        if self.vision_device_ids.is_empty() {
            return;
        }
        let detonations = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let input = (self.tick, self.shop.vision, detonations.clone());
        self.entities
            .ecs
            .run_system_cached_with(step_devices, input)
            .expect("vision device system");
        let ecs = &self.entities.ecs;
        self.state
            .vision_device_ids
            .retain(|id| ecs.get::<VisionDevice>(*id).is_some());
        let detonated = std::mem::take(&mut *detonations.lock().unwrap());
        let blast = self.shop.vision.blast;
        let rules = self.shop.effect_rules.active;
        let mut trap_events = Vec::new();
        for trap in detonated {
            let level = self
                .index(trap.owner)
                .map_or(1, |idx| self.entities[idx].level);
            let raw = rules.trap_damage_base
                + rules.trap_damage_per_level * f32::from(level.saturating_sub(1));
            for j in 0..self.entities.len() {
                let target = &self.entities[j];
                if target.kind != Kind::Hero
                    || target.hp <= 0.
                    || target.team == trap.team
                    || target.position.dist(trap.point) > blast
                {
                    continue;
                }
                let dmg = shop::mitigated(raw, target.stats.shield, 0.);
                self.hurt_item(
                    trap.owner,
                    trap.team,
                    j,
                    dmg,
                    DamageSchool::Crystal,
                    &mut trap_events,
                );
            }
        }
        self.state.pending_item_events.append(&mut trap_events);
    }
}

#[derive(Clone, Copy)]
struct DetonatedTrap {
    owner: u32,
    team: u8,
    point: Position,
}

type DetonationLog = std::sync::Arc<std::sync::Mutex<Vec<DetonatedTrap>>>;

fn step_devices(
    In((tick, rules, detonations)): In<(u64, Rules, DetonationLog)>,
    mut devices: Query<(Handle, &mut VisionDevice)>,
    mut actors: Query<&mut Entity>,
    mut commands: Commands,
) {
    for (handle, mut device) in &mut devices {
        match device.kind {
            VisionDeviceKind::ScoutCam => {
                let expiry = device.expires_at.max(device.born + rules.life);
                if tick >= expiry {
                    commands.entity(handle).despawn();
                }
            }
            VisionDeviceKind::Flare => {
                if tick >= device.expires_at {
                    commands.entity(handle).despawn();
                }
            }
            VisionDeviceKind::ScoutTrap => {
                let in_range = |actor: &Entity, radius| {
                    actor.kind == Kind::Hero
                        && actor.hp > 0.
                        && actor.team != device.team
                        && actor.position.dist(device.point) <= radius
                };
                if let Some(due) = device.detonate {
                    if tick >= due {
                        for mut actor in &mut actors {
                            if in_range(&actor, rules.blast) && !actor.status.blocks_debuffs(tick) {
                                actor.status.reveal_until =
                                    actor.status.reveal_until.max(tick + rules.reveal);
                            }
                        }
                        detonations.lock().unwrap().push(DetonatedTrap {
                            owner: device.owner,
                            team: device.team,
                            point: device.point,
                        });
                        commands.entity(handle).despawn();
                    }
                } else if tick >= device.born + rules.arm
                    && actors.iter().any(|actor| in_range(actor, rules.trigger))
                {
                    device.detonate = Some(tick + rules.detonate);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn aral() -> World {
        let mut w = World::new(19, 6);
        w.use_six_human_roster().unwrap();
        w.use_aral_lane().unwrap();
        w
    }
    #[test]
    fn totem_owns_charges_and_lifetime_and_grants_only_team_vision() {
        let mut w = aral();
        let p = Position { x: 0., y: 10. };
        w.entities[0].position = p;
        let deploy = Command::DeployVisionTotem {
            instance: 2001,
            point: p,
        };
        assert!(
            w.command(
                1,
                Command::DeployVisionTotem {
                    instance: 2000,
                    point: p
                }
            )
            .is_err()
        );
        assert!(
            w.command(
                1,
                Command::DeployVisionTotem {
                    instance: 2001,
                    point: Position { x: f32::NAN, y: 0. }
                }
            )
            .is_err()
        );
        w.command(1, deploy).unwrap();
        w.command(1, deploy).unwrap();
        assert!(w.command(1, deploy).is_err());
        assert_ne!(
            w.vision_devices().next().unwrap().id,
            w.vision_devices().nth(1).unwrap().id
        );
        w.entities[0].position = Position { x: -88., y: 10. };
        assert!(w.device_sight(0, p));
        assert!(!w.device_sight(1, p));
        w.tick = 150 * TICK_HZ;
        w.advance_devices();
        assert!(!w.device_sight(0, p));
        w.entities[0].position = p;
        assert!(w.command(1, deploy).is_err());
        w.tick = 180 * TICK_HZ;
        w.command(1, deploy).unwrap();
        assert!(w.command(1, deploy).is_err());
    }
    #[test]
    fn visible_camera_takes_five_enemy_hits_and_rejects_hits_after_stealth() {
        let mut w = aral();
        let point = w.entities[0].position;
        let deploy = Command::DeployVisionTotem {
            instance: 2001,
            point,
        };
        w.command(1, deploy).unwrap();
        let device = w.vision_devices().next().unwrap().id;
        assert!(w.command(1, Command::HitScoutCam { device }).is_err());
        let enemies: Vec<_> = w
            .entities
            .iter()
            .filter(|e| e.team == 1 && e.kind == Kind::Hero)
            .map(|e| e.id)
            .collect();
        for &enemy in &enemies {
            let i = w.index(enemy).unwrap();
            w.entities[i].position = point;
            w.command(enemy, Command::HitScoutCam { device }).unwrap();
            assert!(w.command(enemy, Command::HitScoutCam { device }).is_err());
        }
        w.tick = 20;
        for &enemy in &enemies[..2] {
            w.command(enemy, Command::HitScoutCam { device }).unwrap();
        }
        assert!(w.vision_devices().len() == 0);
        w.command(1, deploy).unwrap();
        let second = w.vision_devices().next().unwrap().id;
        w.tick = 60;
        assert!(
            w.command(enemies[0], Command::HitScoutCam { device: second })
                .is_err()
        );
    }
    #[test]
    fn trap_consumes_owned_instance_then_reveals_only_enemies_in_blast() {
        let mut w = aral();
        let trap = w.purchase(1, BasicItem::ScoutTrap).unwrap().granted;
        let deploy = Command::DeployScoutTrap {
            instance: trap.instance,
        };
        assert!(w.command(2, deploy).is_err());
        w.command(1, deploy).unwrap();
        assert!(w.command(1, deploy).is_err());
        assert!(w.entities[0].inventory.is_empty());
        let p = w.entities[0].position;
        let enemy = w
            .entities
            .iter()
            .position(|e| e.kind == Kind::Hero && e.team == 1)
            .unwrap();
        w.entities[enemy].position = p;
        w.tick = 39;
        w.advance_devices();
        assert_eq!(w.vision_devices().next().unwrap().detonate, None);
        w.tick = 40;
        w.advance_devices();
        assert_eq!(w.vision_devices().next().unwrap().detonate, Some(66));
        w.tick = 66;
        w.advance_devices();
        assert!(w.vision_devices().len() == 0);
        assert_eq!(w.entities[enemy].status.reveal_until, 166);
        assert_eq!(w.entities[0].status.reveal_until, 0);
    }
}
