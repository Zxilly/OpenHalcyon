use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use halcyon_ce_platform::{Config, Platform};
use halcyon_platform::{AppState, HostedMatch, MatchHost, PlatformConfig, Reservation};
use serde_json::{Value, json};
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
async fn rpc(router: &Router, method: &str, args: Value) -> Value {
    let req = Request::post(format!("/JSONRpc/{method}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            json!({"method":method,"params":args}).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    serde_json::from_slice(&to_bytes(resp.into_body(), 1024 * 1024).await.unwrap()).unwrap()
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
    assert_eq!(guest["code"], 0);
    let recovery = &guest["returnValue"]["playerUUID"];
    assert_eq!(guest["sessionToken"], "");
    let session = rpc(
        &r,
        "startSessionForPlayer",
        json!([
            recovery, "local", 1, "en", "device", "model", "147219", "local", "0", "0", "0"
        ]),
    )
    .await;
    assert_eq!(session["returnValue"]["state"], "menus");
    let token = &session["sessionToken"];
    let bootstrap = &session["returnValue"];
    assert_eq!(
        bootstrap["pingHostPortInfo"],
        json!([{"site":"lab","host":"127.0.0.1","port":41119}])
    );
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
    let skins = rpc(&r, "getSkinManifest", json!([token])).await;
    assert!(skins["returnValue"].is_string());
    let talents = rpc(&r, "getTalentsData", json!([token])).await;
    assert!(talents["returnValue"].is_object());
    let friends = rpc(&r, "friendListAll", json!([token, false])).await;
    assert_eq!(
        friends["returnValue"],
        json!({"confirmed":[],"pending":[],"numOffline":0})
    );
    assert_eq!(
        rpc(
            &r,
            "updatePlatformPlayerConfig",
            json!([token, "{\"setting\":true}"])
        )
        .await["returnValue"],
        json!({})
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
            "joinLobby",
            json!([token, {"lobby":"5v5_bots_solo","difficulty":"operator_invalid"}, "147219"])
        )
        .await["code"],
        0
    );
    assert_eq!(
        rpc(
            &r,
            "joinLobby",
            json!([
                token,
                "{\"lobby\":\"5v5_bots_solo\",\"difficulty\":\"very_easy\"}",
                "147219"
            ])
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
        rpc(&r, "update", json!([token])).await["returnValue"]["state"],
        "match_pending"
    );
    let playing = rpc(&r, "update", json!([token])).await;
    assert_eq!(playing["returnValue"]["state"], "playing");
    assert_eq!(playing["returnValue"]["port"], 7123);
    let already_playing = rpc(&r, "exitLobby", json!([token])).await;
    let looked_up = rpc(&r, "getPlayerForGuestAccount", json!([recovery])).await;
    assert_eq!(looked_up["code"], 0);
    let recovered = &looked_up["returnValue"]["playerUUID"];
    assert_eq!(recovered, recovery);
    let resumed = rpc(&r, "startSessionForPlayer", json!([recovered])).await;
    assert_eq!(resumed["code"], 0);
    assert_ne!(resumed["sessionToken"], *token);
    assert_eq!(rpc(&r, "update", json!([token])).await["code"], -1);
    assert_eq!(
        resumed["returnValue"]["playerUUID"],
        bootstrap["playerUUID"]
    );
    let token = &resumed["sessionToken"];
    let resumed_profile = rpc(&r, "getPlayerInfo", json!([token])).await;
    assert_eq!(resumed_profile["code"], 0);
    for response in [
        already_playing,
        resumed.clone(),
        rpc(&r, "update", json!([token])).await,
        rpc(&r, "exitLobby", json!([token])).await,
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
        assert_eq!(rpc(&r, method, json!([token])).await["code"], -2);
    }
    let record = rpc(
        &r,
        "recordMatchExperienceMetrics",
        json!([
            token,
            playing["returnValue"]["matchId"],
            1,
            "a",
            "b",
            1.5,
            0
        ]),
    )
    .await;
    assert_eq!(record["returnValue"], json!({}));
    app.finish_match(
        playing["returnValue"]["matchId"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        json!({"winner":0}),
    )
    .unwrap();
    assert_eq!(
        rpc(&r, "update", json!([token])).await["returnValue"]["state"],
        "post_match"
    );
    let exit = rpc(&r, "exitLobby", json!([token])).await;
    assert_eq!(exit["returnValue"]["state"], "menus");
    assert!(exit["returnValue"]["expiry"].is_f64());
    assert_eq!(
        rpc(&r, "notifyExitPostMatch", json!([token])).await["returnValue"],
        json!({"success":true})
    );
    let restored = rpc(
        &r,
        "getPlayerForGuestAccount",
        json!([
            recovery, "a", "b", "c", 1, "en", "local", "device", "model", "0", "0"
        ]),
    )
    .await;
    assert_eq!(restored["sessionToken"], "");
    let resumed = rpc(
        &r,
        "startSessionForPlayer",
        json!([restored["returnValue"]["playerUUID"]]),
    )
    .await;
    assert_eq!(
        resumed["returnValue"]["playerUUID"],
        session["returnValue"]["playerUUID"]
    );
    assert_eq!(rpc(&r, "getPlayerInfo", json!([token])).await["code"], -1);
    assert_eq!(
        rpc(&r, "notARealMethod", json!([resumed["sessionToken"]])).await["code"],
        -32601
    );
    let aral = rpc(
        &r,
        "joinLobby",
        json!([
            resumed["sessionToken"],
            "{\"lobby\":\"casual_aral\"}",
            "147219"
        ]),
    )
    .await;
    assert_eq!(
        aral["returnValue"],
        json!({"success":true,"ttl":100,"state":"pending_auto","numPlayers":1})
    );
    let exit = rpc(&r, "exitLobby", json!([resumed["sessionToken"]])).await;
    assert_eq!(exit["returnValue"]["state"], "menus");
    let ended = rpc(&r, "endSession", json!([resumed["sessionToken"]])).await;
    assert_eq!(ended["sessionToken"], "");
    assert_eq!(ended["returnValue"]["state"], "menus");
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
    std::fs::write(&content,serde_json::to_vec(&json!({
        "items":[{"id":"silver","slot":"currency","name":"Silver"},{"id":"card","slot":"card","name":"Card"}],
        "quests":[{"id":"first_win","category":"win_match","title":"Win one game","goal":1,
            "skippable":false,"reward":["silver",2]},
            {"id":"play_twice","category":"complete_match","title":"Finish two games","goal":2,"skippable":false}],
        "exchanges":{"collectHeroChest":{"starter":{"reward":{"silver":10},"limit":1}},
          "forgeCard":{"unit":{"cost":{"silver":7},"reward":{"card":1}}}}
    })).unwrap()).unwrap();
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
        "getPlayerFromPlatform",
        json!(["local@example.test", "wrong-value"]),
    )
    .await;
    assert_eq!(denied["code"], -1);
    let login = rpc(
        &r,
        "getPlayerFromPlatform",
        json!(["LOCAL@example.test", "fixture-only"]),
    )
    .await;
    let session = rpc(
        &r,
        "startSessionForPlayer",
        json!([login["returnValue"]["playerUUID"]]),
    )
    .await;
    let token = &session["sessionToken"];
    assert_eq!(
        session["returnValue"]["playerUUID"],
        guest.player_id.to_string()
    );
    assert!(app.resume_guest(initial).await.is_err());
    let grant = rpc(&r, "collectHeroChest", json!([token, "starter"])).await;
    assert_eq!(grant["code"], 0);
    let duplicate = rpc(&r, "collectHeroChest", json!([token, "starter"])).await;
    assert_eq!(duplicate["code"], 0);
    assert_ne!(
        rpc(
            &r,
            "collectHeroChest",
            json!([grant["sessionToken"], "starter"])
        )
        .await["code"],
        0
    );
    let buy = rpc(&r, "forgeCard", json!([token, "unit"])).await;
    assert_eq!(buy["code"], 0);
    assert_eq!(
        rpc(&r, "forgeCard", json!([token, "unit"])).await["code"],
        0
    );
    assert_ne!(
        rpc(&r, "forgeCard", json!([buy["sessionToken"], "unit"])).await["code"],
        0
    );
    let inventory = rpc(&r, "getPlayerCardInventory", json!([token])).await;
    assert_eq!(inventory["returnValue"]["ownedCards"][0]["count"], 1);
    assert_eq!(
        rpc(&r, "storeProcessIAP", json!([token, {}])).await["code"],
        -6
    );
    for n in [3, 10, 1] {
        assert_eq!(
            rpc(&r, "achievementStoreScore", json!([token, "practice", n])).await["code"],
            0
        );
    }
    assert_eq!(
        rpc(&r, "achievementGetScore", json!([token, "practice"])).await["returnValue"]["score"],
        10
    );

    let other = app.create_guest("Other player".into()).await.unwrap();
    let public = rpc(&r, "getPlayerInfo", json!([token, other.player_id])).await;
    assert_eq!(
        public["returnValue"],
        json!({"handle":"Other player","playerUUID":other.player_id})
    );
    let own = json!([token, guest.player_id]);
    let initial_info = rpc(&r, "getPlayerInfo", own.clone()).await;
    assert_eq!(initial_info["returnValue"]["wins"], 0);
    assert_eq!(initial_info["returnValue"]["tutorialState"]["state"], -1);
    let update = rpc(&r, "setTutorialState", json!([token, 3, 1])).await;
    assert_eq!(
        update["returnValue"]["tutorialState"],
        json!({"state":3,"clientMode":1})
    );

    let joined = rpc(
        &r,
        "joinLobby",
        json!([token, {"lobby":"5v5_bots_solo"},"147219"]),
    )
    .await;
    assert_eq!(joined["code"], 0);
    rpc(&r, "update", json!([token])).await;
    let playing = rpc(&r, "update", json!([token])).await;
    let room = uuid::Uuid::parse_str(playing["returnValue"]["matchId"].as_str().unwrap()).unwrap();
    let result = json!({"reason":"completed","players":{(guest.player_id.to_string()):{"hero":"Ringo","victory":true}}});
    app.finish_match(room, result.clone()).unwrap();
    app.finish_match(room, result).unwrap();
    let progress = rpc(&r, "getQuestDisplayDataForPlayer", json!([token])).await;
    let quests = progress["returnValue"]["questDisplayData"]
        .as_array()
        .unwrap();
    assert_eq!(quests.len(), 2);
    assert!(quests.iter().all(|quest| quest["progress"] == 1));
    assert_eq!(
        rpc(&r, "redeemQuestForPlayer", json!([token, "first_win"])).await["code"],
        0
    );
    let info = rpc(&r, "getPlayerInfo", own.clone()).await;
    let info = &info["returnValue"];
    assert_eq!(info["completed"], 1);
    assert_eq!(info["wins"], 1);
    assert_eq!(info["winsToday"], 1);
    assert_eq!(info["winStreak"], 1);
    assert_eq!(info["completed_non_tutorial"], 1);
    assert_eq!(info["currency"]["silver"], 5);
    assert_eq!(info["currency"], info["targeting"]["currency"]);
    let heroes = rpc(&r, "getHeroMastery", json!([token])).await;
    assert_eq!(
        heroes["returnValue"]["heroStats"],
        json!([{"hero":"Ringo","completed":1,"wins":1}])
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
    let after_restart = rpc(&reopened, "getPlayerInfo", own).await;
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
        &r,
        "startSessionForPlayer",
        json!([guest["returnValue"]["playerUUID"]]),
    )
    .await;
    let token = &session["sessionToken"];
    let mut names = std::collections::BTreeSet::new();
    for row in include_str!("../../../docs/ce-rpc.tsv").lines().skip(1) {
        let method = row.split('\t').next().unwrap();
        assert!(names.insert(method), "duplicate catalog method {method}");
        if method == "endSession" {
            continue;
        }
        let response = rpc(&r, method, json!([token])).await;
        let code = response["code"].as_i64().expect("CE RPC status code");
        assert_ne!(code, -32601, "unimplemented route {method}");
        assert_ne!(code, -3, "internal error in {method}");
    }
    assert_eq!(rpc(&r, "endSession", json!([token])).await["code"], 0);
}
