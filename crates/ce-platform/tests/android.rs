#[path = "support/native_rpc.rs"]
mod native_rpc;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use halcyon_ce_platform::RpcMethod;
use halcyon_ce_platform::{Config, Platform};
use halcyon_platform::{AppState, HostedMatch, MatchHost, PlatformConfig, Reservation};
use native_rpc::{RpcEnvelope, android_guest};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

struct Host;
impl MatchHost for Host {
    fn reserve(&self, room: Reservation) -> halcyon_platform::Result<HostedMatch> {
        assert_eq!(room.players.len(), 1);
        Ok(HostedMatch {
            address: "localhost:7123".into(),
        })
    }
}
async fn rpc(router: &Router, method: &str, args: impl serde::Serialize) -> Value {
    let req = Request::post(format!("/JSONRpc/{method}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            serde_json::to_string(&RpcEnvelope {
                method,
                params: args,
            })
            .unwrap(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    serde_json::from_slice(&to_bytes(resp.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
#[ignore = "requires the 147219 operator data and archived public manifests"]
async fn supplement_serves_native_manifests_and_local_session_settings() {
    let delivery = std::path::PathBuf::from(std::env::var_os("HALCYON_DELIVERY").unwrap());
    let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
    let data = Arc::new(halcyon_data::GameData::load_with_extra(&delivery, Some(&extra)).unwrap());
    for manifests in [None, Some(delivery.parent().unwrap())] {
        check_native_manifests(&data, &delivery, manifests).await;
    }
}

async fn check_native_manifests(
    data: &Arc<halcyon_data::GameData>,
    delivery: &std::path::Path,
    manifests: Option<&std::path::Path>,
) {
    let archived = |name: &str| -> Value {
        let path = delivery.parent().unwrap().join(name);
        let mut envelope: Value =
            serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
        envelope["returnValue"].take()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("platform.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 10,
        },
        Arc::new(Host),
    )
    .unwrap();
    let mut platform = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[72; 32],
        &path,
        manifests,
    )
    .unwrap();
    Platform::configure_game_data(&mut platform, Some(data.clone())).unwrap();
    Platform::configure_native_state_updates(&mut platform, true).unwrap();
    let router = platform.rpc();
    let guest = rpc(
        &router,
        RpcMethod::CreateGuestPlayer.as_str(),
        android_guest("device", "0"),
    )
    .await;
    let session = rpc(
        &router,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&guest["returnValue"]["playerUUID"],),
    )
    .await;
    assert_eq!(session["code"], 0);
    let token = &session["sessionToken"];
    let settings = &session["returnValue"];
    let constants = &data.references().unwrap().constants;
    assert_eq!(settings["platformUrl"], "https://localhost:8443");
    assert_eq!(
        settings["failoverThreshold"],
        constants.official_session_meta.failover_threshold
    );
    assert_eq!(settings["enableStateUpdates"], true);
    assert_eq!(
        settings["constants"]["featuresEnabled"]["inGameChat"],
        false
    );
    for (experiment, flags) in &constants.official_session_constants.experiments {
        assert_eq!(
            settings["constants"][experiment],
            serde_json::to_value(flags).unwrap()
        );
    }
    let skins = rpc(&router, RpcMethod::GetSkinManifest.as_str(), (token,)).await;
    let actual: Value = serde_json::from_str(skins["returnValue"].as_str().unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(archived("official_getSkinManifest.json").as_str().unwrap()).unwrap();
    assert_eq!(actual, expected);
    let talents = rpc(&router, RpcMethod::GetTalentsData.as_str(), (token,)).await;
    let actual = talents["returnValue"].as_object().unwrap();
    let expected = archived("official_getTalentsData.json");
    assert_eq!(actual.len(), expected.as_object().unwrap().len());
    for (id, row) in actual {
        for field in ["id", "def", "hero"] {
            assert_eq!(row[field], expected[id][field], "{id} {field}");
        }
        assert_eq!(row["level"], 0);
        assert_eq!(row["fragments"], 0);
    }
}
#[tokio::test]
async fn android_guest_lobby_notifications_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("platform.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 10,
        },
        Arc::new(Host),
    )
    .unwrap();
    let mut adapter = Platform::new(
        app.clone(),
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[71; 32],
        &path,
        None,
    )
    .unwrap();
    Platform::configure_ping_sites(
        &mut adapter,
        vec![halcyon_ce_platform::PingSite {
            site: "lab".into(),
            host: "127.0.0.1".into(),
            port: 41119,
            node_id: uuid::Uuid::new_v4(),
        }],
    )
    .unwrap();
    let r = adapter.rpc();
    let guest = rpc(
        &r,
        RpcMethod::CreateGuestPlayer.as_str(),
        android_guest("device", "0"),
    )
    .await;
    assert_eq!(guest["code"], 0);
    let recovery = &guest["returnValue"]["playerUUID"];
    assert_eq!(guest["sessionToken"], "");
    let session = rpc(
        &r,
        RpcMethod::StartSessionForPlayer.as_str(),
        (
            &recovery, "local", 1, "en", "device", "model", "147219", "local", "0", "0", "0",
        ),
    )
    .await;
    assert_eq!(session["returnValue"]["state"], "menus");
    let token = &session["sessionToken"];
    let bootstrap = &session["returnValue"];
    for field in ["version", "lastVersion"] {
        let id: uuid::Uuid = serde_json::from_value(bootstrap[field].clone()).unwrap();
        assert_eq!(id.get_version_num(), 4);
    }
    assert_ne!(bootstrap["version"], bootstrap["lastVersion"]);
    assert_eq!(bootstrap["pingHostPortInfo"], sample("ping_sites"));
    for field in [
        "pendingMessages",
        "pendingPartyInvites",
        "pendingGuildInvites",
        "pendingTeamInvites",
        "bucketIncrementer",
        "failoverThreshold",
        "ttl",
        "platformTime",
    ] {
        assert!(bootstrap[field].is_u64(), "{field}");
    }
    for field in [
        "enableStateUpdates",
        "askRegistrationConsent",
        "pingPostToAnalytics",
    ] {
        assert!(bootstrap[field].is_boolean(), "{field}");
    }
    assert!(bootstrap["supportUrls"]["staticContentUrl"].is_string());
    assert!(bootstrap["expiry"].is_f64());
    assert!(bootstrap["experimentDetails"].is_array());
    assert!(bootstrap["unityAdsGameId"].is_string());
    assert!(bootstrap.get("ironsourceAppId").is_some_and(Value::is_null));
    assert_eq!(bootstrap["chatUser"], bootstrap["playerUUID"]);
    let skins = rpc(&r, RpcMethod::GetSkinManifest.as_str(), (&token,)).await;
    assert!(skins["returnValue"].is_string());
    let talents = rpc(&r, RpcMethod::GetTalentsData.as_str(), (&token,)).await;
    assert!(talents["returnValue"].is_object());
    let friends = rpc(&r, RpcMethod::FriendListAll.as_str(), (&token, false)).await;
    assert_eq!(friends["returnValue"], sample("friend_list"));
    assert_eq!(
        rpc(
            &r,
            "updatePlatformPlayerConfig",
            (&token, "{\"setting\":true}",)
        )
        .await["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    assert_ne!(token, &guest["sessionToken"]);
    let notify = session["returnValue"]["notifyFallbackUrl"]
        .as_str()
        .unwrap()
        .strip_prefix("http://localhost:2112")
        .unwrap();
    assert_ne!(
        rpc(
            &r,
            RpcMethod::JoinLobby.as_str(),
            (
                &token,
                LobbyDifficulty {
                    lobby: "5v5_bots_solo",
                    difficulty: "operator_invalid"
                },
                "147219",
            )
        )
        .await["code"],
        0
    );
    assert_eq!(
        rpc(
            &r,
            RpcMethod::JoinLobby.as_str(),
            (
                &token,
                "{\"lobby\":\"5v5_bots_solo\",\"difficulty\":\"very_easy\"}",
                "147219",
            )
        )
        .await["code"],
        0
    );
    let events = adapter
        .notifications()
        .oneshot(Request::get(notify).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let notification: Value =
        serde_json::from_slice(&to_bytes(events.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(notification["text"]["type"], "stateUpdate");
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&token,)).await["returnValue"]["state"],
        "match_pending"
    );
    let playing = rpc(&r, RpcMethod::Update.as_str(), (&token,)).await;
    assert_eq!(playing["returnValue"]["state"], "playing");
    assert_eq!(playing["returnValue"]["port"], 7123);
    let already_playing = rpc(&r, RpcMethod::ExitLobby.as_str(), (&token,)).await;
    let looked_up = rpc(&r, "getPlayerForGuestAccount", (&recovery,)).await;
    assert_eq!(looked_up["code"], 0);
    let recovered = &looked_up["returnValue"]["playerUUID"];
    assert_eq!(recovered, recovery);
    let resumed = rpc(&r, RpcMethod::StartSessionForPlayer.as_str(), (&recovered,)).await;
    assert_eq!(resumed["code"], 0);
    assert_ne!(resumed["sessionToken"], *token);
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&token,)).await["code"],
        -1
    );
    assert_eq!(
        resumed["returnValue"]["playerUUID"],
        bootstrap["playerUUID"]
    );
    let token = &resumed["sessionToken"];
    let resumed_profile = rpc(&r, RpcMethod::GetPlayerInfo.as_str(), (&token,)).await;
    assert_eq!(resumed_profile["code"], 0);
    for response in [
        already_playing,
        resumed.clone(),
        rpc(&r, RpcMethod::Update.as_str(), (&token,)).await,
        rpc(&r, RpcMethod::ExitLobby.as_str(), (&token,)).await,
    ] {
        assert_eq!(response["code"], 0);
        for field in ["state", "matchId", "host", "port", "site"] {
            assert_eq!(
                response["returnValue"][field], playing["returnValue"][field],
                "{field} changed on resume"
            );
        }
    }
    for method in ["rejectMatch", "notifyExitPostMatch"] {
        assert_eq!(rpc(&r, method, (&token,)).await["code"], -2);
    }
    let record = rpc(
        &r,
        "recordMatchExperienceMetrics",
        (
            &token,
            &playing["returnValue"]["matchId"],
            1,
            "a",
            "b",
            1.5,
            0,
        ),
    )
    .await;
    assert_eq!(
        record["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    app.finish_match(
        playing["returnValue"]["matchId"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        serde_json::to_value(Winner { winner: 0 }).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&token,)).await["returnValue"]["state"],
        "post_match"
    );
    let exit = rpc(&r, RpcMethod::ExitLobby.as_str(), (&token,)).await;
    assert_eq!(exit["returnValue"]["state"], "menus");
    assert!(exit["returnValue"]["expiry"].is_f64());
    assert_eq!(
        rpc(&r, RpcMethod::NotifyExitPostMatch.as_str(), (&token,)).await["returnValue"],
        sample("success")
    );
    let restored = rpc(
        &r,
        "getPlayerForGuestAccount",
        (
            &recovery, "a", "b", "c", 1, "en", "local", "device", "model", "0", "0",
        ),
    )
    .await;
    assert_eq!(restored["sessionToken"], "");
    let resumed = rpc(
        &r,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&restored["returnValue"]["playerUUID"],),
    )
    .await;
    assert_eq!(
        resumed["returnValue"]["playerUUID"],
        session["returnValue"]["playerUUID"]
    );
    assert_eq!(
        rpc(&r, RpcMethod::GetPlayerInfo.as_str(), (&token,)).await["code"],
        -1
    );
    assert_eq!(
        rpc(&r, "notARealMethod", (&resumed["sessionToken"],)).await["code"],
        -32601
    );
    let aral = rpc(
        &r,
        RpcMethod::JoinLobby.as_str(),
        (
            &resumed["sessionToken"],
            "{\"lobby\":\"casual_aral\"}",
            "147219",
        ),
    )
    .await;
    assert_eq!(aral["returnValue"], sample("queue_status"));
    let exit = rpc(
        &r,
        RpcMethod::ExitLobby.as_str(),
        (&resumed["sessionToken"],),
    )
    .await;
    assert_eq!(exit["returnValue"]["state"], "menus");
    assert_eq!(exit["returnValue"]["ttl"], 100);
    let delta = exit["returnValue"]["expiry"].as_f64().unwrap()
        - chrono::Utc::now().timestamp_millis() as f64 / 1000.;
    assert!((595.0..=600.1).contains(&delta));
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let ended = rpc(
        &r,
        RpcMethod::EndSession.as_str(),
        (&resumed["sessionToken"],),
    )
    .await;
    assert_eq!(ended["sessionToken"], "");
    assert_eq!(ended["returnValue"]["state"], "menus");
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let snapshot = &ended["returnValue"];
    assert!(snapshot["ttl"].is_f64());
    assert_eq!(snapshot["ttl"].as_f64(), Some(5.0));
    assert!(snapshot["expiry"].is_f64());
    assert!((before + 600.0..=after + 600.0).contains(&snapshot["expiry"].as_f64().unwrap()));
    assert_eq!(snapshot["lastVersion"], exit["returnValue"]["version"]);
    assert_ne!(snapshot["version"], snapshot["lastVersion"]);
    assert_ne!(
        rpc(
            &r,
            RpcMethod::EndSession.as_str(),
            (&resumed["sessionToken"],)
        )
        .await["code"],
        0
    );
}

#[tokio::test]
async fn linked_identity_and_game_currency_survive_transport_retries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("platform.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 1,
        },
        Arc::new(Host),
    )
    .unwrap();
    let guest = app.create_guest("Local player".into()).await.unwrap();
    let initial = guest.recovery_token.unwrap();
    let content = dir.path().join("content.json");
    std::fs::write(
        &content,
        serde_json::to_vec(&sample("community_content")).unwrap(),
    )
    .unwrap();
    app.link_account(
        &guest.session_token,
        "local@example.test",
        "fixture-only".into(),
    )
    .await
    .unwrap();
    let r = Platform::new(
        app.clone(),
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[37; 32],
        &path,
        None,
    )
    .unwrap()
    .with_content(&content)
    .await
    .unwrap()
    .rpc();
    let denied = rpc(
        &r,
        RpcMethod::GetPlayerFromPlatform.as_str(),
        ("local@example.test", "wrong-value"),
    )
    .await;
    assert_eq!(denied["code"], -1);
    let direct = rpc(
        &r,
        RpcMethod::Authenticate.as_str(),
        (Credentials {
            username: "local@example.test",
            password: "fixture-only",
        },),
    )
    .await;
    assert_eq!(direct["code"], 0);
    for field in ["version", "lastVersion"] {
        let version: uuid::Uuid =
            serde_json::from_value(direct["returnValue"][field].clone()).unwrap();
        assert_eq!(version.get_version_num(), 4);
    }
    assert_ne!(
        direct["returnValue"]["version"],
        direct["returnValue"]["lastVersion"]
    );
    let login = rpc(
        &r,
        RpcMethod::GetPlayerFromPlatform.as_str(),
        ("LOCAL@example.test", "fixture-only"),
    )
    .await;
    let session = rpc(
        &r,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&login["returnValue"]["playerUUID"],),
    )
    .await;
    let token = &session["sessionToken"];
    assert_eq!(
        session["returnValue"]["playerUUID"],
        guest.player_id.to_string()
    );
    assert!(app.resume_guest(initial).await.is_err());
    let grant = rpc(
        &r,
        RpcMethod::CollectHeroChest.as_str(),
        (&token, "starter"),
    )
    .await;
    assert_eq!(grant["code"], 0);
    let duplicate = rpc(
        &r,
        RpcMethod::CollectHeroChest.as_str(),
        (&token, "starter"),
    )
    .await;
    assert_eq!(duplicate["code"], 0);
    assert_ne!(
        rpc(
            &r,
            RpcMethod::CollectHeroChest.as_str(),
            (&grant["sessionToken"], "starter",)
        )
        .await["code"],
        0
    );
    let buy = rpc(&r, RpcMethod::ForgeCard.as_str(), (&token, "unit")).await;
    assert_eq!(buy["code"], 0);
    assert_eq!(
        rpc(&r, RpcMethod::ForgeCard.as_str(), (&token, "unit",)).await["code"],
        0
    );
    assert_ne!(
        rpc(
            &r,
            RpcMethod::ForgeCard.as_str(),
            (&buy["sessionToken"], "unit",)
        )
        .await["code"],
        0
    );
    let inventory = rpc(&r, RpcMethod::GetPlayerCardInventory.as_str(), (&token,)).await;
    assert_eq!(inventory["returnValue"]["ownedCards"][0]["count"], 1);
    assert_eq!(
        rpc(
            &r,
            RpcMethod::StoreProcessIAP.as_str(),
            (&token, EmptyReply {},)
        )
        .await["code"],
        -6
    );
    for n in [3, 10, 1] {
        assert_eq!(
            rpc(
                &r,
                RpcMethod::AchievementStoreScore.as_str(),
                (&token, "practice", &n,)
            )
            .await["code"],
            0
        );
    }
    assert_eq!(
        rpc(
            &r,
            RpcMethod::AchievementGetScore.as_str(),
            (&token, "practice",)
        )
        .await["returnValue"]["score"],
        10
    );

    let other = app.create_guest("Other player".into()).await.unwrap();
    let public = rpc(
        &r,
        RpcMethod::GetPlayerInfo.as_str(),
        (&token, &other.player_id),
    )
    .await;
    assert_eq!(
        public["returnValue"],
        serde_json::to_value(HandleResponse {
            handle: "Other player",
            player_uuid: other.player_id
        })
        .unwrap()
    );
    let own = serde_json::to_value((&token, &guest.player_id)).unwrap();
    let initial_info = rpc(&r, RpcMethod::GetPlayerInfo.as_str(), own.clone()).await;
    assert_eq!(initial_info["returnValue"]["wins"], 0);
    assert_eq!(initial_info["returnValue"]["tutorialState"]["state"], -1);
    let update = rpc(&r, RpcMethod::SetTutorialState.as_str(), (&token, 3, 1)).await;
    assert_eq!(
        update["returnValue"]["tutorialState"],
        sample("player_action")
    );

    let joined = rpc(
        &r,
        RpcMethod::JoinLobby.as_str(),
        (
            &token,
            LobbyJoin {
                lobby: "5v5_bots_solo",
            },
            "147219",
        ),
    )
    .await;
    assert_eq!(joined["code"], 0);
    rpc(&r, RpcMethod::Update.as_str(), (&token,)).await;
    let playing = rpc(&r, RpcMethod::Update.as_str(), (&token,)).await;
    let room = uuid::Uuid::parse_str(playing["returnValue"]["matchId"].as_str().unwrap()).unwrap();
    let result = serde_json::to_value(HeroOutcome {
        reason: "completed",
        players: std::collections::BTreeMap::from([(
            guest.player_id,
            HeroVictory {
                hero: "Ringo",
                victory: true,
            },
        )]),
    })
    .unwrap();
    app.finish_match(room, result.clone()).unwrap();
    app.finish_match(room, result).unwrap();
    let progress = rpc(&r, "getQuestDisplayDataForPlayer", (&token,)).await;
    let quests = progress["returnValue"]["questDisplayData"]
        .as_array()
        .unwrap();
    assert_eq!(quests.len(), 2);
    assert!(quests.iter().all(|quest| quest["progress"] == 1));
    assert_eq!(
        rpc(
            &r,
            RpcMethod::RedeemQuestForPlayer.as_str(),
            (&token, "first_win",)
        )
        .await["code"],
        0
    );
    let info = rpc(&r, RpcMethod::GetPlayerInfo.as_str(), own.clone()).await;
    let info = &info["returnValue"];
    assert_eq!(info["completed"], 1);
    assert_eq!(info["wins"], 1);
    assert_eq!(info["winsToday"], 1);
    assert_eq!(info["winStreak"], 1);
    assert_eq!(info["completed_non_tutorial"], 1);
    assert_eq!(info["currency"]["silver"], 5);
    assert_eq!(info["currency"], info["targeting"]["currency"]);
    let heroes = rpc(&r, RpcMethod::GetHeroMastery.as_str(), (&token,)).await;
    assert_eq!(
        heroes["returnValue"]["heroStats"],
        sample("ringo_statistics")
    );
    drop(r);
    let reopened = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[37; 32],
        &path,
        None,
    )
    .unwrap()
    .rpc();
    let after_restart = rpc(&reopened, RpcMethod::GetPlayerInfo.as_str(), own).await;
    assert_eq!(after_restart["returnValue"], *info);
}

#[tokio::test]
async fn cataloged_names_reach_rpc_handlers() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("platform.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 1,
        },
        Arc::new(Host),
    )
    .unwrap();
    let r = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[19; 32],
        &path,
        None,
    )
    .unwrap()
    .rpc();
    let guest = rpc(
        &r,
        RpcMethod::CreateGuestPlayer.as_str(),
        android_guest("device", "0"),
    )
    .await;
    let session = rpc(
        &r,
        RpcMethod::StartSessionForPlayer.as_str(),
        (&guest["returnValue"]["playerUUID"],),
    )
    .await;
    let token = &session["sessionToken"];
    let mut names = std::collections::BTreeSet::new();
    let mut evidence = std::collections::BTreeMap::new();
    let mut local_aliases = std::collections::BTreeSet::new();
    let mut rows = include_str!("../../../docs/ce-rpc.tsv").lines();
    assert_eq!(
        rows.next(),
        Some("method\tevidence\tparser_family\timplementation")
    );
    for row in rows {
        let columns: Vec<_> = row.split('\t').collect();
        let [method, sources, parser, implementation] = columns[..] else {
            panic!("invalid RPC catalog row");
        };
        assert!(!parser.is_empty() && !implementation.is_empty(), "{method}");
        assert!(names.insert(method), "duplicate catalog method {method}");
        let mut seen = std::collections::BTreeSet::new();
        for source in sources.split(',') {
            assert!(
                seen.insert(source),
                "duplicate evidence source for {method}"
            );
            *evidence.entry(source).or_insert(0) += 1;
        }
        if implementation == "local-alias" {
            local_aliases.insert(method);
        }
        if method == "endSession" {
            continue;
        }
        let response = rpc(&r, method, (&token,)).await;
        let code = response["code"].as_i64().expect("CE RPC status code");
        assert_ne!(code, -32601, "unimplemented route {method}");
        assert_ne!(code, -3, "internal error in {method}");
        assert!(response.get("returnValue").is_some(), "{method}");
        assert!(response["sessionToken"].is_string(), "{method}");
        if code != 0 {
            assert_eq!(response["sessionToken"], "", "{method}");
            assert_eq!(response["returnValue"], Value::Null, "{method}");
            assert!(
                response["reason"].as_str().is_some_and(|s| !s.is_empty()),
                "{method}"
            );
        }
    }
    assert_eq!(names.len(), 155);
    assert_eq!(
        evidence,
        [
            ("android-callsite", 141),
            ("android-traffic", 14),
            ("archive-traffic", 30),
            ("ios-candidate", 139),
            ("local-spec", 144),
        ]
        .into()
    );
    assert_eq!(
        local_aliases,
        ["charm", "getSkinProgress", "social_ping_pack"].into()
    );
    let logout = rpc(&r, RpcMethod::EndSession.as_str(), (&token,)).await;
    assert_eq!(logout["code"], 0);
    assert_eq!(logout["sessionToken"], "");
}
#[tokio::test]
async fn aral_offer_projection_is_private_read_only_and_survives_reopen() {
    struct Six;
    impl MatchHost for Six {
        fn reserve(&self, reservation: Reservation) -> halcyon_platform::Result<HostedMatch> {
            assert_eq!(reservation.players.len(), 6);
            Ok(HostedMatch {
                address: "localhost:7012".into(),
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("offers.sqlite");
    let open = || {
        let app = AppState::open(
            PlatformConfig {
                database_path: db.clone(),
                public_address: "localhost:8443".into(),
                seats_per_match: 6,
            },
            Arc::new(Six),
        )
        .unwrap();
        let ce = Platform::new(
            app.clone(),
            Config {
                authority: "localhost:8443".into(),
                notify_authority: "localhost:2112".into(),
            },
            &[19; 32],
            &db,
            None,
        )
        .unwrap();
        (app, ce.rpc())
    };
    let (app, r) = open();
    let mut tokens = Vec::new();
    let mut players = Vec::new();
    for n in 0..7 {
        let guest = rpc(
            &r,
            RpcMethod::CreateGuestPlayer.as_str(),
            android_guest("dev", &n.to_string()),
        )
        .await;
        let session = rpc(
            &r,
            RpcMethod::StartSessionForPlayer.as_str(),
            (&guest["returnValue"]["playerUUID"],),
        )
        .await;
        tokens.push(session["sessionToken"].clone());
        players.push(session["returnValue"]["playerUUID"].clone());
    }
    let malformed = rpc(
        &r,
        RpcMethod::JoinLobby.as_str(),
        (&tokens[6], "{bad", "147219"),
    )
    .await;
    assert_eq!(malformed, sample("invalid_parameters"));
    let ranked = rpc(
        &r,
        RpcMethod::JoinLobby.as_str(),
        (&tokens[6], "{\"lobby\":\"ranked\"}", "147219"),
    )
    .await;
    assert_eq!(ranked["code"], 0);
    assert_eq!(ranked["returnValue"], sample("queue_status"));
    let expiry =
        rpc(&r, RpcMethod::Update.as_str(), (&tokens[6],)).await["returnValue"]["expiry"].clone();
    let stamp = chrono::Utc::now().timestamp_millis() as f64 / 1000.;
    assert!((3595.0..=3600.1).contains(&(expiry.as_f64().unwrap() - stamp)));
    let repeat = rpc(&r, RpcMethod::Update.as_str(), (&tokens[6],)).await;
    assert_eq!(repeat["returnValue"]["ttl"], 10);
    assert_eq!(repeat["returnValue"]["expiry"], expiry);
    drop(r);
    drop(app);
    let (app, r) = open();
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&tokens[6],)).await["returnValue"]["expiry"],
        expiry
    );
    assert_eq!(
        rpc(&r, RpcMethod::ExitLobby.as_str(), (&tokens[6],)).await["code"],
        0
    );
    for t in &tokens[..6] {
        assert_eq!(
            rpc(
                &r,
                RpcMethod::JoinLobby.as_str(),
                (
                    &t,
                    LobbyJoin {
                        lobby: "casual_aral"
                    },
                    "147219",
                )
            )
            .await["code"],
            0
        );
    }
    let stale = rpc(&r, RpcMethod::QueryPendingMatch.as_str(), (&tokens[0],)).await["returnValue"]
        ["matchId"]
        .clone();
    // Local acceptance and rejection both close at the persisted deadline.
    let clock = rusqlite::Connection::open(&db).unwrap();
    clock
        .execute(
            "UPDATE matches SET offered_at=unixepoch()-30 WHERE id=?",
            [stale.as_str().unwrap()],
        )
        .unwrap();
    assert_eq!(
        rpc(&r, RpcMethod::RejectMatch.as_str(), (&tokens[0], &stale,)).await["returnValue"],
        false
    );
    clock
        .execute(
            "UPDATE matches SET offered_at=unixepoch() WHERE id=?",
            [stale.as_str().unwrap()],
        )
        .unwrap();
    drop(clock);
    for (seat, requested) in [
        (0, Value::from("")),
        (0, serde_json::to_value(uuid::Uuid::new_v4()).unwrap()),
        (6, stale.clone()),
    ] {
        assert_eq!(
            rpc(
                &r,
                RpcMethod::RejectMatch.as_str(),
                (&tokens[seat], &requested,)
            )
            .await["returnValue"],
            false
        );
    }
    assert_eq!(
        rpc(&r, RpcMethod::RejectMatch.as_str(), (&tokens[0], &stale,)).await["returnValue"],
        true
    );
    for t in &tokens[..6] {
        assert_eq!(
            rpc(
                &r,
                RpcMethod::JoinLobby.as_str(),
                (
                    &t,
                    LobbyJoin {
                        lobby: "casual_aral"
                    },
                    "147219",
                )
            )
            .await["code"],
            0
        );
    }
    assert_eq!(
        rpc(&r, RpcMethod::RejectMatch.as_str(), (&tokens[0], &stale,)).await["returnValue"],
        true
    );
    let renamed = rpc(
        &r,
        RpcMethod::RenamePlayerHandle.as_str(),
        (&tokens[1], "", "Coral"),
    )
    .await;
    assert_eq!(renamed["returnValue"], sample("honor_status"));
    let offer =
        rpc(&r, RpcMethod::QueryPendingMatch.as_str(), (&tokens[0],)).await["returnValue"].clone();
    assert_eq!(offer["isValid"], false);
    let invite = rpc(&r, "friendRequestIssueByHandle", (&tokens[2], "Coral")).await;
    assert_eq!(
        invite["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    assert_eq!(invite["code"], 0);
    assert_ne!(
        rpc(
            &r,
            "friendRequestIssueByHandle",
            (&tokens[2], "MissingHandle",)
        )
        .await["code"],
        0
    );
    assert_eq!(
        rpc(
            &r,
            RpcMethod::NotifyPlayerAction.as_str(),
            (&tokens[2], "touch", "menu",)
        )
        .await["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    let rows = offer["responses"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for (row, id) in rows.iter().zip(&players) {
        assert_eq!(
            *row,
            serde_json::to_value(PendingPartner {
                player_uuid: id.as_str().unwrap(),
                response: 0,
                accept_delay: 0.0
            })
            .unwrap()
        );
    }
    let stranger = rpc(
        &r,
        RpcMethod::QueryPendingMatch.as_str(),
        (&tokens[6], &offer["matchId"]),
    )
    .await;
    assert_eq!(
        stranger["returnValue"]["responses"],
        serde_json::Value::Array(Vec::new())
    );
    let update = rpc(&r, RpcMethod::Update.as_str(), (&tokens[0],)).await["returnValue"].clone();
    assert_eq!(update["state"], "matched_partners");
    assert_eq!(update["numPlayers"], 6);
    assert_eq!(update["numQueuedEntries"], 6);
    let expiry = update["expiry"].as_i64().unwrap();
    let ttl = update["ttl"].as_i64().unwrap();
    drop(r);
    drop(app);
    let (app, r) = open();
    let reopened = rpc(&r, RpcMethod::Update.as_str(), (&tokens[0],)).await;
    assert_eq!(reopened["returnValue"]["expiry"], expiry);
    // Move the existing offer clock back ten seconds without a wall-clock sleep.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE matches SET offered_at=offered_at-10 WHERE id=?",
            [offer["matchId"].as_str().unwrap()],
        )
        .unwrap();
    let aged = rpc(&r, RpcMethod::Update.as_str(), (&tokens[0],)).await;
    assert_eq!(aged["returnValue"]["expiry"], expiry - 10);
    assert!(aged["returnValue"]["ttl"].as_i64().unwrap() <= ttl - 10);
    assert_eq!(
        rpc(
            &r,
            RpcMethod::AcceptMatch.as_str(),
            (&tokens[0], &offer["matchId"],)
        )
        .await["returnValue"],
        true
    );
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&tokens[0],)).await["returnValue"]["state"],
        "match_pending"
    );
    let partial = rpc(&r, RpcMethod::QueryPendingMatch.as_str(), (&tokens[1],)).await;
    assert_eq!(partial["returnValue"]["isValid"], true);
    assert_eq!(partial["returnValue"]["responses"][0]["response"], 1);
    assert_eq!(partial["returnValue"]["responses"][1]["response"], 0);
    for t in &tokens[1..6] {
        assert_eq!(
            rpc(
                &r,
                RpcMethod::AcceptMatch.as_str(),
                (&t, &offer["matchId"],)
            )
            .await["code"],
            0
        );
    }
    let resumed = rpc(&r, RpcMethod::StartSessionForPlayer.as_str(), (&tokens[2],)).await;
    assert_eq!(resumed["returnValue"]["state"], "playing");
    assert_eq!(resumed["returnValue"]["matchId"], offer["matchId"]);
    assert_eq!(resumed["returnValue"]["numPlayers"], 6);
    assert_eq!(
        rpc(
            &r,
            RpcMethod::RejectMatch.as_str(),
            (&tokens[5], &offer["matchId"],)
        )
        .await["returnValue"],
        false
    );
    let logout = rpc(&r, RpcMethod::EndSession.as_str(), (&tokens[5],)).await;
    assert_eq!(logout["returnValue"]["matchId"], offer["matchId"]);
    assert_eq!(logout["returnValue"]["state"], "playing");
    assert_eq!(logout["returnValue"]["port"], 7012);
    assert!(logout["returnValue"]["ttl"].is_i64());
    assert_ne!(
        rpc(&r, RpcMethod::Update.as_str(), (&tokens[5],)).await["code"],
        0
    );
    let room = offer["matchId"].as_str().unwrap().parse().unwrap();
    let result = serde_json::to_value(MatchRoster {
        winner: 0,
        players: players[..6]
            .iter()
            .map(|p| (p.as_str().unwrap(), MatchTeam { team: 0 }))
            .collect::<std::collections::BTreeMap<_, _>>(),
    })
    .unwrap();
    app.finish_match(room, result.clone()).unwrap();
    rpc(
        &r,
        RpcMethod::RenamePlayerHandle.as_str(),
        (&tokens[1], "", "CoralNew"),
    )
    .await;
    for _ in 0..2 {
        let reply = rpc(
            &r,
            RpcMethod::ReportHonorPlayer.as_str(),
            (&tokens[2], &room, "Coral", "honor"),
        )
        .await;
        assert_eq!(reply["code"], 0);
        assert_eq!(
            reply["returnValue"],
            serde_json::to_value(EmptyReply {}).unwrap()
        );
    }
    for (seat, target) in [(2, "Guest"), (2, "CoralNew"), (6, "Coral"), (1, "Coral")] {
        assert_ne!(
            rpc(
                &r,
                RpcMethod::ReportHonorPlayer.as_str(),
                (&tokens[seat], &room, &target, "honor",)
            )
            .await["code"],
            0
        );
    }
    rpc(&r, RpcMethod::NotifyExitPostMatch.as_str(), (&tokens[0],)).await;
    let end = rpc(&r, RpcMethod::Update.as_str(), (&tokens[2],)).await;
    assert_eq!(end["returnValue"]["state"], "post_match");
    assert_eq!(end["returnValue"]["numPlayers"], 6);
    assert_eq!(end["returnValue"]["notify"], true);
    let settled = end["returnValue"]["expiry"].as_f64().unwrap();
    assert!(end["returnValue"]["ttl"].as_u64().unwrap() <= 100);
    let stamp = end["returnValue"]["timestamp"].clone();
    app.finish_match(room, result).unwrap();
    drop(r);
    drop(app);
    let (_app, r) = open();
    let resumed = rpc(&r, RpcMethod::StartSessionForPlayer.as_str(), (&tokens[2],)).await;
    assert_eq!(resumed["returnValue"]["expiry"], settled);
    assert_eq!(resumed["returnValue"]["timestamp"], stamp);
    // The two captured pairs retain an identical absolute expiry and timestamp.
    let dbw = rusqlite::Connection::open(&db).unwrap();
    dbw.execute(
        "DELETE FROM ce_match_handles WHERE room=? AND player=?",
        rusqlite::params![room.to_string(), players[0].as_str().unwrap()],
    )
    .unwrap();
    assert_ne!(
        rpc(
            &r,
            RpcMethod::ReportHonorPlayer.as_str(),
            (&tokens[2], &room, "Coral", "honor",)
        )
        .await["code"],
        0
    );

    dbw.execute(
        "UPDATE native_settlements SET at_ms=at_ms-30000 WHERE room=?",
        [room.to_string()],
    )
    .unwrap();
    let aged = rpc(&r, RpcMethod::Update.as_str(), (&tokens[2],)).await;
    assert_eq!(aged["returnValue"]["expiry"], settled - 30.0);
    assert!(aged["returnValue"]["ttl"].as_u64().unwrap() <= 70);
    dbw.execute(
        "UPDATE native_settlements SET at_ms=at_ms-100000 WHERE room=?",
        [room.to_string()],
    )
    .unwrap();
    let expired = rpc(&r, RpcMethod::Update.as_str(), (&tokens[2],)).await;
    assert_eq!(expired["returnValue"]["state"], "post_match");
    assert_eq!(expired["returnValue"]["ttl"], 0);
    let metrics = rpc(
        &r,
        "recordMatchExperienceMetrics",
        (&tokens[2], &room, 0, "test-platform", "test-build", 1.25, 0),
    )
    .await;
    assert_eq!(metrics["code"], 0);
    assert_eq!(
        metrics["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    let exit = rpc(&r, RpcMethod::NotifyExitPostMatch.as_str(), (&tokens[2],)).await;
    assert_eq!(exit["code"], 0);
    assert_eq!(exit["returnValue"], sample("success"));
    assert_eq!(
        rpc(&r, RpcMethod::Update.as_str(), (&tokens[2],)).await["returnValue"]["state"],
        "menus"
    );
}

#[tokio::test]
async fn captured_guild_team_and_metric_envelopes_preserve_local_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("platform.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 10,
        },
        Arc::new(Host),
    )
    .unwrap();
    let router = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[77; 32],
        &path,
        None,
    )
    .unwrap()
    .rpc();

    async fn guest(router: &Router) -> Value {
        let guest = rpc(
            router,
            RpcMethod::CreateGuestPlayer.as_str(),
            android_guest("device", "0"),
        )
        .await;
        assert_eq!(guest["code"], 0);
        let session = rpc(
            router,
            RpcMethod::StartSessionForPlayer.as_str(),
            (&guest["returnValue"]["playerUUID"],),
        )
        .await;
        assert_eq!(session["code"], 0);
        session["sessionToken"].clone()
    }

    let owner = guest(&router).await;
    let visitor = guest(&router).await;
    let absent = uuid::Uuid::new_v4();
    let search = |page| {
        serde_json::to_value(GuildSearch {
            minimum_skill_tier: -2,
            min_num_members: 0,
            max_num_members: 200,
            guild_type: "GUILD_TYPE_ANY",
            guild_language_code: "EN",
            guild_invite_only: false,
            name: "",
            tag: "",
            page,
        })
        .unwrap()
        .to_string()
    };
    for (method, params) in [
        ("guildGetData", serde_json::to_value((&visitor,)).unwrap()),
        (
            "queryGuildInvites",
            serde_json::to_value((&visitor,)).unwrap(),
        ),
        (
            "queryTeamInvites",
            serde_json::to_value((&visitor,)).unwrap(),
        ),
        (
            "guildSearch",
            serde_json::to_value((&visitor, &(search(1)))).unwrap(),
        ),
        (
            "queryGuildInfo",
            serde_json::to_value((&visitor, &absent, true)).unwrap(),
        ),
        (
            "createTeam",
            serde_json::to_value((&visitor, "", "team", "tag")).unwrap(),
        ),
        ("joinGuild", serde_json::to_value((&visitor, "")).unwrap()),
        (
            "recordMatchExperienceMetrics",
            serde_json::to_value((&visitor, &absent, 15, "test", "local", 0.08, 0)).unwrap(),
        ),
    ] {
        let result = rpc(&router, method, params).await;
        assert_eq!(result["code"], 0, "{method}");
        assert_eq!(
            result["returnValue"],
            serde_json::to_value(EmptyReply {}).unwrap(),
            "{method}"
        );
        assert!(
            result["sessionToken"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
    }
    assert_eq!(
        rpc(&router, RpcMethod::QueryTeamInfo.as_str(), (&visitor,)).await["returnValue"]["teamUUID"],
        Value::Null
    );

    let private = rpc(
        &router,
        RpcMethod::CreateGuild.as_str(),
        (
            &owner,
            NamedGuild {
                name: "Private Guild",
                invite_only: true,
            },
        ),
    )
    .await;
    assert_eq!(private["code"], 0);
    let private_id = &private["returnValue"]["guildUUID"];
    assert_eq!(
        rpc(
            &router,
            RpcMethod::QueryGuildInfo.as_str(),
            (&visitor, &private_id, true,)
        )
        .await["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    assert_eq!(
        rpc(
            &router,
            RpcMethod::JoinGuild.as_str(),
            (&visitor, &private_id,)
        )
        .await["code"],
        -5
    );
    let own = rpc(&router, RpcMethod::GuildGetData.as_str(), (&owner,)).await;
    assert_eq!(own["returnValue"]["guildUUID"], *private_id);
    assert_eq!(
        rpc(
            &router,
            RpcMethod::GuildSearch.as_str(),
            (&visitor, &(search(1)),)
        )
        .await["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );

    let public_owner = guest(&router).await;
    let public = rpc(
        &router,
        RpcMethod::CreateGuild.as_str(),
        (
            &public_owner,
            NamedGuild {
                name: "Public Guild",
                invite_only: false,
            },
        ),
    )
    .await;
    assert_eq!(public["code"], 0);
    let public_id = &public["returnValue"]["guildUUID"];
    assert_eq!(
        rpc(
            &router,
            RpcMethod::GuildSearch.as_str(),
            (&visitor, &(search(1)),)
        )
        .await["returnValue"]["nearestGuilds"][0]["id"],
        *public_id
    );
    assert_eq!(
        rpc(
            &router,
            RpcMethod::GuildSearch.as_str(),
            (&visitor, &(search(2)),)
        )
        .await["returnValue"],
        serde_json::to_value(EmptyReply {}).unwrap()
    );
    assert_eq!(
        rpc(
            &router,
            RpcMethod::GuildSearch.as_str(),
            (&visitor, &(search(0)),)
        )
        .await["code"],
        -32602
    );
    let lookup = rpc(
        &router,
        RpcMethod::QueryGuildInfo.as_str(),
        (&visitor, &public_id, true),
    )
    .await;
    assert_eq!(lookup["returnValue"]["guildUUID"], *public_id);
    assert_eq!(
        lookup["returnValue"]["members"],
        serde_json::Value::Array(Vec::new())
    );
    let join = rpc(
        &router,
        RpcMethod::JoinGuild.as_str(),
        (&visitor, &public_id),
    )
    .await;
    assert_eq!(join["returnValue"]["numMembers"], 2);
    let created = rpc(
        &router,
        RpcMethod::CreateTeam.as_str(),
        (&visitor, NamedRequest { name: "Local Team" }),
    )
    .await;
    assert_eq!(created["returnValue"]["teamName"], "Local Team");
}

#[derive(serde::Serialize)]
struct NamedRequest<'a> {
    name: &'a str,
}

#[derive(serde::Serialize)]
struct EmptyReply {}

#[derive(serde::Serialize)]
struct NamedGuild<'a> {
    name: &'a str,
    #[serde(rename = "inviteOnly")]
    invite_only: bool,
}

#[derive(serde::Serialize)]
struct GuildSearch<'a> {
    #[serde(rename = "minimumSkillTier")]
    minimum_skill_tier: i32,
    #[serde(rename = "minNumMembers")]
    min_num_members: u32,
    #[serde(rename = "maxNumMembers")]
    max_num_members: u32,
    #[serde(rename = "guildType")]
    guild_type: &'a str,
    #[serde(rename = "guildLanguageCode")]
    guild_language_code: &'a str,
    #[serde(rename = "guildInviteOnly")]
    guild_invite_only: bool,
    name: &'a str,
    tag: &'a str,
    page: u32,
}

#[derive(serde::Serialize)]
struct MatchTeam {
    team: u32,
}

#[derive(serde::Serialize)]
struct PendingPartner<'a> {
    #[serde(rename = "playerUUID")]
    player_uuid: &'a str,
    response: i32,
    #[serde(rename = "acceptDelay")]
    accept_delay: f64,
}

#[derive(serde::Serialize)]
struct LobbyJoin<'a> {
    lobby: &'a str,
}

#[derive(serde::Serialize)]
struct HeroOutcome<'a> {
    reason: &'static str,
    players: std::collections::BTreeMap<uuid::Uuid, HeroVictory<'a>>,
}

#[derive(serde::Serialize)]
struct HandleResponse<'a> {
    handle: &'a str,
    #[serde(rename = "playerUUID")]
    player_uuid: uuid::Uuid,
}

#[derive(serde::Serialize)]
struct Credentials<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(serde::Serialize)]
struct Winner {
    winner: u8,
}

#[derive(serde::Serialize)]
struct LobbyDifficulty<'a> {
    lobby: &'a str,
    difficulty: &'a str,
}

fn sample(name: &str) -> serde_json::Value {
    let samples: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/android-samples.json")).unwrap();
    samples.get(name).unwrap().clone()
}

#[derive(serde::Serialize)]
struct HeroVictory<'a> {
    hero: &'a str,
    victory: bool,
}
#[derive(serde::Serialize)]
struct MatchRoster<T> {
    winner: u8,
    players: T,
}
