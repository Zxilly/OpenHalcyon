use super::{
    BuffStatKind, DamageKind, HeroKitContext, KitEffect, KitEffectKind, PrimitiveEffect,
    clamp_map_pos, dir_between, point_to_segment_dist, seconds_to_ticks, slot_from_idx,
};
use crate::combat::persistent_spells::TimedSpell;
use crate::combat::{AbilitySlot, CastTarget};
use crate::ecs::score::DamageSchool;
use crate::{Event, Kind, Position, Room, TICK_HZ};

impl TimedSpell for KitEffect {
    fn expired_before_step(&self, tick: u64) -> bool {
        match &self.kind {
            KitEffectKind::GroundZone { end_tick, .. }
            | KitEffectKind::TrapZone { end_tick, .. }
            | KitEffectKind::SegmentBarrier { end_tick, .. }
            | KitEffectKind::ChannelBeam { end_tick, .. }
            | KitEffectKind::SummonPet { end_tick, .. }
            | KitEffectKind::Tether { end_tick, .. }
            | KitEffectKind::Attachment { end_tick, .. }
            | KitEffectKind::DoT { end_tick, .. } => tick > *end_tick,
            KitEffectKind::DelayedBlast { .. }
            | KitEffectKind::Projectile { .. }
            | KitEffectKind::DashMotion { .. } => false,
        }
    }

    fn begin_impact(&mut self) -> Self {
        self.clone()
    }

    fn advance(&mut self, world: &mut Room<'_>, events: &mut Vec<Event>) -> bool {
        let Some(owner_idx) = world.index(self.owner_id) else {
            return false;
        };
        let tick = world.tick;
        let owner_alive = world.entities[owner_idx].hp > 0.;
        if !owner_alive
            && !matches!(
                self.kind,
                KitEffectKind::DelayedBlast { .. } | KitEffectKind::DoT { .. }
            )
        {
            return false;
        }

        match &mut self.kind {
            KitEffectKind::DelayedBlast {
                ability_slot,
                release_tick,
                radius,
                inner_radius,
                damage,
                outer_mult,
                school,
                stun_ticks,
                root_ticks,
                sleep_ticks,
                sleep_unwaking,
                slow_pct,
                slow_ticks,
                reveal_ticks,
                bonus_mark,
                minion_mult,
                applies_mark,
            } => {
                if tick < *release_tick {
                    return true;
                }
                let center = self.position;
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let hits = ctx.enemies_in_radius(center, *radius, false);
                for j in hits {
                    let dist = ctx.room.entities[j].position.dist(center);
                    let mut raw = *damage;
                    if *inner_radius > 0.0 && dist > *inner_radius {
                        raw *= *outer_mult;
                    }
                    if let Some((mark_id, mult)) = bonus_mark
                        && ctx.mark_stacks(j, *mark_id) > 0
                    {
                        raw *= 1.0 + *mult;
                    }
                    if !matches!(ctx.room.entities[j].kind, Kind::Hero) {
                        raw *= *minion_mult;
                    }
                    let slot = slot_from_idx(*ability_slot);
                    let mult = ctx.talent_dmg_mult(slot);
                    ctx.deal_damage(j, raw * mult, *school, 0.0, false);
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(j, *stun_ticks);
                    }
                    if *root_ticks > 0 {
                        ctx.apply_root_ticks(j, *root_ticks);
                    }
                    if *sleep_ticks > 0 {
                        ctx.apply_sleep_ticks(j, *sleep_ticks, *sleep_unwaking);
                    }
                    if *slow_ticks > 0 && *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                    }
                    if *reveal_ticks > 0 {
                        let until = tick + *reveal_ticks;
                        ctx.room.entities[j].status.reveal_until =
                            ctx.room.entities[j].status.reveal_until.max(until);
                    }
                    if let Some((m, add, dur_ticks)) = applies_mark {
                        ctx.add_mark_ticks(j, *m, *add, 10, *dur_ticks);
                    }
                }
                false
            }
            KitEffectKind::Projectile {
                ability_slot,
                rank: _,
                speed_per_tick,
                radius,
                damage,
                school,
                piercing,
                returning,
                return_to_caster,
                repeat_mult,
                splash_damage,
                splash_radius,
                stun_ticks,
                root_ticks,
                silence_ticks,
                slow_pct,
                slow_ticks,
                minion_mult,
                resets_cd_on_hit,
                applies_mark,
                destroys_projectiles: _,
                hit_actors,
            } => {
                if *returning {
                    self.goal = world.entities[owner_idx].position;
                }
                let prev = self.position;
                let next = prev.advance(self.goal, *speed_per_tick);
                self.position = next;

                let mut hit_targets = Vec::new();
                for (j, e) in world.entities.iter().enumerate() {
                    if e.team == self.team
                        || e.hp <= 0.0
                        || !matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                    {
                        continue;
                    }
                    if hit_actors.contains(&e.id) {
                        continue;
                    }
                    if point_to_segment_dist(e.position, prev, next) <= *radius + 0.55 {
                        hit_targets.push(j);
                    }
                }

                let mut stop_now = false;
                if !hit_targets.is_empty() {
                    let slot = slot_from_idx(*ability_slot);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    let dmg_mult = ctx.talent_dmg_mult(slot);
                    for j in hit_targets {
                        let target_id = ctx.room.entities[j].id;
                        hit_actors.push(target_id);
                        let is_hero = ctx.room.entities[j].kind == Kind::Hero;
                        let mut raw = *damage * dmg_mult;
                        if !is_hero {
                            raw *= *minion_mult;
                        }
                        if hit_actors.len() > 1 {
                            raw *= *repeat_mult;
                        }
                        ctx.deal_damage(j, raw, *school, 0.0, false);
                        if *stun_ticks > 0 && is_hero {
                            ctx.apply_stun_ticks(j, *stun_ticks);
                        }
                        if *root_ticks > 0 {
                            ctx.apply_root_ticks(j, *root_ticks);
                        }
                        if *silence_ticks > 0 {
                            ctx.apply_silence_ticks(j, *silence_ticks);
                        }
                        if *slow_ticks > 0 && *slow_pct > 0.0 {
                            ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                        }
                        if let Some((m, add, dur_ticks)) = applies_mark {
                            ctx.add_mark_ticks(j, *m, *add, 10, *dur_ticks);
                        }
                        if *resets_cd_on_hit && is_hero {
                            ctx.reset_slot_cd(slot);
                        }
                        if *splash_damage > 0.0 {
                            let splash_pos = ctx.room.entities[j].position;
                            let splash_hits =
                                ctx.enemies_in_radius(splash_pos, *splash_radius, false);
                            for sj in splash_hits {
                                if sj != j {
                                    ctx.deal_damage(
                                        sj,
                                        *splash_damage * dmg_mult,
                                        *school,
                                        0.0,
                                        false,
                                    );
                                }
                            }
                        }
                        if !*piercing && is_hero {
                            stop_now = true;
                            break;
                        }
                    }
                }
                if stop_now || next.dist(self.goal) <= 0.05 {
                    if *return_to_caster && !*returning {
                        *returning = true;
                        hit_actors.clear();
                        return true;
                    }
                    return false;
                }
                true
            }
            KitEffectKind::GroundZone {
                ability_slot,
                zone_kind: _,
                radius,
                ticks_interval,
                next_tick,
                end_tick,
                tick_damage,
                tick_heal,
                school,
                slow_pct,
                ally_speed_bonus,
                end_damage,
                end_root_ticks,
                end_stun_ticks,
                drift_dx,
                drift_dy,
                follow_caster,
                ignited,
                minion_mult,
                applies_mark,
            } => {
                if *follow_caster {
                    self.position = world.entities[owner_idx].position;
                } else if *drift_dx != 0.0 || *drift_dy != 0.0 {
                    self.position = clamp_map_pos(Position {
                        x: self.position.x + *drift_dx,
                        y: self.position.y + *drift_dy,
                    });
                }
                let center = self.position;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                if *ally_speed_bonus > 0.0 {
                    let allies = ctx.allies_in_radius(center, *radius, true, true);
                    for a in allies {
                        ctx.add_buff_ticks(a, BuffStatKind::MoveSpeedFlat, *ally_speed_bonus, 3);
                    }
                }
                if *slow_pct > 0.0 {
                    let enemies = ctx.enemies_in_radius(center, *radius, false);
                    for e in enemies {
                        ctx.apply_slow_ticks(e, *slow_pct, 3);
                    }
                }
                if tick >= *next_tick {
                    *next_tick = tick + (*ticks_interval).max(1);
                    let per_tick = (*tick_damage)
                        * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                        * ctx.talent_dmg_mult(slot);
                    if per_tick > 0.0 || *ignited {
                        let eff_dmg = if *ignited && per_tick <= 0.0 {
                            40.0
                        } else {
                            per_tick
                        };
                        let enemies = ctx.enemies_in_radius(center, *radius, false);
                        for e in enemies {
                            let is_hero = ctx.room.entities[e].kind == Kind::Hero;
                            let d = if is_hero {
                                eff_dmg
                            } else {
                                eff_dmg * *minion_mult
                            };
                            ctx.deal_damage(e, d, *school, 0.0, false);
                            if let Some((m, add, dur)) = applies_mark {
                                ctx.add_mark_ticks(e, *m, *add, 10, *dur);
                            }
                        }
                    }
                    if *tick_heal > 0.0 {
                        let heal_amt = (*tick_heal)
                            * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                            * ctx.talent_heal_mult(slot);
                        let allies = ctx.allies_in_radius(center, *radius, true, true);
                        for a in allies {
                            ctx.heal_unit(a, heal_amt);
                        }
                    }
                }
                if tick >= *end_tick {
                    if *end_damage > 0.0 || *end_root_ticks > 0 || *end_stun_ticks > 0 {
                        let enemies = ctx.enemies_in_radius(center, *radius, false);
                        let dmg = *end_damage * ctx.talent_dmg_mult(slot);
                        for e in enemies {
                            if dmg > 0.0 {
                                let m = if ctx.room.entities[e].kind == Kind::Hero {
                                    1.0
                                } else {
                                    *minion_mult
                                };
                                ctx.deal_damage(e, dmg * m, *school, 0.0, false);
                            }
                            if *end_root_ticks > 0 {
                                ctx.apply_root_ticks(e, *end_root_ticks);
                            }
                            if *end_stun_ticks > 0 {
                                ctx.apply_stun_ticks(e, *end_stun_ticks);
                            }
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::TrapZone {
                ability_slot,
                armed_at,
                end_tick,
                trigger_radius,
                blast_radius,
                damage,
                school,
                root_ticks,
                stun_ticks,
                slow_pct,
                reveals,
                detonate_on_caster_damage: _,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if tick < *armed_at {
                    return true;
                }
                let center = self.position;
                let triggered = world.entities.iter().any(|e| {
                    e.team != self.team
                        && e.hp > 0.0
                        && e.kind == Kind::Hero
                        && e.position.dist(center) <= *trigger_radius
                });
                if !triggered {
                    return true;
                }
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let dmg = *damage * ctx.talent_dmg_mult(slot);
                let enemies = ctx.enemies_in_radius(center, *blast_radius, false);
                for e in enemies {
                    ctx.deal_damage(e, dmg, *school, 0.0, false);
                    if *root_ticks > 0 {
                        ctx.apply_root_ticks(e, *root_ticks);
                    }
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(e, *stun_ticks);
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(e, *slow_pct, TICK_HZ * 2);
                    }
                    if *reveals {
                        ctx.room.entities[e].status.reveal_until = ctx.room.entities[e]
                            .status
                            .reveal_until
                            .max(tick + TICK_HZ * 3);
                    }
                }
                false
            }
            KitEffectKind::SegmentBarrier {
                ability_slot,
                armed_at,
                end_tick,
                end_pos,
                radius,
                is_ring_arena,
                dps_or_hit_damage,
                school,
                stun_ticks,
                silence_ticks,
                slow_pct,
                next_tick,
                struck_recently,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if *is_ring_arena
                    && world.entities[owner_idx].position.dist(self.position) > *radius + 0.5
                {
                    // Ardan leaves Gauntlet perimeter -> barrier immediately collapses.
                    return false;
                }
                if tick < *armed_at {
                    return true;
                }
                struck_recently.retain(|(_, until)| *until > tick);
                let start_pos = self.position;
                let end_p = *end_pos;
                let rad = *radius;
                let ring = *is_ring_arena;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let mut crossed = Vec::new();
                for (j, e) in ctx.room.entities.iter().enumerate() {
                    if e.team == self.team || e.hp <= 0.0 || e.kind != Kind::Hero {
                        continue;
                    }
                    if struck_recently.iter().any(|(id, _)| *id == e.id) {
                        continue;
                    }
                    let hit = if ring {
                        let d = e.position.dist(start_pos);
                        (d - rad).abs() <= 0.9
                    } else {
                        point_to_segment_dist(e.position, start_pos, end_p) <= 0.9
                    };
                    if hit {
                        crossed.push(j);
                    }
                }
                let do_dmg_tick = tick >= *next_tick;
                if do_dmg_tick {
                    *next_tick = tick + TICK_HZ / 2;
                }
                for j in crossed {
                    let eid = ctx.room.entities[j].id;
                    if *dps_or_hit_damage > 0.0 && (*stun_ticks > 0 || do_dmg_tick) {
                        let dmg = *dps_or_hit_damage * ctx.talent_dmg_mult(slot);
                        ctx.deal_damage(j, dmg, *school, 0.0, false);
                    }
                    if *stun_ticks > 0 {
                        ctx.apply_stun_ticks(j, *stun_ticks);
                        struck_recently.push((eid, tick + TICK_HZ * 2));
                    }
                    if *silence_ticks > 0 {
                        ctx.apply_silence_ticks(j, *silence_ticks);
                        struck_recently.push((eid, tick + TICK_HZ));
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, TICK_HZ);
                    }
                }
                true
            }
            KitEffectKind::DashMotion {
                ability_slot,
                start,
                start_tick,
                end_tick,
                invuln,
                unstoppable,
                path_radius,
                path_damage,
                arrival_radius,
                arrival_damage,
                school,
                slow_pct,
                slow_ticks,
                stun_ticks,
                knockback_dist,
                applies_mark,
                hit_actors,
            } => {
                let total = (*end_tick).saturating_sub(*start_tick).max(1);
                let elapsed = tick.saturating_sub(*start_tick).min(total);
                let frac = elapsed as f32 / total as f32;
                let target_pos = Position {
                    x: start.x + (self.goal.x - start.x) * frac,
                    y: start.y + (self.goal.y - start.y) * frac,
                };
                if world.spell_motion_clear(world.entities[owner_idx].position, target_pos) {
                    world.place_spell_actor(owner_idx, target_pos, events);
                }
                let cur_pos = world.entities[owner_idx].position;
                let slot = slot_from_idx(*ability_slot);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                if *invuln {
                    ctx.set_untargetable(owner_idx, 0.15);
                }
                if *unstoppable {
                    ctx.set_unstoppable(owner_idx, 0.15);
                }
                if *path_damage > 0.0 {
                    let mult = ctx.talent_dmg_mult(slot);
                    let enemies = ctx.enemies_in_radius(cur_pos, *path_radius, false);
                    for j in enemies {
                        let id = ctx.room.entities[j].id;
                        if !hit_actors.contains(&id) {
                            hit_actors.push(id);
                            ctx.deal_damage(j, *path_damage * mult, *school, 0.0, false);
                            if *slow_ticks > 0 && *slow_pct > 0.0 {
                                ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                            }
                            if let Some((m, add, dur)) = applies_mark {
                                ctx.add_mark_ticks(j, *m, *add, 10, *dur);
                            }
                        }
                    }
                }
                if tick >= *end_tick {
                    if *arrival_damage > 0.0 || *stun_ticks > 0 || *knockback_dist > 0.0 {
                        let mult = ctx.talent_dmg_mult(slot);
                        let enemies = ctx.enemies_in_radius(cur_pos, *arrival_radius, false);
                        for j in enemies {
                            let id = ctx.room.entities[j].id;
                            if *arrival_damage > 0.0 && !hit_actors.contains(&id) {
                                hit_actors.push(id);
                                ctx.deal_damage(j, *arrival_damage * mult, *school, 0.0, false);
                            }
                            if *stun_ticks > 0 && ctx.room.entities[j].kind == Kind::Hero {
                                ctx.apply_stun_ticks(j, *stun_ticks);
                            }
                            if *slow_ticks > 0 && *slow_pct > 0.0 {
                                ctx.apply_slow_ticks(j, *slow_pct, *slow_ticks);
                            }
                            if *knockback_dist > 0.0 {
                                ctx.knockback(j, cur_pos, *knockback_dist, 0.0);
                            }
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::ChannelBeam {
                ability_slot,
                target_id,
                ticks_interval,
                next_tick,
                end_tick,
                range,
                width,
                tick_damage,
                tick_heal,
                school,
                lock_caster,
                slow_pct,
                stun_target,
                global_all_heroes,
                shield_pierce,
                structure_mult,
                minion_mult,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                let slot = slot_from_idx(*ability_slot);
                if *lock_caster {
                    world.entities[owner_idx].order = crate::Command::Stop;
                }
                if tick < *next_tick {
                    return true;
                }
                *next_tick = tick + (*ticks_interval).max(1);
                let origin = world.entities[owner_idx].position;
                let end_pt = origin.advance(self.goal, *range);
                let mut ctx = HeroKitContext::new(world, events, owner_idx);
                let dmg_mult = ctx.talent_dmg_mult(slot);
                let per_tick_dmg =
                    (*tick_damage) * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32) * dmg_mult;

                if *target_id != 0 {
                    if let Some(tj) = ctx.room.index(*target_id) {
                        if *tick_heal > 0.0 {
                            let heal_amt = (*tick_heal)
                                * ((*ticks_interval).max(1) as f32 / TICK_HZ as f32)
                                * ctx.talent_heal_mult(slot);
                            ctx.heal_unit(tj, heal_amt);
                        }
                        if per_tick_dmg > 0.0 {
                            ctx.deal_damage(tj, per_tick_dmg, *school, *shield_pierce, false);
                        }
                        if *stun_target {
                            if ctx.room.entities[tj].status.blocks_debuffs(tick) {
                                // Koshka C cancels if stun blocked by Reflex Block.
                                return false;
                            }
                            ctx.apply_stun_ticks(tj, (*ticks_interval).max(1) + 1);
                        }
                    } else {
                        return false;
                    }
                    return true;
                }

                if *global_all_heroes {
                    let targets: Vec<usize> = ctx
                        .room
                        .entities
                        .iter()
                        .enumerate()
                        .filter_map(|(j, e)| {
                            (e.team != self.team && e.hp > 0.0 && e.kind == Kind::Hero).then_some(j)
                        })
                        .collect();
                    for j in targets {
                        ctx.deal_damage(j, per_tick_dmg, *school, *shield_pierce, false);
                    }
                    return true;
                }

                let mut hits = Vec::new();
                for (j, e) in ctx.room.entities.iter().enumerate() {
                    if e.team == self.team || e.hp <= 0.0 {
                        continue;
                    }
                    if point_to_segment_dist(e.position, origin, end_pt) <= *width {
                        hits.push(j);
                    }
                }
                for j in hits {
                    let mult = match ctx.room.entities[j].kind {
                        Kind::Hero => 1.0,
                        Kind::Tower | Kind::Crystal => *structure_mult,
                        _ => *minion_mult,
                    };
                    if per_tick_dmg > 0.0 {
                        ctx.deal_damage(j, per_tick_dmg * mult, *school, *shield_pierce, false);
                    }
                    if *slow_pct > 0.0 {
                        ctx.apply_slow_ticks(j, *slow_pct, (*ticks_interval).max(1) + 2);
                    }
                }
                true
            }
            KitEffectKind::SummonPet {
                ability_slot: _,
                pet_kind: _,
                hp: _,
                hit_ticks_remaining,
                damage,
                school,
                attack_interval,
                next_attack,
                end_tick,
                applies_mark,
                locked_target,
            } => {
                if tick >= *end_tick || *hit_ticks_remaining == 0 {
                    return false;
                }
                let owner_pos = world.entities[owner_idx].position;
                if self.position.dist(owner_pos) > 22.0 {
                    // Pets despawn when straying too far from master.
                    return false;
                }
                let target_idx = if *locked_target != 0 {
                    world
                        .index(*locked_target)
                        .filter(|&j| world.entities[j].hp > 0.0)
                } else {
                    None
                }
                .or_else(|| {
                    world
                        .entities
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            e.team != self.team
                                && e.hp > 0.0
                                && matches!(e.kind, Kind::Hero | Kind::Minion | Kind::Neutral)
                        })
                        .min_by(|(_, a), (_, b)| {
                            let ha = if a.kind == Kind::Hero { 0 } else { 1 };
                            let hb = if b.kind == Kind::Hero { 0 } else { 1 };
                            ha.cmp(&hb).then_with(|| {
                                a.position
                                    .dist(self.position)
                                    .total_cmp(&b.position.dist(self.position))
                            })
                        })
                        .map(|(j, _)| j)
                });
                if let Some(tj) = target_idx {
                    *locked_target = world.entities[tj].id;
                    let tpos = world.entities[tj].position;
                    self.position = self.position.advance(tpos, 0.35);
                    if self.position.dist(tpos) <= 2.2 && tick >= *next_attack {
                        *next_attack = tick + (*attack_interval).max(4);
                        let mut ctx = HeroKitContext::new(world, events, owner_idx);
                        ctx.deal_damage(tj, *damage, *school, 0.0, false);
                        if let Some((m, add, dur)) = applies_mark {
                            ctx.add_mark_ticks(tj, *m, *add, 10, *dur);
                        }
                    }
                } else {
                    self.position = self.position.advance(owner_pos, 0.3);
                }
                true
            }
            KitEffectKind::Tether {
                ability_slot: _,
                target_id,
                anchor_pos,
                break_range,
                dps_pct_hp,
                break_damage,
                break_stun_ticks,
                anchor_follows_caster,
                next_tick,
                end_tick,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                let Some(tj) = world.index(*target_id) else {
                    return false;
                };
                if world.entities[tj].hp <= 0.0 {
                    return false;
                }
                if *anchor_follows_caster {
                    *anchor_pos = world.entities[owner_idx].position;
                }
                let dist = world.entities[tj].position.dist(*anchor_pos);
                if dist > *break_range {
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    if *break_damage > 0.0 {
                        ctx.deal_damage(tj, *break_damage, DamageSchool::Crystal, 0.0, false);
                    }
                    if *break_stun_ticks > 0 {
                        ctx.apply_stun_ticks(tj, *break_stun_ticks);
                    }
                    return false;
                }
                if *dps_pct_hp > 0.0 && tick >= *next_tick {
                    *next_tick = tick + TICK_HZ / 2;
                    let max_hp = world.entities[tj].max_hp;
                    let dmg = max_hp * *dps_pct_hp * 0.5;
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(tj, dmg, DamageSchool::Crystal, 0.0, false);
                }
                true
            }
            KitEffectKind::Attachment {
                ability_slot: _,
                host_id,
                is_devour,
                dps,
                end_damage,
                end_radius,
                school,
                next_tick,
                end_tick,
            } => {
                let Some(hj) = world.index(*host_id) else {
                    return false;
                };
                if world.entities[hj].hp <= 0.0 {
                    return false;
                }
                if *is_devour {
                    let belly_pos = world.entities[owner_idx].position;
                    world.place_spell_actor(hj, belly_pos, events);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.apply_stun_ticks(hj, 2);
                    ctx.apply_silence_ticks(hj, 2);
                } else {
                    let host_pos = world.entities[hj].position;
                    world.place_spell_actor(owner_idx, host_pos, events);
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.set_untargetable(owner_idx, 0.15);
                }
                if *dps > 0.0 && tick >= *next_tick {
                    *next_tick = tick + TICK_HZ / 2;
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(hj, *dps * 0.5, *school, 0.0, false);
                }
                if tick >= *end_tick {
                    if *end_damage > 0.0 {
                        let center = world.entities[owner_idx].position;
                        let mut ctx = HeroKitContext::new(world, events, owner_idx);
                        for j in ctx.enemies_in_radius(center, (*end_radius).max(1.5), false) {
                            ctx.deal_damage(j, *end_damage, *school, 0.0, false);
                        }
                    }
                    return false;
                }
                true
            }
            KitEffectKind::DoT {
                ability_slot: _,
                target_id,
                tick_damage,
                school,
                ticks_interval,
                next_tick,
                end_tick,
                minion_mult,
            } => {
                if tick >= *end_tick {
                    return false;
                }
                if tick < *next_tick {
                    return true;
                }
                *next_tick = tick + (*ticks_interval).max(1);
                if let Some(tj) = world.index(*target_id)
                    && world.entities[tj].hp > 0.0
                {
                    let m = if world.entities[tj].kind == Kind::Hero {
                        1.0
                    } else {
                        *minion_mult
                    };
                    let mut ctx = HeroKitContext::new(world, events, owner_idx);
                    ctx.deal_damage(tj, *tick_damage * m, *school, 0.0, false);
                    return true;
                }
                false
            }
        }
    }
}

impl<'a, 'w> HeroKitContext<'a, 'w> {
    /// Execute a slice of declarative `PrimitiveEffect`s for `ability_id`.
    pub(crate) fn exec_primitives(
        &mut self,
        ability_id: &str,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        effects: &[PrimitiveEffect],
    ) {
        for &effect in effects {
            self.exec_one_primitive(ability_id, slot, rank, target, effect);
        }
    }

    fn exec_one_primitive(
        &mut self,
        ability_id: &str,
        slot: AbilitySlot,
        rank: u8,
        target: CastTarget,
        effect: PrimitiveEffect,
    ) {
        let dmg_mult = self.talent_dmg_mult(slot);
        let barrier_mult = self.talent_barrier_mult(slot);
        let heal_mult = self.talent_heal_mult(slot);
        let origin = self.caster_pos();
        match effect {
            PrimitiveEffect::TargetedStrike {
                damage,
                school,
                bonus_if_marked,
                triggers_basic_effects,
                resets_attack_cd,
                stun_seconds,
                slow_pct,
                slow_seconds,
            } => {
                if resets_attack_cd {
                    self.reset_attack_cd();
                }
                if let Some(tj) = self.resolve_target_idx(target) {
                    let mut raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                    if let Some((mk, extra)) = bonus_if_marked
                        && self.mark_stacks(tj, mk.id()) > 0
                    {
                        raw += self.eval_var(ability_id, rank, extra) * dmg_mult;
                    }
                    let actual_raw = match school {
                        DamageKind::PercentMaxHp => self.room.entities[tj].max_hp * raw,
                        DamageKind::PercentMissingHp => {
                            (self.room.entities[tj].max_hp - self.room.entities[tj].hp).max(0.0)
                                * raw
                        }
                        DamageKind::PercentCurrentHp => self.room.entities[tj].hp * raw,
                        _ => raw,
                    };
                    self.deal_damage(tj, actual_raw, school.school(), 0.0, triggers_basic_effects);
                    let stun = self.eval_var(ability_id, rank, stun_seconds);
                    if stun > 0.0 {
                        self.apply_stun(tj, stun);
                    }
                    let slow = self.eval_var(ability_id, rank, slow_pct);
                    let slow_sec = self.eval_var(ability_id, rank, slow_seconds);
                    if slow > 0.0 && slow_sec > 0.0 {
                        self.apply_slow(tj, slow, slow_sec);
                    }
                }
            }
            PrimitiveEffect::SkillshotProjectile {
                damage,
                school,
                range,
                speed,
                radius,
                piercing,
                return_to_caster,
                repeat_hit_mult,
                splash_damage,
                splash_radius,
                stun_seconds,
                root_seconds,
                silence_seconds,
                slow_pct,
                slow_seconds,
                minion_mult,
                resets_cast_cd_on_hit,
                applies_mark,
                destroys_enemy_projectiles,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let goal = self.resolve_target_pos(target, r);
                let (dx, dy) = dir_between(origin, goal);
                let end = clamp_map_pos(Position {
                    x: origin.x + dx * r,
                    y: origin.y + dy * r,
                });
                let spd = self.eval_var(ability_id, rank, speed).max(5.0) / TICK_HZ as f32;
                let rad = self.eval_var(ability_id, rank, radius).max(0.6);
                let raw_dmg = self.eval_var(ability_id, rank, damage);
                let splash = splash_damage.map_or(0.0, |s| self.eval_var(ability_id, rank, s));
                let stun_t = seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds));
                let root_t = seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds));
                let sil_t = seconds_to_ticks(self.eval_var(ability_id, rank, silence_seconds));
                let slow_p = self.eval_var(ability_id, rank, slow_pct);
                let slow_t = seconds_to_ticks(self.eval_var(ability_id, rank, slow_seconds));
                let mark = applies_mark.map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec)));
                self.spawn_effect(
                    KitEffectKind::Projectile {
                        ability_slot: slot.idx() as u8,
                        rank,
                        speed_per_tick: spd,
                        radius: rad,
                        damage: raw_dmg,
                        school: school.school(),
                        piercing,
                        returning: false,
                        return_to_caster,
                        repeat_mult: repeat_hit_mult,
                        splash_damage: splash,
                        splash_radius,
                        stun_ticks: stun_t,
                        root_ticks: root_t,
                        silence_ticks: sil_t,
                        slow_pct: slow_p,
                        slow_ticks: slow_t,
                        minion_mult,
                        resets_cd_on_hit: resets_cast_cd_on_hit,
                        applies_mark: mark,
                        destroys_projectiles: destroys_enemy_projectiles,
                        hit_actors: Vec::new(),
                    },
                    origin,
                    end,
                );
            }
            PrimitiveEffect::GroundAoE {
                radius,
                duration,
                ticks_per_sec,
                dps,
                hps,
                school,
                slow_pct,
                end_damage,
                end_root_seconds,
                end_stun_seconds,
                drifts_toward_aim,
                minion_mult,
                applies_mark,
                zone_kind,
            } => {
                let rad = self.eval_var(ability_id, rank, radius).max(1.0);
                let dur = self.eval_var(ability_id, rank, duration).max(0.2);
                let center = self.resolve_target_pos(target, 20.0);
                let interval = (TICK_HZ as f32 / ticks_per_sec.max(1.0)).round().max(1.0) as u64;
                let (drift_dx, drift_dy) = if drifts_toward_aim {
                    let (dx, dy) = dir_between(origin, center);
                    (dx * 0.1, dy * 0.1)
                } else {
                    (0.0, 0.0)
                };
                self.spawn_effect(
                    KitEffectKind::GroundZone {
                        ability_slot: slot.idx() as u8,
                        zone_kind,
                        radius: rad,
                        ticks_interval: interval,
                        next_tick: self.tick,
                        end_tick: self.tick + seconds_to_ticks(dur),
                        tick_damage: self.eval_var(ability_id, rank, dps),
                        tick_heal: self.eval_var(ability_id, rank, hps),
                        school: school.school(),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        ally_speed_bonus: 0.0,
                        end_damage: end_damage.map_or(0.0, |v| self.eval_var(ability_id, rank, v)),
                        end_root_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            end_root_seconds,
                        )),
                        end_stun_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            end_stun_seconds,
                        )),
                        drift_dx,
                        drift_dy,
                        follow_caster: false,
                        ignited: false,
                        minion_mult,
                        applies_mark: applies_mark
                            .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::DelayedBlast {
                delay,
                radius,
                inner_radius,
                damage,
                outer_mult,
                school,
                stun_seconds,
                root_seconds,
                sleep_seconds,
                slow_pct,
                slow_seconds,
                reveal_seconds,
                bonus_if_marked,
                minion_mult,
                applies_mark,
            } => {
                let d_ticks = seconds_to_ticks(self.eval_var(ability_id, rank, delay));
                let center = self.resolve_target_pos(target, 200.0);
                self.spawn_effect(
                    KitEffectKind::DelayedBlast {
                        ability_slot: slot.idx() as u8,
                        release_tick: self.tick + d_ticks,
                        radius: self.eval_var(ability_id, rank, radius).max(1.0),
                        inner_radius,
                        damage: self.eval_var(ability_id, rank, damage),
                        outer_mult,
                        school: school.school(),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        root_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds)),
                        sleep_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            sleep_seconds,
                        )),
                        sleep_unwaking: false,
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        slow_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, slow_seconds)),
                        reveal_ticks: seconds_to_ticks(reveal_seconds),
                        bonus_mark: bonus_if_marked.map(|(m, mult)| (m.id(), mult)),
                        minion_mult,
                        applies_mark: applies_mark
                            .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::ConeWave {
                range,
                half_angle_deg,
                damage,
                school,
                multi_bolts,
                extra_hit_mult,
                slow_pct,
                slow_seconds,
                stun_seconds,
                fear_seconds,
                silence_seconds,
                knockback_dist,
                reveal_seconds,
                minion_mult,
                applies_mark,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let aim = self.resolve_target_pos(target, r);
                let (ax, ay) = dir_between(origin, aim);
                let cos_thresh = half_angle_deg.to_radians().cos();
                let raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                let total_mult = if multi_bolts > 1 {
                    1.0 + f32::from(multi_bolts - 1) * extra_hit_mult
                } else {
                    1.0
                };
                let hits = self.enemies_in_radius(origin, r, false);
                let stun = self.eval_var(ability_id, rank, stun_seconds);
                let fear = self.eval_var(ability_id, rank, fear_seconds);
                let sil = self.eval_var(ability_id, rank, silence_seconds);
                let slow_p = self.eval_var(ability_id, rank, slow_pct);
                let slow_s = self.eval_var(ability_id, rank, slow_seconds);
                for j in hits {
                    let tpos = self.room.entities[j].position;
                    let (tx, ty) = dir_between(origin, tpos);
                    if origin.dist(tpos) > 0.6 && (ax * tx + ay * ty) < cos_thresh {
                        continue;
                    }
                    let is_hero = self.room.entities[j].kind == Kind::Hero;
                    let mult = if is_hero { 1.0 } else { minion_mult };
                    self.deal_damage(j, raw * total_mult * mult, school.school(), 0.0, false);
                    if stun > 0.0 && is_hero {
                        self.apply_stun(j, stun);
                    }
                    if fear > 0.0 && is_hero {
                        self.apply_fear(j, fear);
                    }
                    if sil > 0.0 {
                        self.apply_silence(j, sil);
                    }
                    if slow_p > 0.0 && slow_s > 0.0 {
                        self.apply_slow(j, slow_p, slow_s);
                    }
                    if knockback_dist > 0.0 && is_hero {
                        self.knockback(j, origin, knockback_dist, 0.0);
                    }
                    if reveal_seconds > 0.0 {
                        self.apply_reveal(j, reveal_seconds);
                    }
                    if let Some((m, add, sec)) = applies_mark {
                        self.add_mark(j, m.id(), add, 10, sec);
                    }
                }
            }
            PrimitiveEffect::DashBlink {
                range,
                duration,
                instant_blink,
                invuln_during,
                unstoppable_during,
                path_radius,
                path_damage,
                arrival_radius,
                arrival_damage,
                school,
                slow_pct,
                slow_seconds,
                stun_seconds,
                knockback_dist,
                backward_hop,
                applies_mark,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let raw_goal = self.resolve_target_pos(target, r);
                let goal = if backward_hop {
                    let (dx, dy) = dir_between(origin, raw_goal);
                    clamp_map_pos(Position {
                        x: origin.x - dx * r,
                        y: origin.y - dy * r,
                    })
                } else {
                    raw_goal
                };
                let dur = self.eval_var(ability_id, rank, duration);
                if instant_blink || dur <= 0.05 {
                    if self.room.spell_motion_clear(origin, goal) {
                        self.room.place_spell_actor(self.caster, goal, self.events);
                    }
                    let arr_dmg = self.eval_var(ability_id, rank, arrival_damage) * dmg_mult;
                    let stun = self.eval_var(ability_id, rank, stun_seconds);
                    let slow_p = self.eval_var(ability_id, rank, slow_pct);
                    let slow_s = self.eval_var(ability_id, rank, slow_seconds);
                    if arr_dmg > 0.0 || stun > 0.0 || knockback_dist > 0.0 {
                        let center = self.caster_pos();
                        for j in self.enemies_in_radius(center, arrival_radius.max(1.5), false) {
                            if arr_dmg > 0.0 {
                                self.deal_damage(j, arr_dmg, school.school(), 0.0, false);
                            }
                            if stun > 0.0 {
                                self.apply_stun(j, stun);
                            }
                            if slow_p > 0.0 && slow_s > 0.0 {
                                self.apply_slow(j, slow_p, slow_s);
                            }
                            if knockback_dist > 0.0 {
                                self.knockback(j, center, knockback_dist, 0.0);
                            }
                            if let Some((m, add, sec)) = applies_mark {
                                self.add_mark(j, m.id(), add, 10, sec);
                            }
                        }
                    }
                } else {
                    let dur_ticks = seconds_to_ticks(dur).max(1);
                    self.spawn_effect(
                        KitEffectKind::DashMotion {
                            ability_slot: slot.idx() as u8,
                            start: origin,
                            start_tick: self.tick,
                            end_tick: self.tick + dur_ticks,
                            invuln: invuln_during,
                            unstoppable: unstoppable_during,
                            path_radius: path_radius.max(1.2),
                            path_damage: self.eval_var(ability_id, rank, path_damage),
                            arrival_radius: arrival_radius.max(1.5),
                            arrival_damage: self.eval_var(ability_id, rank, arrival_damage),
                            school: school.school(),
                            slow_pct: self.eval_var(ability_id, rank, slow_pct),
                            slow_ticks: seconds_to_ticks(self.eval_var(
                                ability_id,
                                rank,
                                slow_seconds,
                            )),
                            stun_ticks: seconds_to_ticks(self.eval_var(
                                ability_id,
                                rank,
                                stun_seconds,
                            )),
                            knockback_dist,
                            applies_mark: applies_mark
                                .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                            hit_actors: Vec::new(),
                        },
                        origin,
                        goal,
                    );
                }
            }
            PrimitiveEffect::LungeHook {
                range,
                damage,
                school,
                pull_distance,
                knockback_distance,
                stun_on_wall_seconds,
                tether_duration,
                tether_break_range,
                tether_dps_pct_hp,
                devour_seconds,
                suppress_untargetable,
            } => {
                let r = self.eval_var(ability_id, rank, range).max(1.5);
                let aim = self.resolve_target_pos(target, r);
                let target_idx = self
                    .resolve_target_idx(target)
                    .or_else(|| self.enemies_in_radius(aim, 2.5, true).into_iter().next());
                if let Some(tj) = target_idx {
                    let raw = self.eval_var(ability_id, rank, damage) * dmg_mult;
                    if raw > 0.0 {
                        self.deal_damage(tj, raw, school.school(), 0.0, false);
                    }
                    let pull = self.eval_var(ability_id, rank, pull_distance);
                    if pull > 0.0 {
                        self.pull_toward(tj, origin, pull);
                    }
                    let kb = self.eval_var(ability_id, rank, knockback_distance);
                    let wall_stun = self.eval_var(ability_id, rank, stun_on_wall_seconds);
                    if kb > 0.0 {
                        self.knockback(tj, origin, kb, wall_stun);
                    }
                    let tether_dur = self.eval_var(ability_id, rank, tether_duration);
                    if tether_dur > 0.0 {
                        let host_id = self.room.entities[tj].id;
                        let anchor = self.room.entities[tj].position;
                        self.spawn_effect(
                            KitEffectKind::Tether {
                                ability_slot: slot.idx() as u8,
                                target_id: host_id,
                                anchor_pos: anchor,
                                break_range: self
                                    .eval_var(ability_id, rank, tether_break_range)
                                    .max(3.5),
                                dps_pct_hp: self.eval_var(ability_id, rank, tether_dps_pct_hp),
                                break_damage: 0.0,
                                break_stun_ticks: seconds_to_ticks(wall_stun),
                                anchor_follows_caster: false,
                                next_tick: self.tick + TICK_HZ / 2,
                                end_tick: self.tick + seconds_to_ticks(tether_dur),
                            },
                            anchor,
                            anchor,
                        );
                    }
                    let dev_sec = self.eval_var(ability_id, rank, devour_seconds);
                    if dev_sec > 0.0 || suppress_untargetable {
                        let host_id = self.room.entities[tj].id;
                        let is_enemy = self.room.entities[tj].team != self.caster_team();
                        let pos = self.room.entities[tj].position;
                        self.spawn_effect(
                            KitEffectKind::Attachment {
                                ability_slot: slot.idx() as u8,
                                host_id,
                                is_devour: is_enemy,
                                dps: 0.0,
                                end_damage: 0.0,
                                end_radius: 3.0,
                                school: school.school(),
                                next_tick: self.tick + TICK_HZ / 2,
                                end_tick: self.tick + seconds_to_ticks(dev_sec.max(1.5)),
                            },
                            pos,
                            pos,
                        );
                    }
                }
            }
            PrimitiveEffect::SelfBuff {
                stat,
                amount,
                duration,
                empowered_attacks,
                empowered_bonus_damage,
                empowered_stun_seconds,
                empowered_slow_pct,
                empowered_slow_seconds,
                empowered_knockback,
                resets_attack_cd,
            } => {
                if resets_attack_cd {
                    self.reset_attack_cd();
                }
                let amt = self.eval_var(ability_id, rank, amount);
                let dur = self.eval_var(ability_id, rank, duration).max(0.5);
                if amt != 0.0 {
                    self.add_buff(self.caster, stat, amt, dur);
                }
                if empowered_attacks > 0 {
                    let bonus = self.eval_var(ability_id, rank, empowered_bonus_damage) * dmg_mult;
                    self.empower_next_attacks(
                        empowered_attacks,
                        bonus,
                        false,
                        empowered_stun_seconds,
                        empowered_slow_pct,
                        empowered_slow_seconds,
                        empowered_knockback,
                        dur,
                    );
                }
            }
            PrimitiveEffect::AllyHealBarrier {
                radius,
                include_self,
                target_only,
                burst_heal,
                hot_per_sec,
                hot_duration,
                barrier,
                barrier_duration,
                fortified,
                fortified_duration,
                self_cast_mult,
                grants_attack_buff_stacks,
                grants_attack_bonus_dmg,
            } => {
                let rad = self.eval_var(ability_id, rank, radius);
                let center = self.resolve_target_pos(target, rad.max(10.0));
                let targets = if target_only {
                    self.resolve_target_idx(target)
                        .filter(|&j| self.room.entities[j].team == self.caster_team())
                        .into_iter()
                        .collect()
                } else {
                    self.allies_in_radius(center, rad.max(1.0), include_self, true)
                };
                let bh = self.eval_var(ability_id, rank, burst_heal) * heal_mult;
                let hot_sec = self.eval_var(ability_id, rank, hot_duration);
                let hot = self.eval_var(ability_id, rank, hot_per_sec) * hot_sec * heal_mult;
                let bar = self.eval_var(ability_id, rank, barrier) * barrier_mult;
                let bar_dur = self.eval_var(ability_id, rank, barrier_duration).max(1.0);
                let fort = self.eval_var(ability_id, rank, fortified);
                let fort_dur = self.eval_var(ability_id, rank, fortified_duration).max(1.0);
                let grant_bonus_dmg = if grants_attack_buff_stacks > 0 {
                    self.eval_var(ability_id, rank, grants_attack_bonus_dmg)
                } else {
                    0.0
                };
                for j in targets {
                    let sm = if j == self.caster && self_cast_mult > 0.0 {
                        self_cast_mult
                    } else {
                        1.0
                    };
                    if bh + hot > 0.0 {
                        self.heal_unit(j, (bh + hot) * sm);
                    }
                    if bar > 0.0 {
                        self.grant_barrier(j, bar * sm, bar_dur, None);
                    }
                    if fort > 0.0 {
                        self.grant_fortified(j, fort * sm, fort_dur);
                    }
                    if grants_attack_buff_stacks > 0 {
                        let k = self.room.entities[j]
                            .hero_kit
                            .get_or_insert_with(Box::default);
                        k.empowered_attacks = grants_attack_buff_stacks;
                        k.empowered_until = self.tick + seconds_to_ticks(bar_dur);
                        k.empowered_bonus_damage = grant_bonus_dmg;
                        k.empowered_is_crystal = true;
                    }
                }
            }
            PrimitiveEffect::ChannelBeam {
                duration,
                ticks_per_sec,
                range,
                width,
                dps,
                school,
                lock_caster,
                slow_pct,
                stun_target,
                global_all_heroes,
                shield_pierce,
                structure_mult,
                minion_mult,
            } => {
                let dur = self.eval_var(ability_id, rank, duration).max(0.2);
                let r = self.eval_var(ability_id, rank, range).max(1.0);
                let aim = self.resolve_target_pos(target, r);
                let target_id = if let CastTarget::Entity(id) = target {
                    if stun_target { id } else { 0 }
                } else {
                    0
                };
                let interval = (TICK_HZ as f32 / ticks_per_sec.max(1.0)).round().max(1.0) as u64;
                self.spawn_effect(
                    KitEffectKind::ChannelBeam {
                        ability_slot: slot.idx() as u8,
                        target_id,
                        ticks_interval: interval,
                        next_tick: self.tick,
                        end_tick: self.tick + seconds_to_ticks(dur),
                        range: r,
                        width,
                        tick_damage: self.eval_var(ability_id, rank, dps),
                        tick_heal: 0.0,
                        school: school.school(),
                        lock_caster,
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        stun_target,
                        global_all_heroes,
                        shield_pierce: self.eval_var(ability_id, rank, shield_pierce),
                        structure_mult,
                        minion_mult,
                    },
                    origin,
                    aim,
                );
            }
            PrimitiveEffect::SummonPet {
                count,
                max_active,
                duration,
                pet_hp,
                pet_damage,
                school,
                hit_ticks,
                applies_mark,
                replace_existing,
                pet_kind,
            } => {
                let owner_id = self.caster_id();
                if replace_existing {
                    let to_remove: Vec<_> = self
                        .room
                        .entities
                        .ecs
                        .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
                        .iter(&self.room.entities.ecs)
                        .filter_map(|(eh, eff)| {
                            (eff.owner_id == owner_id
                                && matches!(
                                    eff.kind,
                                    KitEffectKind::SummonPet { pet_kind: pk, .. } if pk == pet_kind
                                ))
                            .then_some(eh)
                        })
                        .collect();
                    for eh in to_remove {
                        self.room.entities.remove_spell_handle::<KitEffect>(eh);
                    }
                }
                let active_count = self
                    .room
                    .entities
                    .spells::<KitEffect>()
                    .filter(|eff| {
                        eff.owner_id == owner_id
                            && matches!(
                                eff.kind,
                                KitEffectKind::SummonPet { pet_kind: pk, .. } if pk == pet_kind
                            )
                    })
                    .count();
                let allowed = usize::from(max_active).saturating_sub(active_count);
                let spawn_n = usize::from(count).min(allowed);
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let hp = self.eval_var(ability_id, rank, pet_hp).max(100.0);
                let dmg = self.eval_var(ability_id, rank, pet_damage);
                for offset in 0..spawn_n {
                    let spawn_pos = Position {
                        x: origin.x + (offset as f32 - 1.0) * 0.8,
                        y: origin.y + 0.5,
                    };
                    self.spawn_effect(
                        KitEffectKind::SummonPet {
                            ability_slot: slot.idx() as u8,
                            pet_kind,
                            hp,
                            hit_ticks_remaining: hit_ticks.max(1),
                            damage: dmg,
                            school: school.school(),
                            attack_interval: TICK_HZ / 2,
                            next_attack: self.tick + TICK_HZ / 2,
                            end_tick: self.tick + seconds_to_ticks(dur),
                            applies_mark: applies_mark
                                .map(|(m, add, sec)| (m.id(), add, seconds_to_ticks(sec))),
                            locked_target: 0,
                        },
                        spawn_pos,
                        spawn_pos,
                    );
                }
            }
            PrimitiveEffect::TrapZone {
                arm_delay,
                duration,
                trigger_radius,
                blast_radius,
                damage,
                school,
                root_seconds,
                stun_seconds,
                slow_pct,
                max_traps,
                reveals_enemies,
            } => {
                let owner_id = self.caster_id();
                let mut existing: Vec<_> = self
                    .room
                    .entities
                    .ecs
                    .query::<(bevy_ecs::entity::Entity, &KitEffect)>()
                    .iter(&self.room.entities.ecs)
                    .filter_map(|(eh, eff)| {
                        (eff.owner_id == owner_id
                            && matches!(eff.kind, KitEffectKind::TrapZone { .. }))
                        .then_some(eh)
                    })
                    .collect();
                while existing.len() >= usize::from(max_traps.max(1)) {
                    let oldest = existing.remove(0);
                    self.room.entities.remove_spell_handle::<KitEffect>(oldest);
                }
                let center = self.resolve_target_pos(target, 12.0);
                let arm = self.eval_var(ability_id, rank, arm_delay);
                let dur = self.eval_var(ability_id, rank, duration).max(2.0);
                self.spawn_effect(
                    KitEffectKind::TrapZone {
                        ability_slot: slot.idx() as u8,
                        armed_at: self.tick + seconds_to_ticks(arm),
                        end_tick: self.tick + seconds_to_ticks(dur),
                        trigger_radius: self.eval_var(ability_id, rank, trigger_radius).max(0.8),
                        blast_radius: self.eval_var(ability_id, rank, blast_radius).max(1.5),
                        damage: self.eval_var(ability_id, rank, damage),
                        school: school.school(),
                        root_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, root_seconds)),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        reveals: reveals_enemies,
                        detonate_on_caster_damage: false,
                    },
                    center,
                    center,
                );
            }
            PrimitiveEffect::SegmentBarrier {
                radius_or_span,
                duration,
                arm_delay,
                dps_or_hit_damage,
                school,
                stun_seconds,
                silence_seconds,
                slow_pct,
                max_stakes: _,
                is_ring_arena,
            } => {
                let rad = self.eval_var(ability_id, rank, radius_or_span).max(2.0);
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let arm = self.eval_var(ability_id, rank, arm_delay);
                let center = self.resolve_target_pos(target, 12.0);
                let (end_pos, anchor) = if is_ring_arena {
                    (center, center)
                } else {
                    // Connect to previous recent stake if within span, otherwise form perpendicular barrier segment.
                    let owner_id = self.caster_id();
                    let prev_stake = self
                        .room
                        .entities
                        .spells::<KitEffect>()
                        .filter_map(|eff| {
                            (eff.owner_id == owner_id
                                && matches!(
                                    eff.kind,
                                    KitEffectKind::SegmentBarrier {
                                        is_ring_arena: false,
                                        ..
                                    }
                                )
                                && eff.position.dist(center) <= rad * 2.0)
                                .then_some(eff.position)
                        })
                        .last();
                    let other = prev_stake.unwrap_or(Position {
                        x: center.x,
                        y: center.y + 0.1,
                    });
                    (other, center)
                };
                self.spawn_effect(
                    KitEffectKind::SegmentBarrier {
                        ability_slot: slot.idx() as u8,
                        armed_at: self.tick + seconds_to_ticks(arm),
                        end_tick: self.tick + seconds_to_ticks(dur),
                        end_pos,
                        radius: rad,
                        is_ring_arena,
                        dps_or_hit_damage: self.eval_var(ability_id, rank, dps_or_hit_damage),
                        school: school.school(),
                        stun_ticks: seconds_to_ticks(self.eval_var(ability_id, rank, stun_seconds)),
                        silence_ticks: seconds_to_ticks(self.eval_var(
                            ability_id,
                            rank,
                            silence_seconds,
                        )),
                        slow_pct: self.eval_var(ability_id, rank, slow_pct),
                        next_tick: self.tick,
                        struck_recently: Vec::new(),
                    },
                    anchor,
                    end_pos,
                );
            }
            PrimitiveEffect::StealthMode {
                pre_delay: _,
                duration,
                speed_bonus,
                affect_allies,
                break_on_damage: _,
                break_on_attack: _,
                re_stealth_delay: _,
            } => {
                let dur = self.eval_var(ability_id, rank, duration).max(1.0);
                let spd = self.eval_var(ability_id, rank, speed_bonus);
                if affect_allies {
                    let allies = self.allies_in_radius(origin, 200.0, true, true);
                    for a in allies {
                        self.set_stealth(a, dur, spd, 0.5);
                    }
                } else {
                    self.set_stealth(self.caster, dur, spd, 0.25);
                }
            }
            PrimitiveEffect::MarkStacks {
                mark,
                on_self,
                add_stacks,
                max_stacks,
                duration,
                consume_all,
                damage_per_stack,
                heal_per_stack,
                school,
                mortal_wound_seconds,
            } => {
                let idx = if on_self {
                    Some(self.caster)
                } else {
                    self.resolve_target_idx(target)
                };
                if let Some(j) = idx {
                    if add_stacks > 0 {
                        self.add_mark(j, mark.id(), add_stacks as u8, max_stacks, duration);
                    }
                    if mortal_wound_seconds > 0.0 {
                        self.apply_mortal_wound(j, mortal_wound_seconds);
                    }
                    if consume_all {
                        let stacks = self.consume_mark(j, mark.id());
                        if stacks > 0 {
                            let d = self.eval_var(ability_id, rank, damage_per_stack)
                                * f32::from(stacks)
                                * dmg_mult;
                            if d > 0.0 {
                                self.deal_damage(j, d, school.school(), 0.0, false);
                            }
                            let h = self.eval_var(ability_id, rank, heal_per_stack)
                                * f32::from(stacks)
                                * heal_mult;
                            if h > 0.0 {
                                self.heal_unit(self.caster, h);
                            }
                        }
                    }
                }
            }
            PrimitiveEffect::ExecuteDamage {
                threshold_instant_kill_pct,
                missing_hp_pct,
                school,
                nonhero_cap,
            } => {
                if let Some(tj) = self.resolve_target_idx(target) {
                    let thr = self.eval_var(ability_id, rank, threshold_instant_kill_pct);
                    let e = &self.room.entities[tj];
                    if thr > 0.0 && e.max_hp > 0.0 && (e.hp / e.max_hp) <= thr {
                        let kill_dmg = e.hp + 9999.0;
                        self.deal_damage(tj, kill_dmg, DamageSchool::True, 1.0, false);
                    } else {
                        let pct = self.eval_var(ability_id, rank, missing_hp_pct);
                        if pct > 0.0 {
                            let missing = (e.max_hp - e.hp).max(0.0);
                            let mut dmg = missing * pct * dmg_mult;
                            if e.kind != Kind::Hero && nonhero_cap > 0.0 {
                                dmg = dmg.min(nonhero_cap);
                            }
                            self.deal_damage(tj, dmg, school.school(), 0.0, false);
                        }
                    }
                }
            }
            PrimitiveEffect::CleanseImmune {
                cleanse_cc,
                cc_immune_seconds,
                untargetable_seconds,
            } => {
                if cleanse_cc {
                    self.cleanse_cc(self.caster);
                }
                let imm = self.eval_var(ability_id, rank, cc_immune_seconds);
                if imm > 0.0 {
                    self.set_unstoppable(self.caster, imm);
                }
                let untarg = self.eval_var(ability_id, rank, untargetable_seconds);
                if untarg > 0.0 {
                    self.set_untargetable(self.caster, untarg);
                }
            }
            PrimitiveEffect::FormSwap {
                to_form,
                empowers_next_attack,
                bonus_damage,
                slow_pct,
                slow_seconds,
            } => {
                self.swap_form(to_form);
                if empowers_next_attack {
                    let bonus = self.eval_var(ability_id, rank, bonus_damage) * dmg_mult;
                    self.empower_next_attacks(
                        1,
                        bonus,
                        true,
                        0.0,
                        slow_pct,
                        slow_seconds,
                        0.0,
                        4.0,
                    );
                }
            }
            PrimitiveEffect::AmmoPool {
                slot: pool_slot,
                max_charges,
                reload_seconds: _,
                grant_charges,
                grant_resource,
            } => {
                let max_c = self
                    .eval_var(ability_id, rank, max_charges)
                    .round()
                    .max(1.0) as u8;
                let s_idx = pool_slot.base_slot().idx();
                if grant_charges != 0 {
                    let cur = self
                        .room
                        .kit_ammo_charges(self.caster, pool_slot.base_slot());
                    let next = (i16::from(cur) + i16::from(grant_charges))
                        .clamp(0, i16::from(max_c)) as u8;
                    let k = self.room.entities[self.caster]
                        .hero_kit
                        .get_or_insert_with(Box::default);
                    k.charges[s_idx] = next;
                    k.charges_initialized[s_idx] = true;
                }
                if grant_resource != 0.0 {
                    self.add_resource(grant_resource);
                }
            }
            PrimitiveEffect::RefreshCooldown {
                slot: target_slot,
                reduce_pct,
                reduce_flat_sec,
                overdrive_only,
            } => {
                if !overdrive_only || rank >= 5 {
                    let s = target_slot.base_slot();
                    if reduce_pct >= 1.0 {
                        self.reset_slot_cd(s);
                    } else {
                        if reduce_pct > 0.0 {
                            let rem = self.room.entities[self.caster].cooldown[s.idx()]
                                .saturating_sub(self.tick)
                                as f32
                                / TICK_HZ as f32;
                            self.reduce_slot_cd(s, rem * reduce_pct);
                        }
                        if reduce_flat_sec > 0.0 {
                            self.reduce_slot_cd(s, reduce_flat_sec);
                        }
                    }
                }
            }
            PrimitiveEffect::OpenRecast {
                slot: recast_slot,
                window_seconds,
                stage,
            } => {
                let target_id = match target {
                    CastTarget::Entity(id) => id,
                    CastTarget::SelfCast | CastTarget::Point(_) => 0,
                };
                let win = self.eval_var(ability_id, rank, window_seconds).max(1.0);
                self.open_recast(recast_slot.base_slot(), stage, target_id, win);
            }
        }
    }
}
