use crate::{
    Result,
    schema::{
        ce_completed, ce_daily_wins, ce_hero_matches, ce_match_handles, ce_player_matches, ce_wait,
        ce_wait_history, community_players, community_quests, events, federated_queue, matches,
        players,
    },
};
use diesel::{
    connection::SimpleConnection,
    dsl::{exists, sql},
    prelude::*,
    sql_types::{BigInt, Bool, Double, Nullable, Text},
    sqlite::SqliteConnection,
};
use serde_json::Value;
use std::collections::BTreeMap;

diesel::define_sql_function!(fn unixepoch() -> BigInt);

#[derive(Default)]
pub struct ParsedPlayerOutcome {
    pub completed: bool,
    pub won: Option<bool>,
    pub hero: Option<String>,
}

#[derive(QueryableByName)]
struct LegacyCompletedRow {
    #[diesel(sql_type = Text)]
    player: String,
    #[diesel(sql_type = Text)]
    room: String,
    #[diesel(sql_type = Nullable<Bool>)]
    won: Option<bool>,
    #[diesel(sql_type = Nullable<Text>)]
    hero: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    mode: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    day: Option<i64>,
}

#[derive(QueryableByName)]
struct CompletedRow {
    #[diesel(sql_type = Text)]
    player: String,
    #[diesel(sql_type = Nullable<Bool>)]
    won: Option<bool>,
    #[diesel(sql_type = Nullable<Text>)]
    hero: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    mode: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    day: Option<i64>,
}

#[derive(QueryableByName)]
struct DayResult {
    #[diesel(sql_type = BigInt)]
    day: i64,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct PlayerMatchStats {
    pub completed: u64,
    pub wins: u64,
    pub completed_non_tutorial: u64,
    #[serde(rename = "winsToday")]
    pub wins_today: u64,
    pub wins_aral: u64,
    pub wins_blitz: u64,
    pub wins_blitz_rounds_casual: u64,
    pub wins_casual: u64,
    pub wins_ranked: u64,
    #[serde(rename = "winStreak", skip_serializing_if = "Option::is_none")]
    pub win_streak: Option<u64>,
    #[serde(rename = "lossStreak", skip_serializing_if = "Option::is_none")]
    pub loss_streak: Option<u64>,
}

impl PlayerMatchStats {
    pub fn with_streak_defaults(mut self) -> Self {
        self.win_streak.get_or_insert(0);
        self.loss_streak.get_or_insert(0);
        self
    }
}

pub fn extract_player_outcome(result: Option<&Value>, player: &str) -> ParsedPlayerOutcome {
    let completed = result
        .and_then(|v| v.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("completed")
        == "completed";
    let seat = result
        .and_then(|v| v.get("players"))
        .and_then(Value::as_object)
        .and_then(|players| players.get(player))
        .and_then(Value::as_object);
    let hero = seat
        .and_then(|obj| obj.get("hero"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let won = match seat.and_then(|obj| obj.get("victory")) {
        Some(Value::Bool(won)) => Some(*won),
        Some(Value::Number(n)) if n.as_i64() == Some(0) => Some(false),
        Some(Value::Number(n)) if n.as_i64() == Some(1) => Some(true),
        _ => {
            let team = seat
                .and_then(|obj| obj.get("team"))
                .and_then(Value::as_i64)
                .filter(|v| *v == 0 || *v == 1);
            let winner = result
                .and_then(|v| v.get("winner"))
                .and_then(Value::as_i64)
                .filter(|v| *v == 0 || *v == 1);
            match (team, winner) {
                (Some(team), Some(winner)) => Some(team == winner),
                _ => None,
            }
        }
    };
    ParsedPlayerOutcome {
        completed,
        won,
        hero,
    }
}

pub fn record_queued(db: &mut SqliteConnection, players: &[String], mode: &str) -> Result<()> {
    for player in players {
        diesel::replace_into(ce_wait::table)
            .values((
                ce_wait::player.eq(player),
                ce_wait::mode.eq(mode),
                ce_wait::joined.eq(unixepoch()),
            ))
            .execute(db)?;
    }
    Ok(())
}

pub fn record_match_offered(db: &mut SqliteConnection, players: &[String]) -> Result<()> {
    for player in players {
        let wait = ce_wait::table
            .find(player)
            .select((ce_wait::mode, ce_wait::joined))
            .first::<(String, i64)>(db)
            .optional()?;
        if let Some((mode, joined)) = wait {
            let now = diesel::select(unixepoch()).get_result::<i64>(db)?;
            let sample = (now - joined) as f64;
            diesel::insert_into(ce_wait_history::table)
                .values((
                    ce_wait_history::mode.eq(&mode),
                    ce_wait_history::seconds.eq(sample),
                ))
                .on_conflict(ce_wait_history::mode)
                .do_update()
                .set(ce_wait_history::seconds.eq(sql::<Double>(
                    "0.875 * ce_wait_history.seconds + 0.125 * excluded.seconds",
                )))
                .execute(db)?;
            diesel::delete(ce_wait::table.find(player)).execute(db)?;
        }
    }
    Ok(())
}

pub fn record_match_finished(
    db: &mut SqliteConnection,
    room: &str,
    mode: Option<&str>,
    participants: &[String],
    result: Option<&Value>,
) -> Result<()> {
    let seats = result
        .and_then(|v| v.get("players"))
        .and_then(Value::as_object);
    let now_day = current_day(db)?;
    let resolved_mode = match mode {
        Some(m) => Some(m.to_owned()),
        None => resolve_match_mode(db, room)?,
    };

    for player in participants {
        if let Some(seats) = seats
            && seats.contains_key(player)
            && let Some(handle) = players::table
                .find(player)
                .select(players::name)
                .first::<String>(db)
                .optional()?
        {
            diesel::insert_into(ce_match_handles::table)
                .values((
                    ce_match_handles::room.eq(room),
                    ce_match_handles::player.eq(player),
                    ce_match_handles::handle.eq(handle),
                ))
                .on_conflict_do_nothing()
                .execute(db)?;
        }

        let outcome = extract_player_outcome(result, player);
        if !outcome.completed {
            continue;
        }
        let inserted = diesel::insert_into(ce_completed::table)
            .values((
                ce_completed::player.eq(player),
                ce_completed::room.eq(room),
                ce_completed::won.eq(outcome.won),
                ce_completed::hero.eq(outcome.hero.as_deref()),
                ce_completed::mode.eq(resolved_mode.as_deref()),
                ce_completed::day.eq(Some(now_day)),
            ))
            .on_conflict_do_nothing()
            .execute(db)?;
        if inserted == 1 {
            apply_completed_totals(
                db,
                player,
                outcome.won,
                outcome.hero.as_deref(),
                resolved_mode.as_deref(),
                Some(now_day),
                true,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn migrate_legacy_completed(
    db: &mut SqliteConnection,
    has_mode: bool,
    has_day: bool,
) -> Result<()> {
    struct Evidence {
        completed: bool,
        won: Option<bool>,
        hero: Option<String>,
        cursor: i64,
    }

    let old_mode = if has_mode { "mode" } else { "NULL" };
    let old_day = if has_day { "day" } else { "NULL" };
    let legacy_sql = format!(
        "SELECT player, room, won, hero, {old_mode} AS mode, {old_day} AS day
         FROM ce_completed_legacy ORDER BY rowid"
    );

    db.batch_execute(
        "ALTER TABLE ce_completed RENAME TO ce_completed_legacy;
         CREATE TABLE ce_completed (
             player TEXT NOT NULL,
             room TEXT NOT NULL,
             won INTEGER,
             hero TEXT,
             mode TEXT,
             day INTEGER,
             PRIMARY KEY(player, room)
         );",
    )?;

    let legacy_rows = diesel::sql_query(&legacy_sql).load::<LegacyCompletedRow>(db)?;
    let event_rows = events::table
        .order(events::cursor)
        .select((events::cursor, events::player, events::body))
        .load::<(i64, String, String)>(db)?;

    let mut evidence: BTreeMap<(String, String), Evidence> = BTreeMap::new();
    for (cursor, player, raw_body) in event_rows {
        let Ok(body) = serde_json::from_str::<Value>(&raw_body) else {
            continue;
        };
        if body.get("type").and_then(Value::as_str) != Some("match_finished") {
            continue;
        }
        let Some(room) = body.get("matchId").and_then(Value::as_str) else {
            continue;
        };
        let outcome = extract_player_outcome(body.get("result"), &player);
        let key = (player, room.to_owned());
        match evidence.get_mut(&key) {
            Some(existing) if existing.completed && !outcome.completed => {}
            Some(existing) if !existing.completed && outcome.completed => {
                *existing = Evidence {
                    completed: outcome.completed,
                    won: outcome.won,
                    hero: outcome.hero,
                    cursor,
                };
            }
            Some(_) => {}
            None => {
                evidence.insert(
                    key,
                    Evidence {
                        completed: outcome.completed,
                        won: outcome.won,
                        hero: outcome.hero,
                        cursor,
                    },
                );
            }
        }
    }

    let mut migrated_from_legacy = Vec::with_capacity(legacy_rows.len());
    for old in legacy_rows {
        let key = (old.player.clone(), old.room.clone());
        let ev = evidence.get(&key);
        if let Some(ev) = ev
            && !ev.completed
        {
            continue;
        }
        let won = match ev {
            None => old.won,
            Some(ev) => ev.won,
        };
        let hero = ev.and_then(|ev| ev.hero.clone()).or(old.hero);
        let mode = match old.mode {
            Some(mode) => Some(mode),
            None => resolve_match_mode(db, &old.room)?,
        };
        let inserted = diesel::insert_into(ce_completed::table)
            .values((
                ce_completed::player.eq(&old.player),
                ce_completed::room.eq(&old.room),
                ce_completed::won.eq(won),
                ce_completed::hero.eq(hero.as_deref()),
                ce_completed::mode.eq(mode.as_deref()),
                ce_completed::day.eq(old.day),
            ))
            .on_conflict_do_nothing()
            .execute(db)?;
        if inserted == 1 {
            migrated_from_legacy.push(key);
        }
    }

    let mut extra_evidence: Vec<_> = evidence
        .into_iter()
        .filter(|(key, ev)| ev.completed && !migrated_from_legacy.contains(key))
        .collect();
    extra_evidence.sort_by_key(|(_, ev)| ev.cursor);

    for ((player, room), ev) in extra_evidence {
        let mode = resolve_match_mode(db, &room)?;
        diesel::insert_into(ce_completed::table)
            .values((
                ce_completed::player.eq(&player),
                ce_completed::room.eq(&room),
                ce_completed::won.eq(ev.won),
                ce_completed::hero.eq(ev.hero.as_deref()),
                ce_completed::mode.eq(mode.as_deref()),
                ce_completed::day.eq(None::<i64>),
            ))
            .on_conflict_do_nothing()
            .execute(db)?;
    }

    db.batch_execute(
        "DROP TABLE ce_completed_legacy;
         DELETE FROM ce_player_matches;
         DELETE FROM ce_daily_wins;
         DELETE FROM ce_hero_matches;",
    )?;

    let all_completed =
        diesel::sql_query("SELECT player, won, hero, mode, day FROM ce_completed ORDER BY rowid")
            .load::<CompletedRow>(db)?;
    for row in all_completed {
        apply_completed_totals(
            db,
            &row.player,
            row.won,
            row.hero.as_deref(),
            row.mode.as_deref(),
            row.day,
            false,
        )?;
    }
    Ok(())
}

fn apply_completed_totals(
    db: &mut SqliteConnection,
    player: &str,
    won: Option<bool>,
    hero: Option<&str>,
    mode: Option<&str>,
    day: Option<i64>,
    update_quests: bool,
) -> Result<()> {
    let won_flag = i64::from(won == Some(true));
    let mode_lower = mode.map(str::to_ascii_lowercase);
    let has_mode = |needle: &str| mode_lower.as_deref().is_some_and(|m| m.contains(needle));
    let non_tutorial = i64::from(
        mode_lower
            .as_deref()
            .is_some_and(|m| !m.contains("tutorial")),
    );
    let aral = i64::from(won == Some(true) && has_mode("aral"));
    let blitz = i64::from(won == Some(true) && has_mode("blitz"));
    let blitz_rounds_casual = i64::from(won == Some(true) && mode == Some("blitz_rounds_casual"));
    let casual = i64::from(won == Some(true) && has_mode("casual"));
    let ranked = i64::from(won == Some(true) && has_mode("ranked"));
    let initial_streak = i64::from(won.is_some());

    let existing_streak = ce_player_matches::table
        .find(player)
        .select((ce_player_matches::last_won, ce_player_matches::streak))
        .first::<(Option<bool>, i64)>(db)
        .optional()?;

    if let Some((prev_last_won, prev_streak)) = existing_streak {
        let next_streak = match won {
            None => 0,
            Some(_) if won == prev_last_won => prev_streak + 1,
            Some(_) => 1,
        };
        diesel::update(ce_player_matches::table.find(player))
            .set((
                ce_player_matches::completed.eq(ce_player_matches::completed + 1),
                ce_player_matches::wins.eq(ce_player_matches::wins + won_flag),
                ce_player_matches::non_tutorial.eq(ce_player_matches::non_tutorial + non_tutorial),
                ce_player_matches::aral.eq(ce_player_matches::aral + aral),
                ce_player_matches::blitz.eq(ce_player_matches::blitz + blitz),
                ce_player_matches::blitz_rounds_casual
                    .eq(ce_player_matches::blitz_rounds_casual + blitz_rounds_casual),
                ce_player_matches::casual.eq(ce_player_matches::casual + casual),
                ce_player_matches::ranked.eq(ce_player_matches::ranked + ranked),
                ce_player_matches::last_won.eq(won),
                ce_player_matches::streak.eq(next_streak),
            ))
            .execute(db)?;
    } else {
        diesel::insert_into(ce_player_matches::table)
            .values((
                ce_player_matches::player.eq(player),
                ce_player_matches::completed.eq(1_i64),
                ce_player_matches::wins.eq(won_flag),
                ce_player_matches::non_tutorial.eq(non_tutorial),
                ce_player_matches::aral.eq(aral),
                ce_player_matches::blitz.eq(blitz),
                ce_player_matches::blitz_rounds_casual.eq(blitz_rounds_casual),
                ce_player_matches::casual.eq(casual),
                ce_player_matches::ranked.eq(ranked),
                ce_player_matches::last_won.eq(won),
                ce_player_matches::streak.eq(initial_streak),
            ))
            .execute(db)?;
    }

    if won == Some(true)
        && let Some(day) = day
    {
        diesel::insert_into(ce_daily_wins::table)
            .values((
                ce_daily_wins::player.eq(player),
                ce_daily_wins::day.eq(day),
                ce_daily_wins::wins.eq(1_i64),
            ))
            .on_conflict((ce_daily_wins::player, ce_daily_wins::day))
            .do_update()
            .set(ce_daily_wins::wins.eq(ce_daily_wins::wins + 1))
            .execute(db)?;
    }

    if let Some(hero) = hero {
        diesel::insert_into(ce_hero_matches::table)
            .values((
                ce_hero_matches::player.eq(player),
                ce_hero_matches::hero.eq(hero),
                ce_hero_matches::completed.eq(1_i64),
                ce_hero_matches::wins.eq(won_flag),
            ))
            .on_conflict((ce_hero_matches::player, ce_hero_matches::hero))
            .do_update()
            .set((
                ce_hero_matches::completed.eq(ce_hero_matches::completed + 1),
                ce_hero_matches::wins.eq(ce_hero_matches::wins + won_flag),
            ))
            .execute(db)?;
    }

    if update_quests
        && diesel::select(exists(community_players::table.find(player))).get_result::<bool>(db)?
    {
        let quests = if won == Some(true) {
            community_quests::table
                .filter(
                    community_quests::category
                        .eq("complete_match")
                        .or(community_quests::category.eq("win_match")),
                )
                .select((community_quests::id, community_quests::goal))
                .load::<(String, i64)>(db)?
        } else {
            community_quests::table
                .filter(community_quests::category.eq("complete_match"))
                .select((community_quests::id, community_quests::goal))
                .load::<(String, i64)>(db)?
        };
        for (quest_id, goal) in quests {
            diesel::sql_query(
                "INSERT INTO community_quest_states(player, quest, progress)
                 VALUES(?1, ?2, 1)
                 ON CONFLICT(player, quest) DO UPDATE SET progress = min(progress + 1, ?3)
                 WHERE state = 'active'",
            )
            .bind::<Text, _>(player)
            .bind::<Text, _>(&quest_id)
            .bind::<BigInt, _>(goal)
            .execute(db)?;
        }
    }

    Ok(())
}

pub fn read_player_matches(db: &mut SqliteConnection, player: &str) -> Result<PlayerMatchStats> {
    let today = current_day(db)?;
    let stats = ce_player_matches::table
        .find(player)
        .select((
            ce_player_matches::completed,
            ce_player_matches::wins,
            ce_player_matches::non_tutorial,
            ce_player_matches::aral,
            ce_player_matches::blitz,
            ce_player_matches::blitz_rounds_casual,
            ce_player_matches::casual,
            ce_player_matches::ranked,
            ce_player_matches::last_won,
            ce_player_matches::streak,
        ))
        .first::<(i64, i64, i64, i64, i64, i64, i64, i64, Option<bool>, i64)>(db)
        .optional()?;
    let wins_today = ce_daily_wins::table
        .find((player, today))
        .select(ce_daily_wins::wins)
        .first::<i64>(db)
        .optional()?
        .unwrap_or(0) as u64;
    let Some((
        completed,
        wins,
        non_tutorial,
        aral,
        blitz,
        blitz_rounds_casual,
        casual,
        ranked,
        last_won,
        streak,
    )) = stats
    else {
        return Ok(PlayerMatchStats {
            wins_today,
            ..Default::default()
        });
    };
    let streak = streak as u64;
    Ok(PlayerMatchStats {
        completed: completed as u64,
        wins: wins as u64,
        completed_non_tutorial: non_tutorial as u64,
        wins_today,
        wins_aral: aral as u64,
        wins_blitz: blitz as u64,
        wins_blitz_rounds_casual: blitz_rounds_casual as u64,
        wins_casual: casual as u64,
        wins_ranked: ranked as u64,
        win_streak: last_won.filter(|w| *w).map(|_| streak),
        loss_streak: last_won.filter(|w| !*w).map(|_| streak),
    })
}

fn current_day(db: &mut SqliteConnection) -> Result<i64> {
    let row = diesel::sql_query("SELECT unixepoch() / 86400 AS day").get_result::<DayResult>(db)?;
    Ok(row.day)
}

fn resolve_match_mode(db: &mut SqliteConnection, room: &str) -> Result<Option<String>> {
    if let Some(mode) = matches::table
        .find(room)
        .select(matches::mode)
        .first::<String>(db)
        .optional()?
    {
        return Ok(Some(mode));
    }
    Ok(federated_queue::table
        .filter(federated_queue::match_id.eq(room))
        .select(federated_queue::mode)
        .first::<String>(db)
        .optional()?)
}
