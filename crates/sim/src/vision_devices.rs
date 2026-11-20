use super::*;

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
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisionDevice {
    pub id: u32,
    pub owner: u32,
    pub team: u8,
    pub kind: VisionDeviceKind,
    pub point: Position,
    born: u64,
    hits: u8,
    detonate: Option<u64>,
}
#[derive(Default, Clone, Copy, Serialize)]
pub(super) struct Charges {
    spent: u8,
    next: u64,
    trap_ready: u64,
}
impl Charges {
    fn recharge(&mut self, tick: u64) {
        while self.spent > 0 && self.next <= tick {
            self.spent -= 1;
            self.next += 180 * TICK_HZ;
        }
    }
}
impl Room<'_> {
    pub fn visible_enemy_cams(&self, actor: u32) -> Vec<u32> {
        let Some(i) = self.index(actor) else {
            return Vec::new();
        };
        let e = &self.entities[i];
        self.vision_devices
            .iter()
            .filter(|v| {
                v.kind == VisionDeviceKind::ScoutCam
                    && v.team != e.team
                    && self.tick < v.born + self.shop.vision.stealth
                    && self.sees_position(e.team, v.point)
            })
            .map(|v| v.id)
            .collect()
    }
    pub fn vision_devices(&self) -> &[VisionDevice] {
        &self.vision_devices
    }
    pub(super) fn device_sight(&self, team: u8, p: Position) -> bool {
        self.vision_devices.iter().any(|v| {
            v.team == team
                && v.point.dist(p)
                    <= match v.kind {
                        VisionDeviceKind::ScoutCam => 10.,
                        VisionDeviceKind::ScoutTrap => self.shop.vision.sight,
                    }
                && self.layout.clear(v.point, p)
                && self
                    .native_navigation
                    .as_ref()
                    .is_none_or(|nav| nav.can_see(v.point, p))
        })
    }
    pub(super) fn deploy_device(
        &mut self,
        i: usize,
        instance: u32,
        point: Option<Position>,
    ) -> Result<(), &'static str> {
        if !matches!(self.layout, Layout::Aral { .. }) || i >= 6 || self.vision_devices.len() >= 256
        {
            return Err("vision deployment unavailable");
        }
        let hero = &self.entities[i];
        let slots = hero
            .aral_inventory
            .as_ref()
            .ok_or("ARAL inventory unavailable")?;
        let (kind, point) = if let Some(p) = point {
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
        let id = self.next_id;
        let next = id.checked_add(1).ok_or("entity id exhausted")?;
        let state = &mut *self.state;
        let c = &mut state.vision_charges[i];
        match kind {
            VisionDeviceKind::ScoutCam => {
                c.recharge(state.tick);
                if c.spent >= state.shop.vision.charges {
                    return Err("no scout cam charge");
                }
                if c.spent == 0 {
                    c.next = state.tick + 180 * TICK_HZ;
                }
                c.spent += 1;
            }
            VisionDeviceKind::ScoutTrap => {
                if c.trap_ready > state.tick {
                    return Err("trap cooldown");
                }
                let hero = &mut self.entities[i];
                let index = hero
                    .inventory
                    .iter()
                    .position(|item| item.instance == instance && item.kind == BasicItem::ScoutTrap)
                    .ok_or("scout trap not held")?;
                hero.aral_inventory
                    .as_mut()
                    .unwrap()
                    .remove_held(hero.inventory[index])?;
                hero.inventory.remove(index);
                c.trap_ready = state.tick + state.shop.vision.cooldown;
            }
        }
        let hero = &self.entities[i];
        self.state.next_id = next;
        let device = VisionDevice {
            id,
            owner: hero.id,
            team: hero.team,
            kind,
            point,
            born: self.tick,
            hits: 0,
            detonate: None,
        };
        self.vision_devices.push(device);
        self.pending_item_events
            .push(Event::VisionDeployed { device, instance });
        Ok(())
    }
    pub(super) fn hit_device(&mut self, i: usize, id: u32) -> Result<(), &'static str> {
        let e = &self.entities[i];
        let Some(j) = self.vision_devices.iter().position(|v| v.id == id) else {
            return Err("unknown device");
        };
        let v = self.vision_devices[j];
        if v.kind != VisionDeviceKind::ScoutCam
            || v.team == e.team
            || self.tick >= v.born + self.shop.vision.stealth
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
        self.vision_devices[j].hits += 1;
        if self.vision_devices[j].hits == 5 {
            self.vision_devices.remove(j);
        }
        Ok(())
    }
    pub(super) fn advance_devices(&mut self) {
        let state = &mut *self.state;
        let tick = state.tick;
        state.vision_devices.retain_mut(|v| match v.kind {
            VisionDeviceKind::ScoutCam => tick < v.born + state.shop.vision.life,
            VisionDeviceKind::ScoutTrap => {
                if let Some(due) = v.detonate {
                    if tick < due {
                        return true;
                    }
                    for idx in 0..self.entities.len() {
                        let e = &mut self.entities[idx];
                        if e.kind == Kind::Hero
                            && e.hp > 0.
                            && e.team != v.team
                            && e.position.dist(v.point) <= state.shop.vision.blast
                        {
                            e.status.reveal_until =
                                e.status.reveal_until.max(tick + state.shop.vision.reveal);
                        }
                    }
                    return false;
                }
                if tick >= v.born + state.shop.vision.arm
                    && self.entities.iter().any(|e| {
                        e.kind == Kind::Hero
                            && e.hp > 0.
                            && e.team != v.team
                            && e.position.dist(v.point) <= state.shop.vision.trigger
                    })
                {
                    v.detonate = Some(tick + state.shop.vision.detonate);
                }
                true
            }
        });
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
        assert_ne!(w.vision_devices[0].id, w.vision_devices[1].id);
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
        let device = w.vision_devices[0].id;
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
        assert!(w.vision_devices.is_empty());
        w.command(1, deploy).unwrap();
        let second = w.vision_devices[0].id;
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
        assert_eq!(w.vision_devices[0].detonate, None);
        w.tick = 40;
        w.advance_devices();
        assert_eq!(w.vision_devices[0].detonate, Some(66));
        w.tick = 66;
        w.advance_devices();
        assert!(w.vision_devices.is_empty());
        assert_eq!(w.entities[enemy].status.reveal_until, 166);
        assert_eq!(w.entities[0].status.reveal_until, 0);
    }
}
