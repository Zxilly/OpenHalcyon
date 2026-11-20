use std::sync::Arc;

use halcyon_platform::{
    AppState, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
};
use rusqlite::{Connection, params};
use serde_json::Value;
use uuid::Uuid;

use crate::meta::Meta;

struct Host;
impl MatchHost for Host {
    fn reserve(&self, _: Reservation) -> Result<HostedMatch, PlatformError> {
        Err(PlatformError::HostUnavailable)
    }
}

fn result(db: &Connection, player: Uuid, room: Uuid, outcome: Value) {
    db.execute(
        "INSERT INTO events(player,body) VALUES(?,?)",
        params![
            player.to_string(),
            serde_json::to_value(MatchEvent {
                r#type: "match_finished",
                match_id: room,
                result: &outcome
            })
            .unwrap()
            .to_string()
        ],
    )
    .unwrap();
}

#[tokio::test]
async fn legacy_projections_and_incremental_match_statistics() {
    for with_day in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let app = AppState::open(
            PlatformConfig {
                database_path: path.clone(),
                public_address: "localhost:8443".into(),
                seats_per_match: 2,
            },
            Arc::new(Host),
        )
        .unwrap();
        let guest = app.create_guest("Guest".into()).await.unwrap();
        let player = guest.player_id;
        let community = halcyon_community::Community::open(&path).unwrap();
        community
            .observe_player(player, "Guest".into(), true)
            .await
            .unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!(
            "CREATE TABLE ce_completed(player TEXT,room TEXT,won INTEGER NOT NULL,hero TEXT{},
                PRIMARY KEY(player,room));
             INSERT INTO community_quests(id,category,title,goal,skippable) VALUES
                ('complete','complete_match','Complete',20,0),('win','win_match','Win',20,0),
                ('active_complete','complete_match','Active complete',20,0);",
            if with_day {
                ",mode TEXT,day INTEGER"
            } else {
                ""
            },
        ))
        .unwrap();
        db.execute(
            "INSERT INTO community_quest_states(player,quest,progress,state) VALUES(?,'complete',3,'claimed')",
            [player.to_string()],
        ).unwrap();
        let legacy = |outcome: Option<Value>, won: bool, mode: &str, day: &str| {
            let room = Uuid::new_v4();
            db.execute(
                "INSERT INTO matches(id,mode,phase) VALUES(?,?,'finished')",
                params![room.to_string(), mode],
            )
            .unwrap();
            if let Some(outcome) = outcome {
                result(&db, player, room, outcome);
            }
            let statement = if with_day {
                format!("INSERT INTO ce_completed VALUES(?,?,?,'Ardan',?,{day})")
            } else {
                "INSERT INTO ce_completed VALUES(?,?,?,'Ardan')".into()
            };
            if with_day {
                db.execute(
                    &statement,
                    params![player.to_string(), room.to_string(), won, mode],
                )
                .unwrap();
            } else {
                db.execute(
                    &statement,
                    params![player.to_string(), room.to_string(), won],
                )
                .unwrap();
            }
            room
        };
        let preserved = legacy(None, true, "tutorial", "unixepoch()/86400-1");
        legacy(
            Some(
                serde_json::to_value(MatchRoster {
                    winner: 0,
                    players: std::collections::BTreeMap::from([(player, Team { team: 0 })]),
                })
                .unwrap(),
            ),
            false,
            "blitz_rounds_casual",
            "unixepoch()/86400",
        );
        let aborted = legacy(
            Some(
                serde_json::to_value(OperatorReason {
                    reason: "admin_terminated",
                })
                .unwrap(),
            ),
            false,
            "casual_5v5",
            "unixepoch()/86400",
        );
        legacy(
            Some(serde_json::to_value(Winner { winner: 0 }).unwrap()),
            false,
            "aral_casual",
            "NULL",
        );
        let meta = Meta::open(&path).unwrap();
        let migrated = wire_activity(&meta, player).await;
        assert_eq!(migrated["completed"], 3);
        assert_eq!(migrated["wins"], 2);
        assert_eq!(migrated["completed_non_tutorial"], 2);
        assert_eq!(migrated["winsToday"], i32::from(with_day));
        assert_eq!(migrated["wins_blitz"], 1);
        assert_eq!(migrated["wins_blitz_rounds_casual"], 1);
        assert_eq!(migrated["wins_casual"], 1);
        assert_eq!(migrated["wins_aral"], 0);
        assert!(migrated.get("lossStreak").is_none());
        assert!(migrated.get("winStreak").is_none());
        assert_eq!(
            meta.hero_stats(player).await.unwrap(),
            serde_json::to_value([HeroStats {
                hero: "Ardan",
                completed: 3,
                wins: 2
            }])
            .unwrap()
        );
        let leader = &meta.ranking().await.unwrap()["leaderboards"][0]["entries"][0];
        assert_eq!(leader["completed"], 3);
        assert_eq!(leader["wins"], 2);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM ce_completed WHERE room=?",
                [aborted.to_string()],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT day=unixepoch()/86400-1 FROM ce_completed WHERE room=?",
                [preserved.to_string()],
                |row| row.get::<_, Option<bool>>(0),
            )
            .unwrap(),
            with_day.then_some(true)
        );

        let settle = |mode: &str, victory: Value| {
            let room = Uuid::new_v4();
            db.execute(
                "INSERT INTO matches(id,mode,phase) VALUES(?,?,'finished')",
                params![room.to_string(), mode],
            )
            .unwrap();
            for _ in 0..2 {
                result(
                    &db,
                    player,
                    room,
                    serde_json::to_value(HeroOutcome {
                        reason: "completed",
                        players: std::collections::BTreeMap::from([(
                            player,
                            HeroVictory {
                                victory: &victory,
                                hero: "Catherine",
                            },
                        )]),
                    })
                    .unwrap(),
                );
            }
        };
        settle("aral_casual", Value::Bool(false));
        settle("casual_5v5", Value::Bool(false));
        assert_eq!(wire_activity(&meta, player).await["lossStreak"], 2);
        settle("casual_5v5", Value::Null);
        assert!(
            wire_activity(&meta, player)
                .await
                .get("lossStreak")
                .is_none()
        );
        settle("ranked_5v5", Value::Bool(true));
        settle("aral_casual", Value::Bool(true));
        result(
            &db,
            player,
            aborted,
            serde_json::to_value(MatchOutcome {
                winner: 0,
                reason: "admin_terminated",
            })
            .unwrap(),
        );

        let final_stats = wire_activity(&meta, player).await;
        assert_eq!(final_stats["completed"], 8);
        assert_eq!(final_stats["wins"], 4);
        assert_eq!(final_stats["winsToday"], 2 + i32::from(with_day));
        assert_eq!(final_stats["wins_aral"], 1);
        assert_eq!(final_stats["wins_ranked"], 1);
        assert_eq!(final_stats["wins_casual"], 2);
        assert_eq!(final_stats["winStreak"], 2);
        assert_eq!(
            meta.hero_stats(player).await.unwrap(),
            serde_json::to_value([
                HeroStats {
                    hero: "Ardan",
                    completed: 3,
                    wins: 2
                },
                HeroStats {
                    hero: "Catherine",
                    completed: 5,
                    wins: 2
                }
            ])
            .unwrap()
        );
        let quest_progress = |quest: &str| {
            db.query_row(
                "SELECT progress,state FROM community_quest_states WHERE player=? AND quest=?",
                params![player.to_string(), quest],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap()
        };
        assert_eq!(quest_progress("complete"), (3, "claimed".into()));
        assert_eq!(quest_progress("win"), (2, "active".into()));
        assert_eq!(quest_progress("active_complete"), (5, "active".into()));
        drop(meta);
        let reopened = Meta::open(&path).unwrap();
        assert_eq!(wire_activity(&reopened, player).await, final_stats);
        assert_eq!(quest_progress("win"), (2, "active".into()));
    }
}

#[derive(serde::Serialize)]
struct MatchOutcome<'a> {
    winner: u8,
    reason: &'a str,
}

#[derive(serde::Serialize)]
struct HeroOutcome<'a> {
    reason: &'static str,
    players: std::collections::BTreeMap<Uuid, HeroVictory<'a>>,
}

#[derive(serde::Serialize)]
struct Winner {
    winner: u8,
}

#[derive(serde::Serialize)]
struct OperatorReason<'a> {
    reason: &'a str,
}

#[derive(serde::Serialize)]
struct MatchRoster {
    winner: u32,
    players: std::collections::BTreeMap<Uuid, Team>,
}

#[derive(serde::Serialize)]
struct MatchEvent<'a> {
    r#type: &'static str,
    #[serde(rename = "matchId")]
    match_id: Uuid,
    result: &'a Value,
}

#[derive(serde::Serialize)]
struct Team {
    team: u8,
}
#[derive(serde::Serialize)]
struct HeroVictory<'a> {
    hero: &'a str,
    victory: &'a Value,
}
#[derive(serde::Serialize)]
struct HeroStats<'a> {
    hero: &'a str,
    completed: u32,
    wins: u32,
}

async fn wire_activity(meta: &Meta, player: Uuid) -> Value {
    serde_json::to_value(meta.player_fields(player).await.unwrap()).unwrap()
}

struct ReserveHost;
impl MatchHost for ReserveHost {
    fn reserve(&self, _: Reservation) -> Result<HostedMatch, PlatformError> {
        Ok(HostedMatch {
            address: "localhost:8001".into(),
        })
    }
}

#[tokio::test]
async fn diesel_settlement_and_federated_projection_drive_match_stats_and_quests() {
    let temp = tempfile::tempdir().unwrap();
    let path_a = temp.path().join("home-a.sqlite");
    let path_b = temp.path().join("home-b.sqlite");
    let cfg_a = PlatformConfig {
        database_path: path_a.clone(),
        public_address: "localhost:8443".into(),
        seats_per_match: 2,
    };
    let cfg_b = PlatformConfig {
        database_path: path_b.clone(),
        public_address: "localhost:8444".into(),
        seats_per_match: 2,
    };

    let app_a = AppState::open(cfg_a.clone(), Arc::new(ReserveHost)).unwrap();
    let app_b = AppState::open(cfg_b.clone(), Arc::new(ReserveHost)).unwrap();
    let comm_a = halcyon_community::Community::open(&path_a).unwrap();
    let comm_b = halcyon_community::Community::open(&path_b).unwrap();
    let meta_a = Meta::open(&path_a).unwrap();
    let meta_b = Meta::open(&path_b).unwrap();

    let player_a = app_a.create_guest("Alpha".into()).await.unwrap();
    let opponent_a = app_a.create_guest("Bravo".into()).await.unwrap();
    let mut aral_fillers = Vec::with_capacity(4);
    for idx in 0..4 {
        aral_fillers.push(app_a.create_guest(format!("Filler{idx}")).await.unwrap());
    }
    let player_b = app_b.create_guest("Charlie".into()).await.unwrap();

    for (comm, guest, name) in [
        (&comm_a, &player_a, "Alpha"),
        (&comm_a, &opponent_a, "Bravo"),
        (&comm_b, &player_b, "Charlie"),
    ] {
        comm.observe_player(guest.player_id, name.into(), true)
            .await
            .unwrap();
    }
    let db_a = Connection::open(&path_a).unwrap();
    let db_b = Connection::open(&path_b).unwrap();
    for (db, player) in [(&db_a, player_a.player_id), (&db_b, player_b.player_id)] {
        db.execute_batch(
            "INSERT INTO community_quests(id,category,title,goal,skippable) VALUES
                ('complete','complete_match','Complete',10,0),
                ('win','win_match','Win',10,0);",
        )
        .unwrap();
        for quest in ["complete", "win"] {
            db.execute(
                "INSERT INTO community_quest_states(player,quest,progress,state) VALUES(?,?,0,'active')",
                params![player.to_string(), quest],
            )
            .unwrap();
        }
    }

    // 1. Local match settlement via Diesel finish_match, including rollback & retry & duplicate checks.
    let mut aral_guests = vec![&player_a, &opponent_a];
    aral_guests.extend(aral_fillers.iter());
    for guest in &aral_guests {
        app_a
            .join(&guest.session_token, "aral_casual".into())
            .await
            .unwrap();
    }
    let local_room = app_a
        .profile(&player_a.session_token)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .match_id
        .unwrap();
    for guest in &aral_guests {
        app_a
            .accept(&guest.session_token, local_room)
            .await
            .unwrap();
    }

    let mut local_players = std::collections::BTreeMap::from([
        (
            player_a.player_id,
            HeroVictory {
                hero: "Ringo",
                victory: &Value::Bool(true),
            },
        ),
        (
            opponent_a.player_id,
            HeroVictory {
                hero: "Catherine",
                victory: &Value::Bool(false),
            },
        ),
    ]);
    for guest in &aral_fillers {
        local_players.insert(
            guest.player_id,
            HeroVictory {
                hero: "Ringo",
                victory: &Value::Bool(false),
            },
        );
    }
    let local_outcome = serde_json::to_value(HeroOutcome {
        reason: "completed",
        players: local_players,
    })
    .unwrap();
    let conflicting_outcome = serde_json::to_value(HeroOutcome {
        reason: "completed",
        players: std::collections::BTreeMap::from([(
            player_a.player_id,
            HeroVictory {
                hero: "Ringo",
                victory: &Value::Bool(false),
            },
        )]),
    })
    .unwrap();

    db_a.execute_batch(
        "CREATE TRIGGER reject_ce_match_finished BEFORE INSERT ON events
         WHEN json_extract(NEW.body,'$.type')='match_finished'
         BEGIN SELECT RAISE(ABORT, 'injected trigger failure'); END;",
    )
    .unwrap();
    assert!(
        app_a
            .finish_match(local_room, local_outcome.clone())
            .is_err()
    );
    assert_eq!(
        db_a.query_row("SELECT count(*) FROM ce_completed", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db_a.query_row("SELECT count(*) FROM ce_match_handles", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    db_a.execute_batch("DROP TRIGGER reject_ce_match_finished")
        .unwrap();

    app_a
        .finish_match(local_room, local_outcome.clone())
        .unwrap();
    app_a.finish_match(local_room, local_outcome).unwrap();
    assert!(matches!(
        app_a.finish_match(local_room, conflicting_outcome.clone()),
        Err(PlatformError::Conflict(_))
    ));
    for guest in &aral_guests {
        app_a
            .acknowledge_post_match(&guest.session_token)
            .await
            .unwrap();
    }

    // 2. Cross-home federated projection via Diesel federated_project, including rollback, duplicate, and conflict.
    let pool = Uuid::new_v4();
    let fed_room = Uuid::new_v4();
    let intent_a = app_a
        .federated_join(&player_a.session_token, pool, "ranked_5v5".into(), 2)
        .await
        .unwrap();
    let intent_b = app_b
        .federated_join(&player_b.session_token, pool, "ranked_5v5".into(), 2)
        .await
        .unwrap();
    for (app, ticket) in [(&app_a, intent_a.ticket), (&app_b, intent_b.ticket)] {
        app.federated_project(ticket, "offered", Some(fed_room), None, None)
            .await
            .unwrap();
        app.federated_action(ticket, Some(fed_room), false)
            .await
            .unwrap();
        app.federated_project(
            ticket,
            "playing",
            Some(fed_room),
            Some("127.0.0.1:9200".into()),
            None,
        )
        .await
        .unwrap();
    }

    let fed_outcome = serde_json::to_value(HeroOutcome {
        reason: "completed",
        players: std::collections::BTreeMap::from([
            (
                player_a.player_id,
                HeroVictory {
                    hero: "Kestrel",
                    victory: &Value::Bool(true),
                },
            ),
            (
                player_b.player_id,
                HeroVictory {
                    hero: "Kestrel",
                    victory: &Value::Bool(true),
                },
            ),
        ]),
    })
    .unwrap();

    db_a.execute_batch(
        "CREATE TRIGGER reject_fed_finished BEFORE INSERT ON events
         WHEN json_extract(NEW.body,'$.type')='match_finished'
         BEGIN SELECT RAISE(ABORT, 'injected fed failure'); END;",
    )
    .unwrap();
    assert!(
        app_a
            .federated_project(
                intent_a.ticket,
                "finished",
                Some(fed_room),
                None,
                Some(fed_outcome.clone()),
            )
            .await
            .is_err()
    );
    assert_eq!(
        db_a.query_row(
            "SELECT count(*) FROM ce_completed WHERE player=? AND room=?",
            params![player_a.player_id.to_string(), fed_room.to_string()],
            |r| r.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    db_a.execute_batch("DROP TRIGGER reject_fed_finished")
        .unwrap();

    for (app, ticket) in [(&app_a, intent_a.ticket), (&app_b, intent_b.ticket)] {
        app.federated_project(
            ticket,
            "finished",
            Some(fed_room),
            None,
            Some(fed_outcome.clone()),
        )
        .await
        .unwrap();
        app.federated_project(
            ticket,
            "finished",
            Some(fed_room),
            None,
            Some(fed_outcome.clone()),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.federated_project(
                ticket,
                "finished",
                Some(fed_room),
                None,
                Some(conflicting_outcome.clone()),
            )
            .await,
            Err(PlatformError::Conflict(_))
        ));
    }

    let stats_a = wire_activity(&meta_a, player_a.player_id).await;
    assert_eq!(stats_a["completed"], 2);
    assert_eq!(stats_a["wins"], 2);
    assert_eq!(stats_a["winsToday"], 2);
    assert_eq!(stats_a["wins_aral"], 1);
    assert_eq!(stats_a["wins_casual"], 1);
    assert_eq!(stats_a["wins_ranked"], 1);
    assert_eq!(stats_a["winStreak"], 2);
    assert_eq!(
        meta_a.hero_stats(player_a.player_id).await.unwrap(),
        serde_json::to_value([
            HeroStats {
                hero: "Kestrel",
                completed: 1,
                wins: 1
            },
            HeroStats {
                hero: "Ringo",
                completed: 1,
                wins: 1
            },
        ])
        .unwrap()
    );
    let stats_b = wire_activity(&meta_b, player_b.player_id).await;
    assert_eq!(stats_b["completed"], 1);
    assert_eq!(stats_b["wins"], 1);
    assert_eq!(stats_b["wins_ranked"], 1);
    assert_eq!(stats_b["winStreak"], 1);
    for (db, player, expected) in [
        (&db_a, player_a.player_id, 2_u32),
        (&db_b, player_b.player_id, 1_u32),
    ] {
        for quest in ["complete", "win"] {
            let progress: u32 = db
                .query_row(
                    "SELECT progress FROM community_quest_states WHERE player=? AND quest=?",
                    params![player.to_string(), quest],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(progress, expected);
        }
        let handles: i64 = db
            .query_row(
                "SELECT count(*) FROM ce_match_handles WHERE room=? AND player=?",
                params![fed_room.to_string(), player.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(handles, 1);
    }

    // 3. Reopen both homes and acknowledge federated post-match state.
    drop(meta_a);
    drop(meta_b);
    drop(comm_a);
    drop(comm_b);
    drop(app_a);
    drop(app_b);

    let reopened_a = AppState::open(cfg_a, Arc::new(ReserveHost)).unwrap();
    let reopened_b = AppState::open(cfg_b, Arc::new(ReserveHost)).unwrap();
    let reopened_meta_a = Meta::open(&path_a).unwrap();
    assert_eq!(
        wire_activity(&reopened_meta_a, player_a.player_id).await,
        stats_a
    );
    reopened_a
        .acknowledge_post_match(&player_a.session_token)
        .await
        .unwrap();
    reopened_b.federated_release(intent_b.ticket).await.unwrap();
    assert!(
        reopened_a
            .profile(&player_a.session_token)
            .await
            .unwrap()
            .lobby
            .is_none()
    );
    assert!(
        reopened_b
            .profile(&player_b.session_token)
            .await
            .unwrap()
            .lobby
            .is_none()
    );
}
