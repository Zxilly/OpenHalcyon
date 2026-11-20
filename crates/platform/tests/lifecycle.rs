use halcyon_platform::{Chat, CreateGuest, Queue, ResumeGuest};
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
use serde_json::Value;
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
        serde_json::to_value(CreateGuest {
            name: "First".to_string(),
        })
        .unwrap(),
    )
    .await;
    let (_, second) = request(
        &app,
        "POST",
        "/api/v1/guests",
        None,
        serde_json::to_value(CreateGuest {
            name: "Second".to_string(),
        })
        .unwrap(),
    )
    .await;
    let (_, resumed) = request(
        &app,
        "POST",
        "/api/v1/sessions",
        None,
        serde_json::to_value(ResumeGuest {
            recovery_token: first["recoveryToken"].as_str().unwrap().to_owned(),
        })
        .unwrap(),
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
            request(
                &app,
                "POST",
                "/api/v1/queue",
                key,
                serde_json::to_value(Queue {
                    mode: "duo".to_string(),
                    difficulty: None
                })
                .unwrap()
            )
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
        .finish_match(
            match_id,
            serde_json::to_value(Winner {
                winner: TestTeam::Blue,
            })
            .unwrap(),
        )
        .unwrap();
    state
        .finish_match(
            match_id,
            serde_json::to_value(Winner {
                winner: TestTeam::Blue,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        state.finish_match(
            match_id,
            serde_json::to_value(Winner {
                winner: TestTeam::Red
            })
            .unwrap()
        ),
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

    let state = AppState::open(config.clone(), host.clone()).unwrap();
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
            Some(
                serde_json::to_value(Winner {
                    winner: TestTeam::Blue,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let settled = state
        .profile(key)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .settled_at_ms
        .unwrap();
    let reopened = AppState::open(config.clone(), host.clone()).unwrap();
    reopened
        .federated_project(
            intent.ticket,
            "finished",
            Some(match_id),
            None,
            Some(
                serde_json::to_value(Winner {
                    winner: TestTeam::Blue,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let lobby = reopened.profile(key).await.unwrap().lobby.unwrap();
    assert_eq!(lobby.settled_at_ms, Some(settled));
    assert_eq!(lobby.match_id, Some(match_id));
    assert_eq!(
        lobby.result,
        Some(
            serde_json::to_value(Winner {
                winner: TestTeam::Blue
            })
            .unwrap()
        )
    );
    state.federated_release(intent.ticket).await.unwrap();
    assert!(state.federated_intent(key).await.unwrap().is_none());
    assert!(state.profile(key).await.unwrap().lobby.is_none());
    state.join(key, "duo".into()).await.unwrap();
}

async fn party(
    community: &halcyon_community::Community,
    state: &AppState,
) -> Vec<halcyon_platform::Guest> {
    use halcyon_community::{Action, GroupKind, GroupSettings};
    let mut guests = Vec::new();
    for name in ["Captain", "Wayfinder"] {
        let guest = state.create_guest(name.into()).await.unwrap();
        community
            .observe_player(guest.player_id, name.into(), true)
            .await
            .unwrap();
        guests.push(guest);
    }
    let party = community
        .execute(
            guests[0].player_id,
            Action::CreateGroup {
                kind: GroupKind::Party,
                name: "Party".into(),
                settings: GroupSettings::default(),
            },
        )
        .await
        .unwrap();
    let group = Uuid::parse_str(party["id"].as_str().unwrap()).unwrap();
    community
        .execute(
            guests[0].player_id,
            Action::Invite {
                kind: GroupKind::Party,
                target: guests[1].player_id,
            },
        )
        .await
        .unwrap();
    community
        .execute(
            guests[1].player_id,
            Action::ResolveInvite {
                group,
                accept: true,
            },
        )
        .await
        .unwrap();
    guests
}

#[tokio::test]
async fn local_party_claim_is_atomic_and_every_player_confirms() {
    use halcyon_community::{Action, Community, CommunityError, GroupKind};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("party.sqlite");
    let state = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        },
        Arc::new(Host::default()),
    )
    .unwrap();
    let community = Community::open(&path).unwrap();
    let players = party(&community, &state).await;
    let captain = &players[0].session_token;
    let member = &players[1].session_token;
    assert!(matches!(
        state.join_party_or_solo(member, "duo".into(), 2).await,
        Err(PlatformError::Conflict(_))
    ));

    state
        .join_with_seats(member, "independent".into(), 3)
        .await
        .unwrap();
    assert!(matches!(
        state.join_party_or_solo(captain, "duo".into(), 2).await,
        Err(PlatformError::Conflict(_))
    ));
    assert!(state.profile(captain).await.unwrap().lobby.is_none());
    state.leave(member).await.unwrap();

    state
        .join_party_or_solo(captain, "duo".into(), 3)
        .await
        .unwrap();
    assert_eq!(
        state.profile(member).await.unwrap().lobby.unwrap().phase,
        "queued"
    );
    assert!(matches!(
        community
            .execute(
                players[1].player_id,
                Action::LeaveGroup {
                    kind: GroupKind::Party
                }
            )
            .await,
        Err(CommunityError::Conflict(_)),
    ));
    state.leave(member).await.unwrap();
    assert!(state.profile(captain).await.unwrap().lobby.is_none());
    assert!(state.profile(member).await.unwrap().lobby.is_none());

    state
        .join_party_or_solo(captain, "duo".into(), 2)
        .await
        .unwrap();
    let game = state
        .profile(captain)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .match_id
        .unwrap();
    assert_eq!(
        state.profile(member).await.unwrap().lobby.unwrap().match_id,
        Some(game)
    );
    state.accept(captain, game).await.unwrap();
    assert_eq!(
        state.profile(captain).await.unwrap().lobby.unwrap().phase,
        "offered"
    );
    state.accept(member, game).await.unwrap();
    assert_eq!(
        state.profile(captain).await.unwrap().lobby.unwrap().phase,
        "playing"
    );
    state
        .finish_match(
            game,
            serde_json::to_value(Winner {
                winner: TestTeam::Blue,
            })
            .unwrap(),
        )
        .unwrap();
    state.leave(captain).await.unwrap();
    assert!(state.profile(captain).await.unwrap().lobby.is_none());
    assert_eq!(
        state.profile(member).await.unwrap().lobby.unwrap().phase,
        "finished"
    );
    community
        .execute(
            players[1].player_id,
            Action::LeaveGroup {
                kind: GroupKind::Party,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn federated_party_keeps_roster_and_acknowledges_each_player() {
    use halcyon_community::Community;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("federated.sqlite");
    let config = PlatformConfig {
        database_path: path.clone(),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let host = Arc::new(Host::default());
    let state = AppState::open(config.clone(), host.clone()).unwrap();
    let community = Community::open(&path).unwrap();
    let players = party(&community, &state).await;
    let captain = &players[0].session_token;
    let member = &players[1].session_token;
    let pool = Uuid::new_v4();
    let solo = state
        .federated_join(captain, pool, "duo".into(), 3)
        .await
        .unwrap();
    assert!(matches!(
        state
            .federated_join_party_or_solo(captain, pool, "duo".into(), 3)
            .await,
        Err(PlatformError::Conflict(_)),
    ));
    state
        .federated_project(solo.ticket, "closed", None, None, None)
        .await
        .unwrap();
    state.federated_release(solo.ticket).await.unwrap();
    let intent = state
        .federated_join_party_or_solo(captain, pool, "duo".into(), 3)
        .await
        .unwrap();
    assert_eq!(intent.players, [players[0].player_id, players[1].player_id]);
    assert!(matches!(
        state.join(member, "duo".into()).await,
        Err(PlatformError::Conflict(_))
    ));
    let id = Uuid::new_v4();
    state
        .federated_project(intent.ticket, "offered", Some(id), None, None)
        .await
        .unwrap();
    assert!(matches!(
        state
            .federated_accept_in_window(captain, intent.ticket, id, 0)
            .await,
        Err(PlatformError::Conflict(_))
    ));
    assert!(
        state
            .federated_offer_binding(captain, pool)
            .await
            .unwrap()
            .unwrap()
            .accepted
            .is_empty()
    );
    let first = state
        .federated_accept_in_window(captain, intent.ticket, id, i64::MAX)
        .await
        .unwrap();
    assert!(first.accepted_match.is_none());
    drop(state);

    let state = AppState::open(config, host).unwrap();
    assert_eq!(
        state
            .federated_intent(member)
            .await
            .unwrap()
            .unwrap()
            .match_id,
        Some(id)
    );
    let confirmed = state
        .federated_action_as(member, intent.ticket, Some(id), false)
        .await
        .unwrap();
    assert_eq!(confirmed.accepted_match, Some(id));
    state
        .federated_project(
            intent.ticket,
            "playing",
            Some(id),
            Some("localhost:9001".into()),
            None,
        )
        .await
        .unwrap();
    for token in [captain, member] {
        let lobby = state.profile(token).await.unwrap().lobby.unwrap();
        assert_eq!(lobby.phase, "playing");
        assert_eq!(lobby.match_id, Some(id));
    }
    state
        .federated_project(
            intent.ticket,
            "finished",
            Some(id),
            None,
            Some(
                serde_json::to_value(Winner {
                    winner: TestTeam::Blue,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    state
        .federated_acknowledge(captain, intent.ticket)
        .await
        .unwrap();
    assert!(state.profile(captain).await.unwrap().lobby.is_none());
    assert_eq!(
        state.profile(member).await.unwrap().lobby.unwrap().phase,
        "finished"
    );
    let new = state
        .federated_join(captain, pool, "duo".into(), 3)
        .await
        .unwrap();
    assert_ne!(new.ticket, intent.ticket);
    assert_eq!(state.federated_intents(pool).await.unwrap().len(), 2);
    let pending = state.pending_federated_intents(pool).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].ticket, new.ticket);
    state
        .federated_acknowledge(member, intent.ticket)
        .await
        .unwrap();
    assert_eq!(state.federated_intents(pool).await.unwrap().len(), 1);
}

#[test]
fn team_layout_keeps_parties_together_when_possible() {
    let teams = halcyon_platform::plan_teams(&[2, 1]);
    assert_eq!(teams.len(), 3);
    assert_eq!(teams[0], teams[1]);
    assert_ne!(teams[0], teams[2]);
    let crowded = halcyon_platform::plan_teams(&[4, 4, 2]);
    assert_eq!(crowded.iter().filter(|team| **team == 0).count(), 5);
    assert_eq!(crowded.iter().filter(|team| **team == 1).count(), 5);
    assert_eq!(
        crowded
            .chunks(4)
            .filter(|party| party.iter().any(|team| *team != party[0]))
            .count(),
        1
    );
    let aral = halcyon_platform::plan_teams_for_mode("casual_aral", &[2, 2, 2]);
    assert_eq!(aral.iter().filter(|team| **team == 0).count(), 3);
    assert_eq!(aral.iter().filter(|team| **team == 1).count(), 3);
    assert!(halcyon_platform::plan_teams(&[1, 1]).is_empty());
}

#[tokio::test]
async fn legacy_single_player_federation_ticket_migrates_without_reissuing() {
    let directory = tempfile::tempdir().unwrap();
    let config = PlatformConfig {
        database_path: directory.path().join("legacy.sqlite"),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let host = Arc::new(Host::default());
    let state = AppState::open(config.clone(), host.clone()).unwrap();
    let guest = state.create_guest("Existing".into()).await.unwrap();
    drop(state);
    let ticket = Uuid::new_v4();
    let pool = Uuid::new_v4();
    let game = Uuid::new_v4();
    {
        let db = rusqlite::Connection::open(&config.database_path).unwrap();
        db.execute_batch(
            "DROP TRIGGER native_offer_assignment;
             DROP TRIGGER native_offer_accept;
             DROP TABLE native_offer_seats;
             DROP TRIGGER native_local_settlement;
             DROP TABLE native_settlements;
             DROP TABLE federated_queue_members;
             DROP TABLE federated_queue;
             ALTER TABLE tickets DROP COLUMN seat_limit;
             ALTER TABLE tickets DROP COLUMN difficulty;
             ALTER TABLE matches DROP COLUMN difficulty;
             ALTER TABLE tickets DROP COLUMN batch;
             CREATE TABLE federated_intents(
                ticket TEXT PRIMARY KEY, player TEXT NOT NULL UNIQUE, pool TEXT NOT NULL,
                mode TEXT NOT NULL, seats INTEGER NOT NULL, accepted_match TEXT,
                cancel INTEGER NOT NULL, phase TEXT NOT NULL, match_id TEXT, address TEXT, result TEXT
             );",
        ).unwrap();
        db.execute(
            "INSERT INTO federated_intents VALUES(?,?,?,'duo',2,?,0,'offered',?,NULL,NULL)",
            rusqlite::params![
                ticket.to_string(),
                guest.player_id.to_string(),
                pool.to_string(),
                game.to_string(),
                game.to_string()
            ],
        )
        .unwrap();
    }
    let reopened = AppState::open(config.clone(), host.clone()).unwrap();
    let intent = reopened
        .federated_intent(&guest.session_token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.ticket, ticket);
    assert_eq!(intent.players, [guest.player_id]);
    assert_eq!(intent.accepted_match, Some(game));
    assert_eq!(
        reopened
            .profile(&guest.session_token)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .match_id,
        Some(game)
    );
    reopened
        .federated_project(ticket, "closed", None, None, None)
        .await
        .unwrap();
    reopened.federated_release(ticket).await.unwrap();
    drop(reopened);
    let reopened = AppState::open(config, host).unwrap();
    assert!(
        reopened
            .federated_intent(&guest.session_token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rest_difficulty_keeps_queue_variants_separate_and_survives_reopen() {
    use halcyon_platform::BotDifficulty;
    #[derive(Default)]
    struct DifficultyHost(std::sync::Mutex<Vec<Reservation>>);
    impl MatchHost for DifficultyHost {
        fn reserve(&self, reservation: Reservation) -> Result<HostedMatch, PlatformError> {
            self.0.lock().unwrap().push(reservation);
            Ok(HostedMatch {
                address: "localhost:8001".into(),
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let config = PlatformConfig {
        database_path: temp.path().join("difficulty.sqlite"),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let host = Arc::new(DifficultyHost::default());
    let state = AppState::open(config.clone(), host.clone()).unwrap();
    let a = state.create_guest("A".into()).await.unwrap();
    let b = state.create_guest("B".into()).await.unwrap();
    let c = state.create_guest("C".into()).await.unwrap();
    let app = router(state.clone());
    for (guest, difficulty) in [(&a, "easy"), (&b, "very_hard")] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/v1/queue",
                Some(&guest.session_token),
                serde_json::to_value(Queue {
                    mode: "5v5_bots_coop".to_string(),
                    difficulty: Some(
                        halcyon_platform::BotDifficulty::from_wire(difficulty).unwrap()
                    )
                })
                .unwrap()
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert!(
            state
                .profile(&guest.session_token)
                .await
                .unwrap()
                .lobby
                .unwrap()
                .match_id
                .is_none()
        );
    }
    drop(app);
    drop(state);
    let state = AppState::open(config, host.clone()).unwrap();
    let app = router(state.clone());
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/queue",
            Some(&c.session_token),
            serde_json::to_value(Queue {
                mode: "5v5_bots_coop".to_string(),
                difficulty: Some(halcyon_platform::BotDifficulty::Easy)
            })
            .unwrap()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let room = state
        .profile(&a.session_token)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .match_id
        .unwrap();
    assert!(
        state
            .profile(&b.session_token)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .match_id
            .is_none()
    );
    for guest in [&a, &c] {
        state.accept(&guest.session_token, room).await.unwrap();
    }
    let reservations = host.0.lock().unwrap();
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0].difficulty, Some(BotDifficulty::Easy));
    assert_eq!(reservations[0].mode, "5v5_bots_coop");
}

#[tokio::test]
async fn public_match_routes_dispatch_to_the_selected_home_authority() {
    use halcyon_platform::{BotDifficulty, PublicMatches, Result, router_with_matches};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Authority(Mutex<Vec<&'static str>>);
    const TEST_BEARER: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[async_trait::async_trait]
    impl PublicMatches for Authority {
        async fn enqueue(
            &self,
            token: &str,
            mode: String,
            difficulty: Option<BotDifficulty>,
        ) -> Result<()> {
            assert_eq!(token, TEST_BEARER);
            assert_eq!(mode, "5v5_bots_solo");
            assert!(difficulty.is_some());
            self.0.lock().unwrap().push("join");
            Ok(())
        }
        async fn cancel_queue(&self, token: &str) -> Result<()> {
            assert_eq!(token, TEST_BEARER);
            self.0.lock().unwrap().push("cancel");
            Ok(())
        }
        async fn accept_offer(&self, token: &str, id: Uuid) -> Result<()> {
            assert_eq!(token, TEST_BEARER);
            assert_eq!(id, Uuid::nil());
            self.0.lock().unwrap().push("accept");
            Err(PlatformError::Conflict("offer expired"))
        }
        async fn match_chat(&self, token: &str, id: Uuid, text: String) -> Result<()> {
            assert_eq!(token, TEST_BEARER);
            assert_eq!(id, Uuid::nil());
            assert_eq!(text, "hello");
            self.0.lock().unwrap().push("chat");
            Ok(())
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(
        PlatformConfig {
            database_path: dir.path().join("platform.sqlite"),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        },
        Arc::new(Host::default()),
    )
    .unwrap();
    let authority = Arc::new(Authority::default());
    let app = router_with_matches(state, authority.clone());
    let room = format!("/api/v1/matches/{}", Uuid::nil());
    let token = Some(TEST_BEARER);
    let join = serde_json::to_value(Queue {
        mode: "5v5_bots_solo".to_string(),
        difficulty: Some(halcyon_platform::BotDifficulty::Hard),
    })
    .unwrap();
    assert_eq!(
        request(&app, "POST", "/api/v1/queue", None, join.clone())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "POST", "/api/v1/queue", token, join).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "POST", &format!("{room}/accept"), token, Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{room}/chat"),
            token,
            serde_json::to_value(Chat {
                text: "hello".to_string()
            })
            .unwrap()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "DELETE", "/api/v1/queue", token, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        *authority.0.lock().unwrap(),
        ["join", "accept", "chat", "cancel"]
    );
}

#[tokio::test]
async fn legacy_waiting_tickets_gain_one_persistent_timestamp_on_migration() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("queue.sqlite");
    let cfg = PlatformConfig {
        database_path: db.clone(),
        public_address: "localhost:8000".into(),
        seats_per_match: 2,
    };
    let open = || AppState::open(cfg.clone(), Arc::new(Host::default())).unwrap();
    let state = open();
    let local = state
        .create_guest("Local".into())
        .await
        .unwrap()
        .session_token;
    let peer = state
        .create_guest("Remote".into())
        .await
        .unwrap()
        .session_token;
    let pool = Uuid::new_v4();
    state.join(&local, "duo".into()).await.unwrap();
    let ticket = state
        .federated_join(&peer, pool, "duo".into(), 2)
        .await
        .unwrap()
        .ticket;
    let before = state
        .profile(&peer)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .queued_at_ms
        .unwrap();
    state
        .federated_join(&peer, pool, "duo".into(), 2)
        .await
        .unwrap();
    state
        .federated_project(ticket, "queued", None, None, None)
        .await
        .unwrap();
    assert_eq!(
        state
            .profile(&peer)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .queued_at_ms,
        Some(before)
    );
    drop(state);
    // Recreate the previous queue schemas while retaining their live ticket rows.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "ALTER TABLE tickets DROP COLUMN queued_at_ms;
         ALTER TABLE federated_queue DROP COLUMN queued_at_ms;",
        )
        .unwrap();
    let state = open();
    let local_stamp = state
        .profile(&local)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .queued_at_ms
        .unwrap();
    let peer_stamp = state
        .profile(&peer)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .queued_at_ms
        .unwrap();
    drop(state);
    let state = open();
    for (key, stamp) in [(local, local_stamp), (peer, peer_stamp)] {
        for _ in 0..2 {
            let lobby = state.profile(&key).await.unwrap().lobby.unwrap();
            assert_eq!(lobby.phase, "queued");
            assert_eq!(lobby.queued_at_ms, Some(stamp));
        }
    }
}

#[derive(serde::Serialize)]
struct Winner {
    winner: TestTeam,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum TestTeam {
    Blue,
    Red,
}

#[test]
fn rest_request_wire_forms_preserve_spelling_and_optional_difficulty() {
    let resume = ResumeGuest {
        recovery_token: "example".into(),
    };
    assert_eq!(
        serde_json::to_string(&resume).unwrap(),
        r#"{"recoveryToken":"example"}"#
    );
    let queue = Queue {
        mode: "duo".into(),
        difficulty: None,
    };
    assert_eq!(serde_json::to_string(&queue).unwrap(), r#"{"mode":"duo"}"#);
    for raw in [r#"{"mode":"duo"}"#, r#"{"mode":"duo","difficulty":null}"#] {
        assert!(
            serde_json::from_str::<Queue>(raw)
                .unwrap()
                .difficulty
                .is_none()
        );
    }
}
#[tokio::test]
async fn unrecognized_federated_phase_roundtrips_without_authorizing_seat_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("platform.sqlite");
    let state = AppState::open(
        PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        },
        Arc::new(Host::default()),
    )
    .unwrap();
    assert_eq!(state.seats_for_mode("community_example"), 2);
    let guest = state.create_guest("Local".into()).await.unwrap();
    let key = &guest.session_token;
    let intent = state
        .federated_join(key, Uuid::new_v4(), "community_example".into(), 2)
        .await
        .unwrap();
    assert_eq!(
        intent.phase_kind(),
        Some(halcyon_platform::LobbyPhase::Pending)
    );
    let room = Uuid::new_v4();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE federated_queue SET phase=?,match_id=? WHERE ticket=?",
        [
            "future_phase",
            &room.to_string(),
            &intent.ticket.to_string(),
        ],
    )
    .unwrap();
    let saved = state.federated_intent(key).await.unwrap().unwrap();
    assert_eq!(saved.phase, "future_phase");
    assert_eq!(saved.phase_kind(), None);
    assert_eq!(
        state
            .profile(key)
            .await
            .unwrap()
            .lobby
            .unwrap()
            .phase_kind(),
        None
    );
    for (accept, cancel) in [(Some(room), false), (None, true)] {
        assert!(matches!(
            state.federated_action(intent.ticket, accept, cancel).await,
            Err(PlatformError::Conflict(_))
        ));
    }
    let after = state.federated_intent(key).await.unwrap().unwrap();
    assert_eq!(after.phase, saved.phase);
    assert!(!after.cancel);
    assert!(after.accepted_match.is_none());
}

#[tokio::test]
async fn notification_reader_reopens_legacy_events_and_authenticates_in_memory() {
    let directory = tempfile::tempdir().unwrap();
    for path in [":memory:".into(), directory.path().join("old.sqlite")] {
        let config = PlatformConfig {
            database_path: path.clone(),
            public_address: "localhost:8000".into(),
            seats_per_match: 2,
        };
        let mut state = AppState::open(config.clone(), Arc::new(Host::default())).unwrap();
        let one = state.create_guest("One".into()).await.unwrap();
        let other = state.create_guest("Other".into()).await.unwrap();
        state
            .refresh_friends(vec![one.player_id, other.player_id])
            .await
            .unwrap();
        state.refresh_friends(vec![one.player_id]).await.unwrap();

        if path != std::path::Path::new(":memory:") {
            // Older DBs had only the common events table. Opening again backfills
            // recipient-local ordinals without rewriting their event bodies.
            drop(state);
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("DROP TRIGGER event_notifications; DROP TABLE notification_events;")
                .unwrap();
            drop(db);
            state = AppState::open(config, Arc::new(Host::default())).unwrap();
        }

        for (after, initial, expected) in [(0, false, 1), (0, true, 2), (500, false, 2)] {
            let item = state
                .next_notification(&one.session_token, after, initial)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(item.cursor, expected);
            assert_eq!(
                item.kind(),
                Some(halcyon_platform::EventKind::FriendsChanged)
            );
        }
        let item = state
            .next_notification(&other.session_token, 0, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(item.cursor, 1);

        let restored = state
            .resume_guest(one.recovery_token.unwrap())
            .await
            .unwrap();
        assert!(matches!(
            state.next_notification(&one.session_token, 0, true).await,
            Err(PlatformError::Unauthorized)
        ));
        assert_eq!(
            state
                .next_notification(&restored.session_token, 0, true)
                .await
                .unwrap()
                .unwrap()
                .cursor,
            2
        );
    }
}
