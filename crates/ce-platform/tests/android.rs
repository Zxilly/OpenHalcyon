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
    let adapter = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:2112".into(),
        },
        &[71; 32],
        &path,
        None,
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
    assert_eq!(guest["sessionToken"],"");
    let session = rpc(&r, "startSessionForPlayer", json!([recovery])).await;
    assert_eq!(session["returnValue"]["state"], "menus");
    let token = &session["sessionToken"];
    assert_ne!(token, &guest["sessionToken"]);
    let notify = session["returnValue"]["notifyFallbackUrl"]
        .as_str()
        .unwrap()
        .strip_prefix("http://localhost:2112")
        .unwrap();
    assert_eq!(
        rpc(
            &r,
            "joinLobby",
            json!([token, "{\"lobby\":\"5v5_bots_solo\"}", "147219"])
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
    let restored = rpc(&r, "getPlayerForGuestAccount", json!([recovery])).await;
    assert_eq!(restored["sessionToken"],"");
    let resumed=rpc(&r,"startSessionForPlayer",json!([restored["returnValue"]["playerUUID"]])).await;
    assert_eq!(
        resumed["returnValue"]["playerUUID"],
        session["returnValue"]["playerUUID"]
    );
    assert_eq!(rpc(&r, "getPlayerInfo", json!([token])).await["code"], -1);
    assert_eq!(
        rpc(&r, "notARealMethod", json!([resumed["sessionToken"]])).await["code"],
        -32601
    );
}
