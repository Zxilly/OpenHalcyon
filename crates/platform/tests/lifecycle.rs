use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use halcyon_platform::{
    AppState, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation, router,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Default)]
struct Host(AtomicUsize);

impl MatchHost for Host {
    fn reserve(&self, reservation: Reservation) -> Result<HostedMatch, PlatformError> {
        assert_eq!(reservation.players.len(), 2);
        assert_eq!(reservation.mode, "duo");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(HostedMatch {
            address: "localhost:8001".into(),
        })
    }
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(key) = key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn guest_rotation_match_acceptance_and_result_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let config = PlatformConfig {
        database_path: directory.path().join("platform.sqlite"),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let host = Arc::new(Host::default());
    let state = AppState::open(config.clone(), host.clone()).unwrap();
    let app = router(state.clone());
    let (_, first) = request(
        &app,
        "POST",
        "/api/v1/guests",
        None,
        json!({"name":"First"}),
    )
    .await;
    let (_, second) = request(
        &app,
        "POST",
        "/api/v1/guests",
        None,
        json!({"name":"Second"}),
    )
    .await;
    let (_, resumed) = request(
        &app,
        "POST",
        "/api/v1/sessions",
        None,
        json!({"recoveryToken":first["recoveryToken"]}),
    )
    .await;
    assert_eq!(first["playerId"], resumed["playerId"]);
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/me",
            first["sessionToken"].as_str(),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
    );

    let first_key = resumed["sessionToken"].as_str();
    let second_key = second["sessionToken"].as_str();
    for key in [first_key, second_key] {
        assert_eq!(
            request(&app, "POST", "/api/v1/queue", key, json!({"mode":"duo"}))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    let (_, events) = request(&app, "GET", "/api/v1/events", first_key, Value::Null).await;
    let match_id = events["events"][1]["body"]["matchId"].as_str().unwrap();
    let endpoint = format!("/api/v1/matches/{match_id}/accept");
    let (first_accept, second_accept) = tokio::join!(
        request(&app, "POST", &endpoint, first_key, Value::Null),
        request(&app, "POST", &endpoint, second_key, Value::Null),
    );
    assert_eq!(first_accept.0, StatusCode::NO_CONTENT);
    assert_eq!(second_accept.0, StatusCode::NO_CONTENT);
    assert_eq!(host.0.load(Ordering::SeqCst), 1);
    let match_id = Uuid::parse_str(match_id).unwrap();
    state
        .finish_match(match_id, json!({"winner":"blue"}))
        .unwrap();
    state
        .finish_match(match_id, json!({"winner":"blue"}))
        .unwrap();
    assert!(matches!(
        state.finish_match(match_id, json!({"winner":"red"})),
        Err(PlatformError::Conflict(_))
    ));
    drop(app);
    drop(state);

    let app = router(AppState::open(config, host).unwrap());
    let (_, profile) = request(&app, "GET", "/api/v1/me", first_key, Value::Null).await;
    assert_eq!(profile["name"], "First");
    assert_eq!(profile["lobby"]["phase"], "finished");
    assert_eq!(profile["lobby"]["result"]["winner"], "blue");
    let uri = format!("/api/v1/events?after={}", events["cursor"]);
    let (_, events) = request(&app, "GET", &uri, first_key, Value::Null).await;
    assert_eq!(events["events"].as_array().unwrap().len(), 2);
    assert_eq!(events["events"][0]["body"]["type"], "match_playing");
    assert_eq!(events["events"][1]["body"]["type"], "match_finished");
}

#[tokio::test]
async fn federation_intent_reopens_and_excludes_local_queue_until_confirmed_release() {
    let directory = tempfile::tempdir().unwrap();
    let config = PlatformConfig {
        database_path: directory.path().join("platform.sqlite"),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let pool = Uuid::new_v4();
    let host = Arc::new(Host::default());
    let state = AppState::open(config.clone(), host.clone()).unwrap();
    let guest = state.create_guest("Remote".into()).await.unwrap();
    let key = &guest.session_token;
    let intent = state
        .federated_join(key, pool, "cross_pool".into(), 2)
        .await
        .unwrap();
    assert_eq!(intent.phase, "pending");
    assert!(state.events(key, 0, false).await.unwrap().events.is_empty());
    assert!(matches!(
        state.join(key, "duo".into()).await,
        Err(PlatformError::Conflict(_))
    ));
    assert!(matches!(
        state
            .federated_join(key, Uuid::new_v4(), "cross_pool".into(), 2)
            .await,
        Err(PlatformError::Conflict(_)),
    ));
    assert!(matches!(
        state.federated_release(intent.ticket).await,
        Err(PlatformError::Conflict(_))
    ));

    state
        .federated_project(intent.ticket, "queued", None, None, None)
        .await
        .unwrap();
    let match_id = Uuid::new_v4();
    state
        .federated_project(intent.ticket, "offered", Some(match_id), None, None)
        .await
        .unwrap();
    state
        .federated_action(intent.ticket, Some(match_id), false)
        .await
        .unwrap();
    assert_eq!(state.counts().await.unwrap().queued_players, 1);
    drop(state);

    let state = AppState::open(config, host).unwrap();
    let loaded = state.federated_intents(pool).await.unwrap().remove(0);
    assert_eq!(loaded.ticket, intent.ticket);
    assert_eq!(loaded.accepted_match, Some(match_id));
    let address = Some("127.0.0.1:9000".into());
    state
        .federated_project(
            intent.ticket,
            "playing",
            Some(match_id),
            address.clone(),
            None,
        )
        .await
        .unwrap();
    state
        .federated_project(intent.ticket, "playing", Some(match_id), address, None)
        .await
        .unwrap();
    let lobby = state.profile(key).await.unwrap().lobby.unwrap();
    assert_eq!(lobby.phase, "playing");
    assert_eq!(lobby.match_id, Some(match_id));
    assert_eq!(state.counts().await.unwrap().queued_players, 0);
    assert_eq!(
        state
            .events(key, 0, false)
            .await
            .unwrap()
            .events
            .iter()
            .filter(|event| event.body["type"] == "match_playing")
            .count(),
        1,
    );
    state
        .federated_project(
            intent.ticket,
            "finished",
            Some(match_id),
            None,
            Some(json!({"winner":"blue"})),
        )
        .await
        .unwrap();
    state.federated_release(intent.ticket).await.unwrap();
    assert!(state.federated_intent(key).await.unwrap().is_none());
    assert!(state.profile(key).await.unwrap().lobby.is_none());
    state.join(key, "duo".into()).await.unwrap();
}
