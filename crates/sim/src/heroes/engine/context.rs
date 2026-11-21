use super::{
    AbilityKitSpec, BuffStatKind, CastGate, CastTargeting, EvalInputs, HeroKitBook, HeroKitSpec,
    HeroSlot, KitEffect, KitEffectKind, KitStatBonus, MarkInstance, ResourceKind, TimedBuff,
    ValueSpec, apply_stat_to_bonus, clamp_map_pos, dir_between, marks, seconds_to_ticks,
};
use crate::combat::{AbilitySlot, CastTarget};
use crate::ecs::score::DamageSchool;
use crate::heroes::talents::{self, ActiveTalentState, TalentSpecial, find_talent};
use crate::{Event, Kind, Position, Room, TICK_HZ};

/// Safe, ergonomic runtime execution context handed to declarative primitives and custom callbacks.
pub(crate) struct HeroKitContext<'a, 'w> {
    pub room: &'a mut Room<'w>,
    pub events: &'a mut Vec<Event>,
    pub caster: usize,
    pub tick: u64,
}

impl<'a, 'w> HeroKitContext<'a, 'w> {
    pub(crate) fn new(room: &'a mut Room<'w>, events: &'a mut Vec<Event>, caster: usize) -> Self {
        let tick = room.tick;
        Self {
            room,
            events,
            caster,
            tick,
        }
    }

    pub(crate) fn caster_id(&self) -> u32 {
        self.room.entities[self.caster].id
    }

    pub(crate) fn caster_team(&self) -> u8 {
        self.room.entities[self.caster].team
    }

    pub(crate) fn caster_pos(&self) -> Position {
        self.room.entities[self.caster].position
    }

    pub(crate) fn caster_type(&self) -> u16 {
        self.room.entities[self.caster].type_id
    }

    pub(crate) fn spec(&self) -> Option<&'static HeroKitSpec> {
        crate::heroes::kits::kit_spec(self.caster_type())
    }

    pub(crate) fn eval_inputs(&self) -> EvalInputs {
        let e = &self.room.entities[self.caster];
        let bonus = self.room.kit_bonus_stats(self.caster);
        let cp =
            e.item_stats.crystal + bonus.crystal_flat + e.item_stats.crystal * bonus.crystal_pct;
        let base_wp = e.stats.weapon;
        let wp = (base_wp + bonus.weapon_flat) * (1.0 + bonus.weapon_pct);
        let bwp = e.item_stats.bonus_weapon + bonus.weapon_flat + base_wp * bonus.weapon_pct;
        EvalInputs {
            cp,
            wp,
            bonus_wp: bwp.max(0.0),
            bonus_hp: e.bonus_hp,
            alt: (e.max_energy - 200.0).max(0.0),
            bonus_as: (e.item_stats.attack_speed + bonus.attack_speed).max(0.0),
        }
    }

    pub(crate) fn eval_var(&self, ability_id: &str, rank: u8, fallback: ValueSpec) -> f32 {
        let inputs = self.eval_inputs();
        if let Some(book) = &self.room.abilities.hero_kits {
            book.eval_value(self.caster_type(), ability_id, rank, fallback, inputs)
        } else {
            fallback.eval_fallback(rank, inputs)
        }
    }

    pub(crate) fn resolve_target_idx(&self, target: CastTarget) -> Option<usize> {
        match target {
            CastTarget::SelfCast => Some(self.caster),
            CastTarget::Entity(id) => self.room.index(id),
            CastTarget::Point(_) => None,
        }
    }

    pub(crate) fn resolve_target_pos(&self, target: CastTarget, max_range: f32) -> Position {
        let origin = self.caster_pos();
        let raw = match target {
            CastTarget::SelfCast => origin,
            CastTarget::Entity(id) => self
                .room
                .index(id)
                .map_or(origin, |j| self.room.entities[j].position),
            CastTarget::Point(p) => p,
        };
        if max_range <= 0.0 || origin.dist(raw) <= max_range {
            clamp_map_pos(raw)
        } else {
            let (dx, dy) = dir_between(origin, raw);
            clamp_map_pos(Position {
                x: origin.x + dx * max_range,
                y: origin.y + dy * max_range,
            })
        }
    }

    pub(crate) fn active_talent(&self) -> Option<&ActiveTalentState> {
        self.room.entities[self.caster]
            .hero_kit
            .as_deref()
            .and_then(|k| k.talent.as_ref())
    }

    pub(crate) fn has_talent(&self, id: &str) -> bool {
        self.active_talent().is_some_and(|t| t.id == id)
    }

    pub(crate) fn talent_special(&self) -> TalentSpecial {
        self.active_talent()
            .and_then(|t| find_talent(&t.id))
            .map_or(TalentSpecial::None, |s| s.special)
    }

    pub(crate) fn talent_mod(&self, label: &str) -> Option<f32> {
        let t = self.active_talent()?;
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents::eval_talent_mod(&t.id, label, t.level, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_dmg_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents::talent_damage_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_barrier_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents::talent_barrier_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn talent_heal_mult(&self, slot: AbilitySlot) -> f32 {
        let Some(t) = self.active_talent() else {
            return 1.0;
        };
        let inputs = self.eval_inputs();
        let loaded = self.room.abilities.hero_kits.as_ref().map(|b| &b.talents);
        talents::talent_heal_multiplier(t, slot, inputs.cp, inputs.wp, loaded)
    }

    pub(crate) fn deal_damage(
        &mut self,
        target_idx: usize,
        raw: f32,
        school: DamageSchool,
        extra_pierce: f32,
        triggers_basic: bool,
    ) {
        if raw <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        if self.room.entities[target_idx]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.untargetable_until)
        {
            return;
        }
        let src_id = self.caster_id();
        let team = self.caster_team();
        let caster_bonus = self.room.kit_bonus_stats(self.caster);
        let target_bonus = self.room.kit_bonus_stats(target_idx);
        let target = &self.room.entities[target_idx];
        let caster = &self.room.entities[self.caster];
        let amp = (1.0 + caster_bonus.damage_amp + target_bonus.damage_amp).max(0.1);
        let effective_raw = raw * amp;
        let mitigated = match school {
            DamageSchool::Weapon => {
                let defense =
                    (target.stats.armor + target_bonus.armor_flat) * (1.0 + target_bonus.armor_pct);
                let pierce =
                    (caster.item_stats.armor_pierce + caster_bonus.armor_pierce + extra_pierce)
                        .clamp(0.0, 1.0);
                crate::items::shop::mitigated(effective_raw, defense, pierce)
            }
            DamageSchool::Crystal => {
                let defense = (target.stats.shield + target_bonus.shield_flat)
                    * (1.0 + target_bonus.shield_pct);
                let pierce =
                    (caster.item_stats.shield_pierce + caster_bonus.shield_pierce + extra_pierce)
                        .clamp(0.0, 1.0);
                crate::items::shop::mitigated(effective_raw, defense, pierce)
            }
            DamageSchool::True => effective_raw,
        };
        let hp_before = self.room.entities[target_idx].hp;
        self.room
            .hurt(src_id, team, target_idx, mitigated, school, self.events);
        let actual_dmg = (hp_before - self.room.entities[target_idx].hp).max(0.0);
        if caster_bonus.lifesteal > 0.0 && actual_dmg > 0.0 {
            self.heal_unit(self.caster, actual_dmg * caster_bonus.lifesteal);
        }
        if let Some(spec) = self.spec()
            && let Some(cb) = spec.perk.on_ability_damage
        {
            cb(self, target_idx, mitigated);
        }
        if triggers_basic {
            let _ = self
                .room
                .item_stats_trigger_basic_placeholder(self.caster, target_idx);
        }
    }

    pub(crate) fn heal_unit(&mut self, target_idx: usize, amount: f32) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut adjusted = amount;
        if let Some(spec) = crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            adjusted *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        if self.room.entities[target_idx]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.mortal_wound_until)
        {
            adjusted *= 0.67;
        }
        let src_id = self.caster_id();
        self.room.heal(src_id, target_idx, adjusted, self.events);
    }

    pub(crate) fn grant_barrier(
        &mut self,
        target_idx: usize,
        amount: f32,
        duration_sec: f32,
        stack_cap: Option<(f32, u8)>,
    ) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut scaled = amount;
        if let Some(spec) = crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            scaled *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let t = &mut self.room.entities[target_idx];
        let prev = if t.status.barrier_until > self.tick {
            t.status.barrier
        } else {
            0.0
        };
        if let Some((cap_mult, _)) = stack_cap {
            t.status.barrier = (prev + scaled).min(scaled * cap_mult);
        } else {
            t.status.barrier = prev.max(scaled);
        }
        t.status.barrier_until = t.status.barrier_until.max(until);
    }

    pub(crate) fn grant_fortified(&mut self, target_idx: usize, amount: f32, duration_sec: f32) {
        if amount <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let mut scaled = amount;
        if let Some(spec) = crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id) {
            let lvl = f32::from(self.room.entities[target_idx].level.saturating_sub(1));
            scaled *= spec.perk.healing_received_mult_base
                + lvl * spec.perk.healing_received_mult_per_level;
        }
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        let prev = if self.tick < state.fortified_until {
            state.fortified_health
        } else {
            0.0
        };
        state.fortified_health = prev.max(scaled);
        state.fortified_until = state.fortified_until.max(until);
    }

    pub(crate) fn target_blocks_debuffs(&self, target_idx: usize) -> bool {
        let e = &self.room.entities[target_idx];
        e.status.blocks_debuffs(self.tick)
            || e.hero_kit.as_deref().is_some_and(|k| {
                self.tick < k.unstoppable_until || self.tick < k.untargetable_until
            })
    }

    pub(crate) fn apply_stun(&mut self, target_idx: usize, sec: f32) {
        self.apply_stun_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_stun_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let phinn = crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let t = &mut self.room.entities[target_idx];
        if phinn {
            t.status.silence_until = t.status.silence_until.max(self.tick + dur);
        } else {
            t.status.stun_until = t.status.stun_until.max(self.tick + dur);
            t.cast = None;
        }
    }

    pub(crate) fn apply_slow(&mut self, target_idx: usize, factor: f32, sec: f32) {
        self.apply_slow_ticks(target_idx, factor, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_slow_ticks(&mut self, target_idx: usize, slow_pct: f32, dur: u64) {
        if dur == 0 || slow_pct <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        let e = &self.room.entities[target_idx];
        if self.target_blocks_debuffs(target_idx) || e.status.blocks_movement_impairment(self.tick)
        {
            return;
        }
        let phinn = crate::heroes::kits::kit_spec(e.type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let eff_slow = if phinn {
            (slow_pct * 0.5).min(0.4)
        } else {
            slow_pct.clamp(0.05, 0.85)
        };
        let speed_factor = (1.0 - eff_slow).clamp(0.15, 0.95);
        let t = &mut self.room.entities[target_idx];
        let existing_factor = if self.tick < t.status.slow_until {
            t.status.slow_factor.min(speed_factor)
        } else {
            speed_factor
        };
        t.status.slow_until = t.status.slow_until.max(self.tick + dur);
        t.status.slow_factor = existing_factor;
    }

    pub(crate) fn apply_root(&mut self, target_idx: usize, sec: f32) {
        self.apply_root_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_root_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let e = &self.room.entities[target_idx];
        if e.status.blocks_movement_impairment(self.tick) {
            return;
        }
        let phinn = crate::heroes::kits::kit_spec(e.type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade);
        let t = &mut self.room.entities[target_idx];
        if phinn {
            t.status.slow_until = t.status.slow_until.max(self.tick + dur);
            t.status.slow_factor = t.status.slow_factor.max(0.35);
        } else {
            t.status.root_until = t.status.root_until.max(self.tick + dur);
        }
    }

    pub(crate) fn apply_silence(&mut self, target_idx: usize, sec: f32) {
        self.apply_silence_ticks(target_idx, seconds_to_ticks(sec));
    }

    pub(crate) fn apply_silence_ticks(&mut self, target_idx: usize, dur: u64) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let t = &mut self.room.entities[target_idx];
        t.status.silence_until = t.status.silence_until.max(self.tick + dur);
    }

    pub(crate) fn apply_fear(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        if crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            self.apply_slow_ticks(target_idx, 0.35, dur);
            return;
        }
        let src = self.caster_id();
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.fear_until = state.fear_until.max(until);
        state.fear_source = src;
    }

    pub(crate) fn apply_sleep_ticks(&mut self, target_idx: usize, dur: u64, unwaking: bool) {
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        if crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            self.apply_silence_ticks(target_idx, dur);
            return;
        }
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.sleep_until = state.sleep_until.max(until);
        state.sleep_unwaking |= unwaking;
    }

    pub(crate) fn apply_taunt(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0
            || target_idx >= self.room.entities.len()
            || self.target_blocks_debuffs(target_idx)
        {
            return;
        }
        let src = self.caster_id();
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.taunt_until = state.taunt_until.max(until);
        state.taunt_target = src;
    }

    pub(crate) fn apply_mortal_wound(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let state = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        state.mortal_wound_until = state.mortal_wound_until.max(until);
    }

    pub(crate) fn apply_reveal(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        self.room.entities[target_idx].status.reveal_until = self.room.entities[target_idx]
            .status
            .reveal_until
            .max(until);
    }

    pub(crate) fn cleanse_cc(&mut self, target_idx: usize) {
        if target_idx >= self.room.entities.len() {
            return;
        }
        let t = &mut self.room.entities[target_idx];
        t.status.stun_until = 0;
        t.status.root_until = 0;
        t.status.slow_until = 0;
        t.status.bulwark_slow_until = 0;
        t.status.silence_until = 0;
        if let Some(k) = t.hero_kit.as_deref_mut() {
            k.fear_until = 0;
            k.sleep_until = 0;
            k.taunt_until = 0;
        }
    }

    pub(crate) fn set_unstoppable(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.unstoppable_until = k.unstoppable_until.max(until);
    }

    pub(crate) fn set_untargetable(&mut self, target_idx: usize, sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.untargetable_until = k.untargetable_until.max(until);
    }

    pub(crate) fn set_stealth(&mut self, target_idx: usize, sec: f32, speed: f32, grace_sec: f32) {
        let dur = seconds_to_ticks(sec);
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let grace = self.tick + seconds_to_ticks(grace_sec);
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.stealth_until = k.stealth_until.max(until);
        k.stealth_grace_until = k.stealth_grace_until.max(grace);
        k.stealth_speed = k.stealth_speed.max(speed);
    }

    pub(crate) fn knockback(
        &mut self,
        target_idx: usize,
        from: Position,
        distance: f32,
        wall_stun_sec: f32,
    ) -> bool {
        if distance <= 0.0 || target_idx >= self.room.entities.len() {
            return false;
        }
        if crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            return false;
        }
        let tpos = self.room.entities[target_idx].position;
        let (dx, dy) = dir_between(from, tpos);
        let dest = clamp_map_pos(Position {
            x: tpos.x + dx * distance,
            y: tpos.y + dy * distance,
        });
        let clear = self.room.spell_motion_clear(tpos, dest);
        if clear {
            self.room.place_spell_actor(target_idx, dest, self.events);
            false
        } else {
            // Stop at half step if blocked; stun if wall stun is configured.
            let half = clamp_map_pos(Position {
                x: tpos.x + dx * (distance * 0.4),
                y: tpos.y + dy * (distance * 0.4),
            });
            if self.room.spell_motion_clear(tpos, half) {
                self.room.place_spell_actor(target_idx, half, self.events);
            }
            if wall_stun_sec > 0.0 {
                self.apply_stun(target_idx, wall_stun_sec);
            }
            true
        }
    }

    pub(crate) fn pull_toward(&mut self, target_idx: usize, toward: Position, distance: f32) {
        if distance <= 0.0 || target_idx >= self.room.entities.len() {
            return;
        }
        if crate::heroes::kits::kit_spec(self.room.entities[target_idx].type_id)
            .is_some_and(|s| s.perk.unstoppable_cc_downgrade)
        {
            return;
        }
        let tpos = self.room.entities[target_idx].position;
        let next = tpos.advance(toward, distance);
        if self.room.spell_motion_clear(tpos, next) {
            self.room.place_spell_actor(target_idx, next, self.events);
        }
    }

    pub(crate) fn add_buff(
        &mut self,
        target_idx: usize,
        stat: BuffStatKind,
        amount: f32,
        duration_sec: f32,
    ) {
        self.add_buff_ticks(
            target_idx,
            stat,
            amount,
            seconds_to_ticks(duration_sec).max(1),
        );
    }

    pub(crate) fn add_buff_ticks(
        &mut self,
        target_idx: usize,
        stat: BuffStatKind,
        amount: f32,
        dur: u64,
    ) {
        if dur == 0 || target_idx >= self.room.entities.len() {
            return;
        }
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        if let Some(existing) = k
            .buffs
            .iter_mut()
            .find(|b| b.stat == stat && b.until > self.tick)
        {
            existing.amount = existing.amount.max(amount);
            existing.until = existing.until.max(until);
        } else {
            k.buffs.push(TimedBuff {
                stat,
                amount,
                until,
            });
        }
    }

    pub(crate) fn add_mark(
        &mut self,
        target_idx: usize,
        mark: u8,
        add: u8,
        max: u8,
        duration_sec: f32,
    ) -> u8 {
        self.add_mark_ticks(
            target_idx,
            mark,
            add,
            max,
            seconds_to_ticks(duration_sec).max(1),
        )
    }

    pub(crate) fn add_mark_ticks(
        &mut self,
        target_idx: usize,
        mark: u8,
        add: u8,
        max: u8,
        dur: u64,
    ) -> u8 {
        if target_idx >= self.room.entities.len() {
            return 0;
        }
        let src_id = self.caster_id();
        let until = self.tick + dur;
        let k = self.room.entities[target_idx]
            .hero_kit
            .get_or_insert_with(Box::default);
        let entry = k.marks.entry(mark).or_insert(MarkInstance {
            stacks: 0,
            until,
            source_id: src_id,
        });
        if entry.until <= self.tick {
            entry.stacks = 0;
        }
        entry.stacks = entry.stacks.saturating_add(add).min(max.max(1));
        entry.until = until;
        entry.source_id = src_id;
        entry.stacks
    }

    pub(crate) fn mark_stacks(&self, target_idx: usize, mark: u8) -> u8 {
        self.room
            .entities
            .get(target_idx)
            .and_then(|e| e.hero_kit.as_deref())
            .and_then(|k| k.marks.get(&mark))
            .filter(|m| m.until > self.tick)
            .map_or(0, |m| m.stacks)
    }

    pub(crate) fn consume_mark(&mut self, target_idx: usize, mark: u8) -> u8 {
        let Some(k) = self
            .room
            .entities
            .get_mut(target_idx)
            .and_then(|e| e.hero_kit.as_deref_mut())
        else {
            return 0;
        };
        if let Some(m) = k.marks.remove(&mark)
            && m.until > self.tick
        {
            m.stacks
        } else {
            0
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn empower_next_attacks(
        &mut self,
        count: u8,
        bonus_dmg: f32,
        is_crystal: bool,
        stun_sec: f32,
        slow_pct: f32,
        slow_sec: f32,
        knockback: f32,
        duration_sec: f32,
    ) {
        let until = self.tick + seconds_to_ticks(duration_sec).max(1);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.empowered_attacks = count.max(1);
        k.empowered_until = until;
        k.empowered_bonus_damage = bonus_dmg;
        k.empowered_is_crystal = is_crystal;
        k.empowered_stun_sec = stun_sec;
        k.empowered_slow_pct = slow_pct;
        k.empowered_slow_sec = slow_sec;
        k.empowered_knockback = knockback;
    }

    pub(crate) fn reset_attack_cd(&mut self) {
        self.room.entities[self.caster].next_attack = self.tick;
    }

    pub(crate) fn reset_slot_cd(&mut self, slot: AbilitySlot) {
        self.room.entities[self.caster].cooldown[slot.idx()] = self.tick;
    }

    pub(crate) fn reduce_slot_cd(&mut self, slot: AbilitySlot, seconds: f32) {
        let ticks = seconds_to_ticks(seconds);
        let cur = self.room.entities[self.caster].cooldown[slot.idx()];
        self.room.entities[self.caster].cooldown[slot.idx()] =
            cur.saturating_sub(ticks).max(self.tick);
    }

    pub(crate) fn open_recast(
        &mut self,
        slot: AbilitySlot,
        stage: u8,
        target_id: u32,
        window_sec: f32,
    ) {
        let until = self.tick + seconds_to_ticks(window_sec).max(1);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.step_stage[slot.idx()] = stage;
        k.recast_until[slot.idx()] = until;
        k.recast_target[slot.idx()] = target_id;
    }

    pub(crate) fn clear_recast(&mut self, slot: AbilitySlot) {
        if let Some(k) = self.room.entities[self.caster].hero_kit.as_deref_mut() {
            k.step_stage[slot.idx()] = 0;
            k.recast_until[slot.idx()] = 0;
            k.recast_target[slot.idx()] = 0;
        }
    }

    pub(crate) fn swap_form(&mut self, new_form: u8) {
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.form = if new_form == 0xFF {
            u8::from(k.form == 0)
        } else {
            new_form
        };
    }

    pub(crate) fn resource(&self) -> f32 {
        self.room.kit_resource_value(self.caster)
    }

    pub(crate) fn resource_max(&self) -> f32 {
        self.room.kit_resource_max(self.caster)
    }

    pub(crate) fn add_resource(&mut self, delta: f32) {
        let max = self.resource_max();
        let cur = self.resource();
        let next = (cur + delta).clamp(0.0, max);
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.resource = next;
        k.resource_initialized = true;
    }

    pub(crate) fn spend_resource(&mut self, amount: f32) -> bool {
        let cur = self.resource();
        if cur + 0.001 < amount {
            return false;
        }
        let k = self.room.entities[self.caster]
            .hero_kit
            .get_or_insert_with(Box::default);
        k.resource = (cur - amount).max(0.0);
        k.resource_initialized = true;
        true
    }

    pub(crate) fn spawn_effect(&mut self, kind: KitEffectKind, position: Position, goal: Position) {
        let owner_id = self.caster_id();
        let team = self.caster_team();
        self.room.entities.spawn_spell(KitEffect {
            owner_id,
            team,
            position,
            goal,
            kind,
        });
    }

    pub(crate) fn enemies_in_radius(
        &self,
        center: Position,
        radius: f32,
        heroes_only: bool,
    ) -> Vec<usize> {
        let team = self.caster_team();
        self.room
            .entities
            .iter()
            .enumerate()
            .filter_map(|(j, e)| {
                let valid_kind = if heroes_only {
                    e.kind == Kind::Hero
                } else {
                    matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                };
                (e.team != team && e.hp > 0.0 && valid_kind && e.position.dist(center) <= radius)
                    .then_some(j)
            })
            .collect()
    }

    pub(crate) fn allies_in_radius(
        &self,
        center: Position,
        radius: f32,
        include_self: bool,
        heroes_only: bool,
    ) -> Vec<usize> {
        let team = self.caster_team();
        self.room
            .entities
            .iter()
            .enumerate()
            .filter_map(|(j, e)| {
                if e.team != team || e.hp <= 0.0 || (!include_self && j == self.caster) {
                    return None;
                }
                let valid_kind = if heroes_only {
                    e.kind == Kind::Hero
                } else {
                    matches!(e.kind, Kind::Hero | Kind::Minion)
                };
                (valid_kind && e.position.dist(center) <= radius).then_some(j)
            })
            .collect()
    }
}

impl Room<'_> {
    pub(crate) fn hero_kit_enabled(&self) -> bool {
        self.abilities.hero_kits.is_some()
    }

    pub fn enable_builtin_hero_kits(&mut self) {
        let book = std::sync::Arc::make_mut(&mut self.abilities);
        if book.hero_kits.is_none() {
            book.hero_kits = Some(HeroKitBook::builtin());
        }
    }

    /// Resolve active `AbilityKitSpec` for `(caster_idx, slot)`, honoring forms, recasts, and Viola chords.
    pub(crate) fn active_ability_spec(
        &self,
        caster_idx: usize,
        slot: AbilitySlot,
    ) -> Option<&'static AbilityKitSpec> {
        let e = &self.entities[caster_idx];
        let spec = crate::heroes::kits::kit_spec(e.type_id)?;
        let kit = e.hero_kit.as_deref();
        let recast_active = kit.is_some_and(|k| self.tick < k.recast_until[slot.idx()]);
        let stage = kit.map_or(0, |k| k.step_stage[slot.idx()]);
        let form = kit.map_or(0, |k| k.form);

        let variant = match slot {
            AbilitySlot::A => {
                if recast_active && stage == 2 {
                    spec.variant(HeroSlot::C2)
                        .or_else(|| spec.variant(HeroSlot::A2))
                } else if recast_active || form == 1 {
                    spec.variant(HeroSlot::A2)
                } else {
                    None
                }
            }
            AbilitySlot::B => {
                if recast_active || form == 1 {
                    spec.variant(HeroSlot::B2)
                } else {
                    None
                }
            }
            AbilitySlot::C => {
                if spec.hero_id == "Viola"
                    && let Some(k) = kit
                    && k.musical_note_count >= 3
                {
                    let b_count = k.musical_notes.iter().filter(|&&n| n == 2).count();
                    let chord = match b_count {
                        0 => HeroSlot::C,  // AAA -> Fleeting Song
                        1 => HeroSlot::C2, // AAB -> Tempo
                        2 => HeroSlot::C3, // ABB -> Dissonance
                        _ => HeroSlot::C4, // BBB -> Lullaby
                    };
                    spec.variant(chord).or(Some(&spec.c))
                } else if recast_active || form == 1 {
                    spec.variant(HeroSlot::C2)
                } else {
                    None
                }
            }
        };
        Some(variant.unwrap_or(match slot {
            AbilitySlot::A => &spec.a,
            AbilitySlot::B => &spec.b,
            AbilitySlot::C => &spec.c,
        }))
    }

    pub(crate) fn kit_resource_max(&self, i: usize) -> f32 {
        let e = &self.entities[i];
        let Some(spec) = crate::heroes::kits::kit_spec(e.type_id) else {
            return e.max_energy;
        };
        if spec.resource == ResourceKind::Energy {
            return e.max_energy;
        }
        let talent_bonus = e
            .hero_kit
            .as_deref()
            .and_then(|k| k.talent.as_ref())
            .map_or(0.0, |t| {
                let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
                talents::talent_passive_stats(t, e.item_stats.crystal, e.stats.weapon, loaded)
                    .max_resource_bonus
            });
        let bonus_energy = (e.max_energy - 200.0).max(0.0);
        spec.resource_max + bonus_energy * spec.resource_energy_conversion.0 + talent_bonus
    }

    pub(crate) fn kit_resource_value(&self, i: usize) -> f32 {
        let e = &self.entities[i];
        let Some(spec) = crate::heroes::kits::kit_spec(e.type_id) else {
            return e.energy;
        };
        if spec.resource == ResourceKind::Energy {
            return e.energy;
        }
        match &e.hero_kit {
            Some(k) if k.resource_initialized => k.resource,
            _ => spec.resource_initial,
        }
    }

    pub(crate) fn kit_ammo_charges(&self, i: usize, slot: AbilitySlot) -> u8 {
        let e = &self.entities[i];
        let max_c = self.kit_max_charges(i, slot);
        if let Some(k) = &e.hero_kit
            && k.charges_initialized[slot.idx()]
        {
            return k.charges[slot.idx()].min(max_c);
        }
        max_c
    }

    pub(crate) fn kit_max_charges(&self, i: usize, slot: AbilitySlot) -> u8 {
        let e = &self.entities[i];
        let Some(ab) = self.active_ability_spec(i, slot) else {
            return 0;
        };
        let rank = e.ability_ranks[slot.idx()].max(1);
        let mut base = ab
            .charges
            .eval_fallback(rank, EvalInputs::default())
            .round() as u8;
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            base = base.saturating_add(talents::talent_bonus_charges(
                t,
                slot,
                e.item_stats.crystal,
                e.stats.weapon,
                loaded,
            ));
        }
        base
    }

    pub(crate) fn kit_can_learn(&self, i: usize, slot: AbilitySlot) -> bool {
        if !self.hero_kit_enabled() {
            return false;
        }
        let e = &self.entities[i];
        let Some(spec) = crate::heroes::kits::kit_spec(e.type_id) else {
            return false;
        };
        let rank = e.ability_ranks[slot.idx()];
        let max_rank = self.active_ability_spec(i, slot).map_or(
            if slot == AbilitySlot::C {
                spec.ult_max_rank
            } else {
                5
            },
            |ab| {
                if slot == AbilitySlot::C {
                    ab.max_rank.min(spec.ult_max_rank)
                } else {
                    ab.max_rank.min(5)
                }
            },
        );
        if e.skill_points == 0 || rank >= max_rank {
            return false;
        }
        let needed = if slot == AbilitySlot::C {
            if spec.ult_from_level_one {
                1 + 2 * rank
            } else {
                6 + 3 * rank
            }
        } else {
            1 + 2 * rank
        };
        e.level >= needed
    }

    pub(crate) fn kit_effective_range(&self, i: usize, slot: AbilitySlot) -> Option<f32> {
        let e = self.entities.get(i)?;
        let ab = self.active_ability_spec(i, slot)?;
        let eval_rank = e.ability_ranks[slot.idx()].max(1);
        let bonus = self.kit_bonus_stats(i);
        let inputs = EvalInputs {
            cp: e.item_stats.crystal + bonus.crystal_flat,
            wp: e.stats.weapon + bonus.weapon_flat,
            bonus_wp: e.item_stats.bonus_weapon + bonus.weapon_flat,
            bonus_hp: e.bonus_hp,
            alt: (e.max_energy - 200.0).max(0.0),
            bonus_as: (e.item_stats.attack_speed + bonus.attack_speed).max(0.0),
        };
        let mut range = if let Some(book) = &self.abilities.hero_kits {
            book.eval_value(e.type_id, ab.ability_id, eval_rank, ab.range, inputs)
        } else {
            ab.range.eval_fallback(eval_rank, inputs)
        };
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            range += talents::talent_bonus_range(t, slot, inputs.cp, inputs.wp, loaded);
        }
        Some(range.max(1.5))
    }

    pub(crate) fn project_spell_landing(&self, goal: Position) -> Option<Position> {
        if let Some(map) = &self.native_navigation {
            if map.has_ground(goal) {
                Some(goal)
            } else {
                map.project_within(goal, 3.0)
            }
        } else {
            self.layout.walkable(goal).then_some(goal)
        }
    }

    pub(crate) fn kit_castable(
        &self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
    ) -> Result<(u8, f32, f32, u64, bool), &'static str> {
        self.kit_castable_with_leeway(i, slot, target, 0.0)
    }

    pub(crate) fn kit_castable_with_leeway(
        &self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
        extra_range: f32,
    ) -> Result<(u8, f32, f32, u64, bool), &'static str> {
        if !self.hero_kit_enabled() {
            return Err("ability unavailable");
        }
        let e = &self.entities[i];
        let Some(spec) = crate::heroes::kits::kit_spec(e.type_id) else {
            return Err("ability unavailable");
        };
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let rank = e.ability_ranks[slot.idx()];
        let allow_unlearned_ult = slot == AbilitySlot::C && spec.ult_from_level_one && rank == 0;
        if rank == 0 && !allow_unlearned_ult {
            return Err("ability unlearned");
        }
        let eval_rank = rank.max(1);

        if !ab.castable_while_cc
            && (e.status.silence_until > self.tick
                || e.status.stun_until > self.tick
                || e.hero_kit
                    .as_deref()
                    .is_some_and(|k| self.tick < k.fear_until || self.tick < k.sleep_until))
        {
            return Err("crowd controlled");
        }

        let is_recast = e
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.recast_until[slot.idx()]);

        let max_c = self.kit_max_charges(i, slot);
        if !is_recast {
            if max_c > 0 {
                if self.kit_ammo_charges(i, slot) == 0 || self.tick < e.cooldown[slot.idx()] {
                    return Err("cooldown");
                }
            } else if self.tick < e.cooldown[slot.idx()] {
                return Err("cooldown");
            }
        }

        // Check preconditions.
        match ab.gate {
            CastGate::None => {}
            CastGate::RequiresFullResource => {
                if self.kit_resource_value(i) + 0.01 < self.kit_resource_max(i) {
                    return Err("insufficient resource");
                }
            }
            CastGate::RequiresTetheredTarget => {
                let owner_id = e.id;
                let has_tether = self.entities.spells::<KitEffect>().any(|eff| {
                    eff.owner_id == owner_id && matches!(eff.kind, KitEffectKind::Tether { .. })
                });
                if !has_tether {
                    return Err("no tethered target");
                }
            }
            CastGate::RequiresLockedTarget => {
                let has_lock = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.lock_target_id != 0 && self.tick < k.lock_target_until);
                if !has_lock {
                    return Err("no locked target");
                }
            }
            CastGate::RequiresRebootReady => {
                let ready = match e.reboot {
                    None => true,
                    Some(crate::heroes::native::reboot::Phase::Recharging { until }) => {
                        self.tick >= until
                    }
                    Some(crate::heroes::native::reboot::Phase::Rebooting { .. }) => false,
                };
                if !ready {
                    return Err("reboot on cooldown");
                }
            }
            CastGate::RequiresNotRecentlyDamaged => {
                let last_dmg = e.hero_kit.as_deref().map_or(0, |k| k.last_damaged_tick);
                let lock_ticks = (seconds_to_ticks(3.5))
                    .saturating_sub(((e.item_stats.bonus_weapon * 0.015) * TICK_HZ as f32) as u64);
                if last_dmg > 0 && self.tick < last_dmg + lock_ticks {
                    return Err("recently damaged");
                }
            }
            CastGate::RequiresMusicalChord3 => {
                let ready = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.musical_note_count >= 3);
                if !ready {
                    return Err("requires 3 musical notes");
                }
            }
            CastGate::RequiresEmpowered => {
                let ready = e
                    .hero_kit
                    .as_deref()
                    .is_some_and(|k| k.empowered_attacks > 0 || self.tick < k.empowered_until);
                if !ready {
                    return Err("requires empowerment");
                }
            }
            CastGate::RequiresOutOfCombat => {
                let last = e
                    .hero_kit
                    .as_deref()
                    .map_or(0, |k| k.last_damaged_tick.max(k.last_attack_tick));
                if last > 0 && self.tick < last + TICK_HZ * 3 {
                    return Err("in combat");
                }
            }
        }

        let bonus = self.kit_bonus_stats(i);
        let inputs = EvalInputs {
            cp: e.item_stats.crystal + bonus.crystal_flat,
            wp: e.stats.weapon + bonus.weapon_flat,
            bonus_wp: e.item_stats.bonus_weapon + bonus.weapon_flat,
            bonus_hp: e.bonus_hp,
            alt: (e.max_energy - 200.0).max(0.0),
            bonus_as: (e.item_stats.attack_speed + bonus.attack_speed).max(0.0),
        };
        let eval_f = |vs: ValueSpec| -> f32 {
            if let Some(book) = &self.abilities.hero_kits {
                book.eval_value(e.type_id, ab.ability_id, eval_rank, vs, inputs)
            } else {
                vs.eval_fallback(eval_rank, inputs)
            }
        };

        let mut cost = if is_recast { 0.0 } else { eval_f(ab.cost) };
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            if let Some(em) = talents::eval_talent_mod(
                &t.id,
                "ENERGY_MODIFIER",
                t.level,
                inputs.cp,
                inputs.wp,
                loaded,
            ) && find_talent(&t.id).is_some_and(|s| s.scope.matches(slot))
            {
                cost *= em.max(0.0);
            }
        }

        match spec.resource {
            ResourceKind::Energy => {
                if e.energy + 0.01 < cost {
                    return Err("insufficient energy");
                }
            }
            ResourceKind::None => {}
            _ => {
                if self.kit_resource_value(i) + 0.01 < cost {
                    return Err("insufficient resource");
                }
            }
        }

        let range = self.kit_effective_range(i, slot).unwrap_or(6.0);
        let range_limit = range + 0.75 + extra_range.max(0.0);

        match (ab.targeting, target) {
            (CastTargeting::TargetEnemy, CastTarget::Entity(id)) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.team == e.team || t.hp <= 0.0 {
                    return Err("invalid target");
                }
                if e.position.dist(t.position) > range_limit {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::TargetEnemy, _) => {
                return Err("target required");
            }
            (
                CastTargeting::TargetAlly | CastTargeting::TargetAllyOrSelf,
                CastTarget::Entity(id),
            ) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.team != e.team || t.hp <= 0.0 {
                    return Err("invalid ally target");
                }
                if ab.targeting == CastTargeting::TargetAlly && j == i {
                    return Err("cannot target self");
                }
                if e.position.dist(t.position) > range_limit {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::TargetAnyHero, CastTarget::Entity(id)) => {
                let j = self.index(id).ok_or("unknown target")?;
                let t = &self.entities[j];
                if t.kind != Kind::Hero || t.hp <= 0.0 {
                    return Err("invalid hero target");
                }
                if e.position.dist(t.position) > range_limit {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::GroundPoint, CastTarget::Point(p))
            | (CastTargeting::TargetLocationOrEntity, CastTarget::Point(p)) => {
                if range < 180.0 && e.position.dist(p) > range_limit {
                    return Err("cast out of range");
                }
            }
            (CastTargeting::Directional | CastTargeting::InstantSelf, _)
            | (
                CastTargeting::TargetAllyOrSelf
                | CastTargeting::TargetAnyHero
                | CastTargeting::GroundPoint
                | CastTargeting::TargetLocationOrEntity,
                _,
            ) => {}
            (CastTargeting::TargetAlly, _) => {
                return Err("ally target required");
            }
        }

        let mut cd_sec = eval_f(ab.cooldown);
        if let Some(t) = e.hero_kit.as_deref().and_then(|k| k.talent.as_ref()) {
            let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
            cd_sec =
                talents::apply_cooldown_modifier(t, slot, cd_sec, inputs.cp, inputs.wp, loaded);
        }
        let cda = 1.0 + e.item_stats.cooldown + bonus.cooldown_accel;
        let cd_ticks = seconds_to_ticks((cd_sec / cda.max(0.2)).max(0.0));
        let windup_ticks = seconds_to_ticks(eval_f(ab.startup));

        Ok((
            eval_rank,
            cost,
            range,
            cd_ticks,
            windup_ticks == 0 || is_recast,
        ))
    }

    /// Execute a validated HeroKit cast on entity `i`.
    pub(crate) fn kit_begin_or_release_cast(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        target: CastTarget,
        events: &mut Vec<Event>,
    ) -> Result<bool, &'static str> {
        let (eval_rank, cost, _range, cd_ticks, _immediate) = self.kit_castable(i, slot, target)?;
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let spec =
            crate::heroes::kits::kit_spec(self.entities[i].type_id).ok_or("ability unavailable")?;

        let is_recast = self.entities[i]
            .hero_kit
            .as_deref()
            .is_some_and(|k| self.tick < k.recast_until[slot.idx()]);

        if is_recast {
            let mut ctx = HeroKitContext::new(self, events, i);
            let handled = if let Some(custom) = ab.custom_exec {
                custom(&mut ctx, slot, eval_rank, target)?
            } else {
                false
            };
            if !handled {
                ctx.exec_primitives(ab.ability_id, slot, eval_rank, target, ab.effects);
            }
            let still_recast = ctx.room.entities[i]
                .hero_kit
                .as_deref()
                .is_some_and(|k| ctx.tick < k.recast_until[slot.idx()]);
            if !still_recast {
                ctx.room.entities[i].cooldown[slot.idx()] = ctx.tick + cd_ticks;
            }
            return Ok(true);
        }

        // Spend cost and apply cooldown/charges cleanly.
        match spec.resource {
            ResourceKind::Energy => self.entities[i].energy -= cost,
            ResourceKind::None => {}
            _ => {
                let max = self.kit_resource_max(i);
                let cur = self.kit_resource_value(i);
                let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
                k.resource = (cur - cost).clamp(0.0, max);
                k.resource_initialized = true;
            }
        }

        let max_c = self.kit_max_charges(i, slot);
        if max_c > 0 {
            let cur_c = self.kit_ammo_charges(i, slot);
            let next_c = cur_c.saturating_sub(1);
            let tick = self.tick;
            let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
            k.charges[slot.idx()] = next_c;
            k.charges_initialized[slot.idx()] = true;
            if k.charge_regen_at[slot.idx()] <= tick {
                k.charge_regen_at[slot.idx()] = tick + cd_ticks.max(1);
            }
            let between = seconds_to_ticks(
                ab.between_charges_cd
                    .eval_fallback(eval_rank, EvalInputs::default()),
            );
            self.entities[i].cooldown[slot.idx()] = self.tick + between;
        } else {
            self.entities[i].cooldown[slot.idx()] = self.tick + cd_ticks;
        }

        self.kit_execute_release(i, slot, eval_rank, target, events)?;
        Ok(true)
    }

    pub(crate) fn kit_windup_ticks(&self, i: usize, slot: AbilitySlot) -> u64 {
        let Some(ab) = self.active_ability_spec(i, slot) else {
            return 0;
        };
        let rank = self.entities[i].ability_ranks[slot.idx()].max(1);
        let inputs = EvalInputs::default();
        let sec = if let Some(book) = &self.abilities.hero_kits {
            book.eval_value(
                self.entities[i].type_id,
                ab.ability_id,
                rank,
                ab.startup,
                inputs,
            )
        } else {
            ab.startup.eval_fallback(rank, inputs)
        };
        seconds_to_ticks(sec)
    }

    pub(crate) fn kit_execute_release(
        &mut self,
        i: usize,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        events: &mut Vec<Event>,
    ) -> Result<(), &'static str> {
        let ab = self
            .active_ability_spec(i, slot)
            .ok_or("ability unavailable")?;
        let spec =
            crate::heroes::kits::kit_spec(self.entities[i].type_id).ok_or("ability unavailable")?;
        let mut ctx = HeroKitContext::new(self, events, i);
        let handled = if let Some(custom) = ab.custom_exec {
            custom(&mut ctx, slot, rank, target)?
        } else {
            false
        };
        if !handled {
            ctx.exec_primitives(ab.ability_id, slot, rank, target, ab.effects);
        }
        if let Some(on_cast) = spec.perk.on_ability_cast {
            on_cast(&mut ctx, slot, rank, target);
        }
        Ok(())
    }

    /// Aggregate active temporary buffs, passive skill bonuses, and talent stats on entity `i`.
    pub(crate) fn kit_bonus_stats(&self, i: usize) -> KitStatBonus {
        let mut b = KitStatBonus::default();
        let e = &self.entities[i];
        if !self.hero_kit_enabled() && e.hero_kit.is_none() {
            return b;
        }
        if let Some(spec) = crate::heroes::kits::kit_spec(e.type_id) {
            for (slot_idx, ab) in [(0usize, &spec.a), (1usize, &spec.b), (2usize, &spec.c)] {
                let r = e.ability_ranks[slot_idx];
                if r > 0
                    && let Some((stat, vs)) = ab.passive_bonus
                {
                    let val = vs.eval_fallback(r, EvalInputs::default());
                    apply_stat_to_bonus(&mut b, stat, val);
                }
            }
        }
        if let Some(k) = e.hero_kit.as_deref() {
            for buff in &k.buffs {
                if self.tick < buff.until {
                    apply_stat_to_bonus(&mut b, buff.stat, buff.amount);
                }
            }
            if self.tick < k.stealth_until {
                b.move_speed_flat += k.stealth_speed;
            }
            if let Some(t) = &k.talent {
                let loaded = self.abilities.hero_kits.as_ref().map(|bk| &bk.talents);
                let tb =
                    talents::talent_passive_stats(t, e.item_stats.crystal, e.stats.weapon, loaded);
                b.weapon_pct += tb.weapon_pct;
                b.weapon_flat += tb.weapon_flat;
                b.crystal_pct += tb.crystal_pct;
                b.crystal_flat += tb.crystal_flat;
                b.attack_speed += tb.attack_speed;
                b.armor_pct += tb.armor_pct;
                b.armor_flat += tb.armor_flat;
                b.shield_pct += tb.shield_pct;
                b.shield_flat += tb.shield_flat;
                b.move_speed_flat += tb.move_speed_flat;
                b.move_speed_pct += tb.move_speed_pct;
                b.attack_range += tb.attack_range;
                b.cooldown_accel += tb.cooldown_accel;
                b.crit_chance += tb.crit_chance;
                b.crit_damage += tb.crit_damage;
                b.armor_pierce += tb.armor_pierce;
                b.shield_pierce += tb.shield_pierce;
                b.lifesteal += tb.lifesteal;
                b.hp_regen_per_sec += tb.hp_regen_per_sec;
                b.max_resource_bonus += tb.max_resource_bonus;
            }
        }
        b
    }

    /// 20Hz per-actor HeroKit maintenance: resource regen, ammo recharge, HP regen talents, perk ticks.
    pub(crate) fn tick_hero_kits(&mut self, events: &mut Vec<Event>) {
        if !self.hero_kit_enabled() {
            return;
        }
        let actor_count = self.entities.len();
        for i in 0..actor_count {
            if self.entities[i].hp <= 0.0 || self.entities[i].kind != Kind::Hero {
                continue;
            }
            let type_id = self.entities[i].type_id;
            let Some(spec) = crate::heroes::kits::kit_spec(type_id) else {
                continue;
            };

            // Prune expired buffs/marks cleanly.
            let tick = self.tick;
            let is_stopped = matches!(self.entities[i].order, crate::Command::Stop);
            if is_stopped && spec.perk.on_tick.is_some() {
                self.entities[i].hero_kit.get_or_insert_with(Box::default);
            }
            if let Some(k) = self.entities[i].hero_kit.as_deref_mut() {
                k.buffs.retain(|b| b.until > tick);
                k.marks.retain(|_, m| m.until > tick);
                if is_stopped {
                    k.stationary_ticks = k.stationary_ticks.saturating_add(1);
                } else {
                    k.stationary_ticks = 0;
                }
            }

            // Regenerate non-Energy resources (Focus / Stamina / Vengeance / Bloodrage decay).
            if spec.resource != ResourceKind::Energy && spec.resource != ResourceKind::None {
                let regen = spec.resource_regen_per_sec
                    + self.entities[i].item_stats.energy_regen * spec.resource_energy_conversion.1;
                if regen != 0.0 {
                    let max_r = self.kit_resource_max(i);
                    let cur_r = self.kit_resource_value(i);
                    let delta = regen / TICK_HZ as f32;
                    let next = (cur_r + delta).clamp(0.0, max_r);
                    if (next - cur_r).abs() > 1e-5 {
                        let k = self.entities[i].hero_kit.get_or_insert_with(Box::default);
                        k.resource = next;
                        k.resource_initialized = true;
                    }
                }
            }

            // Recharge ability ammo charges when needed.
            for slot in [AbilitySlot::A, AbilitySlot::B, AbilitySlot::C] {
                let max_c = self.kit_max_charges(i, slot);
                if max_c == 0 {
                    continue;
                }
                let cur_c = self.kit_ammo_charges(i, slot);
                if cur_c < max_c
                    && self.entities[i]
                        .hero_kit
                        .as_deref()
                        .is_some_and(|k| tick >= k.charge_regen_at[slot.idx()])
                {
                    let slot_rank = self.entities[i].ability_ranks[slot.idx()].max(1);
                    let next_cd = self.active_ability_spec(i, slot).map_or(3.0, |a| {
                        a.cooldown.eval_fallback(slot_rank, EvalInputs::default())
                    });
                    if let Some(k) = self.entities[i].hero_kit.as_deref_mut() {
                        k.charges[slot.idx()] = (cur_c + 1).min(max_c);
                        k.charges_initialized[slot.idx()] = true;
                        if k.charges[slot.idx()] < max_c {
                            k.charge_regen_at[slot.idx()] = tick + seconds_to_ticks(next_cd).max(1);
                        }
                    }
                }
            }

            // Passive HP regen from talents (e.g., Talent_BonusRegen, Phinn RaidBoss, Flicker FairyHealing).
            let bonus = self.kit_bonus_stats(i);
            let mut regen_hp = bonus.hp_regen_per_sec / TICK_HZ as f32;
            if let Some(t) = self.entities[i]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                && let Some(sp) = find_talent(&t.id).map(|s| s.special)
            {
                let loaded = self.abilities.hero_kits.as_ref().map(|b| &b.talents);
                let cp = self.entities[i].item_stats.crystal;
                let wp = self.entities[i].stats.weapon;
                match sp {
                    TalentSpecial::PhinnRapidRegen => {
                        if let Some(pct) = talents::eval_talent_mod(
                            &t.id,
                            "CURRENT_HEALTH_REGEN",
                            t.level,
                            cp,
                            wp,
                            loaded,
                        ) {
                            regen_hp += (self.entities[i].hp * pct) / TICK_HZ as f32;
                        }
                    }
                    TalentSpecial::FlickerStealthRegen => {
                        let stealthed = self.entities[i]
                            .hero_kit
                            .as_deref()
                            .is_some_and(|k| tick < k.stealth_until);
                        if stealthed
                            && let Some(pct) = talents::eval_talent_mod(
                                &t.id,
                                "MAX_HEALTH_TO_HEALING",
                                t.level,
                                cp,
                                wp,
                                loaded,
                            )
                        {
                            regen_hp += (self.entities[i].max_hp * pct) / TICK_HZ as f32;
                        }
                    }
                    _ => {}
                }
            }
            if regen_hp > 0.0 && self.entities[i].hp < self.entities[i].max_hp {
                let id = self.entities[i].id;
                self.heal(id, i, regen_hp, events);
            }

            // Heroic perk per-tick callback.
            if let Some(on_tick) = spec.perk.on_tick {
                let mut ctx = HeroKitContext::new(self, events, i);
                on_tick(&mut ctx);
            }
        }
    }

    /// Hook called inside `apply_damage` for HeroKit fortified HP absorption & perk reactions.
    pub(crate) fn kit_modify_incoming_damage(
        &mut self,
        source_id: u32,
        target_idx: usize,
        raw: f32,
        school: DamageSchool,
        events: &mut Vec<Event>,
    ) -> f32 {
        if target_idx >= self.entities.len() || raw <= 0.0 {
            return raw;
        }
        let src_idx = self.index(source_id);
        let mut amount = raw;

        // Apply active damage reduction from buffs/perks.
        let bonus = self.kit_bonus_stats(target_idx);
        if bonus.damage_reduction > 0.0 {
            amount *= (1.0 - bonus.damage_reduction.clamp(0.0, 0.85)).max(0.15);
        }

        // Tony Legendary C: immune to taunted attacker.
        if let Some(si) = src_idx
            && self.entities[si].hero_kit.as_deref().is_some_and(|k| {
                self.tick < k.taunt_until && k.taunt_target == self.entities[target_idx].id
            })
            && self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                .and_then(|t| find_talent(&t.id))
                .is_some_and(|s| s.special == TalentSpecial::TonyImmuneToTaunted)
        {
            return 0.0;
        }

        // Fortified Health absorbs 50% of remaining damage.
        let tick = self.tick;
        if let Some(k) = self.entities[target_idx].hero_kit.as_deref_mut()
            && tick < k.fortified_until
            && k.fortified_health > 0.0
        {
            let saved = k.fortified_health.min(amount * 0.5);
            k.fortified_health -= saved;
            amount -= saved;
        }

        // Record combat timestamp & trigger on_take_damage perk.
        if let Some(k) = self.entities[target_idx].hero_kit.as_deref_mut() {
            k.last_damaged_tick = tick;
            if tick < k.sleep_until && !k.sleep_unwaking {
                k.sleep_until = 0;
            }
        }
        if self.hero_kit_enabled()
            && let Some(spec) = crate::heroes::kits::kit_spec(self.entities[target_idx].type_id)
            && let Some(cb) = spec.perk.on_take_damage
        {
            let mut ctx = HeroKitContext::new(self, events, target_idx);
            let mult = cb(&mut ctx, src_idx, amount, school);
            amount *= mult.clamp(0.0, 2.0);
        }

        // Adagio Arcane Renewal: any damage to enemy burning with Arcane Fire restores energy to Adagio.
        if self.hero_kit_enabled()
            && let Some(mark) = self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.marks.get(&marks::ARCANE_FIRE).copied())
            && self.tick < mark.until
            && let Some(adagio_idx) = self.index(mark.source_id)
            && adagio_idx != target_idx
            && self.entities[adagio_idx].type_id == 244
        {
            let adagio = &mut self.entities[adagio_idx];
            adagio.energy = (adagio.energy + amount * 0.35).min(adagio.max_energy);
        }

        // Churnwalker Futility of Life: heal Churnwalker & share damage with other tethered enemies.
        if self.hero_kit_enabled()
            && let Some(cw_id) = self.entities[target_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER).copied())
                .filter(|m| self.tick < m.until)
                .map(|m| m.source_id)
            && source_id != cw_id
            && let Some(cw_idx) = self.index(cw_id)
            && self.entities[cw_idx].hp > 0.0
        {
            let (heal_ratio, share_ratio) = if self.entities[cw_idx]
                .hero_kit
                .as_deref()
                .and_then(|k| k.talent.as_ref())
                .and_then(|t| find_talent(&t.id))
                .is_some_and(|s| s.special == TalentSpecial::ChurnwalkerHealShareTrade)
            {
                (0.5, 0.15)
            } else {
                (0.25, 0.30)
            };
            self.heal(cw_id, cw_idx, amount * heal_ratio, events);
            let other_chained: Vec<usize> = self
                .entities
                .iter()
                .enumerate()
                .filter_map(|(j, e)| {
                    (j != target_idx
                        && e.hp > 0.0
                        && e.hero_kit
                            .as_deref()
                            .and_then(|k| k.marks.get(&marks::CHURNWALKER_TETHER))
                            .is_some_and(|m| self.tick < m.until && m.source_id == cw_id))
                    .then_some(j)
                })
                .collect();
            for oj in other_chained {
                let team = self.entities[cw_idx].team;
                self.hurt(
                    cw_id,
                    team,
                    oj,
                    amount * share_ratio,
                    DamageSchool::True,
                    events,
                );
            }
        }

        amount.max(0.0)
    }

    /// Placeholder invoked cleanly from `deal_damage(..., triggers_basic=true)` when an ability triggers basic-attack items.
    fn item_stats_trigger_basic_placeholder(&mut self, _caster: usize, _target: usize) -> bool {
        false
    }

    /// Telemetry helper for UI and unit tests: query active alternate form (`0` or `1`).
    pub fn kit_form(&self, actor_id: u32) -> u8 {
        self.index(actor_id)
            .and_then(|i| self.entities[i].hero_kit.as_deref())
            .map_or(0, |k| k.form)
    }

    /// Telemetry helper: query active charges on `slot` (or Caine's Peacemaker ammo clip on slot A).
    pub fn kit_ammo(&self, actor_id: u32, slot: AbilitySlot) -> u8 {
        self.index(actor_id).map_or(0, |i| {
            let e = &self.entities[i];
            if slot == AbilitySlot::A
                && crate::heroes::kits::kit_spec(e.type_id).is_some_and(|s| s.hero_id == "Caine")
            {
                let has_double = e
                    .hero_kit
                    .as_deref()
                    .and_then(|k| k.talent.as_ref())
                    .and_then(|t| find_talent(&t.id))
                    .is_some_and(|s| s.special == TalentSpecial::CaineDoubleBullets);
                let cap = if has_double { 12 } else { 6 };
                return e.hero_kit.as_deref().map_or(cap, |k| {
                    if k.timers[0] == 0 {
                        cap
                    } else {
                        k.counters[0].min(255) as u8
                    }
                });
            }
            self.kit_ammo_charges(i, slot)
        })
    }

    /// Telemetry helper: query current non-energy resource (`Focus`, `Stamina`, `Vengeance`, `Bloodrage`).
    pub fn kit_resource(&self, actor_id: u32) -> f32 {
        self.index(actor_id)
            .map_or(0.0, |i| self.kit_resource_value(i))
    }

    /// Telemetry helper: query active mark stacks on an actor.
    pub fn kit_mark_stacks(&self, actor_id: u32, mark: u8) -> u8 {
        self.index(actor_id)
            .and_then(|i| self.entities[i].hero_kit.as_deref())
            .and_then(|k| k.marks.get(&mark))
            .filter(|m| self.tick < m.until)
            .map_or(0, |m| m.stacks)
    }

    /// Hook called when hero `attacker` releases a basic attack hitting `target_idx`.
    /// Handles empowered basic-attack charges, perk basic-attack callbacks, cleave, and Target Lock.
    pub(crate) fn kit_on_basic_attack(
        &mut self,
        attacker: usize,
        target_idx: usize,
        base_damage: f32,
        was_crit: bool,
        events: &mut Vec<Event>,
    ) {
        if !self.hero_kit_enabled()
            || attacker >= self.entities.len()
            || target_idx >= self.entities.len()
        {
            return;
        }
        let type_id = self.entities[attacker].type_id;
        let target_id = self.entities[target_idx].id;

        let tick = self.tick;
        if let Some(k) = self.entities[attacker].hero_kit.as_deref_mut() {
            k.last_attack_tick = tick;
            if tick >= k.stealth_grace_until {
                k.stealth_until = 0;
            }
            if k.lock_target_id == 0 || k.lock_target_id != target_id || tick >= k.lock_target_until
            {
                k.lock_target_id = target_id;
            }
            k.lock_target_until = tick + TICK_HZ * 4;
        }

        // Consume empowered basic attack charge if active.
        let emp = self.entities[attacker]
            .hero_kit
            .as_deref_mut()
            .and_then(|k| {
                if k.empowered_attacks > 0 && tick < k.empowered_until {
                    k.empowered_attacks -= 1;
                    Some((
                        k.empowered_bonus_damage,
                        k.empowered_is_crystal,
                        k.empowered_stun_sec,
                        k.empowered_slow_pct,
                        k.empowered_slow_sec,
                        k.empowered_knockback,
                    ))
                } else {
                    None
                }
            });
        let mut ctx = HeroKitContext::new(self, events, attacker);
        if let Some((bonus_dmg, is_cp, stun_sec, slow_pct, slow_sec, kb)) = emp {
            if bonus_dmg > 0.0 {
                let sch = if is_cp {
                    DamageSchool::Crystal
                } else {
                    DamageSchool::Weapon
                };
                ctx.deal_damage(target_idx, bonus_dmg, sch, 0.0, false);
            }
            if stun_sec > 0.0 {
                ctx.apply_stun(target_idx, stun_sec);
            }
            if slow_pct > 0.0 && slow_sec > 0.0 {
                ctx.apply_slow(target_idx, slow_pct, slow_sec);
            }
            if kb > 0.0 {
                let pos = ctx.caster_pos();
                ctx.knockback(target_idx, pos, kb, 0.0);
            }
        }

        // Trigger Heroic Perk basic attack hook & built-in cleave/splash.
        if let Some(spec) = crate::heroes::kits::kit_spec(type_id) {
            if let Some(on_aa) = spec.perk.on_basic_attack {
                let (bonus_dmg, school) = on_aa(&mut ctx, target_idx, base_damage, was_crit);
                if bonus_dmg > 0.0 {
                    ctx.deal_damage(target_idx, bonus_dmg, school, 0.0, false);
                }
            }
            if spec.perk.basic_cleave_radius > 0.0 && spec.perk.basic_cleave_fraction > 0.0 {
                let center = ctx.room.entities[target_idx].position;
                let cleave_dmg = base_damage * spec.perk.basic_cleave_fraction;
                let nearby = ctx.enemies_in_radius(center, spec.perk.basic_cleave_radius, false);
                for oj in nearby {
                    if oj != target_idx {
                        let m = if ctx.room.entities[oj].kind == Kind::Hero {
                            1.0
                        } else {
                            0.5
                        };
                        ctx.deal_damage(oj, cleave_dmg * m, DamageSchool::Weapon, 0.0, false);
                    }
                }
            }
        }
    }

    /// Hook called on unit kill or assist to trigger Heroic Perks & kill/assist Talents.
    pub(crate) fn kit_on_kill_assist(
        &mut self,
        actor: usize,
        victim: usize,
        is_killer: bool,
        events: &mut Vec<Event>,
    ) {
        if !self.hero_kit_enabled() || actor >= self.entities.len() || victim >= self.entities.len()
        {
            return;
        }
        let is_hero = self.entities[victim].kind == Kind::Hero;
        let type_id = self.entities[actor].type_id;
        let mut ctx = HeroKitContext::new(self, events, actor);

        if is_hero
            && let Some(t) = ctx.active_talent().cloned()
            && let Some(s) = find_talent(&t.id)
        {
            match s.special {
                TalentSpecial::RingoAnotherRoundKillReset => {
                    ctx.reset_slot_cd(AbilitySlot::C);
                }
                TalentSpecial::AnkaRefreshAllOnKill => {
                    ctx.reset_slot_cd(AbilitySlot::A);
                    ctx.reset_slot_cd(AbilitySlot::B);
                    ctx.reset_slot_cd(AbilitySlot::C);
                }
                TalentSpecial::IshtarKillStackDefenses => {
                    ctx.add_mark(actor, marks::ISHTAR_INDOMITABLE, 1, 10, 3600.0);
                }
                _ => {}
            }
        }

        if let Some(spec) = crate::heroes::kits::kit_spec(type_id)
            && let Some(cb) = spec.perk.on_kill_assist
        {
            cb(&mut ctx, victim, is_killer, is_hero);
        }
    }

    /// Clean temporary HeroKit combat state on death while retaining permanent stacks & talent.
    pub(crate) fn kit_on_death(&mut self, i: usize) {
        if i >= self.entities.len() {
            return;
        }
        let type_id = self.entities[i].type_id;
        let tick = self.tick;
        let Some(k) = self.entities[i].hero_kit.as_deref_mut() else {
            return;
        };
        k.fear_until = 0;
        k.fear_source = 0;
        k.sleep_until = 0;
        k.sleep_unwaking = false;
        k.taunt_until = 0;
        k.taunt_target = 0;
        k.stealth_until = 0;
        k.stealth_grace_until = 0;
        k.stealth_speed = 0.0;
        k.unstoppable_until = 0;
        k.untargetable_until = 0;
        k.fortified_health = 0.0;
        k.fortified_until = 0;
        k.mortal_wound_until = 0;
        k.empowered_attacks = 0;
        k.empowered_until = 0;
        k.recast_until = [0; 3];
        k.recast_target = [0; 3];
        k.step_stage = [0; 3];
        k.buffs.retain(|b| b.until >= tick + TICK_HZ * 600);
        if crate::heroes::kits::kit_spec(type_id).is_some_and(|s| s.hero_id == "Ishtar")
            && let Some(m) = k.marks.get_mut(&marks::ISHTAR_INDOMITABLE)
        {
            m.stacks = m.stacks.saturating_sub(1);
            if m.stacks == 0 {
                k.marks.remove(&marks::ISHTAR_INDOMITABLE);
            }
        }
    }
}
