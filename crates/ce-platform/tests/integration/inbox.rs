use crate::native_rpc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use futures_util::StreamExt;
use halcyon_ce_platform::{Config, Platform, RpcMethod};
use halcyon_platform::{
    AppState, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
};
use native_rpc::{RpcEnvelope, android_guest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

struct Host;
impl MatchHost for Host {
    fn reserve(&self, _reservation: Reservation) -> Result<HostedMatch, PlatformError> {
        Err(PlatformError::HostUnavailable)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RpcResponseContract<T> {
    code: i32,
    session_token: String,
    return_value: T,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuestReturn {
    #[serde(rename = "playerUUID")]
    player_uuid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionReturn {
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    pending_messages: u32,
    notify_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerInfoPending {
    pending_messages: u32,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageSentContract {
    message_id: Uuid,
    success: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InboxContract {
    messages: Vec<InboxMessageContract>,
    pending_messages: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InboxMessageContract {
    message_id: String,
    message: String,
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    time: i64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SuccessContract {
    success: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationEnvelope {
    http_status: u16,
    tag: String,
}

#[derive(Serialize)]
struct ObjectFriendNotify<'a> {
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    message: &'a str,
}

#[derive(Serialize)]
struct ObjectFriendReply<'a> {
    #[serde(rename = "targetUUID")]
    target_uuid: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct ObjectDismissMessageId<'a> {
    #[serde(rename = "messageId")]
    message_id: &'a str,
}

#[derive(Serialize)]
struct ObjectDismissId<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct MissingMessageField<'a> {
    #[serde(rename = "playerUUID")]
    player_uuid: Uuid,
    #[serde(rename = "wrongField")]
    wrong_field: &'a str,
}

fn open_platform(database: &Path, notify_authority: &str) -> Arc<Platform> {
    let app = AppState::open(
        PlatformConfig {
            database_path: database.to_path_buf(),
            public_address: "localhost:8443".into(),
            seats_per_match: 3,
        },
        Arc::new(Host),
    )
    .unwrap();
    Platform::new(
        app,
        Config {
            authority: "localhost:8443".into(),
            notify_authority: notify_authority.into(),
        },
        &[51; 32],
        database,
        None,
    )
    .unwrap()
}

async fn rpc_raw(api: &Router, method: RpcMethod, params: impl Serialize) -> Value {
    let name = method.as_str();
    let req = Request::post(format!("/JSONRpc/{name}"))
        .body(Body::from(
            serde_json::to_string(&RpcEnvelope {
                method: name,
                params,
            })
            .unwrap(),
        ))
        .unwrap();
    let resp = api.clone().oneshot(req).await.unwrap();
    serde_json::from_slice(&to_bytes(resp.into_body(), 1 << 20).await.unwrap()).unwrap()
}

async fn rpc_ok<T: for<'de> Deserialize<'de>>(
    api: &Router,
    method: RpcMethod,
    params: impl Serialize,
) -> (RpcResponseContract<T>, Value) {
    let raw = rpc_raw(api, method, params).await;
    let map = raw.as_object().unwrap();
    assert_eq!(map["code"], 0, "{} failed: {raw}", method.as_str());
    assert!(
        !map.contains_key("reason"),
        "reason must be omitted on success: {raw}"
    );
    let typed = serde_json::from_value::<RpcResponseContract<T>>(raw.clone()).unwrap();
    assert_eq!(typed.code, 0);
    assert!(typed.reason.is_none());
    (typed, raw)
}

fn assert_rpc_error(raw: &Value, expected_code: i32, expected_reason: &str) {
    let parsed = serde_json::from_value::<RpcResponseContract<Value>>(raw.clone()).unwrap();
    assert_eq!(
        parsed.code, expected_code,
        "unexpected error payload: {raw}"
    );
    assert!(parsed.session_token.is_empty());
    assert!(
        parsed.return_value.is_null(),
        "returnValue must be null: {raw}"
    );
    assert_eq!(parsed.reason.as_deref(), Some(expected_reason));
}

struct TestGuest {
    id: Uuid,
    guest_token: String,
    session_token: String,
    notify_url: String,
}

async fn create_guest(api: &Router, handle: &str) -> TestGuest {
    let (guest_resp, _) = rpc_ok::<GuestReturn>(
        api,
        RpcMethod::CreateGuestPlayer,
        android_guest("inbox-device", "0"),
    )
    .await;
    let guest_token = guest_resp.return_value.player_uuid;
    let (session_resp, session_raw) =
        rpc_ok::<SessionReturn>(api, RpcMethod::StartSessionForPlayer, (&guest_token,)).await;
    assert!(session_raw["returnValue"]["pendingMessages"].is_u64());
    let id = session_resp.return_value.player_uuid;
    let notify_url = session_resp.return_value.notify_url;
    let session_token = session_resp.session_token;
    rpc_ok::<Value>(api, RpcMethod::RenamePlayerHandle, (&session_token, handle)).await;
    TestGuest {
        id,
        guest_token,
        session_token,
        notify_url,
    }
}

async fn become_friends(
    api: &Router,
    requester: &TestGuest,
    target: &TestGuest,
    target_handle: &str,
) {
    rpc_ok::<Value>(
        api,
        RpcMethod::FriendRequestIssueByHandle,
        (&requester.session_token, target_handle),
    )
    .await;
    rpc_ok::<Value>(
        api,
        RpcMethod::FriendRequestConfirm,
        (&target.session_token, requester.id),
    )
    .await;
}

#[tokio::test]
async fn inbox_lifecycle_schemas_object_params_and_idempotency() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("inbox.sqlite");
    let platform = open_platform(&database, "localhost:2112");
    let api = platform.rpc();

    let alice = create_guest(&api, "Beacon").await;
    let bob = create_guest(&api, "Anchor").await;

    let (initial_info, initial_info_raw) =
        rpc_ok::<PlayerInfoPending>(&api, RpcMethod::GetPlayerInfo, (&bob.session_token,)).await;
    assert_eq!(initial_info.return_value.pending_messages, 0);
    assert!(initial_info_raw["returnValue"]["pendingMessages"].is_u64());

    let (empty_inbox, empty_inbox_raw) = rpc_ok::<InboxContract>(
        &api,
        RpcMethod::QueryPlayerInboxMessages,
        (&bob.session_token,),
    )
    .await;
    assert_eq!(
        empty_inbox.return_value,
        InboxContract {
            messages: Vec::new(),
            pending_messages: false,
        }
    );
    assert!(empty_inbox_raw["returnValue"]["pendingMessages"].is_boolean());

    // Strangers cannot send friend notifications (-5 forbidden).
    for err in [
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, "hi"),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::AskInGameFriendToPlay,
            (&alice.session_token, bob.id),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendReplyFromInGame,
            (&alice.session_token, bob.id, "busy"),
        )
        .await,
    ] {
        assert_rpc_error(&err, -5, "operation forbidden");
    }

    // Pending (unconfirmed) friend requests remain rejected with -5.
    rpc_ok::<Value>(
        &api,
        RpcMethod::FriendRequestIssueByHandle,
        (&alice.session_token, "Anchor"),
    )
    .await;
    let pending_err = rpc_raw(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "still pending"),
    )
    .await;
    assert_rpc_error(&pending_err, -5, "operation forbidden");

    // Confirm friendship, then exercise positional + object request variants.
    rpc_ok::<Value>(
        &api,
        RpcMethod::FriendRequestConfirm,
        (&bob.session_token, alice.id),
    )
    .await;

    let (sent1, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "party up?"),
    )
    .await;
    assert!(sent1.return_value.success);

    let (sent2, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::AskInGameFriendToPlay,
        (&alice.session_token, bob.id),
    )
    .await;
    assert!(sent2.return_value.success);

    let bob_uuid_str = bob.id.to_string();
    let (sent3, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::FriendReplyFromInGame,
        (
            &alice.session_token,
            ObjectFriendReply {
                target_uuid: &bob_uuid_str,
                text: "omw mid",
            },
        ),
    )
    .await;
    assert!(sent3.return_value.success);

    let (sent4, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::FriendNotify,
        (
            &alice.session_token,
            ObjectFriendNotify {
                player_uuid: bob.id,
                message: "ready check",
            },
        ),
    )
    .await;
    assert!(sent4.return_value.success);

    let (bob_info, bob_info_raw) =
        rpc_ok::<PlayerInfoPending>(&api, RpcMethod::GetPlayerInfo, (&bob.session_token,)).await;
    assert_eq!(bob_info.return_value.pending_messages, 4);
    assert!(bob_info_raw["returnValue"]["pendingMessages"].is_u64());

    let (inbox, inbox_raw) = rpc_ok::<InboxContract>(
        &api,
        RpcMethod::QueryPlayerInboxMessages,
        (&bob.session_token,),
    )
    .await;
    assert!(inbox_raw["returnValue"]["pendingMessages"].is_boolean());
    assert!(inbox.return_value.pending_messages);
    assert_eq!(inbox.return_value.messages.len(), 4);

    let expected = [
        (sent1.return_value.message_id.to_string(), "party up?"),
        (
            sent2.return_value.message_id.to_string(),
            RpcMethod::AskInGameFriendToPlay.as_str(),
        ),
        (sent3.return_value.message_id.to_string(), "omw mid"),
        (sent4.return_value.message_id.to_string(), "ready check"),
    ];
    for ((entry, (expected_id, expected_text)), raw_entry) in inbox
        .return_value
        .messages
        .iter()
        .zip(expected)
        .zip(inbox_raw["returnValue"]["messages"].as_array().unwrap())
    {
        assert_eq!(entry.message_id, expected_id);
        assert_eq!(entry.message, expected_text);
        assert_eq!(entry.player_uuid, alice.id);
        assert!(entry.time > 0);
        assert!(raw_entry["time"].is_i64());
    }

    // Sender cannot dismiss recipient's message; unknown UUID fails with -6.
    let cross_recipient = rpc_raw(
        &api,
        RpcMethod::ProcessMessage,
        (&alice.session_token, sent1.return_value.message_id),
    )
    .await;
    assert_rpc_error(&cross_recipient, -6, "no such message");
    let missing_id = rpc_raw(
        &api,
        RpcMethod::DismissReliableMessage,
        (&bob.session_token, Uuid::new_v4()),
    )
    .await;
    assert_rpc_error(&missing_id, -6, "no such message");

    // ProcessMessage transitions dismissed=0 -> 1 and reduces pendingMessages.
    let (dismiss1, _) = rpc_ok::<SuccessContract>(
        &api,
        RpcMethod::ProcessMessage,
        (&bob.session_token, sent1.return_value.message_id),
    )
    .await;
    assert_eq!(dismiss1.return_value, SuccessContract { success: true });

    // Re-dismissing the same message is idempotent across both RPC aliases.
    for method in [RpcMethod::ProcessMessage, RpcMethod::DismissReliableMessage] {
        let (again, _) = rpc_ok::<SuccessContract>(
            &api,
            method,
            (&bob.session_token, sent1.return_value.message_id),
        )
        .await;
        assert_eq!(again.return_value, SuccessContract { success: true });
    }

    let (after_one, _) =
        rpc_ok::<PlayerInfoPending>(&api, RpcMethod::GetPlayerInfo, (&bob.session_token,)).await;
    assert_eq!(after_one.return_value.pending_messages, 3);

    // Dismiss the remaining three via object ("messageId", "id") and positional forms.
    let id2 = sent2.return_value.message_id.to_string();
    let id3 = sent3.return_value.message_id.to_string();
    rpc_ok::<SuccessContract>(
        &api,
        RpcMethod::DismissReliableMessage,
        (
            &bob.session_token,
            ObjectDismissMessageId { message_id: &id2 },
        ),
    )
    .await;
    rpc_ok::<SuccessContract>(
        &api,
        RpcMethod::ProcessMessage,
        (&bob.session_token, ObjectDismissId { id: &id3 }),
    )
    .await;
    rpc_ok::<SuccessContract>(
        &api,
        RpcMethod::DismissReliableMessage,
        (&bob.session_token, sent4.return_value.message_id),
    )
    .await;

    let (cleared_inbox, _) = rpc_ok::<InboxContract>(
        &api,
        RpcMethod::QueryPlayerInboxMessages,
        (&bob.session_token,),
    )
    .await;
    assert_eq!(
        cleared_inbox.return_value,
        InboxContract {
            messages: Vec::new(),
            pending_messages: false,
        }
    );
    let (cleared_info, _) =
        rpc_ok::<PlayerInfoPending>(&api, RpcMethod::GetPlayerInfo, (&bob.session_token,)).await;
    assert_eq!(cleared_info.return_value.pending_messages, 0);

    // Unfriending immediately blocks further friend notifications with -5.
    rpc_ok::<Value>(
        &api,
        RpcMethod::FriendDelete,
        (&alice.session_token, bob.id),
    )
    .await;
    let unfriended_err = rpc_raw(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "after delete"),
    )
    .await;
    assert_rpc_error(&unfriended_err, -5, "operation forbidden");
}

#[tokio::test]
async fn inbox_validation_and_hundred_message_cap_with_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("cap.sqlite");
    let platform = open_platform(&database, "localhost:2112");
    let api = platform.rpc();

    let alice = create_guest(&api, "Quarry").await;
    let bob = create_guest(&api, "Lantern").await;
    become_friends(&api, &alice, &bob, "Lantern").await;

    let too_long = "x".repeat(513);
    for invalid in [
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, ""),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, "line\nbreak"),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, "null\0byte"),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, too_long.as_str()),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, "not-a-uuid", "hi"),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::FriendNotify,
            (
                &alice.session_token,
                MissingMessageField {
                    player_uuid: bob.id,
                    wrong_field: "hi",
                },
            ),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::ProcessMessage,
            (&bob.session_token, "bad-uuid\n"),
        )
        .await,
        rpc_raw(
            &api,
            RpcMethod::DismissReliableMessage,
            (&bob.session_token, "not-a-uuid"),
        )
        .await,
    ] {
        assert_rpc_error(&invalid, -32602, "invalid parameters");
    }

    // 512-byte boundary message is accepted.
    let exact_max = "m".repeat(512);
    let (first, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, exact_max.as_str()),
    )
    .await;

    for idx in 1..100 {
        rpc_ok::<MessageSentContract>(
            &api,
            RpcMethod::FriendReplyFromInGame,
            (&alice.session_token, bob.id, format!("msg-{idx}")),
        )
        .await;
    }

    let (info_full, _) =
        rpc_ok::<PlayerInfoPending>(&api, RpcMethod::GetPlayerInfo, (&bob.session_token,)).await;
    assert_eq!(info_full.return_value.pending_messages, 100);

    let full_err = rpc_raw(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "overflow"),
    )
    .await;
    assert_rpc_error(&full_err, -6, "inbox full");

    let invite_full_err = rpc_raw(
        &api,
        RpcMethod::AskInGameFriendToPlay,
        (&alice.session_token, bob.id),
    )
    .await;
    assert_rpc_error(&invite_full_err, -6, "inbox full");

    // Dismissing one message drops unread count to 99 and allows one more insert.
    rpc_ok::<SuccessContract>(
        &api,
        RpcMethod::DismissReliableMessage,
        (&bob.session_token, first.return_value.message_id),
    )
    .await;

    let (recovered, _) = rpc_ok::<MessageSentContract>(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "recovered-slot"),
    )
    .await;
    assert!(recovered.return_value.success);

    let full_again = rpc_raw(
        &api,
        RpcMethod::FriendNotify,
        (&alice.session_token, bob.id, "overflow-again"),
    )
    .await;
    assert_rpc_error(&full_again, -6, "inbox full");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn friend_message_wakes_notification_readers_and_persists_across_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("persist.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let message_id;
    let alice_id;
    let bob_guest_token;
    {
        let platform = open_platform(&database, &addr.to_string());
        let api = platform.rpc();
        let notify = platform.notifications();
        let server = tokio::spawn(async move { axum::serve(listener, notify).await.unwrap() });

        let alice = create_guest(&api, "Sender").await;
        let bob = create_guest(&api, "Receiver").await;
        alice_id = alice.id;
        bob_guest_token = bob.guest_token.clone();
        become_friends(&api, &alice, &bob, "Receiver").await;

        let ws_uri: axum::http::Uri = bob.notify_url.parse().unwrap();
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}{}", ws_uri.path()))
                .await
                .unwrap();

        let initial_frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let initial_notice: NotificationEnvelope =
            serde_json::from_str(initial_frame.to_text().unwrap()).unwrap();
        assert_eq!(initial_notice.http_status, 200);

        let (sent, _) = rpc_ok::<MessageSentContract>(
            &api,
            RpcMethod::FriendNotify,
            (&alice.session_token, bob.id, "wake up"),
        )
        .await;
        message_id = sent.return_value.message_id;

        let wake_frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let wake_notice: NotificationEnvelope =
            serde_json::from_str(wake_frame.to_text().unwrap()).unwrap();
        assert_eq!(wake_notice.http_status, 200);
        assert_ne!(wake_notice.tag, initial_notice.tag);

        drop(socket);
        server.abort();
    }

    // Reopen AppState + Platform on the same SQLite database and verify inbox persistence.
    let reopened = open_platform(&database, "localhost:2112");
    let reopened_api = reopened.rpc();
    let (resumed, resumed_raw) = rpc_ok::<SessionReturn>(
        &reopened_api,
        RpcMethod::StartSessionForPlayer,
        (&bob_guest_token,),
    )
    .await;
    assert_eq!(resumed.return_value.pending_messages, 1);
    assert!(resumed_raw["returnValue"]["pendingMessages"].is_u64());

    let (persisted_inbox, _) = rpc_ok::<InboxContract>(
        &reopened_api,
        RpcMethod::QueryPlayerInboxMessages,
        (&resumed.session_token,),
    )
    .await;
    assert!(persisted_inbox.return_value.pending_messages);
    assert_eq!(persisted_inbox.return_value.messages.len(), 1);
    assert_eq!(
        persisted_inbox.return_value.messages[0].message_id,
        message_id.to_string()
    );
    assert_eq!(persisted_inbox.return_value.messages[0].message, "wake up");
    assert_eq!(
        persisted_inbox.return_value.messages[0].player_uuid,
        alice_id
    );

    rpc_ok::<SuccessContract>(
        &reopened_api,
        RpcMethod::DismissReliableMessage,
        (&resumed.session_token, message_id),
    )
    .await;
    let (after_restart_info, _) = rpc_ok::<PlayerInfoPending>(
        &reopened_api,
        RpcMethod::GetPlayerInfo,
        (&resumed.session_token,),
    )
    .await;
    assert_eq!(after_restart_info.return_value.pending_messages, 0);
}
