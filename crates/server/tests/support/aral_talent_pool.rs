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
        other: [("bonuses".into(), json!([{"stat_id":stat,"value":10}]))].into(),
    })
    .collect();
    Arc::new(GameData {
        units,
        items,
        manifest,
        rules: Vec::new(),
        official_rules: json!({
            "five_v_five":{"waves":{"first_spawn_s":45,"interval_s":30}},
            "patch_413_ability_deltas":{"changes":[
                {"hero":"Idris","B_weapon_ratio":0.8},
                {"hero":"Adagio","cooldown_s":[1,1,1,1,1],"burst_heal_health_ratio":1,"empowered_attacks":[1,1,1,1,1]},
                {"hero":"Lyra","A_heal_per_second":[1,1,1,1,1],"A_cooldown_s":[1,1,1,1,1],"A_hot_health_ratio":1},
                {"hero":"SAW","B_dps":[1,1,1,1,1],"B_cooldown_s":[1,1,1,1,1]},
                {"hero":"Karas","B_damage":[1,1,1,1,1]},
                {"hero":"Shin","C_damage_per_hit":1,"C_hits":1}
            ]}
        }),
        abilities: Vec::new(),
        combat_constants: json!({}),
        content_digest: "synthetic-aral-idris".into(),
        hero_names: vec!["Idris".into()],
        skin_names: Vec::new(),
    })
}

fn content(path: &Path) -> (PathBuf, [String; 3]) {
    let mut rows = serde_json::Map::new();
    let mut bindings = serde_json::Map::new();
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
            json!({"id":id,"hero":"*Idris*","def":format!("*Talent_Idris_{definition}*"),"level":0,"maxLevel":false,"levelRequirements":{}}),
        );
        bindings.insert(id.clone(), json!({"levelItem":id,"fragmentItem":fragments}));
        for item in [&id, &fragments] {
            items.push(json!({"id":item,"name":item,"slot":"test"}));
        }
        id
    });
    let write = |name: &str, value: Value| {
        fs::write(path.join(name), value.to_string()).unwrap();
    };
    write("official_getTalentsData.json", json!({"returnValue":rows}));
    write("official_getSkinManifest.json", json!({"returnValue":"{}"}));
    write(
        "content.json",
        json!({"items":items,"talentProgress":bindings}),
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
        member.rpc = platform.rpc();
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
            "createGuestPlayer",
            json!([
                "kindred",
                "en",
                "US",
                "production",
                "en",
                "BAKLAVA",
                "android",
                "",
                "147219",
                1,
                "device",
                "model",
                "0"
            ]),
        )
        .await;
        let session = rpc(
            &member.rpc,
            "startSessionForPlayer",
            json!([guest["returnValue"]["playerUUID"]]),
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
        let request = json!([token,{"lobby":"casual_aral"},"147219"]);
        rpc(&member.rpc, "joinLobby", request.clone()).await;
        if index == 0 {
            // An identical home retry must retain one entitlement for the same seat.
            rpc(&member.rpc, "joinLobby", request).await;
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
        state(home_rpc, token, "matched_partners").await;
        let pending = rpc(home_rpc, "queryPendingMatch", json!([token])).await;
        assert_eq!(pending["returnValue"]["matchId"], proposal.id.to_string());
        rpc(home_rpc, "acceptMatch", json!([token])).await;
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

    let mut sockets = Vec::new();
    let cipher = MatchCipher::new(&allocation.id.to_string());
    for (home, player, token, expected) in guests {
        let playing = state(&node(&cluster, home).rpc, &token, "playing").await;
        assert_eq!(playing["matchId"], allocation.id.to_string());
        let address = format!("{}:{}", playing["host"].as_str().unwrap(), playing["port"]);
        let mut socket = login(&address, player, &cipher).await;
        let m = until(&mut socket, &cipher, 0x045a).await;
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
            while next(&mut socket, &cipher).await.opcode != 0x046c {}
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
        write(socket, &cipher, Message::new(0x046e, [0; 6])).await;
        let mut ready = [0; 6];
        ready[..4].copy_from_slice(&actor.to_be_bytes());
        ready[4] = 1;
        write(socket, &cipher, Message::new(0x0471, ready)).await;
        until(socket, &cipher, 0x0471).await;
        write(socket, &cipher, Message::new(0x046d, [0; 6])).await;
        until(socket, &cipher, 0x046d).await;
    }
    for (socket, _, _) in &mut sockets {
        until(socket, &cipher, 0x03ef).await;
    }
    for (socket, actor, expected) in &mut sockets {
        let selection = TalentSelection::Slot1;
        let barrier = Message::new(0, [0; 6]);
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
    for member in cluster.into_iter().flatten() {
        member.stop().await;
    }
}
