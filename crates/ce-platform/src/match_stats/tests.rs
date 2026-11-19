use std::sync::Arc;

use halcyon_platform::{
    AppState, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
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
            json!({
                "type":"match_finished","matchId":room,"result":outcome
            })
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
            Some(json!({"winner":0,"players":{player.to_string():{"team":0}}})),
            false,
            "blitz_rounds_casual",
            "unixepoch()/86400",
        );
        let aborted = legacy(
            Some(json!({"reason":"admin_terminated"})),
            false,
            "casual_5v5",
            "unixepoch()/86400",
        );
        legacy(Some(json!({"winner":0})), false, "aral_casual", "NULL");
        let meta = Meta::open(&path).unwrap();
        let migrated = meta.player_fields(player).await.unwrap();
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
            json!([{"hero":"Ardan","completed":3,"wins":2}])
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
                    json!({"reason":"completed",
                    "players":{player.to_string():{"victory":victory,"hero":"Catherine"}}}),
                );
            }
        };
        settle("aral_casual", json!(false));
        settle("casual_5v5", json!(false));
        assert_eq!(meta.player_fields(player).await.unwrap()["lossStreak"], 2);
        settle("casual_5v5", Value::Null);
        assert!(
            meta.player_fields(player)
                .await
                .unwrap()
                .get("lossStreak")
                .is_none()
        );
        settle("ranked_5v5", json!(true));
        settle("aral_casual", json!(true));
        result(
            &db,
            player,
            aborted,
            json!({"reason":"admin_terminated","winner":0}),
        );

        let final_stats = meta.player_fields(player).await.unwrap();
        assert_eq!(final_stats["completed"], 8);
        assert_eq!(final_stats["wins"], 4);
        assert_eq!(final_stats["winsToday"], 2 + i32::from(with_day));
        assert_eq!(final_stats["wins_aral"], 1);
        assert_eq!(final_stats["wins_ranked"], 1);
        assert_eq!(final_stats["wins_casual"], 2);
        assert_eq!(final_stats["winStreak"], 2);
        assert_eq!(
            meta.hero_stats(player).await.unwrap(),
            json!([
                {"hero":"Ardan","completed":3,"wins":2},
                {"hero":"Catherine","completed":5,"wins":2},
            ])
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
        assert_eq!(reopened.player_fields(player).await.unwrap(), final_stats);
        assert_eq!(quest_progress("win"), (2, "active".into()));
    }
}
