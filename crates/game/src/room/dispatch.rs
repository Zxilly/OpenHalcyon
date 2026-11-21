use super::actions::commands;
use super::state::{Pick, Room};
use crate::abilities;
use crate::host::admin_views::{Channel, EventDetail, EventKind, RoomPhase};
use crate::modes::{aral, draft_5v5, draft_preferences, fold_blitz, hud, scene};
use crate::replication::{
    Batch,
    messages::{self, wire_id},
    slots::point_move,
};
use halcyon_protocol::{Fields, LiveFields, Message, Opcode, ce::KnownQueue, wire_opcode};
use halcyon_sim::{Command, Position};
use std::time::Instant;

struct PreambleStep {
    scene_requested: bool,
    scene_confirmed: bool,
}

impl Room {
    pub(crate) fn receive(&mut self, i: usize, ch: usize, m: Message) -> std::io::Result<Batch> {
        self.last_active = Instant::now();
        let legacy = self.legacy_aral();
        let solo_scene = self.solo_scene()
            && self.seats[i].loading_roster_sent
            && self.seats[i].peers[1].is_none();
        let preamble = self.observe_scene_preamble(i, ch, legacy, solo_scene, &m);
        let game = self.phase == RoomPhase::Running
            && (ch == 1
                || ((legacy || solo_scene) && ch == 0 && self.seats[i].preamble.confirmed()));
        let pending_world_input = solo_scene
            && ch == 0
            && self.seats[i].scene_written
            && self.seats[i].preamble.scene_requested()
            && !self.seats[i].preamble.confirmed();
        let mut accepted_world_input = false;
        let mut first_point_ack = None;
        let sync_after_pick =
            !legacy && ch == 0 && m.is(Opcode::DraftSelectHero) && !self.seats[i].draft_sync_sent;
        self.record_first_seen_opcode(i, ch, &m);
        if ch == 0
            && self.phase == RoomPhase::HeroSelect
            && !legacy
            && !solo_scene
            && self.known_queue != Some(KnownQueue::Ranked)
            && let Some(out) = self.try_handle_draft_5v5(i, &m, sync_after_pick)
        {
            return Ok(out);
        }
        let mut out = vec![];
        match m.fields()? {
            _ if game && m.is(Opcode::RequestCancelAbility) => {
                out = abilities::receive(self, i, ch, m);
            }
            Fields::Live(LiveFields::RecommendedBuildPath(path))
                if game
                    || pending_world_input
                    || (legacy
                        && ch == 0
                        && self.seats[i].preamble.scene_requested()
                        && matches!(self.phase, RoomPhase::Loading | RoomPhase::Running)) =>
            {
                out = self.set_recommended_path(i, path);
                accepted_world_input = pending_world_input;
            }
            Fields::Live(LiveFields::MovePoint { x, y }) | Fields::Movement { x, y }
                if ch == 0
                    && self.seats[i].world_actor_written
                    && (legacy
                        || m.is(Opcode::RequestMovePoint)
                            && solo_scene
                            && self.seats[i].scene_written)
                    && (game || pending_world_input) =>
            {
                let actor = &self.world.entities[i];
                let slot = self.hero_slot(actor);
                if let Some(slot) = slot
                    && let Some(ack) = point_move::submit(
                        &mut self.world,
                        i as u32 + 1,
                        slot,
                        Position { x, y },
                        &m.body,
                    )
                {
                    if pending_world_input {
                        accepted_world_input = true;
                        first_point_ack = Some(ack);
                    } else {
                        if legacy {
                            point_move::publish_aral(self, i, &ack);
                        }
                        out.push(ack);
                    }
                }
            }
            Fields::Movement { x, y } if pending_world_input => {
                const MOVE_JOYSTICK_PADDED_LEN: usize =
                    (halcyon_protocol::MoveCmd::BODY_LEN + 2).next_multiple_of(8) - 2;
                accepted_world_input = m.body.len() == MOVE_JOYSTICK_PADDED_LEN
                    && x.is_finite()
                    && y.is_finite()
                    && self
                        .world
                        .command(i as u32 + 1, Command::MoveTo(Position { x, y }))
                        .is_ok();
            }
            Fields::Clock(seconds) if pending_world_input => {
                accepted_world_input = halcyon_protocol::ClockSyncBody::read_with_padding(&m.body)
                    .is_ok_and(|(_, pad)| !pad.is_empty())
                    && seconds.is_finite()
                    && seconds >= 0.;
                out.push(m);
            }
            Fields::Movement { x, y } if game && !(legacy && ch == 0) => {
                let _ = self
                    .world
                    .command(i as u32 + 1, Command::MoveTo(Position { x, y }));
            }
            Fields::Live(LiveFields::Item(halcyon_protocol::ItemAction::Reorder {
                actor,
                slots,
            })) => aral::inventory::reorder(self, i, ch, actor, slots),
            Fields::Live(LiveFields::Item(halcyon_protocol::ItemAction::Sell {
                actor,
                instance,
                suffix,
            })) if game && suffix.iter().all(|&byte| byte == 0) => {
                out = aral::item_actions::sell(self, i, actor, instance);
            }
            Fields::Live(LiveFields::Item(halcyon_protocol::ItemAction::Activate {
                instance,
                suffix,
            })) if game && suffix.iter().all(|&byte| byte == 0) => {
                if !self.handle_item_activation(i, instance) {
                    return Ok(out);
                }
            }
            Fields::Live(LiveFields::Item(halcyon_protocol::ItemAction::TargetRequest {
                arguments: [target_wire, instance],
                suffix,
            })) if game && suffix.iter().all(|&byte| byte == 0) => {
                if !self.handle_item_target_request(i, target_wire, instance) {
                    return Ok(out);
                }
            }
            Fields::Live(LiveFields::Item(halcyon_protocol::ItemAction::VectorRequest {
                vector_bits: [x_bits, _, z_bits],
                argument: instance,
                suffix,
            })) if game && suffix.iter().all(|&byte| byte == 0) => {
                self.handle_item_vector_request(i, x_bits, z_bits, instance);
            }
            Fields::Live(LiveFields::TalentSelection(selection)) => {
                out = if fold_blitz::is_blitz(self.known_queue) {
                    fold_blitz::talent_response(self, i, selection)
                } else {
                    aral::talent_response(self, i, ch, selection)
                };
            }
            Fields::Live(LiveFields::MovePoint { .. }) if ch == 0 && (legacy || solo_scene) => {}
            Fields::Live(
                LiveFields::LearnSelection(_)
                | LiveFields::AbilityPoint { .. }
                | LiveFields::AbilityTarget { .. },
            )
            | Fields::Outbound(halcyon_protocol::OutboundAction {
                operands: halcyon_protocol::OutboundOperands::AbilityAlternate { .. },
                ..
            }) if game => {
                out = abilities::receive(self, i, ch, m);
            }
            Fields::Live(LiveFields::SurrenderRequest(vote))
                if game
                    && (ch == 1 || self.seats[i].world_actor_written)
                    && halcyon_protocol::SurrenderRequestBody::read_with_padding(&m.body)
                        .is_ok_and(|(_, tail)| tail.iter().all(|&b| b == 0)) =>
            {
                self.vote_surrender(i, vote);
            }
            Fields::Live(request @ (LiveFields::MapPing(_) | LiveFields::HudQuickMessage(_)))
                if game && self.room_hud_connection(i, ch) =>
            {
                out = hud::submit(self, i, request);
            }
            Fields::Live(request) if game => {
                self.handle_live_command(i, ch, legacy, &m, request, &mut out);
            }
            Fields::ShopRequest { actor, item }
                if game && actor == u32::from(wire_id(i as u32 + 1)) =>
            {
                out = self.buy(i, actor, item);
            }
            Fields::ControlByte {
                value: 0,
                remaining,
            } if m.is(Opcode::DraftRerollHero)
                && ch == 0
                && remaining.iter().all(|&byte| byte == 0) =>
            {
                out = aral::reroll(self, i);
            }
            Fields::ControlByte { value, remaining }
                if ch == 0
                    && self.phase == RoomPhase::HeroSelect
                    && self.seats[i].peers[0].is_some()
                    && remaining.iter().all(|b| *b == 0)
                    && let Some(choice) =
                        draft_preferences::Choice::parse(m.known_opcode(), value) =>
            {
                choice.apply(&mut self.picks[i].preferences);
                out.push(self.roster_with_echo(Some(i), Some(&m)));
                out.push(m);
            }
            Fields::ControlByte {
                value: 0,
                remaining: [0, 0, 0, 0, 0],
            } if m.is(Opcode::DraftRefreshRequest)
                && (legacy
                    || self.solo_scene()
                    || self.known_queue == Some(KnownQueue::Ranked)
                    || fold_blitz::is_managed(self.known_queue))
                && ch == 0
                && self.phase == RoomPhase::HeroSelect
                && self.seats[i].peers[0].is_some() =>
            {
                out = if legacy {
                    aral::refresh(self, i)
                } else {
                    messages::draft_catalog(
                        &self.world,
                        &self.players,
                        &self.catalog,
                        &self.queue,
                        &self.picks,
                    )
                };
                out.push(m);
            }
            Fields::HeroLock if ch == 0 && !legacy => {
                self.handle_hero_lock(i, m, &mut out);
            }
            Fields::HeroSelection { template, choice }
                if ch == 0 && !legacy && !self.seats[i].locked =>
            {
                self.handle_hero_selection(i, template, choice, m, &mut out);
            }
            Fields::SkinSelection(skin)
                if ch == 0
                    && (!legacy || self.phase == RoomPhase::HeroSelect)
                    && !self.seats[i].loading_roster_sent =>
            {
                self.picks[i].skin = self.validated_skin(&self.picks[i].name, skin);
                out.push(self.roster_update(Some(i)));
                out.push(m);
            }
            Fields::Clock(_) | Fields::Heartbeat => out.push(m),
            _ if (legacy || solo_scene)
                && matches!(
                    m.opcode,
                    wire_opcode::RequestSceneSnapshot
                        | wire_opcode::ConfirmSceneOwner
                        | wire_opcode::SceneReadyConfirm
                ) =>
            {
                out = self.handle_scene_preamble_message(i, solo_scene, &preamble, m)?;
            }
            _ if [
                wire_opcode::DraftRefreshRequest,
                wire_opcode::DraftSelectHero,
                wire_opcode::DraftReadyControl,
                wire_opcode::DraftSelectSkin,
                wire_opcode::RequestSceneSnapshot,
                wire_opcode::ConfirmSceneOwner,
                wire_opcode::SceneReadyConfirm,
            ]
            .contains(&m.opcode) =>
            {
                out.push(m);
            }
            _ => {}
        }
        if accepted_world_input {
            self.seats[i].preamble.confirm_world_input();
            self.phase(RoomPhase::Running);
            out.push(halcyon_protocol::StartMatchBody.encode());
            out.extend(first_point_ack);
        }
        if sync_after_pick {
            // Android's first 0x045E response precedes this one-time draft message.
            out.push(
                halcyon_protocol::EndMatchBody {
                    team_word: 0,
                    reason: halcyon_protocol::EndMatchReason::DraftSync,
                }
                .encode_padded(),
            );
            self.seats[i].draft_sync_sent = true;
        }
        if ch == 0 && preamble.scene_requested {
            self.scene_views[i].declarations(&out);
            if self.legacy_aral() {
                self.scene_views[i].accept_guard_links(&out);
            }
        }
        Ok(out)
    }

    fn observe_scene_preamble(
        &mut self,
        i: usize,
        ch: usize,
        legacy: bool,
        solo_scene: bool,
        m: &Message,
    ) -> PreambleStep {
        let mut scene_requested = false;
        let mut scene_confirmed = false;
        if (legacy || solo_scene)
            && ch == 0
            && matches!(self.phase, RoomPhase::Loading | RoomPhase::Running)
            && !self.seats[i].preamble.confirmed()
        {
            let preamble = &mut self.seats[i].preamble;
            let requested = preamble.scene_requested();
            preamble.observe(m, u32::from(wire_id(self.world.entities[i].id)));
            scene_requested = preamble.scene_requested() && !requested;
            scene_confirmed = preamble.confirmed();
            if solo_scene && scene_requested {
                self.seats[i].control_world = true;
            }
        }
        PreambleStep {
            scene_requested,
            scene_confirmed,
        }
    }

    fn record_first_seen_opcode(&mut self, i: usize, ch: usize, m: &Message) {
        let seen = &mut self.seats[i].seen[ch];
        if !m.is(Opcode::DraftReadyAccept) && seen.len() < 32 && seen.insert(m.opcode) {
            let generation = self.seats[i].peers[ch].as_ref().map(|peer| peer.generation);
            self.event(
                EventKind::Protocol,
                "First client message of this type",
                EventDetail::Connection {
                    player_id: self.players[i],
                    channel: Channel::for_game(ch != 0),
                    generation,
                    opcode: Some(m.opcode.into()),
                },
            );
        }
    }

    fn try_handle_draft_5v5(
        &mut self,
        i: usize,
        m: &Message,
        sync_after_pick: bool,
    ) -> Option<Batch> {
        let action = halcyon_protocol::draft::DraftAction::decode(m)?;
        let mut draft = self.draft_5v5.take()?;
        if let halcyon_protocol::draft::DraftAction::SelectSkin { skin_id, .. } = action
            && !self.seats[i].loading_roster_sent
        {
            self.picks[i].skin = self.validated_skin(&self.picks[i].name, skin_id);
        }
        let outcome = draft.handle_action(
            i,
            action,
            &mut self.world,
            &mut self.seats,
            &mut self.picks,
            self.data.as_deref(),
        );
        let ranked_msg = draft
            .is_ranked()
            .then(|| draft.ranked_packet(&self.world, &self.picks).encode());
        self.draft_5v5 = Some(draft);
        let mut out = vec![];
        if let Ok(outcome) = outcome {
            if outcome.state_changed {
                let roster = self.roster_with_echo(Some(i), outcome.echo.then_some(m));
                out.push(roster);
                if let Some(pkt) = ranked_msg {
                    out.push(pkt);
                }
            }
            if outcome.echo {
                out.push(m.clone());
            }
            if outcome.start_loading {
                self.enter_5v5_loading(i, &mut out);
            }
            if sync_after_pick {
                out.push(
                    halcyon_protocol::EndMatchBody {
                        team_word: 0,
                        reason: halcyon_protocol::EndMatchReason::DraftSync,
                    }
                    .encode_padded(),
                );
                self.seats[i].draft_sync_sent = true;
            }
        }
        Some(out)
    }

    fn handle_item_activation(&mut self, i: usize, instance: u32) -> bool {
        use halcyon_sim::{AralStarter, BasicItem, ItemTarget};
        let hero = &self.world.entities[i];
        let command = if instance == AralStarter::HealingFlask.grant().instance {
            Command::ActivateHealingFlask { instance }
        } else {
            match hero
                .inventory
                .iter()
                .find(|h| h.instance == instance)
                .map(|h| h.kind)
            {
                Some(BasicItem::WeaponInfusion | BasicItem::CrystalInfusion) => {
                    Command::ConsumeInfusion { instance }
                }
                Some(BasicItem::ReflexBlock) => Command::ActivateReflexBlock { instance },
                Some(BasicItem::SprintBoots) => Command::ActivateSprintBoots { instance },
                Some(BasicItem::ScoutTrap) => Command::DeployScoutTrap { instance },
                Some(_) => Command::ActivateItem {
                    instance,
                    target: ItemTarget::SelfCast,
                },
                None => return false,
            }
        };
        let _ = self.world.command(hero.id, command);
        true
    }

    fn handle_item_target_request(&mut self, i: usize, target_wire: u32, instance: u32) -> bool {
        let hero_id = self.world.entities[i].id;
        let target = if target_wire == u32::MAX {
            halcyon_sim::ItemTarget::SelfCast
        } else if let Some(actor_id) = target_wire
            .checked_sub(u32::from(wire_id(0)))
            .filter(|&id| id != 0)
        {
            halcyon_sim::ItemTarget::Entity(actor_id)
        } else {
            return false;
        };
        let _ = self
            .world
            .command(hero_id, Command::ActivateItem { instance, target });
        true
    }

    fn handle_item_vector_request(&mut self, i: usize, x_bits: u32, z_bits: u32, instance: u32) {
        let (x, y) = (f32::from_bits(x_bits), f32::from_bits(z_bits));
        if x.is_finite() && y.is_finite() {
            let hero_id = self.world.entities[i].id;
            let point = Position { x, y };
            let cmd = if instance == halcyon_sim::AralStarter::VisionTotem.grant().instance {
                Command::DeployVisionTotem { instance, point }
            } else {
                Command::ActivateItem {
                    instance,
                    target: halcyon_sim::ItemTarget::Point(point),
                }
            };
            let _ = self.world.command(hero_id, cmd);
        }
    }

    fn handle_live_command(
        &mut self,
        i: usize,
        ch: usize,
        legacy: bool,
        m: &Message,
        request: LiveFields<'_>,
        out: &mut Batch,
    ) {
        if commands::clears_attack(&request) {
            if self.world.entities[i].attack_target().is_some()
                && self.world.command(i as u32 + 1, Command::Stop).is_ok()
                && !legacy
            {
                out.push(abilities::modify_basic_attack_target_message(None));
            }
        } else if let Some(command) = commands::from_live(request) {
            let aral_target = legacy && ch == 0 && matches!(command, Command::Attack(_));
            let accepted = self.world.command(i as u32 + 1, command).is_ok();
            if accepted {
                if aral_target {
                    out.push(m.clone());
                } else if !legacy && let Command::Attack(target) = command {
                    out.push(abilities::modify_basic_attack_target_message(Some(target)));
                }
            }
        }
    }

    fn handle_hero_lock(&mut self, i: usize, m: Message, out: &mut Batch) {
        let op = m.opcode;
        out.push(m);
        if !self.seats[i].locked {
            self.seats[i].locked = true;
            self.picks[i].selected = true;
            self.picks[i].locked = true;
            self.event(
                EventKind::Protocol,
                "Hero locked",
                EventDetail::Action {
                    player_id: self.players[i],
                    opcode: op.into(),
                },
            );
            let roster = self.roster_update(Some(i));
            if op == wire_opcode::DraftLockHero {
                out.insert(0, roster);
            } else {
                out.push(roster);
            }
            if self.seats.iter().all(|s| s.locked) {
                let _ = fold_blitz::prepare_world(self);
                self.phase(RoomPhase::Loading);
            }
        }
    }

    fn handle_hero_selection(
        &mut self,
        i: usize,
        template: u32,
        choice: halcyon_protocol::DraftHeroChoice,
        m: Message,
        out: &mut Batch,
    ) {
        let name = draft_5v5::hero_name_for_template(self.data.as_deref(), template);
        if let Some(name) = name {
            let valid = if let Some(d) = &self.data {
                self.world.select_hero(i, &name, d).is_ok()
            } else {
                name == "Ringo"
            };
            if valid {
                let requested = match choice {
                    halcyon_protocol::DraftHeroChoice::Skin(skin) => skin,
                    halcyon_protocol::DraftHeroChoice::Flags(_) if self.picks[i].name == name => {
                        self.picks[i].skin
                    }
                    halcyon_protocol::DraftHeroChoice::Flags(_) => 0,
                };
                let skin = self.validated_skin(&name, requested);
                self.picks[i] = Pick {
                    preferences: self.picks[i].preferences,
                    name,
                    skin,
                    selected: true,
                    locked: false,
                };
                out.push(self.roster_update(Some(i)));
            }
        }
        out.push(m);
    }

    fn handle_scene_preamble_message(
        &mut self,
        i: usize,
        solo_scene: bool,
        preamble: &PreambleStep,
        m: Message,
    ) -> std::io::Result<Batch> {
        let mut out = vec![];
        let late = solo_scene && self.seats[i].preamble.take_late_confirmation(&m);
        if preamble.scene_confirmed || late {
            if solo_scene {
                out.push(Message::known(Opcode::SceneLoadedDismiss, [0]));
            }
            out.push(m);
            if solo_scene {
                out.push(Message::known(Opcode::SceneLoadedDismiss, [0]));
            }
            if solo_scene && preamble.scene_confirmed {
                self.phase(RoomPhase::Running);
                out.push(halcyon_protocol::StartMatchBody.encode());
            } else if !solo_scene && self.phase == RoomPhase::Running {
                out.push(halcyon_protocol::StartMatchBody.encode());
            }
        } else if preamble.scene_requested {
            let team = (self.phase == RoomPhase::Running).then_some(self.world.entities[i].team);
            out = if solo_scene {
                scene::solo_opening(self, i)?
            } else {
                scene::announce_aral_entry(self, i, &m);
                aral::opening(self, team)
            };
            out.push(Message::known(Opcode::RequestSceneSnapshot, [0; 6]));
            out.push(m);
            if solo_scene {
                out.push(scene::solo_table(self));
            }
        }
        Ok(out)
    }
}
