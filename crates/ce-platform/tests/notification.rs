use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use futures_util::StreamExt;
use halcyon_ce_platform::{Config, Platform};
use halcyon_chat::Directory;
use halcyon_platform::{
    AppState, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;
use uuid::Uuid;

struct Host;
impl MatchHost for Host {
    fn reserve(&self, _reservation: Reservation) -> Result<HostedMatch, PlatformError> {
        Err(PlatformError::HostUnavailable)
    }
}

async fn rpc_response(api: &Router, method: &str, params: Value) -> Value {
    let response = api
        .clone()
        .oneshot(
            Request::post(format!("/JSONRpc/{method}"))
                .body(Body::from(
                    json!({"method":method,"params":params}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap()
}

async fn rpc(api: &Router, method: &str, params: Value) -> Value {
    let body = rpc_response(api, method, params).await;
    assert_eq!(body["code"], 0, "{method} failed");
    body
}

async fn guest(api: &Router, handle: &str) -> Value {
    let user = rpc(
        api,
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
        api,
        "startSessionForPlayer",
        json!([user["returnValue"]["playerUUID"]]),
    )
    .await;
    rpc(
        api,
        "renamePlayerHandle",
        json!([session["sessionToken"], handle]),
    )
    .await;
    session
}

async fn http(addr: std::net::SocketAddr, path: &str) -> (u16, Value) {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers
        .split_ascii_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn path(value: &Value) -> String {
    let uri: axum::http::Uri = value.as_str().unwrap().parse().unwrap();
    uri.path().to_owned()
}

fn resume(path: &str, cursor: &Value) -> String {
    format!(
        "{path}?time={}&tag={}",
        cursor["time"].as_str().unwrap(),
        cursor["tag"].as_str().unwrap()
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn socket_and_long_poll_share_recipient_cursor_and_social_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("app.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: database.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 3,
        },
        Arc::new(Host),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let platform = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: addr.to_string(),
        },
        &[51; 32],
        &database,
        None,
    )
    .unwrap()
    .with_xmpp("chat.example.test:5222")
    .unwrap();
    let api = platform.rpc();
    let notify = platform.notifications();
    let listener_task = tokio::spawn(async move { axum::serve(listener, notify).await.unwrap() });
    // Produce a fresh native session endpoint without running the TLS RPC application.
    let a = guest(&api, "Harbor").await;
    let b = guest(&api, "Relay").await;
    let a_rv = &a["returnValue"];
    assert_eq!(a_rv["chatUrl"], "xmpp://chat.example.test:5222");
    assert_eq!(a_rv["chatUser"], a_rv["playerUUID"]);
    assert_eq!(a_rv["constants"]["featuresEnabled"]["inGameChat"], true);
    assert_eq!(a_rv["chatPass"].as_str().unwrap().len(), 40);
    let a_id = serde_json::from_value::<Uuid>(a_rv["playerUUID"].clone()).unwrap();
    let b_id = serde_json::from_value::<Uuid>(b["returnValue"]["playerUUID"].clone()).unwrap();
    let pass = a_rv["chatPass"].as_str().unwrap();
    let chat_session = platform.authenticate(a_id, pass).await.unwrap();
    assert!(platform.authenticate(b_id, pass).await.is_none());
    assert!(platform.contacts(&chat_session).await.unwrap().is_empty());
    assert!(a_rv["jsonChatUrl"].as_str().unwrap().starts_with("ws://"));
    let lp_path = path(&a_rv["notifyFallbackUrl"]);
    let ws_path = path(&a_rv["notifyUrl"]);
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{ws_path}"))
        .await
        .unwrap();
    let mut next = async || {
        let item = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str::<Value>(item.to_text().unwrap()).unwrap()
    };
    let base = next().await;
    assert_eq!(base["httpStatus"], 200);
    assert_eq!(base["text"]["states"]["state"], "menus");
    assert_eq!(http(addr, &lp_path).await.1["tag"], base["tag"]);

    rpc(
        &api,
        "joinLobby",
        json!([b["sessionToken"],{"lobby":"test"}]),
    )
    .await;
    rpc(
        &api,
        "joinLobby",
        json!([a["sessionToken"],{"lobby":"test"}]),
    )
    .await;
    let queued = next().await;
    assert_eq!(queued["tag"], Uuid::from_u128(1).to_string());
    assert_eq!(queued["text"]["states"]["state"], "pending_auto");
    let (status, polled) = http(addr, &resume(&lp_path, &base)).await;
    assert_eq!(status, 200);
    assert_eq!(polled["tag"], queued["tag"]);
    assert_eq!(polled["text"]["states"]["version"], queued["tag"]);

    rpc(
        &api,
        "friendRequestIssueByHandle",
        json!([a["sessionToken"], "Relay"]),
    )
    .await;
    let social = next().await;
    assert_eq!(social["tag"], Uuid::from_u128(2).to_string());
    assert_eq!(social["text"]["type"], "friendsListUpdate");
    let (_, polled) = http(addr, &resume(&lp_path, &queued)).await;
    assert_eq!(polled["tag"], social["tag"]);
    assert_eq!(polled["text"], social["text"]);
    assert!(platform.contacts(&chat_session).await.unwrap().is_empty());
    rpc(
        &api,
        "friendRequestConfirm",
        json!([b["sessionToken"], a_id]),
    )
    .await;
    let contacts = platform.contacts(&chat_session).await.unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].id, b_id);
    assert!(platform.presence_allowed(&chat_session).await);
    assert!(platform.receives_presence(&chat_session).await);
    rpc(
        &api,
        "setPresenceInvisibility",
        json!([a["sessionToken"], true]),
    )
    .await;
    rpc(
        &api,
        "presenceSetReceiveBroadcast",
        json!([a["sessionToken"], false]),
    )
    .await;
    assert!(!platform.presence_allowed(&chat_session).await);
    assert!(!platform.receives_presence(&chat_session).await);
    assert_eq!(platform.contacts(&chat_session).await.unwrap()[0].id, b_id);

    rpc(&api, "endSession", json!([a["sessionToken"]])).await;
    assert!(platform.authenticate(a_id, pass).await.is_none());
    assert!(platform.contacts(&chat_session).await.is_none());
    assert_eq!(http(addr, &lp_path).await.0, 401);
    listener_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn party_notices_are_private_replayable_and_revoked_with_membership() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("party.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: database.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 3,
        },
        Arc::new(Host),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut platform = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: addr.to_string(),
        },
        &[52; 32],
        &database,
        None,
    )
    .unwrap();
    Platform::configure_native_state_updates(&mut platform, true).unwrap();
    let api = platform.rpc();
    let notify = platform.notifications();
    let task = tokio::spawn(async move { axum::serve(listener, notify).await.unwrap() });

    let owner = guest(&api, "Captain").await;
    let member = guest(&api, "Ranger").await;
    let invited = guest(&api, "Scout").await;
    let outsider = guest(&api, "Traveler").await;
    let member_id = &member["returnValue"]["playerUUID"];
    let invited_id = &invited["returnValue"]["playerUUID"];
    let owner_lp = path(&owner["returnValue"]["notifyFallbackUrl"]);
    let member_lp = path(&member["returnValue"]["notifyFallbackUrl"]);
    let invited_lp = path(&invited["returnValue"]["notifyFallbackUrl"]);
    let outsider_lp = path(&outsider["returnValue"]["notifyFallbackUrl"]);
    assert_eq!(owner["returnValue"]["enableStateUpdates"], true);
    let owner_base = http(addr, &owner_lp).await.1;
    let member_base = http(addr, &member_lp).await.1;
    let invited_base = http(addr, &invited_lp).await.1;
    let outsider_base = http(addr, &outsider_lp).await.1;
    let ws = path(&member["returnValue"]["notifyUrl"]);
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{ws}"))
        .await
        .unwrap();
    let mut next = async || {
        let item = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str::<Value>(item.to_text().unwrap()).unwrap()
    };
    assert_eq!(next().await["tag"], member_base["tag"]);

    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_party_notice BEFORE INSERT ON events
           WHEN json_extract(NEW.body,'$.type')='quintPartyStateUpdate'
           BEGIN SELECT RAISE(ABORT, 'injected notice write failure'); END;",
    )
    .unwrap();
    let failed = rpc_response(&api, "createQuintParty", json!([owner["sessionToken"]])).await;
    assert_ne!(failed["code"], 0);
    assert_eq!(
        rpc(&api, "queryPartyInfo", json!([owner["sessionToken"]])).await["returnValue"]["partyUUID"],
        Value::Null
    );
    db.execute_batch("DROP TRIGGER reject_party_notice")
        .unwrap();

    let party = rpc(
        &api,
        "createQuintParty",
        json!([owner["sessionToken"], {"mode":"casual_5v5"}]),
    )
    .await;
    let party_id = &party["returnValue"]["partyUUID"];
    let created = http(addr, &resume(&owner_lp, &owner_base)).await.1;
    assert_eq!(
        created["text"],
        json!({"code":0,"quintPartyStateUpdate":{}})
    );

    for _ in 0..2 {
        rpc(
            &api,
            "partyInviteSend",
            json!([owner["sessionToken"], member_id]),
        )
        .await;
    }
    let invitation = next().await;
    assert_eq!(
        invitation["text"],
        json!({"code":0,"quintPartyJoinRequest":{}})
    );
    let polled = http(addr, &resume(&member_lp, &member_base)).await.1;
    assert_eq!(polled["tag"], invitation["tag"]);
    assert_eq!(polled["text"], invitation["text"]);
    for _ in 0..2 {
        rpc(
            &api,
            "partyInviteConfirm",
            json!([member["sessionToken"], party_id]),
        )
        .await;
    }
    let joined = next().await;
    assert_eq!(joined["tag"], Uuid::from_u128(2).to_string());
    assert_eq!(joined["text"], json!({"code":0,"quintPartyStateUpdate":{}}));
    assert_eq!(
        http(addr, &resume(&member_lp, &member_base)).await.1["text"]["type"],
        "stateUpdate"
    );
    rpc(&api, "leaveQuintParty", json!([member["sessionToken"]])).await;
    let owner_joined = http(addr, &resume(&owner_lp, &created)).await.1;
    let left = http(addr, &resume(&owner_lp, &owner_joined)).await.1;
    assert_eq!(left["text"], json!({"code":0,"quintPartyPlayerLeft":{}}));

    // The old invite and roster cursors remain usable after the member returns and is removed.
    rpc(
        &api,
        "joinQuintParty",
        json!([member["sessionToken"], party["returnValue"]["quintCode"]]),
    )
    .await;
    let rejoined = next().await;
    assert_eq!(rejoined["tag"], Uuid::from_u128(3).to_string());
    rpc(
        &api,
        "partyMemberKick",
        json!([owner["sessionToken"], member_id]),
    )
    .await;
    let kicked = next().await;
    assert_eq!(kicked["text"], json!({"code":0,"quintPartyMemberKick":{}}));
    let stale = http(addr, &resume(&member_lp, &joined)).await.1;
    assert_eq!(stale["tag"], rejoined["tag"]);
    assert_eq!(stale["text"]["type"], "stateUpdate");

    rpc(
        &api,
        "partyInviteSend",
        json!([owner["sessionToken"], invited_id]),
    )
    .await;
    rpc(&api, "destroyQuintParty", json!([owner["sessionToken"]])).await;
    let withdrawn = http(addr, &resume(&invited_lp, &invited_base)).await.1;
    assert_eq!(withdrawn["text"]["type"], "stateUpdate");

    // An unrelated player gets their own first event, with no hidden ordinal increments.
    rpc(
        &api,
        "joinLobby",
        json!([outsider["sessionToken"], {"lobby":"test"}]),
    )
    .await;
    let outsider_event = http(addr, &resume(&outsider_lp, &outsider_base)).await.1;
    assert_eq!(outsider_event["tag"], Uuid::from_u128(1).to_string());
    assert_eq!(outsider_event["text"]["type"], "stateUpdate");
    task.abort();
}
