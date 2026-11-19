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

#[tokio::test]
async fn match_acceptance_is_bound_to_the_supplied_offer() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("offer.sqlite");
    let app = AppState::open(
        PlatformConfig {
            database_path: database.clone(),
            public_address: "localhost:8443".into(),
            seats_per_match: 3,
        },
        Arc::new(Host),
    )
    .unwrap();
    let platform = Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: "localhost:8080".into(),
        },
        &[72; 32],
        &database,
        None,
    )
    .unwrap();
    let api = platform.rpc();
    let mut tokens = Vec::new();
    for handle in ["North", "South", "West"] {
        let player = guest(&api, handle).await;
        let token = player["sessionToken"].clone();
        rpc(
            &api,
            "joinLobby",
            json!([token, {"lobby":"5v5_pvp_casual"}]),
        )
        .await;
        tokens.push(token);
    }
    let db = rusqlite::Connection::open(database).unwrap();
    let offered: String = db
        .query_row("SELECT id FROM matches WHERE phase='offered'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let accepted = || {
        db.query_row(
            "SELECT sum(accepted) FROM tickets WHERE match_id=?",
            [&offered],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };
    for (wrong, code) in [
        (json!(Uuid::new_v4()), -2),
        (json!("malformed"), -32602),
        (json!(1), -32602),
    ] {
        assert_eq!(
            rpc_response(&api, "acceptMatch", json!([tokens[0], wrong])).await["code"],
            code
        );
        assert_eq!(accepted(), 0);
    }
    for _ in 0..2 {
        let response = rpc(&api, "acceptMatch", json!([tokens[0], offered])).await;
        assert_eq!(response["returnValue"], true);
        assert_eq!(accepted(), 1);
    }
    assert_eq!(
        rpc(&api, "acceptMatch", json!([tokens[1], ""])).await["returnValue"],
        true
    );
    assert_eq!(accepted(), 2);
}

#[tokio::test]
async fn talent_catalog_preserves_public_fields_and_projects_only_the_callers_inventory() {
    use std::{fs, path::Path};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let database = dir.join("talents.sqlite");
    let talent = dir.join("official_getTalentsData.json");
    let write =
        |path: &Path, value: Value| fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    write(
        &dir.join("official_getSkinManifest.json"),
        json!({"returnValue":"{}"}),
    );
    let public = json!({"id":"example_rare","def":"*Talent_Example_A*","hero":"*Example*","maxLevel":false,
                      "levelRequirements":{"[fragments]":90,"silver":1300}});
    let mut archived = public.clone();
    archived["level"] = json!(10);
    archived["fragments"] = json!(44);
    archived["privateField"] = json!("discard");
    let open = || {
        let app = AppState::open(
            PlatformConfig {
                database_path: database.clone(),
                public_address: "localhost:8443".into(),
                seats_per_match: 1,
            },
            Arc::new(Host),
        )
        .unwrap();
        Platform::new(
            app,
            Config {
                authority: "localhost:8443".into(),
                notify_authority: "localhost:8080".into(),
            },
            &[94; 32],
            &database,
            Some(dir),
        )
    };
    for bad in [json!([]), json!({"example_rare":{"id":"bad"}}), {
        let mut bad = archived.clone();
        bad["levelRequirements"] = json!([]);
        json!({"example_rare":bad})
    }] {
        write(&talent, json!({"returnValue":bad}));
        assert!(open().is_err());
    }
    write(
        &talent,
        json!({"sessionToken":"discard", "returnValue":{"example_rare":archived}}),
    );
    let content = dir.join("content.json");
    write(
        &content,
        json!({
            "items":[{"id":"exampleLevel","slot":"progress","name":"Example Level"},
                     {"id":"examplePieces","slot":"progress","name":"Example Pieces"}],
            "talentProgress":{"example_rare":{"levelItem":"exampleLevel","fragmentItem":"examplePieces"}},
            "exchanges":{"openRewardChest":{"welcome":{"limit":1,"reward":{"examplePieces":9}}},
                         "upgradeTalent":{"example_rare":{"cost":{"examplePieces":2},"reward":{"exampleLevel":1}}}}
        }),
    );
    let platform = open().unwrap().with_content(&content).await.unwrap();
    let api = platform.rpc();
    let a = guest(&api, "Amber").await;
    let b = guest(&api, "Slate").await;
    let stat = |level, fragments| {
        let mut v = public.clone();
        v["levelRequirements"] = json!({});
        v["level"] = json!(level);
        v["fragments"] = json!(fragments);
        json!({"example_rare":v})
    };
    let read = |token: Value| rpc(&api, "getTalentsData", json!([token]));
    let ta = &a["sessionToken"];
    let tb = &b["sessionToken"];
    assert_eq!(read(ta.clone()).await["returnValue"], stat(0, 0));
    assert_eq!(read(tb.clone()).await["returnValue"], stat(0, 0));
    assert_ne!(
        rpc_response(&api, "upgradeTalent", json!([tb, "example_rare"])).await["code"],
        0
    );
    rpc(&api, "openRewardChest", json!([ta, "welcome"])).await;
    let upgraded = rpc(&api, "upgradeTalent", json!([ta, "example_rare"])).await;
    assert_ne!(upgraded["sessionToken"], *ta);
    // Simulate a receipt left by the earlier bearer-independent implementation.
    use sha2::{Digest, Sha256};
    let previous =
        Sha256::digest(serde_json::to_vec(&("upgradeTalent", json!(["example_rare"]))).unwrap());
    let db = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE community_redemptions SET request=? WHERE operation='upgradeTalent'",
            [previous.as_slice()]
        )
        .unwrap(),
        1
    );
    rpc(&api, "upgradeTalent", json!([ta, "example_rare"])).await;
    rpc(&api, "upgradeTalent", json!([ta, "example_rare"])).await;
    assert_eq!(read(ta.clone()).await["returnValue"], stat(1, 7));
    let next = rpc(
        &api,
        "upgradeTalent",
        json!([upgraded["sessionToken"], "example_rare"]),
    )
    .await;
    assert_eq!(read(ta.clone()).await["returnValue"], stat(2, 5));
    let stable = json!({"sku":"example_rare","operationId":Uuid::new_v4()});
    let first = rpc(&api, "upgradeTalent", json!([next["sessionToken"], stable])).await;
    rpc(
        &api,
        "upgradeTalent",
        json!([first["sessionToken"], stable]),
    )
    .await;
    assert_eq!(read(ta.clone()).await["returnValue"], stat(3, 3));
    assert_eq!(
        rpc_response(
            &api,
            "upgradeTalent",
            json!([ta,{"sku":"example_rare","operationId":"bad"}])
        )
        .await["code"],
        -32602
    );
    assert_eq!(read(tb.clone()).await["returnValue"], stat(0, 0));
    let reopened = open().unwrap().with_content(&content).await.unwrap();
    let a_id = Uuid::parse_str(a["returnValue"]["playerUUID"].as_str().unwrap()).unwrap();
    assert_eq!(
        reopened
            .talent_progress(a_id, "example_rare")
            .await
            .unwrap()
            .unwrap()
            .level,
        3
    );
    assert!(
        reopened
            .talent_progress(a_id, "absent")
            .await
            .unwrap()
            .is_none()
    );
}
