use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use argon2::{Algorithm, Argon2, Params, PasswordHasher, Version, password_hash::SaltString};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use halcyon_admin::{AdminConfig, router};
use halcyon_game::Rooms;
use halcyon_platform::{AppState, PlatformConfig};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://admin.example.test";
const PASSWORD: &str = "test-only admin password";

fn service() -> (Router, AppState) {
    let rooms = Rooms::new(IpAddr::V4(Ipv4Addr::LOCALHOST), "localhost".into(), 4, None);
    let platform = AppState::open(
        PlatformConfig {
            database_path: ":memory:".into(),
            public_address: "localhost".into(),
            seats_per_match: 1,
        },
        Arc::new(rooms.clone()),
    )
    .unwrap();
    let salt = SaltString::encode_b64(b"test-only salt").unwrap();
    let password_hash = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(64, 1, 1, None).unwrap(),
    )
    .hash_password(PASSWORD.as_bytes(), &salt)
    .unwrap()
    .to_string();
    let app = router(
        AdminConfig {
            origin: ORIGIN.into(),
            username: "owner".into(),
            password_hash,
            instance_id: Uuid::new_v4(),
            started_at: chrono::Utc::now(),
            client_build: 147219,
        },
        platform.clone(),
        rooms,
        Arc::new(Vec::new),
    )
    .unwrap();
    (app, platform)
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/admin/v1{path}"));
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let request = request
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    assert_eq!(parts.headers[header::CACHE_CONTROL], "no-store");
    if parts.status.is_client_error() {
        assert_eq!(
            parts.headers[header::CONTENT_TYPE],
            "application/problem+json"
        );
    }
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (parts.status, parts.headers, value)
}

fn cookie(headers: &HeaderMap) -> &str {
    let full = headers[header::SET_COOKIE].to_str().unwrap();
    for attr in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
        assert!(full.contains(attr));
    }
    assert!(!full.contains("Domain"));
    full.split(';').next().unwrap()
}

#[tokio::test]
async fn admin_auth_origin_rotation_players_and_termination() {
    let (app, platform) = service();
    let credentials = json!({"username":"owner", "password":PASSWORD});
    let denied = request(&app, "POST", "/session", None, None, credentials.clone()).await;
    assert_eq!(denied.0, StatusCode::FORBIDDEN);
    assert_eq!(denied.2["code"], "origin_rejected");
    assert_eq!(
        request(&app, "GET", "/overview", None, None, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED,
    );
    let first = request(
        &app,
        "POST",
        "/session",
        Some(ORIGIN),
        None,
        credentials.clone(),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.2["username"], "owner");
    assert_eq!(first.2.as_object().unwrap().len(), 2);
    let old = cookie(&first.1);
    let next = request(
        &app,
        "POST",
        "/session",
        Some(ORIGIN),
        Some(old),
        credentials,
    )
    .await;
    assert_eq!(next.0, StatusCode::OK);
    let key = cookie(&next.1);
    assert_ne!(old, key);
    assert_eq!(
        request(&app, "GET", "/session", None, Some(old), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED,
    );

    let a = platform.create_guest("Harbor".into()).await.unwrap();
    let b = platform.create_guest("Lighthouse".into()).await.unwrap();
    let first_page = request(
        &app,
        "GET",
        "/players?limit=1&online=true",
        None,
        Some(key),
        Value::Null,
    )
    .await
    .2;
    let cursor = first_page["nextCursor"].as_str().unwrap();
    let second_page = request(
        &app,
        "GET",
        &format!("/players?limit=1&online=true&cursor={cursor}"),
        None,
        Some(key),
        Value::Null,
    )
    .await
    .2;
    assert_ne!(first_page["items"][0]["id"], second_page["items"][0]["id"]);
    assert!(second_page["nextCursor"].is_null());
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/players?online=false&cursor={cursor}"),
            None,
            Some(key),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
    );
    platform.end_session(&b.session_token).await.unwrap();
    let overview = request(&app, "GET", "/overview", None, Some(key), Value::Null)
        .await
        .2;
    assert_eq!(overview["onlinePlayers"], 1);

    platform
        .join(&a.session_token, "solo".into())
        .await
        .unwrap();
    let offered = platform
        .profile(&a.session_token)
        .await
        .unwrap()
        .lobby
        .unwrap();
    let match_id = offered.match_id.unwrap();
    platform.accept(&a.session_token, match_id).await.unwrap();
    let endpoint = format!("/matches/{match_id}");
    let detail = request(&app, "GET", &endpoint, None, Some(key), Value::Null)
        .await
        .2;
    assert_eq!(detail["participants"][0]["displayName"], "Harbor");
    let endpoint = format!("{endpoint}/termination");
    let reason = json!({"reason":"admin integration test"});
    assert_eq!(
        request(
            &app,
            "PUT",
            &endpoint,
            Some("https://elsewhere.test"),
            Some(key),
            reason.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &endpoint,
            Some(ORIGIN),
            Some(key),
            reason.clone()
        )
        .await
        .0,
        StatusCode::ACCEPTED,
    );
    let confirmed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let response = request(
                &app,
                "PUT",
                &endpoint,
                Some(ORIGIN),
                Some(key),
                reason.clone(),
            )
            .await;
            if response.0 == StatusCode::OK {
                break response.2;
            }
            assert_eq!(response.0, StatusCode::ACCEPTED);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(confirmed["requestedBy"], "owner");
    assert_eq!(
        request(
            &app,
            "PUT",
            &endpoint,
            Some(ORIGIN),
            Some(key),
            json!({"reason":"changed"})
        )
        .await
        .0,
        StatusCode::CONFLICT,
    );
    let events = request(
        &app,
        "GET",
        &format!("/matches/{match_id}/events"),
        None,
        Some(key),
        Value::Null,
    )
    .await
    .2;
    assert!(
        events["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "admin_action")
    );
    let logout = request(
        &app,
        "DELETE",
        "/session",
        Some(ORIGIN),
        Some(key),
        Value::Null,
    )
    .await;
    assert_eq!(logout.0, StatusCode::NO_CONTENT);
    assert!(
        logout.1[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
}

#[tokio::test]
async fn login_rate_limit_unifies_invalid_credentials() {
    let (app, _) = service();
    for index in 0..5 {
        let username = if index == 0 { "owner" } else { "unknown" };
        let response = request(
            &app,
            "POST",
            "/session",
            Some(ORIGIN),
            None,
            json!({
                "username":username, "password":"incorrect"
            }),
        )
        .await;
        assert_eq!(response.0, StatusCode::UNAUTHORIZED);
        assert_eq!(response.2["code"], "unauthorized");
    }
    let response = request(
        &app,
        "POST",
        "/session",
        Some(ORIGIN),
        None,
        json!({
            "username":"owner", "password":PASSWORD
        }),
    )
    .await;
    assert_eq!(response.0, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        response.1[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );
}
