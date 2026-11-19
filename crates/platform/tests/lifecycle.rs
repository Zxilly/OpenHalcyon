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
