use super::*;
use halcyon_community::Community;
use halcyon_data::{GameData, Item, ManifestEntry, Unit};
use halcyon_protocol::{EquippedTalent, Fields, LiveFields, TalentSelection};
use std::fs;

fn idris_only() -> Arc<GameData> {
    let (units, manifest): (Vec<_>, Vec<_>) = [
        ("Ringo", 243),
        ("OuterTurret5v5", 4),
        ("VainCrystal_Home_5v5", 5),
        ("VainCrystal_Away_5v5", 6),
        ("5v5_Minion_Melee", 7),
        ("5v5_Minion_Range", 8),
        ("5v5_Minion_Siege", 9),
        ("5v5_Minion_Captain", 10),
        ("Idris", 273),
    ]
    .into_iter()
    .map(|(name, template)| {
        (
            Unit {
                name: name.into(),
                hp: 700.,
                energy: 0.,
                weapon: 80.,
                armor: 20.,
                shield: 20.,
                range: 3.,
                attack_speed: 1.,
                move_speed: 4.,
                hp_per_level: 10.,
                energy_per_level: 0.,
                weapon_per_level: 2.,
                armor_per_level: 1.,
                shield_per_level: 1.,
                other: BTreeMap::new(),
            },
            ManifestEntry {
                name: name.into(),
                entity_type_id: template,
                pair_index: 0,
                path: format!("build://Characters/{name}"),
            },
        )
    })
    .unzip();
    let items = [
        ("WeaponBlade", 4),
        ("LightArmor", 7),
        ("LightShield", 8),
        ("Oakheart", 0),
        ("EnergyBattery", 2),
    ]
    .into_iter()
    .map(|(name, stat)| Item {
        id: format!("Item_{name}"),
        name: name.into(),
        cost_a: Some(300),
        cost_b: None,
        other: [(
            "bonuses".into(),
            serde_json::to_value((ItemBonus {
                stat_id: stat,
                value: 10,
            },))
            .unwrap(),
        )]
        .into(),
    })
    .collect();
    Arc::new(GameData {
        extra: Default::default(),
        units,
        items,
        manifest,
        rules: Vec::new(),
        official_rules: sample("aral_rules"),
        abilities: Vec::new(),
        combat_constants: serde_json::to_value(EmptyReply {}).unwrap(),
        content_digest: "synthetic-aral-idris".into(),
        hero_names: vec!["Idris".into()],
        skin_names: Vec::new(),
    })
}

fn content(path: &Path) -> (PathBuf, [String; 3]) {
    let mut rows = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    let mut items = Vec::new();
    let talent_ids = [
        ("rare", "PathOfTheSeeker"),
        ("epic", "ChaChaChakram"),
        ("legendary", "ShimmerHeal"),
    ]
    .map(|(tier, definition)| {
        let id = format!("idris_{tier}");
        let fragments = format!("{id}_fragments");
        rows.insert(
            id.clone(),
            PlayerTalent {
                id: id.clone(),
                hero: "*Idris*",
                def: format!("*Talent_Idris_{definition}*"),
                level: 0,
                max_level: false,
                level_requirements: EmptyReply {},
            },
        );
        bindings.insert(
            id.clone(),
            TalentBinding {
                level_item: id.clone(),
                fragment_item: fragments.clone(),
            },
        );
        for item in [&id, &fragments] {
            items.push(ContentItem {
                id: item.clone(),
                name: item.clone(),
                slot: "test",
            });
        }
        id
    });
    let write = |name: &str, value: Value| {
        fs::write(path.join(name), value.to_string()).unwrap();
    };
    write(
        "official_getTalentsData.json",
        serde_json::to_value(ArchivedReply {
            return_value: &rows,
        })
        .unwrap(),
    );
    write(
        "official_getSkinManifest.json",
        serde_json::to_value(ArchivedReply { return_value: "{}" }).unwrap(),
    );
    write(
        "content.json",
        serde_json::to_value(TalentContent {
            items,
            talent_progress: bindings,
        })
        .unwrap(),
    );
    (path.join("content.json"), talent_ids)
}

async fn write(socket: &mut TcpStream, cipher: &MatchCipher, frame: Message) {
    socket
        .write_all(&cipher.encode(&frame).unwrap())
        .await
        .unwrap();
}

async fn next(socket: &mut TcpStream, cipher: &MatchCipher) -> Message {
    let mut body = vec![0; socket.read_u16().await.unwrap() as usize];
    socket.read_exact(&mut body).await.unwrap();
    cipher.decode(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pooled_aral_reroll_rules_split_tickets_and_select_only_compatible_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let (configs, listeners) = pool::configs(tmp.path());
    let host_id = configs[2].node_id;
    let mut members = Vec::new();
    for (index, (config, listener)) in configs.iter().zip(listeners).enumerate() {
        let member =
            Member::start(tmp.path(), index, config.clone(), Some(listener), host_id).await;
        member.rooms.set_free_aral_reroll(index != 1);
        members.push(Some(member));
    }
    let (leader, _) = elected(&members, 1).await;
    wait_replication(&members, leader).await;
    let reader = node(&members, leader);
    let join = |index: usize| {
        let member = node(&members, index);
        async move {
            let guest = rpc(
                &member.rpc,
                RpcMethod::CreateGuestPlayer.as_str(),
                android_guest("device", "0"),
            )
            .await;
            let session = rpc(
                &member.rpc,
                RpcMethod::StartSessionForPlayer.as_str(),
                (&guest["returnValue"]["playerUUID"],),
            )
            .await;
            rpc(
                &member.rpc,
                RpcMethod::JoinLobby.as_str(),
                (
                    &session["sessionToken"],
                    LobbyJoin {
                        lobby: "casual_aral",
                    },
                    "147219",
                ),
            )
            .await;
        }
    };
    for index in [0, 0, 0, 1, 1, 1] {
        join(index).await;
    }
    let mixed = wait_pool(reader, |pool| pool.tickets().len() == 6).await;
    assert!(mixed.allocations().is_empty());
    let legacy = format!(
        "{}validation-v1",
        halcyon_platform::ARAL_TALENT_RULES_PREFIX
    );
    let reroll = format!(
        "{}free-aral-reroll:validation-v1",
        halcyon_platform::ARAL_TALENT_RULES_PREFIX
    );
    for entry in mixed.tickets().values() {
        let ticket = &entry.ticket;
        assert_eq!(
            ticket.queue.rules_digest,
            if ticket.home == configs[1].node_id {
                &legacy
            } else {
                &reroll
            }
            .as_str()
        );
    }
    for _ in 0..3 {
        join(2).await;
    }
    let offered = wait_pool(reader, |pool| {
        pool.allocations()
            .values()
            .any(|a| a.endpoint.is_some() || a.phase == Phase::Aborted)
    })
    .await;
    assert_eq!(offered.allocations().len(), 1);
    let allocation = offered.allocations().values().next().unwrap();
    assert_eq!(allocation.host, host_id);
    assert_eq!(allocation.phase, Phase::Preparing);
    assert!(allocation.endpoint.is_some());
    assert_eq!(allocation.tickets.len(), 6);
    for ticket in &allocation.tickets {
        assert_eq!(
            offered.tickets()[&ticket.id].ticket.queue.rules_digest,
            reroll
        );
    }
    assert_eq!(
        offered
            .tickets()
            .values()
            .filter(|e| e.phase == TicketPhase::Queued)
            .count(),
        3
    );
    for member in members.into_iter().flatten() {
        member.peer_play.shutdown().await;
        member.raft.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_home_aral_equips_only_committed_numeric_entitlements() {
    let tmp = tempfile::tempdir().unwrap();
    let (configs, listeners) = pool::configs(tmp.path());
    let host_id = configs[2].node_id;
    let (content_path, talent_ids) = content(tmp.path());
    let data = idris_only();
    halcyon_sim::World::from_data(1, 6, &data).expect("valid synthetic Idris fixture");
    let mut cluster = Vec::new();
    for (index, (config, listener)) in configs.iter().zip(listeners).enumerate() {
        let mut member = Member::start_with_data(
            tmp.path(),
            index,
            config.clone(),
            Some(listener),
            host_id,
            Some(data.clone()),
        )
        .await;
        let platform = Platform::with_peers(
            member.app.clone(),
            CeConfig {
                authority: "localhost:8443".into(),
                notify_authority: "localhost:2112".into(),
            },
            &[index as u8 + 40; 32],
            &member.path,
            Some(tmp.path()),
            member.peer_play.clone(),
        )
        .unwrap()
        .with_content(&content_path)
        .await
        .unwrap();
        member.rpc = platform.rpc().merge(platform.public_api());
        cluster.push(Some(member));
    }
    let (leader, _) = elected(&cluster, 1).await;
    wait_replication(&cluster, leader).await;

    let mut guests = Vec::new();
    for (index, level) in [Some(7u8), Some(4), None, None, None, None]
        .into_iter()
        .enumerate()
    {
        let home = index % 2;
        let member = node(&cluster, home);
        let guest = rpc(
            &member.rpc,
            RpcMethod::CreateGuestPlayer.as_str(),
            android_guest("device", "0"),
        )
        .await;
        let session = rpc(
            &member.rpc,
            RpcMethod::StartSessionForPlayer.as_str(),
            (&guest["returnValue"]["playerUUID"],),
        )
        .await;
        let player: Uuid =
            serde_json::from_value(session["returnValue"]["playerUUID"].clone()).unwrap();
        if let Some(level) = level {
            let inventory = Community::open(&member.path).unwrap();
            for item in &talent_ids {
                inventory
                    .award_item(Uuid::new_v4(), player, item.clone(), u32::from(level))
                    .await
                    .unwrap();
            }
        }
        let token = session["sessionToken"].clone();
        let request = serde_json::to_value((
            &token,
            LobbyJoin {
                lobby: "casual_aral",
            },
            "147219",
        ))
        .unwrap();
        rpc(&member.rpc, RpcMethod::JoinLobby.as_str(), request.clone()).await;
        if index == 0 {
            // An identical home retry must retain one entitlement for the same seat.
            rpc(&member.rpc, RpcMethod::JoinLobby.as_str(), request).await;
        }
        guests.push((home, player, token, level));
    }
    let offered = wait_pool(node(&cluster, leader), |p| {
        p.allocations()
            .values()
            .any(|a| a.endpoint.is_some() || a.phase == Phase::Aborted)
    })
    .await;
    let proposal = offered.allocations().values().next().unwrap();
    assert_eq!(proposal.host, host_id);
    assert_eq!(proposal.phase, Phase::Preparing, "{proposal:?}");
    for (home, _, token, _) in &guests {
        let home_rpc = &node(&cluster, *home).rpc;
        state(home_rpc, token, LobbyState::MatchedPartners).await;
        let pending = rpc(home_rpc, RpcMethod::QueryPendingMatch.as_str(), (&token,)).await;
        assert_eq!(pending["returnValue"]["matchId"], proposal.id.to_string());
        rpc(home_rpc, RpcMethod::AcceptMatch.as_str(), (&token,)).await;
    }
    let pool = wait_pool(node(&cluster, leader), |p| {
        p.allocations().values().any(|a| a.phase == Phase::Ready)
    })
    .await;
    assert_eq!(pool.allocations().len(), 1);
    let allocation = pool.allocations().values().next().unwrap();
    assert_eq!(allocation.host, host_id);
    assert_eq!(allocation.tickets.len(), 6);
    assert_eq!(pool.tickets().len(), 6);

    let (action_home, action_player, action_session, _) = guests[0].clone();
    let mut sockets = Vec::new();
    let cipher = MatchCipher::new(&allocation.id.to_string());
    for (home, player, token, expected) in guests {
        let playing = state(&node(&cluster, home).rpc, &token, LobbyState::Playing).await;
        assert_eq!(playing["matchId"], allocation.id.to_string());
        let address = format!("{}:{}", playing["host"].as_str().unwrap(), playing["port"]);
        let mut socket = login(&address, player, &cipher).await;
        let m = until(&mut socket, &cipher, wire_opcode::DraftRosterAral).await;
        let Fields::AralRoster(roster) = m.fields().unwrap() else {
            panic!("missing ARAL roster")
        };
        let seat = (0..6)
            .find(|&seat| roster.identity(seat) == Some(player.to_string().as_bytes()))
            .unwrap();
        let actor = 1500 + seat as u32;
        assert_eq!(roster.slot(seat).unwrap().template, 273);
        sockets.push((socket, actor, expected));
    }
    let mut loading = tokio::task::JoinSet::new();
    for (mut socket, actor, expected) in sockets.drain(..) {
        let cipher = MatchCipher::new(&allocation.id.to_string());
        loading.spawn(async move {
            // Every client drains its draft refreshes during the native 20-second timer.
            while next(&mut socket, &cipher).await.opcode != wire_opcode::DraftTransitionToLoading {
            }
            (socket, actor, expected)
        });
    }
    if timeout(Duration::from_secs(24), async {
        while let Some(result) = loading.join_next().await {
            sockets.push(result.unwrap());
        }
    })
    .await
    .is_err()
    {
        panic!(
            "ARAL draft did not enter world loading: {:?}",
            node(&cluster, 2).rooms.detail(allocation.id)
        );
    }

    for (socket, actor, _) in &mut sockets {
        write(
            socket,
            &cipher,
            Message::new(wire_opcode::RequestSceneSnapshot, [0; 6]),
        )
        .await;
        let mut ready = [0; 6];
        ready[..4].copy_from_slice(&actor.to_be_bytes());
        ready[4] = 1;
        write(
            socket,
            &cipher,
            Message::new(wire_opcode::ConfirmSceneOwner, ready),
        )
        .await;
        until(socket, &cipher, wire_opcode::ConfirmSceneOwner).await;
        write(
            socket,
            &cipher,
            Message::new(wire_opcode::SceneReadyConfirm, [0; 6]),
        )
        .await;
        until(socket, &cipher, wire_opcode::SceneReadyConfirm).await;
    }
    for (socket, _, _) in &mut sockets {
        until(socket, &cipher, wire_opcode::StartMatch).await;
    }
    for (socket, actor, expected) in &mut sockets {
        let selection = TalentSelection::Slot1;
        let barrier = Message::known(halcyon_protocol::Opcode::ClockSync, [0; 6]);
        write(socket, &cipher, selection.request()).await;
        write(socket, &cipher, barrier.clone()).await;
        let received = timeout(Duration::from_secs(4), async {
            let mut own = Vec::new();
            loop {
                let frame = next(socket, &cipher).await;
                if frame == barrier {
                    return own;
                }
                if let Fields::Live(LiveFields::TalentEquipped(equip)) = frame.fields().unwrap()
                    && equip.actor == *actor
                {
                    own.push(equip);
                }
            }
        })
        .await
        .unwrap();
        let expected = expected.map(|level| EquippedTalent {
            actor: *actor,
            selection,
            level,
        });
        assert_eq!(received, expected.into_iter().collect::<Vec<_>>());
    }
    {
        use axum::http::StatusCode;
        use halcyon_protocol::player_actions::{ActionRequest, PlayerAction};
        let member = node(&cluster, action_home);
        let secret = Uuid::new_v4().to_string();
        rpc(
            &member.rpc,
            RpcMethod::LinkAccount.as_str(),
            (&action_session, "LocalActionAccount", &secret),
        )
        .await;
        let public_guest = member
            .app
            .login_account("LocalActionAccount", secret)
            .await
            .unwrap();
        assert_eq!(action_player, public_guest.player_id);
        let router = &node(&cluster, action_home).rpc;
        let token = Some(public_guest.session_token.as_str());
        let route = format!("/api/v1/matches/{}/actions", allocation.id);
        let private = rest(router, "GET", &route, token, Value::Null, StatusCode::OK).await;
        let req = ActionRequest {
            sequence: private["nextSequence"].as_u64().unwrap(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 25_000,
            action: PlayerAction::DeployVisionTotem {
                instance: 2001,
                x: private["x"].as_f64().unwrap() as f32,
                z: private["z"].as_f64().unwrap() as f32,
            },
        };
        let payload = serde_json::to_value(req).unwrap();
        let call = || {
            rest(
                router,
                "POST",
                &route,
                token,
                payload.clone(),
                StatusCode::NO_CONTENT,
            )
        };
        tokio::join!(call(), call());
        let next = rest(router, "GET", &route, token, Value::Null, StatusCode::OK).await;
        assert_eq!(next["nextSequence"], req.sequence + 1);
        // The retry spent no second charge. A second, fresh placement succeeds; a third fails.
        for (offset, code) in [(1, StatusCode::NO_CONTENT), (2, StatusCode::CONFLICT)] {
            rest(
                router,
                "POST",
                &route,
                token,
                serde_json::to_value(ActionRequest {
                    sequence: req.sequence + offset,
                    ..req
                })
                .unwrap(),
                code,
            )
            .await;
        }
    }
    for member in cluster.into_iter().flatten() {
        member.stop().await;
    }
}

#[derive(serde::Serialize)]
struct LobbyJoin<'a> {
    lobby: &'a str,
}

#[derive(serde::Serialize)]
struct TalentContent {
    items: Vec<ContentItem>,
    #[serde(rename = "talentProgress")]
    talent_progress: BTreeMap<String, TalentBinding>,
}

#[derive(serde::Serialize)]
struct ArchivedReply<T> {
    #[serde(rename = "returnValue")]
    return_value: T,
}

#[derive(serde::Serialize)]
struct ContentItem {
    id: String,
    name: String,
    slot: &'static str,
}

#[derive(serde::Serialize)]
struct TalentBinding {
    #[serde(rename = "levelItem")]
    level_item: String,
    #[serde(rename = "fragmentItem")]
    fragment_item: String,
}

#[derive(serde::Serialize)]
struct PlayerTalent {
    id: String,
    hero: &'static str,
    def: String,
    level: u32,
    #[serde(rename = "maxLevel")]
    max_level: bool,
    #[serde(rename = "levelRequirements")]
    level_requirements: EmptyReply,
}

#[derive(serde::Serialize)]
struct EmptyReply {}

#[derive(serde::Serialize)]
struct ItemBonus {
    stat_id: u32,
    value: u32,
}

fn sample(name: &str) -> serde_json::Value {
    let samples: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/aral_talent_pool-samples.json")).unwrap();
    samples.get(name).unwrap().clone()
}
