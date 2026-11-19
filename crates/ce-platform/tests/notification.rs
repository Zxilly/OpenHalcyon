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

async fn rpc(api: &Router, method: &str, params: Value) -> Value {
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
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap();
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
