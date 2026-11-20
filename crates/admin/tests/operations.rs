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
use halcyon_admin::{AdminConfig, PoolHealth, PoolRole, PoolStatus, router};
use halcyon_game::Rooms;
use halcyon_platform::{AppState, PlatformConfig};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://admin.example.test";
const PASSWORD: &str = "test-only admin password";

fn service() -> (Router, AppState) {
    service_with_data(None)
}

fn service_with_data(data: Option<Arc<halcyon_data::GameData>>) -> (Router, AppState) {
    let rooms = Rooms::new(IpAddr::V4(Ipv4Addr::LOCALHOST), "localhost".into(), 4, data);
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
        Arc::new(|| {
            vec![PoolStatus {
                pool_id: Uuid::nil(),
                state: PoolHealth::NoQuorum,
                local_role: PoolRole::Follower,
                leader_id: None,
                voter_count: 3,
                last_contact_at: None,
                eligible_hosts: None,
                local_tickets: 2,
            }]
        }),
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
async fn duplicate_admin_cookies_are_rejected_across_all_header_lines() {
    let (app, _) = service();
    let credentials = serde_json::to_value(Credentials {
        username: "owner",
        password: PASSWORD,
    })
    .unwrap();
    let (status, headers, _) =
        request(&app, "POST", "/session", Some(ORIGIN), None, credentials).await;
    assert_eq!(status, StatusCode::OK);
    let issued = ::cookie::Cookie::parse(headers[header::SET_COOKIE].to_str().unwrap()).unwrap();
    assert_eq!(issued.name(), "__Host-oh_admin");
    assert_eq!(issued.value().len(), 64);
    assert_eq!(issued.path(), Some("/"));
    assert_eq!(issued.domain(), None);
    assert_eq!(issued.secure(), Some(true));
    assert_eq!(issued.http_only(), Some(true));
    assert_eq!(issued.same_site(), Some(::cookie::SameSite::Strict));
    assert_eq!(issued.max_age(), Some(::cookie::time::Duration::hours(8)));
    let valid = cookie(&headers);
    for (fields, expected) in [
        (vec!["unrelated=x", valid], StatusCode::OK),
        (vec![valid, valid], StatusCode::UNAUTHORIZED),
        (vec![valid, "__Host-oh_admin=bad"], StatusCode::UNAUTHORIZED),
        (vec!["__Host-oh_admin=bad", valid], StatusCode::UNAUTHORIZED),
        (vec![valid, "__Host-oh_admin"], StatusCode::UNAUTHORIZED),
        (
            vec![valid, "__Host-oh_admin = invalid"],
            StatusCode::UNAUTHORIZED,
        ),
        (vec![valid, "__Host-oh_admin="], StatusCode::UNAUTHORIZED),
        (vec![valid, "broken-unrelated"], StatusCode::OK),
    ] {
        let mut request = Request::get("/api/admin/v1/session");
        for field in &fields {
            request = request.header(header::COOKIE, *field);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    for (line, status) in [
        (&b"__Host-oh_admin=\xff"[..], StatusCode::UNAUTHORIZED),
        (&b"unrelated=\xff"[..], StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/admin/v1/session")
                    .header(header::COOKIE, valid)
                    .header(
                        header::COOKIE,
                        axum::http::HeaderValue::from_bytes(line).unwrap(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let duplicate = format!("{valid}; __Host-oh_admin=bad");
    assert_eq!(
        request(&app, "GET", "/session", None, Some(&duplicate), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn admin_auth_origin_rotation_players_and_termination() {
    let (app, platform) = service();
    let credentials = serde_json::to_value(Credentials {
        username: "owner",
        password: PASSWORD,
    })
    .unwrap();
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
    let (_, _, pools) = request(
        &app,
        "GET",
        "/federation/pools",
        None,
        Some(key),
        Value::Null,
    )
    .await;
    let pool = &pools["items"][0];
    assert_eq!(pool["poolId"], Uuid::nil().to_string());
    assert_eq!(pool["state"], "no_quorum");
    assert_eq!(pool["localRole"], "follower");
    assert_eq!(pool["voterCount"], 3);
    assert_eq!(pool["localTickets"], 2);
    for key in ["leaderId", "lastContactAt", "eligibleHosts"] {
        assert_eq!(pool.get(key), Some(&Value::Null));
    }

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
    assert!(detail["poolId"].is_null());
    assert!(detail["participants"][0]["homeInstanceId"].is_string());
    assert!(detail["participants"][1]["homeInstanceId"].is_null());
    let endpoint = format!("{endpoint}/termination");
    let reason = serde_json::to_value(OperatorReason {
        reason: "admin integration test",
    })
    .unwrap();
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
            serde_json::to_value(OperatorReason { reason: "changed" }).unwrap()
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
            serde_json::to_value(Credentials {
                username,
                password: "incorrect",
            })
            .unwrap(),
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
        serde_json::to_value(Credentials {
            username: "owner",
            password: PASSWORD,
        })
        .unwrap(),
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

#[derive(serde::Serialize)]
struct Credentials<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(serde::Serialize)]
struct OperatorReason<'a> {
    reason: &'a str,
}

async fn content_admin_cookie(app: &Router) -> String {
    let credentials = serde_json::to_value(Credentials {
        username: "owner",
        password: PASSWORD,
    })
    .unwrap();
    let (status, headers, _) =
        request(app, "POST", "/session", Some(ORIGIN), None, credentials).await;
    assert_eq!(status, StatusCode::OK);
    cookie(&headers).to_owned()
}

async fn content_get(app: &Router, path: &str, session: Option<&str>) -> (StatusCode, Value) {
    let (status, _, value) = request(app, "GET", path, None, session, Value::Null).await;
    (status, value)
}

#[tokio::test]
async fn content_routes_require_login_and_validate_filters_before_querying() {
    let (app, _) = service();
    for endpoint in ["/content", "/content/heroes", "/content/unknown?limit=99"] {
        assert_eq!(
            content_get(&app, endpoint, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let session = content_admin_cookie(&app).await;
    let session = Some(session.as_str());
    let (status, install) = content_get(&app, "/content", session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(install["installed"], false);
    assert!(install["rulesContentDigest"].is_null());
    assert!(install["referenceDigest"].is_null());
    assert!(install["collections"].as_array().unwrap().is_empty());
    assert_eq!(
        content_get(&app, "/content/heroes", session).await.0,
        StatusCode::NOT_FOUND
    );
    for path in [
        "/content/unknown",
        "/content/heroes?limit=0",
        "/content/heroes?limit=51",
        "/content/heroes?limit=bad",
        "/content/heroes?offset=-1",
        "/content/heroes?unknown=1",
        "/content/messages?language=123456789012345678901234567890123",
    ] {
        assert_eq!(
            content_get(&app, path, session).await.0,
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    let oversized_query = format!("/content/heroes?q={}", "a".repeat(129));
    assert_eq!(
        content_get(&app, &oversized_query, session).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
#[ignore = "requires HALCYON_DELIVERY and HALCYON_DATA_EXTRA operator fixtures"]
async fn content_catalog_queries_the_complete_operator_supplement() {
    let base = std::env::var_os("HALCYON_DELIVERY").unwrap();
    let extra = std::path::PathBuf::from(std::env::var_os("HALCYON_DATA_EXTRA").unwrap());
    let data = Arc::new(halcyon_data::GameData::load_with_extra(base, Some(&extra)).unwrap());
    let (app, _) = service_with_data(Some(data.clone()));
    let session = content_admin_cookie(&app).await;
    let session = Some(session.as_str());
    let (status, installed) = content_get(&app, "/content", session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["rulesContentDigest"], data.content_digest);
    assert_eq!(
        installed["referenceDigest"],
        data.references().unwrap().digest
    );
    let expected = [
        "heroes",
        "abilities",
        "units",
        "talents",
        "items",
        "stores",
        "skins",
        "themes",
        "skin-assets",
        "lobby-manifests",
        "levels",
        "camps",
        "meshes",
        "languages",
        "messages",
        "assets",
        "opcodes",
        "replay-entities",
        "stats",
        "session-constants",
    ];
    let collections = installed["collections"].as_array().unwrap();
    assert_eq!(collections.len(), expected.len());
    for path in expected {
        assert!(collections.iter().any(|entry| entry == path), "{path}");
        let (status, page) = content_get(&app, &format!("/content/{path}?limit=1"), session).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{path}");
        assert!(!items[0]["key"].as_str().unwrap().is_empty(), "{path}");
        assert!(!items[0]["data"].is_null(), "{path}");
    }

    let (_, first) = content_get(&app, "/content/levels?limit=2", session).await;
    let (_, second) = content_get(&app, "/content/levels?offset=2&limit=2", session).await;
    let (_, all_four) = content_get(&app, "/content/levels?limit=4", session).await;
    assert_eq!(first["nextOffset"], 2);
    assert_eq!(second["nextOffset"], 4);
    let combined: Vec<_> = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
        .collect();
    assert_eq!(
        combined,
        all_four["items"]
            .as_array()
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
    );
    let (_, empty) = content_get(&app, "/content/levels?offset=1000", session).await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    assert!(empty["nextOffset"].is_null());

    let (_, search) = content_get(&app, "/content/levels?q=sovereIGnsrise", session).await;
    let found = search["items"].as_array().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["key"], "SovereignsRiseLevel_Base");

    let key = "RETRIEVE_CREDENTIALS_DESCRIPTION";
    let (_, english) = content_get(
        &app,
        &format!("/content/messages?language=en&q={key}"),
        session,
    )
    .await;
    let (_, fallback) = content_get(
        &app,
        &format!("/content/messages?language=zh-CN&q={key}"),
        session,
    )
    .await;
    assert_eq!(fallback, english);
    assert_eq!(fallback["items"][0]["key"], key);
    assert_eq!(fallback["items"][0]["data"]["language"], "en");

    let (_, meshes) = content_get(&app, "/content/meshes?limit=50", session).await;
    let entries = meshes["items"].as_array().unwrap();
    assert_eq!(entries.len(), 16);
    assert!(meshes["nextOffset"].is_null());
    for row in entries {
        let mesh = row["data"].as_object().unwrap();
        assert_eq!(mesh["source_path"], row["key"]);
        assert_eq!(mesh["sha256"].as_str().unwrap().len(), 64);
        assert!(mesh["detour_header"]["tile_count"].as_u64().unwrap() > 0);
        assert!(mesh["size_bytes"].as_u64().unwrap() > 0);
        assert!(mesh.keys().all(|key| {
            [
                "source_path",
                "obb_md5",
                "file",
                "sha256",
                "detour_header",
                "size_bytes",
            ]
            .contains(&key.as_str())
        }));
    }
    let observed = data
        .references()
        .unwrap()
        .observations
        .values()
        .find(|row| !row.sample_hex.is_empty())
        .expect("fixture has original payload metadata");
    let (_, opcodes) = content_get(
        &app,
        &format!("/content/opcodes?q={}", observed.opcode_hex),
        session,
    )
    .await;
    let summary = &opcodes["items"][0];
    assert_eq!(summary["key"], observed.opcode_hex);
    assert_eq!(summary["data"]["opcode_dec"], observed.opcode_dec);
    assert!(
        !summary["data"]
            .as_object()
            .unwrap()
            .contains_key("sample_hex")
    );
}
