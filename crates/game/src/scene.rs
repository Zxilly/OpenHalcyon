//! Native scene negotiation on an authenticated control stream.
pub(crate) mod mode_variables;
use super::{Batch, Message, Room, messages};
#[derive(Default)]
pub(super) struct Preamble {
    stage: usize,
    implicit: bool,
}

impl Preamble {
    const OPS: [u16; 3] = [
        crate::wire_opcode::RequestSceneSnapshot,
        crate::wire_opcode::ConfirmSceneOwner,
        crate::wire_opcode::SceneReadyConfirm,
    ];

    pub fn observe(&mut self, message: &Message, actor: u32) {
        let known = match message.known_opcode() {
            Some(crate::Opcode::RequestSceneSnapshot | crate::Opcode::SceneReadyConfirm) => {
                message.body == [0; 6]
            }
            Some(crate::Opcode::ConfirmSceneOwner) => {
                let body = &message.body;
                body.len() == 6 && body[..4] == actor.to_be_bytes() && body[4..] == [1, 0]
            }
            _ => false,
        };
        if known && Self::OPS.get(self.stage) == Some(&message.opcode) {
            self.stage += 1;
        }
    }

    pub fn confirm_world_input(&mut self) {
        debug_assert!(self.scene_requested() && !self.confirmed());
        self.stage = Self::OPS.len();
        self.implicit = true;
    }
    pub fn take_late_confirmation(&mut self, m: &Message) -> bool {
        m.is(crate::Opcode::SceneReadyConfirm)
            && m.body == [0; 6]
            && std::mem::take(&mut self.implicit)
    }
    pub fn scene_requested(&self) -> bool {
        self.stage >= 2
    }

    pub fn confirmed(&self) -> bool {
        self.stage == Self::OPS.len()
    }
}

/// An already connected draft seat can observe a different reserved seat's first registration.
pub(super) fn announce_aral_registration(room: &Room, entering: usize) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != entering)
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let mut body = [0; 64];
    let id = room.players[entering].as_hyphenated();
    id.encode_lower(&mut body[..36]);
    let notice = super::transport::TeamBatch::new(vec![Message::known(
        crate::Opcode::RegisterPlayerUuid,
        body,
    )]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(notice.clone()));
    }
}

/// An ARAL connection already in the scene can observe another seat joining it.
pub(super) fn announce_aral_entry(room: &Room, entering: usize, registration: &Message) {
    let mut recipients = room
        .seats
        .iter()
        .enumerate()
        .filter(|(index, seat)| {
            *index != entering && seat.world_actor_written && seat.preamble.scene_requested()
        })
        .filter_map(|(_, seat)| seat.peers[0].as_ref())
        .peekable();
    if recipients.peek().is_none() {
        return;
    }
    let mut handle = [0; 78];
    let name = messages::GUEST_HANDLE.as_bytes();
    handle[..name.len()].copy_from_slice(name);
    handle[64..68].copy_from_slice(&(entering as u32).to_be_bytes());
    let actor = messages::wire_id(room.world.entities[entering].id);
    handle[68..72].copy_from_slice(&u32::from(actor).to_be_bytes());
    let batch = super::transport::TeamBatch::new(vec![
        Message::known(crate::Opcode::PlayerLoadedScene, handle),
        Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        registration.clone(),
    ]);
    for peer in recipients {
        peer.send(super::transport::Delivery::Team(batch.clone()));
    }
}

pub(super) fn players(room: &Room) -> impl Iterator<Item = Message> {
    (0..room.picks.len()).map(|i| messages::info(&room.world, &room.players, &room.picks, i, true))
}
pub(super) fn begin_loading(room: &Room) -> Batch {
    players(room)
        .chain(std::iter::once(Message::known(
            crate::Opcode::DraftTransitionToLoading,
            [0],
        )))
        .collect()
}

pub(super) fn solo_table(room: &Room) -> Message {
    let mut body = [0u8; 96];
    for (row, actor) in body
        .as_chunks_mut::<6>()
        .0
        .iter_mut()
        .zip(room.world.entities.iter().take(room.picks.len()))
    {
        row[..4].copy_from_slice(&u32::from(messages::wire_id(actor.id)).to_be_bytes());
        row[4] = 1;
        row[5] = u8::from(actor.bot);
    }
    Message::known(crate::Opcode::SceneReadinessRows, body)
}
pub(super) fn solo_opening(room: &mut Room, seat: usize) -> std::io::Result<Batch> {
    if let Some(batch) = super::modes_3v3_blitz::solo_opening(room, seat)? {
        return Ok(batch);
    }
    let team = room.world.entities[seat].team;
    let initial = room.phase == crate::RoomPhase::Loading && room.world.tick == 0;
    if initial {
        room.world
            .use_solo_mid_lane()
            .map_err(std::io::Error::other)?;
        if let Some(data) = &room.data {
            room.world
                .install_map_data(data)
                .map_err(std::io::Error::other)?;
        }
    }
    let mut out = vec![
        messages::scene_mode(&room.queue, 1),
        Message::known(crate::Opcode::SceneLoadedDismiss, [0]),
    ];
    out.extend(players(room));
    out.push(messages::world_clock(room.world.tick));
    room.visible[team as usize].clear();
    if !initial {
        room.solo_slots.retain_living(&room.world.entities);
    }
    for e in &room.world.entities {
        if e.hp <= 0. {
            continue;
        }
        let visible = room.world.visible(team, e);
        // The recorded initial scene declares all ten starting heroes. On reconnect
        // live opposing positions are subject to the team's usual visibility.
        if (initial && e.kind == halcyon_sim::Kind::Hero) || (!initial && visible) {
            let Some(message) =
                room.solo_slots
                    .declaration(&room.world, e, &room.picks, room.data.as_deref())
            else {
                continue;
            };
            out.push(message);
            if initial {
                super::native_cooldown::opening(
                    &mut out,
                    e,
                    halcyon_protocol::ce::NativeMode::SoloBots,
                );
            }
            messages::append_inventory(&mut out, &room.world, e);
            if visible {
                room.visible[team as usize].insert(e.id);
            }
        }
    }
    if initial {
        out.extend(mode_variables::Opening::Solo.messages());
    } else {
        room.solo_structure_snapshot.append(&mut out);
    }
    out.extend(room.recommended_paths(Some(team)));
    Ok(out)
}

/// First authoritative tick, before stepping the pristine world. Mark only structures
/// actually sent to this team; normal replication handles everything else.
pub(super) fn initial_structures(room: &mut Room) {
    let Some(peer) = room.seats[0].gameplay_peer() else {
        return;
    };
    let team = room.world.entities[0].team;
    let known = &mut room.visible[usize::from(team)];
    let mut batch = Vec::new();
    let mut effects = Vec::new();
    for entity in &room.world.entities {
        if !known.contains(&entity.id)
            && room.world.visible(team, entity)
            && let Some(message) = super::compact::structure(room.world.tick, entity)
        {
            known.insert(entity.id);
            if let Some(opening) = super::structure_effects::Opening::solo(message.body[116])
                && opening.actor() == u32::from(messages::wire_id(entity.id))
            {
                effects.push(opening);
            }
            batch.push(message);
        }
    }
    if !batch.is_empty() {
        super::structure_effects::append_solo(&mut batch, &mut effects);
        super::structure_effects::append_crystal_start(&mut batch, &effects);
        if let Some((actor, _, message)) = halcyon_protocol::SoloStructure::base_shopkeeper(team) {
            batch.push(message);
            batch.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                actor,
            ));
            room.scene_views[0].shop_base_declared = true;
        }
        room.scene_views[0].declarations(&batch);
        peer.send(super::transport::Delivery::Private(batch));
    }
}

/// Keeps 5v5 base/river shopkeepers (`329`/`330`) and `Buff_Shop_AllowStorePurchase` (`174`)
/// synchronized on confirmed human seats across both `5v5_bots_solo` and 5v5 PvP.
pub(super) fn sync_5v5_shops(room: &mut Room) {
    let solo_active = room.solo_scene() && room.seats[0].control_world;
    if !solo_active && !room.world.is_5v5_enabled() {
        return;
    }
    let river_ready = room.world.is_5v5_enabled()
        && room.world.tick >= halcyon_sim::map_5v5::RIVER_SHOP_SPAWN_TICK;
    for seat_idx in 0..room.seats.len() {
        let peer_ch = if room.seats[seat_idx].peers[1].is_some() {
            Some(1usize)
        } else if room.seats[seat_idx].peers[0].is_some()
            && room.seats[seat_idx].world_actor_written
            && room.seats[seat_idx].preamble.confirmed()
        {
            Some(0usize)
        } else {
            None
        };
        let Some(ch) = peer_ch else {
            continue;
        };
        let Some((actor_id, team)) = room
            .world
            .entities
            .get(seat_idx)
            .filter(|e| e.kind == halcyon_sim::Kind::Hero)
            .map(|e| (e.id, e.team))
        else {
            continue;
        };
        let mut batch = Vec::new();
        if !room.scene_views[seat_idx].shop_base_declared
            && let Some((actor, _, message)) =
                halcyon_protocol::SoloStructure::base_shopkeeper(team)
        {
            let mut decl = vec![message];
            decl.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                actor,
            ));
            room.scene_views[seat_idx].declarations(&decl);
            room.scene_views[seat_idx].shop_base_declared = true;
            batch.extend(decl);
        }
        if river_ready && !room.scene_views[seat_idx].shop_river_declared {
            let mut decl = Vec::new();
            for right in [false, true] {
                if let Some(slot) = room.solo_slots.river_shop_slot(right) {
                    let (actor, message) =
                        halcyon_protocol::SoloStructure::river_shopkeeper(right, slot);
                    decl.push(message);
                    decl.extend(halcyon_protocol::SoloStructure::shopkeeper_opening_buffs(
                        actor,
                    ));
                }
            }
            if !decl.is_empty() {
                room.scene_views[seat_idx].declarations(&decl);
                room.scene_views[seat_idx].shop_river_declared = true;
                batch.extend(decl);
            }
        }
        let wire_actor = u32::from(messages::wire_id(actor_id));
        if !room.scene_views[seat_idx].shop_can_icon_sent {
            batch.push(halcyon_protocol::SoloStructure::shop_can_icon_message(
                wire_actor,
                0x10_0000 + seat_idx as u32,
            ));
            room.scene_views[seat_idx].shop_can_icon_sent = true;
        }
        if let Some(keeper_actor) = room.world.shop_5v5_keeper_for_actor(actor_id) {
            let due = room.scene_views[seat_idx].shop_allow_next_tick;
            if due.is_none_or(|tick| room.world.tick >= tick) {
                let seq = room.scene_views[seat_idx].shop_allow_seq;
                room.scene_views[seat_idx].shop_allow_seq = seq.wrapping_add(1);
                let instance = 0x20_0000 + (seat_idx as u32) * 0x1_0000 + (seq & 0xFFFF);
                batch.push(
                    halcyon_protocol::SoloStructure::shop_allow_purchase_message(
                        wire_actor,
                        keeper_actor,
                        instance,
                    ),
                );
                room.scene_views[seat_idx].shop_allow_active_instance = Some(instance);
                room.scene_views[seat_idx].shop_allow_next_tick =
                    Some(room.world.tick + halcyon_sim::TICK_HZ);
            }
        } else if let Some(active_instance) =
            room.scene_views[seat_idx].shop_allow_active_instance.take()
        {
            room.scene_views[seat_idx].shop_allow_next_tick = None;
            if let Ok(cancel) = (halcyon_protocol::BuffMutation {
                target: wire_actor,
                instance: active_instance,
                change: halcyon_protocol::BuffChange::Cancel,
                padding: &[],
            })
            .to_message()
            {
                batch.push(cancel);
            }
        }
        if !batch.is_empty()
            && let Some(peer) = &room.seats[seat_idx].peers[ch]
        {
            peer.send(super::transport::Delivery::Private(batch));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rooms, transport::Delivery};
    use halcyon_platform::{MatchHost, Reservation};
    use std::time::{Duration, Instant};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, hello};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    async fn login(address: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket
            .write_all(&hello("127.0.0.1").unwrap())
            .await
            .unwrap();
        let mut ack = [0; 5];
        socket.read_exact(&mut ack).await.unwrap();
        assert_eq!(ack, ACK_FRAME);
        let identity = Message::known(
            crate::Opcode::RegisterPlayerUuid,
            player.to_string().as_bytes(),
        );
        socket
            .write_all(&cipher.encode(&identity).unwrap())
            .await
            .unwrap();
        socket
    }
    async fn read_until(socket: &mut TcpStream, cipher: &MatchCipher, end: u16) -> Batch {
        timeout(Duration::from_secs(3), async {
            let mut messages = Vec::new();
            loop {
                let body = crate::transport::frame(socket).await.unwrap();
                let message = cipher.decode_owned(body).unwrap();
                let done = message.opcode == end;
                messages.push(message);
                if done {
                    return messages;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn quiet_control_disconnect_gets_the_whole_room_rejoin_window() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let room_id = Uuid::new_v4();
        let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        let seat = 2;
        let address = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: room_id,
                players: players.clone(),
                teams: Vec::new(),
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap()
            .address;
        let cipher = MatchCipher::new(&room_id.to_string());
        let mut socket = login(&address, players[seat], &cipher).await;
        read_until(&mut socket, &cipher, crate::wire_opcode::DraftRosterAral).await;
        let shared = rooms.room(room_id).unwrap();
        let previous = {
            let mut room = shared.lock().unwrap();
            // An authenticated, receive-only control connection may be quiet for
            // up to 600 seconds. Advance its last input without a wall-clock sleep.
            room.last_active = Instant::now() - Duration::from_secs(301);
            room.tick();
            assert!(!*room.stop.borrow());
            room.seats[seat].peers[0].as_ref().unwrap().generation
        };
        async fn disconnect(mut socket: TcpStream, shared: &crate::Shared, seat: usize) {
            socket.shutdown().await.unwrap();
            timeout(Duration::from_secs(2), socket.read_to_end(&mut Vec::new()))
                .await
                .unwrap()
                .unwrap();
            timeout(Duration::from_secs(2), async {
                while shared.lock().unwrap().seats[seat].peers[0].is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        disconnect(socket, &shared, seat).await;
        {
            let mut room = shared.lock().unwrap();
            room.tick();
            assert!(
                !*room.stop.borrow(),
                "room expired immediately after a quiet player disconnected"
            );
        }
        let mut fresh = login(&address, players[seat], &cipher).await;
        read_until(&mut fresh, &cipher, crate::wire_opcode::DraftRosterAral).await;
        {
            let mut room = shared.lock().unwrap();
            let generation = room.seats[seat].peers[0].as_ref().unwrap().generation;
            assert!(generation > previous);
            room.detach(seat, 0, previous);
            assert_eq!(
                room.seats[seat].peers[0].as_ref().unwrap().generation,
                generation
            );
        }
        disconnect(fresh, &shared, seat).await;
        let mut room = shared.lock().unwrap();
        room.last_active = Instant::now() - Duration::from_secs(301);
        room.detach(seat, 0, previous);
        room.tick();
        assert!(*room.stop.borrow(), "an empty room still expires");
    }
    #[tokio::test]
    async fn aral_draft_registers_new_seats_once_to_connected_players() {
        async fn registrations(socket: &mut TcpStream, cipher: &MatchCipher) -> Batch {
            let barrier = Message::known(crate::Opcode::ClockSync, [0; 6]);
            socket
                .write_all(&cipher.encode(&barrier).unwrap())
                .await
                .unwrap();
            read_until(socket, cipher, 0)
                .await
                .into_iter()
                .filter(|m| m.is(crate::Opcode::RegisterPlayerUuid))
                .collect()
        }
        fn assert_notice(messages: Batch, expected: Uuid) {
            assert_eq!(messages.len(), 1);
            let notice = &messages[0];
            assert_eq!(
                notice.fields().unwrap(),
                Fields::PlayerUuid(&expected.to_string())
            );
            assert_eq!(notice.body.len(), 70);
            assert_eq!(notice.body[36..], [0; 34]);
        }

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let room_id = Uuid::new_v4();
        let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        let address = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: room_id,
                players: players.clone(),
                teams: Vec::new(),
                mode: "casual_aral".into(),
                difficulty: None,
            })
            .unwrap()
            .address;
        let cipher = MatchCipher::new(&room_id.to_string());
        let mut a = login(&address, players[0], &cipher).await;
        read_until(&mut a, &cipher, crate::wire_opcode::DraftRosterAral).await;
        let mut b = login(&address, players[1], &cipher).await;
        assert!(
            read_until(&mut b, &cipher, crate::wire_opcode::DraftRosterAral)
                .await
                .iter()
                .all(|m| !m.is(crate::Opcode::RegisterPlayerUuid))
        );
        assert_notice(registrations(&mut a, &cipher).await, players[1]);
        assert!(registrations(&mut b, &cipher).await.is_empty());

        for denied in [players[1], Uuid::new_v4()] {
            let mut socket = login(&address, denied, &cipher).await;
            assert_eq!(
                timeout(Duration::from_secs(3), socket.read(&mut [0; 1]))
                    .await
                    .unwrap()
                    .unwrap(),
                0,
            );
        }
        assert!(registrations(&mut a, &cipher).await.is_empty());

        assert!(rooms.observe(room_id, Uuid::new_v4()).is_err());
        b.shutdown().await.unwrap();
        timeout(Duration::from_secs(3), b.read_to_end(&mut Vec::new()))
            .await
            .unwrap()
            .unwrap();
        let mut b = login(&address, players[1], &cipher).await;
        read_until(&mut b, &cipher, crate::wire_opcode::DraftRosterAral).await;
        assert!(registrations(&mut a, &cipher).await.is_empty());

        let mut c = login(&address, players[2], &cipher).await;
        read_until(&mut c, &cipher, crate::wire_opcode::DraftRosterAral).await;
        assert_notice(registrations(&mut a, &cipher).await, players[2]);
        assert_notice(registrations(&mut b, &cipher).await, players[2]);
        assert!(registrations(&mut c, &cipher).await.is_empty());
    }

    #[tokio::test]
    async fn solo_promotes_only_the_requesting_socket_and_reconnects_with_fog() {
        use halcyon_protocol::{ACK_FRAME, Fields, MatchCipher, hello};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        async fn send(socket: &mut TcpStream, cipher: &MatchCipher, m: Message) {
            socket.write_all(&cipher.encode(&m).unwrap()).await.unwrap();
        }
        async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
            let b = tokio::time::timeout(Duration::from_secs(2), crate::transport::frame(socket))
                .await
                .unwrap()
                .unwrap();
            cipher.decode_owned(b).unwrap()
        }
        async fn login(address: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket
                .write_all(&hello("127.0.0.1").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            send(
                &mut socket,
                cipher,
                Message::known(
                    crate::Opcode::RegisterPlayerUuid,
                    player.to_string().as_bytes(),
                ),
            )
            .await;
            socket
        }
        async fn until(socket: &mut TcpStream, cipher: &MatchCipher, op: u16) -> Message {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let m = next(socket, cipher).await;
                    if m.opcode == op {
                        return m;
                    }
                }
            })
            .await
            .unwrap()
        }
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("replay");
        rooms.set_replay_directory(&root).unwrap();
        let player = Uuid::new_v4();
        let id = Uuid::new_v4();
        let cipher = MatchCipher::new(&id.to_string());
        let hosted = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut socket = login(&hosted.address, player, &cipher).await;
        until(&mut socket, &cipher, crate::wire_opcode::DraftRosterFull).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::DraftLockHero, [0; 6]),
        )
        .await;
        until(&mut socket, &cipher, crate::wire_opcode::DraftLockHero).await;
        shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
        until(
            &mut socket,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        for reconnect in [false, true] {
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            )
            .await;
            // A different player's request and an early confirmation cannot enter the world.
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 221, 1, 0]),
            )
            .await;
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            )
            .await;
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .await;
            let mode = next(&mut socket, &cipher).await;
            assert_eq!(
                (mode.opcode, mode.body[68]),
                (crate::wire_opcode::SceneSettings, 1)
            );
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneLoadedDismiss
            );
            for _ in 0..10 {
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::PlayerInfo
                );
            }
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SyncWorldClock
            );
            let mut hero_ids = Vec::new();
            let mut restored = Vec::new();
            let mut variables = Vec::new();
            let mut full_structures = Vec::new();
            let mut compact_structures = Vec::new();
            loop {
                let m = next(&mut socket, &cipher).await;
                assert!(
                    !m.is(crate::Opcode::ApplyBuffCompact),
                    "scene hydration reapplied a live effect"
                );
                if m.is(crate::Opcode::RequestSceneSnapshot) {
                    break;
                }
                if m.is(crate::Opcode::EntitySpawn) {
                    let template = u32::from_be_bytes(m.body[..4].try_into().unwrap());
                    compact_structures.push(template);
                    if reconnect && matches!(template, 339..=341) {
                        let actor = u32::from_be_bytes(m.body[8..12].try_into().unwrap());
                        let expected = match actor {
                            2008 => (8, 3000f32),
                            2009 => (9, 3000.),
                            2010 => (10, 3000.),
                            2046 => (24, 4000.),
                            2051 => (25, 5000.),
                            2060 => (26, 3800.),
                            2065 => (27, 5000.),
                            2074 => (28, 4000.),
                            2079 => (29, 5000.),
                            _ => panic!("unexpected friendly tower {actor}"),
                        };
                        assert_eq!(m.body[116], expected.0);
                        assert_eq!(m.body[36..40], expected.1.to_be_bytes());
                    }
                }
                if m.is(crate::Opcode::ApplyBuffExtended) {
                    assert!(crate::structure_effects::recordable(&m));
                    let Fields::BuffApplication(effect) = m.fields().unwrap() else {
                        panic!("expected structure state");
                    };
                    restored.push((
                        effect.prefix.target,
                        effect.prefix.definition,
                        effect.extras.unwrap().stacks,
                    ));
                }
                if m.is(crate::Opcode::ModifyGameModeVar) {
                    variables.push(m.clone());
                }
                if m.is(crate::Opcode::HeroSpawn) {
                    let template = u16::from_be_bytes(m.body[2..4].try_into().unwrap());
                    if matches!(template, 295 | 296 | 339..=341) {
                        full_structures.push(template);
                    }
                    let id = u16::from_be_bytes(m.body[10..12].try_into().unwrap());
                    if (1500..1510).contains(&id) {
                        if !reconnect && id == 1500 {
                            assert_eq!(m.body[18..22], (-90.0_f32).to_be_bytes());
                            assert_eq!(m.body[22..26], 0x3d75_be29_u32.to_be_bytes());
                            assert_eq!(f32::from_be_bytes(m.body[26..30].try_into().unwrap()), 0.);
                            assert_eq!(m.body[745], 14);
                        }
                        hero_ids.push(id);
                    }
                }
            }
            if reconnect {
                assert!(variables.is_empty());
                assert_eq!(restored.len(), 49);
                assert_eq!(
                    restored
                        .iter()
                        .copied()
                        .filter(|row| row.2 == 3)
                        .collect::<Vec<_>>(),
                    [(2006, 205, 3), (2006, 180, 3)]
                );
                assert!(!restored.iter().any(|row| row.0 == 2007));
                assert!(hero_ids.len() < 10 && hero_ids.contains(&1500));
                assert!(full_structures.is_empty());
                compact_structures.sort_unstable();
                assert_eq!(
                    compact_structures,
                    [
                        295, 297, 297, 297, 339, 339, 339, 340, 340, 340, 341, 341, 341
                    ]
                );
            } else {
                assert_eq!(
                    variables,
                    mode_variables::Opening::Solo.messages().collect::<Vec<_>>()
                );
                assert_eq!(hero_ids.len(), 10);
                assert!(full_structures.is_empty());
                assert!(compact_structures.is_empty());
                assert_eq!(shared.lock().unwrap().world.tick, 0);
                let (tx, _) = mpsc::channel(8);
                assert!(
                    shared
                        .lock()
                        .unwrap()
                        .attach(player, tx, watch::channel(false).0)
                        .is_none()
                );
            }
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::ConfirmSceneOwner
            );
            let table = next(&mut socket, &cipher).await;
            assert_eq!(table.opcode, crate::wire_opcode::SceneReadinessRows);
            let Fields::ControlTable { entries, .. } = table.fields().unwrap() else {
                panic!()
            };
            assert_eq!(
                (entries[0].reference, entries[0].gate, entries[0].flag),
                (1500, 1, 0)
            );
            assert!(entries[1..10].iter().all(|r| r.gate == 1 && r.flag == 1));
            if !reconnect {
                let clock = Message::known(crate::Opcode::ClockSync, [0, 0, 0, 0, 0, 0]);
                send(&mut socket, &cipher, clock.clone()).await;
                assert_eq!(next(&mut socket, &cipher).await, clock);
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::StartMatch
                );
                let expected: Vec<_> = {
                    let room = shared.lock().unwrap();
                    room.world
                        .entities
                        .iter()
                        .filter(|e| e.team == 0 && crate::compact::structure_slot(e).is_some())
                        .map(|e| {
                            (
                                crate::compact::structure_slot(e).unwrap(),
                                u32::from(e.type_id),
                                u32::from(messages::wire_id(e.id)),
                                e.hp,
                                e.max_hp,
                            )
                        })
                        .collect()
                };
                assert_eq!(expected.len(), 13);
                let declared: std::collections::BTreeSet<_> =
                    expected.iter().map(|e| e.2).collect();
                for (slot, template, actor, hp, max_hp) in expected {
                    let m = next(&mut socket, &cipher).await;
                    assert_eq!(m.opcode, crate::wire_opcode::EntitySpawn);
                    assert_eq!(m.body.len(), 126);
                    assert_eq!(m.body[121], 1);
                    assert_eq!(m.body[116], slot);
                    assert_eq!(
                        u32::from_be_bytes(m.body[0..4].try_into().unwrap()),
                        template
                    );
                    assert_eq!(u32::from_be_bytes(m.body[8..12].try_into().unwrap()), actor);
                    assert_eq!(f32::from_be_bytes(m.body[36..40].try_into().unwrap()), hp);
                    assert_eq!(
                        f32::from_be_bytes(m.body[40..44].try_into().unwrap()),
                        max_hp
                    );
                }
                let mut applications = std::collections::BTreeMap::new();
                for _ in 0..47 {
                    let m = next(&mut socket, &cipher).await;
                    assert!(crate::structure_effects::recordable(&m));
                    let Fields::BuffApplication(effect) = m.fields().unwrap() else {
                        panic!("expected a native opening effect");
                    };
                    let target = effect.prefix.target;
                    assert!(declared.contains(&target));
                    assert!(
                        applications
                            .insert(effect.prefix.instance, target)
                            .is_none()
                    );
                }
                assert_eq!(
                    applications
                        .into_values()
                        .collect::<std::collections::BTreeSet<_>>(),
                    declared
                );
                for (i, instance) in (2176..2182).enumerate() {
                    let message = next(&mut socket, &cipher).await;
                    assert!(crate::structure_effects::recordable(&message));
                    let Fields::BuffApplication(effect) = message.fields().unwrap() else {
                        panic!("expected crystal startup");
                    };
                    assert_eq!(effect.prefix.target, 2006);
                    assert_eq!(effect.prefix.instance, instance);
                    assert_eq!(effect.prefix.definition, [205, 180][i % 2]);
                }
                let shop = next(&mut socket, &cipher).await;
                assert_eq!(shop.opcode, crate::wire_opcode::EntitySpawn);
                assert_eq!(u32::from_be_bytes(shop.body[..4].try_into().unwrap()), 329);
                assert_eq!(
                    u32::from_be_bytes(shop.body[8..12].try_into().unwrap()),
                    halcyon_protocol::SoloStructure::BASE_SHOP_LEFT_ACTOR
                );
                assert_eq!(
                    shop.body[116],
                    halcyon_protocol::SoloStructure::BASE_SHOP_LEFT_SLOT
                );
                for expected_def in [177u16, 27u16] {
                    let buff = next(&mut socket, &cipher).await;
                    assert_eq!(buff.opcode, crate::wire_opcode::ApplyBuffCompact);
                    let Fields::BuffApplication(effect) = buff.fields().unwrap() else {
                        panic!("expected shopkeeper buff");
                    };
                    assert_eq!(
                        effect.prefix.target,
                        halcyon_protocol::SoloStructure::BASE_SHOP_LEFT_ACTOR
                    );
                    assert_eq!(effect.prefix.definition, expected_def);
                }
            }
            send(
                &mut socket,
                &cipher,
                Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
            )
            .await;
            until(&mut socket, &cipher, crate::wire_opcode::SceneLoadedDismiss).await;
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneReadyConfirm
            );
            assert_eq!(
                next(&mut socket, &cipher).await.opcode,
                crate::wire_opcode::SceneLoadedDismiss
            );
            if reconnect {
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::StartMatch
                );
            } else {
                send(
                    &mut socket,
                    &cipher,
                    Message::known(crate::Opcode::ClockSync, 1.25f32.to_be_bytes()),
                )
                .await;
                loop {
                    let m = next(&mut socket, &cipher).await;
                    assert_ne!(m.opcode, crate::wire_opcode::StartMatch);
                    assert_ne!(m.opcode, crate::wire_opcode::EntitySpawn);
                    if m.is(crate::Opcode::HeroSpawn) {
                        let template = u16::from_be_bytes(m.body[2..4].try_into().unwrap());
                        assert!(!matches!(template, 295 | 296 | 339..=341));
                    }
                    if m.is(crate::Opcode::ClockSync) {
                        break;
                    }
                }
            }
            assert_eq!(shared.lock().unwrap().phase, crate::RoomPhase::Running);
            if !reconnect {
                drop(socket);
                tokio::time::timeout(Duration::from_secs(2), async {
                    while shared.lock().unwrap().seats[0].peers[0].is_some() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                {
                    let mut room = shared.lock().unwrap();
                    room.world
                        .entities
                        .find_mut(|e| e.id == 2060 - 1499)
                        .unwrap()
                        .hp = 3800.;
                }
                socket = login(&hosted.address, player, &cipher).await;
                for _ in 0..10 {
                    assert_eq!(
                        next(&mut socket, &cipher).await.opcode,
                        crate::wire_opcode::PlayerInfo
                    );
                }
                let settings = next(&mut socket, &cipher).await;
                assert_eq!(&settings.body[32..56], b"*GameMode_5v5_SoloBots*\0");
                assert_eq!(settings.opcode, crate::wire_opcode::MatchSettings);
                assert_eq!(&settings.body[..8], &[0, 0, 5, 220, 0, 0, 0, 0]);
                assert_eq!(
                    next(&mut socket, &cipher).await.opcode,
                    crate::wire_opcode::DraftTransitionToLoading
                );
            }
        }
        // Bring the previously hidden away crystal into live vision and run two
        // replication passes before the clock barrier on the same TCP stream.
        {
            let mut room = shared.lock().unwrap();
            let away = room
                .world
                .entities
                .iter()
                .find(|e| e.id == 2007 - 1499)
                .unwrap()
                .position;
            room.world.entities[0].position = away;
            room.replicate(&[]);
            room.replicate(&[]);
        }
        send(&mut socket, &cipher, Message::clock(88.0)).await;
        let mut declared = false;
        let mut crystal = Vec::new();
        for m in read_until(&mut socket, &cipher, crate::wire_opcode::ClockSync).await {
            if m.is(crate::Opcode::EntitySpawn) && m.body[8..12] == 2007_u32.to_be_bytes() {
                declared = true;
            }
            if let Ok(Fields::BuffApplication(effect)) = m.fields()
                && effect.prefix.target == 2007
            {
                assert!(declared);
                assert!(m.is(crate::Opcode::ApplyBuffExtended));
                crystal.push((
                    effect.prefix.instance,
                    effect.prefix.definition,
                    effect.extras.unwrap().stacks,
                ));
            }
        }
        assert_eq!(
            crystal,
            [
                (2040, 178, 1),
                (2041, 179, 1),
                (2170, 205, 3),
                (2171, 180, 3)
            ]
        );
        drop(socket);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let mut complete = 0;
                if let Ok(dirs) = std::fs::read_dir(root.join(id.to_string())) {
                    for dir in dirs.flatten() {
                        let path = dir.path().join("00000000.vgr.partial");
                        let Ok(bytes) = std::fs::read(path) else {
                            continue;
                        };
                        let identity = player.to_string();
                        assert!(!bytes.windows(36).any(|w| w == identity.as_bytes()));
                        let mut reader = halcyon_protocol::ReplayReader::new(bytes.as_slice());
                        let mut ops = Vec::new();
                        while let Ok(Some(record)) = reader.next_record() {
                            ops.push(record.message.opcode);
                        }
                        if ops.contains(&crate::wire_opcode::StartMatch) {
                            assert_eq!(ops.first(), Some(&crate::wire_opcode::SceneSettings));
                            assert!(
                                ops.contains(&crate::wire_opcode::SceneReadinessRows)
                                    && !ops.contains(&crate::wire_opcode::DraftRosterFull)
                            );
                            complete += 1;
                        }
                    }
                }
                if complete == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn first_valid_solo_move_only_starts_after_the_scene_write_and_changes_position() {
        for op in [
            crate::wire_opcode::RequestMoveJoystick,
            crate::wire_opcode::RequestMovePoint,
        ] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let player = Uuid::new_v4();
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
                    talent_levels: Vec::new(),
                    match_id: id,
                    players: vec![player],
                    teams: vec![0],
                    mode: "solo".into(),
                    difficulty: None,
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let mut room = shared.lock().unwrap();
            let (tx, mut rx) = mpsc::channel(16);
            room.attach(player, tx, watch::channel(false).0).unwrap();
            let locked = room
                .receive(0, 0, Message::known(crate::Opcode::DraftLockHero, [0; 6]))
                .unwrap();
            assert_eq!(
                locked.iter().map(|m| m.opcode).collect::<Vec<_>>(),
                [
                    crate::wire_opcode::DraftRosterFull,
                    crate::wire_opcode::DraftLockHero
                ]
            );
            room.phase_start = Instant::now() - Duration::from_secs(8);
            room.tick();
            rx.try_recv().unwrap();
            room.receive(
                0,
                0,
                Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
            )
            .unwrap();
            room.receive(
                0,
                0,
                Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
            )
            .unwrap();
            let target = |x: f32| {
                let mut body = [0u8; 14];
                body[..4].copy_from_slice(&x.to_be_bytes());
                Message::new(op, body)
            };
            assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
            assert_eq!(room.phase, crate::RoomPhase::Loading);
            room.seats[0].scene_written = true;
            if op == crate::wire_opcode::RequestMovePoint {
                assert!(room.receive(0, 0, target(6.)).unwrap().is_empty());
                assert_eq!(room.phase, crate::RoomPhase::Loading);
            }
            room.seats[0].world_actor_written = true;
            if op == crate::wire_opcode::RequestMovePoint {
                let mut invalid = target(6.);
                invalid.body[13] = 1;
                assert!(room.receive(0, 0, invalid).unwrap().is_empty());
                assert_eq!(room.phase, crate::RoomPhase::Loading);
            }
            assert!(room.receive(0, 0, target(f32::NAN)).unwrap().is_empty());
            assert!(room.receive(0, 0, target(10000.)).unwrap().is_empty());
            assert_eq!(room.phase, crate::RoomPhase::Loading);
            let before = room.world.entities[0].position;
            let out = room.receive(0, 0, target(6.)).unwrap();
            let expected = if op == crate::wire_opcode::RequestMovePoint {
                &[crate::wire_opcode::StartMatch, crate::wire_opcode::MoveTo][..]
            } else {
                &[crate::wire_opcode::StartMatch][..]
            };
            assert_eq!(out.iter().map(|m| m.opcode).collect::<Vec<_>>(), expected);
            if op == crate::wire_opcode::RequestMovePoint {
                assert_eq!(out[1].body, [14, 0x40, 0xc0, 0, 0, 0, 0, 0, 0]);
            }
            assert_eq!(room.phase, crate::RoomPhase::Running);
            room.tick();
            assert_ne!(room.world.entities[0].position, before);
            let repeated = room.receive(0, 0, target(6.)).unwrap();
            assert_eq!(repeated, out[1..]);
        }
    }
    #[tokio::test]
    async fn solo_build_request_after_scene_write_can_confirm_and_rehydrate() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut room = shared.lock().unwrap();
        let (tx, _rx) = mpsc::channel(16);
        room.attach(player, tx, watch::channel(false).0).unwrap();
        room.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, [0; 6]))
            .unwrap();
        room.phase_start = Instant::now() - Duration::from_secs(8);
        room.tick();
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .unwrap();
        room.receive(
            0,
            0,
            Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .unwrap();
        let request = || {
            Message::known(
                crate::Opcode::RequestRecommendedBuildPath,
                [2, 0, 0, 0, 0, 0],
            )
        };
        assert!(room.receive(0, 0, request()).unwrap().is_empty());
        room.seats[0].scene_written = true;
        for body in [vec![3], vec![0xff], vec![1, 1, 0, 0, 0, 0], vec![2, 0]] {
            assert!(
                room.receive(
                    0,
                    0,
                    Message::known(crate::Opcode::RequestRecommendedBuildPath, body)
                )
                .unwrap()
                .is_empty()
            );
        }
        assert_eq!(room.phase, crate::RoomPhase::Loading);
        let out = room.receive(0, 0, request()).unwrap();
        assert_eq!(
            out.iter().map(|m| m.opcode).collect::<Vec<_>>(),
            [
                crate::wire_opcode::RequestRecommendedBuildPath,
                crate::wire_opcode::SetRecommendedBuildPath,
                crate::wire_opcode::StartMatch
            ]
        );
        assert_eq!(out[1].body, [0, 0, 5, 220, 2]);
        assert_eq!(room.phase, crate::RoomPhase::Running);
        assert_eq!(room.receive(0, 0, request()).unwrap().len(), 2);
        let hydrated = solo_opening(&mut room, 0).unwrap();
        assert_eq!(
            hydrated
                .iter()
                .filter(|m| m.is(crate::Opcode::SetRecommendedBuildPath))
                .count(),
            1
        );
        assert_eq!(hydrated.last().unwrap(), &out[1]);
    }

    #[tokio::test]
    async fn solo_loads_selected_roster_once_and_allows_separate_data_until_scene_request() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut r = shared.lock().unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let (_, ch, _, _) = r.attach(player, tx, watch::channel(false).0).unwrap();
        assert_eq!(ch, 0);
        r.receive(0, 0, Message::known(crate::Opcode::DraftLockHero, [0; 6]))
            .unwrap();
        assert_eq!(r.phase, crate::RoomPhase::Loading);
        r.tick();
        assert!(rx.try_recv().is_err());
        r.phase_start = Instant::now() - Duration::from_secs(8);
        r.tick();
        let Delivery::Private(batch) = rx.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(batch.len(), 11);
        assert_eq!(
            batch.last().unwrap().opcode,
            crate::wire_opcode::DraftTransitionToLoading
        );
        for (i, m) in batch[..10].iter().enumerate() {
            assert_eq!(m.opcode, crate::wire_opcode::PlayerInfo);
            assert_eq!(m.body.len(), 222);
            assert_eq!(
                u32::from_be_bytes(m.body[160..164].try_into().unwrap()),
                1500 + i as u32
            );
        }
        r.last_roster = Instant::now() - Duration::from_secs(2);
        r.tick();
        assert!(rx.try_recv().is_err());
        let (tx, _) = mpsc::channel(16);
        assert_eq!(r.attach(player, tx, watch::channel(false).0).unwrap().1, 1);
        assert_eq!(r.phase, crate::RoomPhase::Running);
        assert!(!r.seats[0].control_world);
    }

    #[tokio::test]
    async fn dead_hero_solo_reconnect_sets_world_actor_written_and_respawns() {
        use halcyon_protocol::{ACK_FRAME, MatchCipher, hello};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        async fn send(socket: &mut TcpStream, cipher: &MatchCipher, m: Message) {
            socket.write_all(&cipher.encode(&m).unwrap()).await.unwrap();
        }
        async fn login_sock(address: &str, player: Uuid, cipher: &MatchCipher) -> TcpStream {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket
                .write_all(&hello("127.0.0.1").unwrap())
                .await
                .unwrap();
            let mut ack = [0; 5];
            socket.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, ACK_FRAME);
            send(
                &mut socket,
                cipher,
                Message::known(
                    crate::Opcode::RegisterPlayerUuid,
                    player.to_string().as_bytes(),
                ),
            )
            .await;
            socket
        }

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let player = Uuid::new_v4();
        let id = Uuid::new_v4();
        let cipher = MatchCipher::new(&id.to_string());
        let hosted = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut socket = login_sock(&hosted.address, player, &cipher).await;
        read_until(&mut socket, &cipher, crate::wire_opcode::DraftRosterFull).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::DraftLockHero, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::DraftLockHero).await;
        shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
        read_until(
            &mut socket,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::SceneReadinessRows).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::StartMatch).await;

        drop(socket);
        timeout(Duration::from_secs(2), async {
            while shared.lock().unwrap().seats[0].peers[0].is_some() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        {
            let mut room = shared.lock().unwrap();
            let tower_pos = room
                .world
                .entities
                .iter()
                .find(|e| e.kind == halcyon_sim::Kind::Tower && e.team == 1)
                .unwrap()
                .position;
            room.world.entities[0].position = tower_pos;
            room.world.entities[0].hp = 0.1;
            while room.world.entities[0].hp > 0.0 {
                room.tick();
            }
        }

        let mut socket = login_sock(&hosted.address, player, &cipher).await;
        read_until(
            &mut socket,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .await;
        let opening =
            read_until(&mut socket, &cipher, crate::wire_opcode::SceneReadinessRows).await;
        assert!(
            opening
                .iter()
                .all(|m| !m.is(crate::Opcode::HeroSpawn) || m.body[8..12] != 1500u32.to_be_bytes()),
            "dead hero must not be spawned in solo_opening"
        );
        // Wait briefly for the server write-completion path to record the flushed ModeName.
        timeout(Duration::from_secs(2), async {
            loop {
                let ready = {
                    let r = shared.lock().unwrap();
                    r.seats[0].scene_written && r.seats[0].world_actor_written
                };
                if ready {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(!shared.lock().unwrap().scene_views[0].knows(1));
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::StartMatch).await;

        // Advance simulation ticks while draining the TCP stream so 240 ticks of updates
        // do not overflow the 16-batch peer output queue before respawn.
        let mut saw_spawn = false;
        let mut saw_respawn = false;
        timeout(Duration::from_secs(4), async {
            while !(saw_spawn && saw_respawn) {
                if shared.lock().unwrap().world.entities[0].hp <= 0.0 {
                    shared.lock().unwrap().tick();
                }
                while let Ok(Ok(body)) = timeout(
                    Duration::from_millis(5),
                    crate::transport::frame(&mut socket),
                )
                .await
                {
                    let msg = cipher.decode_owned(body).unwrap();
                    if msg.is(crate::Opcode::HeroSpawn) && msg.body[8..12] == 1500u32.to_be_bytes()
                    {
                        saw_spawn = true;
                    }
                    if msg.is(crate::Opcode::ActorRespawn) && msg.body[..4] == 1500u32.to_be_bytes()
                    {
                        assert!(saw_spawn, "EntitySpawn must precede ActorRespawn");
                        saw_respawn = true;
                    }
                }
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn solo_fog_reentry_emits_no_duplicate_declarations_or_additive_modifiers() {
        use halcyon_protocol::{ACK_FRAME, MatchCipher, hello};
        use halcyon_sim::{BasicItem, Event, Position};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        async fn send(socket: &mut TcpStream, cipher: &MatchCipher, m: Message) {
            socket.write_all(&cipher.encode(&m).unwrap()).await.unwrap();
        }

        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let player = Uuid::new_v4();
        let id = Uuid::new_v4();
        let cipher = MatchCipher::new(&id.to_string());
        let hosted = rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: "5v5_bots_solo".into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let mut socket = TcpStream::connect(&hosted.address).await.unwrap();
        socket
            .write_all(&hello("127.0.0.1").unwrap())
            .await
            .unwrap();
        let mut ack = [0; 5];
        socket.read_exact(&mut ack).await.unwrap();
        assert_eq!(ack, ACK_FRAME);
        send(
            &mut socket,
            &cipher,
            Message::known(
                crate::Opcode::RegisterPlayerUuid,
                player.to_string().as_bytes(),
            ),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::DraftRosterFull).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::DraftLockHero, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::DraftLockHero).await;
        shared.lock().unwrap().phase_start = Instant::now() - Duration::from_secs(8);
        read_until(
            &mut socket,
            &cipher,
            crate::wire_opcode::DraftTransitionToLoading,
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
        )
        .await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::SceneReadinessRows).await;
        send(
            &mut socket,
            &cipher,
            Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
        )
        .await;
        read_until(&mut socket, &cipher, crate::wire_opcode::StartMatch).await;
        // Run the first authoritative tick so `initial_structures` and `solo_structure_snapshot` start.
        shared.lock().unwrap().tick();
        send(&mut socket, &cipher, Message::clock(5.0)).await;
        read_until(&mut socket, &cipher, crate::wire_opcode::ClockSync).await;

        let (home_pos, away_crystal_pos, enemy_pos) = {
            let mut r = shared.lock().unwrap();
            let home_pos = r.world.entities[0].position;
            let away_crystal_pos = r
                .world
                .entities
                .iter()
                .find(|e| e.id == 2007 - 1499)
                .unwrap()
                .position;
            r.world.entities[1].gold = 600;
            r.world.purchase(2, BasicItem::WeaponBlade).unwrap();
            r.world.entities[1].position = away_crystal_pos;
            // First sight of enemy hero 1501 (already declared in initial solo_opening) and away crystal 2007.
            r.world.entities[0].position = away_crystal_pos;
            r.replicate(&[Event::Move {
                id: 2,
                position: away_crystal_pos,
            }]);
            (home_pos, away_crystal_pos, away_crystal_pos)
        };
        send(&mut socket, &cipher, Message::clock(11.0)).await;
        let first_sight = read_until(&mut socket, &cipher, crate::wire_opcode::ClockSync).await;
        assert!(
            first_sight.iter().all(|m| {
                !(m.is(crate::Opcode::HeroSpawn) && m.body[8..12] == 1501u32.to_be_bytes())
                    && !m.is(crate::Opcode::ModifyActorAttribute)
                    && !m.is(crate::Opcode::ItemGrant)
            }),
            "enemy hero declared at solo_opening must not re-declare on first sighting"
        );
        assert!(
            first_sight.iter().any(|m| {
                m.is(crate::Opcode::MoveToCorrection) && m.body[..4] == 1501u32.to_be_bytes()
            }),
            "enemy hero entering vision must still receive 0x042E movement"
        );
        assert_eq!(
            first_sight
                .iter()
                .filter(
                    |m| m.is(crate::Opcode::EntitySpawn) && m.body[8..12] == 2007u32.to_be_bytes()
                )
                .count(),
            1
        );
        assert_eq!(
            first_sight
                .iter()
                .filter(|m| {
                    m.is(crate::Opcode::ApplyBuffExtended) && m.body[..4] == 2007u32.to_be_bytes()
                })
                .count(),
            4
        );

        // Return to fog, then re-enter vision of both 1501 and 2007.
        {
            let mut r = shared.lock().unwrap();
            r.world.entities[0].position = home_pos;
            r.replicate(&[]);
            assert!(!r.visible[0].contains(&2));
            assert!(!r.visible[0].contains(&(2007 - 1499)));

            let moved = Position {
                x: enemy_pos.x + 1.0,
                y: enemy_pos.y,
            };
            r.world.entities[0].position = away_crystal_pos;
            r.world.entities[1].position = moved;
            r.replicate(&[Event::Move {
                id: 2,
                position: moved,
            }]);
        }
        send(&mut socket, &cipher, Message::clock(22.0)).await;
        let reentry = read_until(&mut socket, &cipher, crate::wire_opcode::ClockSync).await;
        assert!(
            reentry.iter().all(|m| !matches!(
                m.opcode,
                crate::wire_opcode::HeroSpawn
                    | crate::wire_opcode::EntitySpawn
                    | crate::wire_opcode::ModifyActorAttribute
                    | crate::wire_opcode::ItemGrant
                    | crate::wire_opcode::ApplyBuffExtended
            )),
            "fog re-entry emitted duplicate declarations/modifiers: {:?}",
            reentry.iter().map(|m| m.opcode).collect::<Vec<_>>()
        );
        assert!(
            reentry
                .iter()
                .any(|m| m.is(crate::Opcode::MoveToCorrection)
                    && m.body[..4] == 1501u32.to_be_bytes())
        );
    }

    fn setup_confirmed_solo_room(
        mode: &str,
    ) -> (
        crate::Rooms,
        Uuid,
        tokio::sync::mpsc::Receiver<crate::transport::Delivery>,
    ) {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        let player = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                talent_levels: Vec::new(),
                match_id: id,
                players: vec![player],
                teams: vec![0],
                mode: mode.into(),
                difficulty: None,
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let (tx, rx) = mpsc::channel(4096);
        {
            let mut r = shared.lock().unwrap();
            r.admitted = true;
            r.phase(crate::RoomPhase::Running);
            r.seats[0].control_world = true;
            r.seats[0].world_actor_written = true;
            r.seats[0].preamble.observe(
                &Message::known(crate::Opcode::RequestSceneSnapshot, [0; 6]),
                1500,
            );
            r.seats[0].preamble.observe(
                &Message::known(crate::Opcode::ConfirmSceneOwner, [0, 0, 5, 220, 1, 0]),
                1500,
            );
            r.seats[0].preamble.observe(
                &Message::known(crate::Opcode::SceneReadyConfirm, [0; 6]),
                1500,
            );
            r.seats[0].peers[0] = Some(crate::Peer {
                generation: 1,
                tx,
                stalled: watch::channel(false).0,
            });
        }
        (rooms, id, rx)
    }

    #[tokio::test]
    async fn solo_5v5_destroys_dead_minions_via_040b_and_recycles_slots_past_three_minutes() {
        let (rooms, id, mut rx) = setup_confirmed_solo_room("5v5_bots_solo");
        let shared = rooms.room(id).unwrap();
        {
            let mut r = shared.lock().unwrap();
            r.world.use_solo_mid_lane().unwrap();
            // Position human hero in mid lane so wave clashes are continuously visible,
            // and fast-forward past opening crystal grace to accumulate 3.5+ minutes of waves.
            r.world.entities[0].position = halcyon_sim::Position { x: 0.0, y: 0.0 };
            r.world.entities[0].hp = 50_000.0;
            r.world.entities[0].max_hp = 50_000.0;
        }
        let mut live_slots: std::collections::BTreeMap<u8, u32> = std::collections::BTreeMap::new();
        let mut live_actors: std::collections::BTreeMap<u32, u8> =
            std::collections::BTreeMap::new();
        let mut destroyed = 0usize;
        let mut dead_announcements = 0usize;
        for _ in 0..4200 {
            shared.lock().unwrap().tick();
            while let Ok(delivery) = rx.try_recv() {
                for m in delivery.into_records() {
                    if m.is(crate::Opcode::EntitySpawn) && m.body.len() == 122 {
                        let slot = m.body[116];
                        if slot >= 52 {
                            let actor = u32::from_be_bytes(m.body[8..12].try_into().unwrap());
                            if let Some(prev) = live_slots.insert(slot, actor) {
                                assert_eq!(
                                    prev, actor,
                                    "slot {slot} reused for actor {actor} before 0x040B destroyed {prev}"
                                );
                            }
                            live_actors.insert(actor, slot);
                            assert!(
                                live_slots.len() < 148,
                                "client pool grew unboundedly ({} live slots)",
                                live_slots.len()
                            );
                        }
                    } else if m.is(crate::Opcode::ActorDead) {
                        dead_announcements += 1;
                    } else if m.is(crate::Opcode::DestroyEntity) {
                        destroyed += 1;
                        let actor = u32::from_be_bytes(m.body[..4].try_into().unwrap());
                        if let Some(slot) = live_actors.remove(&actor) {
                            live_slots.remove(&slot);
                        }
                    }
                }
            }
        }
        assert!(
            destroyed >= 10,
            "expected sustained 0x040B DestroyEntity releases over 3.5 minutes, saw {destroyed}"
        );
        assert!(dead_announcements >= destroyed);

        // Verify a minion declared earlier that later dies in fog ALSO emits 0x040B DestroyEntity.
        let fog_minion = {
            let mut r = shared.lock().unwrap();
            let minion_id = r
                .world
                .entities
                .iter()
                .find(|e| e.kind == halcyon_sim::Kind::Minion && e.team == 1)
                .map(|e| e.id)
                .unwrap();
            let home_pos = halcyon_sim::Position { x: 0.0, y: 0.0 };
            r.world.entities[0].position = home_pos;
            r.world
                .entities
                .find_mut(|e| e.id == minion_id)
                .unwrap()
                .position = home_pos;
            r.tick();
            r.world
                .entities
                .find_mut(|e| e.id == minion_id)
                .unwrap()
                .position = halcyon_sim::Position { x: 80.0, y: 80.0 };
            r.tick();
            // Simulate World::step() removing a minion that died while in fog of war.
            let fog_death = [halcyon_sim::Event::Death {
                id: minion_id,
                killer: 2,
            }];
            r.world.entities.retain(|e| e.id != minion_id);
            crate::aral_death::schedule(&mut r, &fog_death);
            r.replicate(&fog_death);
            minion_id
        };
        while rx.try_recv().is_ok() {}
        let mut saw_fog_destroy = false;
        let wire_fog = u32::from(messages::wire_id(fog_minion));
        for _ in 0..85 {
            shared.lock().unwrap().tick();
            while let Ok(delivery) = rx.try_recv() {
                for m in delivery.into_records() {
                    if m.is(crate::Opcode::DestroyEntity) && m.body[..4] == wire_fog.to_be_bytes() {
                        saw_fog_destroy = true;
                    }
                }
            }
        }
        assert!(
            saw_fog_destroy,
            "minion killed in fog must still emit 0x040B"
        );
    }

    #[tokio::test]
    async fn solo_and_5v5_shops_grant_store_access_buff_174_and_cancel_on_exit() {
        let (rooms, id, mut rx) = setup_confirmed_solo_room("5v5_bots_solo");
        let shared = rooms.room(id).unwrap();
        {
            let mut r = shared.lock().unwrap();
            r.world.use_sovereigns_rise_5v5(None).unwrap();
        }
        let drain_all = |rx: &mut mpsc::Receiver<Delivery>| -> Vec<Message> {
            let mut out = Vec::new();
            while let Ok(d) = rx.try_recv() {
                out.extend(d.into_records());
            }
            out
        };

        // Tick 0 at base: expect BaseShop 329 (6530) + Buff 177/27 + Buff 173 + Buff 174 on hero 1500.
        shared.lock().unwrap().tick();
        let open_msgs = drain_all(&mut rx);
        assert!(open_msgs.iter().any(|m| {
            m.is(crate::Opcode::EntitySpawn)
                && u32::from_be_bytes(m.body[..4].try_into().unwrap()) == 329
                && u32::from_be_bytes(m.body[8..12].try_into().unwrap())
                    == halcyon_protocol::SoloStructure::BASE_SHOP_LEFT_ACTOR
        }));
        let mut saw_173 = false;
        let mut allow_inst = None;
        for m in &open_msgs {
            if let Ok(Fields::BuffApplication(b)) = m.fields()
                && b.prefix.target == 1500
            {
                if b.prefix.definition == 173 {
                    saw_173 = true;
                }
                if b.prefix.definition == 174 {
                    assert_eq!(
                        b.prefix.source,
                        halcyon_protocol::SoloStructure::BASE_SHOP_LEFT_ACTOR
                    );
                    assert_eq!(b.prefix.duration_bits, 0x3e00);
                    assert_eq!(b.extras.unwrap().words[2], 451);
                    allow_inst = Some(b.prefix.instance);
                }
            }
        }
        assert!(saw_173, "expected Buff_Shop_CanShopIcon (173)");
        let allow_inst = allow_inst.expect("expected Buff_Shop_AllowStorePurchase (174)");

        // Walk outside shop -> expect CancelBuff (0x0445) for the active Buff 174 instance.
        {
            let mut r = shared.lock().unwrap();
            r.world.entities[0].position = halcyon_sim::Position { x: 0.0, y: 0.0 };
        }
        shared.lock().unwrap().tick();
        let exit_msgs = drain_all(&mut rx);
        assert!(
            exit_msgs.iter().any(|m| {
                matches!(
                    m.buff_mutation(),
                    Some(muta)
                        if muta.target == 1500
                            && muta.instance == allow_inst
                            && muta.change == halcyon_protocol::BuffChange::Cancel
                )
            }),
            "expected CancelBuff when leaving shop range"
        );

        // At 4:00 (RIVER_SHOP_SPAWN_TICK), RiverShop 330 (6532/6533) spawns and LRiverShop grants 174.
        {
            let mut r = shared.lock().unwrap();
            r.world.tick = halcyon_sim::map_5v5::RIVER_SHOP_SPAWN_TICK;
            r.world.entities[0].position = halcyon_sim::Position { x: -16.3, y: -33.6 };
        }
        shared.lock().unwrap().tick();
        let river_msgs = drain_all(&mut rx);
        let river_decls: Vec<_> = river_msgs
            .iter()
            .filter(|m| {
                m.is(crate::Opcode::EntitySpawn)
                    && u32::from_be_bytes(m.body[..4].try_into().unwrap()) == 330
            })
            .map(|m| u32::from_be_bytes(m.body[8..12].try_into().unwrap()))
            .collect();
        assert_eq!(
            river_decls,
            [
                halcyon_protocol::SoloStructure::RIVER_SHOP_LEFT_ACTOR,
                halcyon_protocol::SoloStructure::RIVER_SHOP_RIGHT_ACTOR,
            ]
        );
        assert!(river_msgs.iter().any(|m| {
            matches!(
                m.fields(),
                Ok(Fields::BuffApplication(b))
                    if b.prefix.target == 1500
                        && b.prefix.source
                            == halcyon_protocol::SoloStructure::RIVER_SHOP_LEFT_ACTOR
                        && b.prefix.definition == 174
            )
        }));
    }

    #[tokio::test]
    async fn enemy_turrets_2011_and_2012_spawn_compact_03f2_after_tick_zero() {
        let (rooms, id, mut rx) = setup_confirmed_solo_room("5v5_bots_solo");
        let shared = rooms.room(id).unwrap();
        {
            let mut r = shared.lock().unwrap();
            r.world.use_solo_mid_lane().unwrap();
        }
        shared.lock().unwrap().tick();
        while rx.try_recv().is_ok() {}

        {
            let mut r = shared.lock().unwrap();
            r.world.tick = 1800;
            for (wire, hp) in [(2011u32, 2438.125f32), (2012u32, 2750.0f32)] {
                let e = r.world.entities.find_mut(|e| e.id == wire - 1499).unwrap();
                e.hp = hp;
            }
            let pos_2012 = r
                .world
                .entities
                .iter()
                .find(|e| e.id == 2012 - 1499)
                .unwrap()
                .position;
            r.world.entities[0].position = pos_2012;
            r.tick();
            let pos_2011 = r
                .world
                .entities
                .iter()
                .find(|e| e.id == 2011 - 1499)
                .unwrap()
                .position;
            r.world.entities[0].position = pos_2011;
            r.tick();
        }

        let mut seen = Vec::new();
        while let Ok(d) = rx.try_recv() {
            for m in d.into_records() {
                if m.is(crate::Opcode::HeroSpawn) {
                    let tpl = u16::from_be_bytes(m.body[2..4].try_into().unwrap());
                    assert!(
                        !matches!(tpl, 295..=297 | 339..=341),
                        "structure template {tpl} fell back to 0x03F3"
                    );
                }
                if m.is(crate::Opcode::EntitySpawn) && m.body.len() == 122 {
                    let actor = u32::from_be_bytes(m.body[8..12].try_into().unwrap());
                    if matches!(actor, 2011 | 2012) {
                        let tpl = u32::from_be_bytes(m.body[..4].try_into().unwrap());
                        let hp = f32::from_be_bytes(m.body[36..40].try_into().unwrap());
                        seen.push((actor, tpl, m.body[116], hp));
                    }
                }
            }
        }
        seen.sort_by_key(|row| row.0);
        assert_eq!(seen, [(2011, 340, 11, 2438.125), (2012, 340, 12, 2750.0)]);
    }
}
