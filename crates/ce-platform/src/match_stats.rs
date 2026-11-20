use rusqlite::{Transaction, params};

use super::Error;

// An explicit victory takes precedence over older results which provide only
// a winning team. A bare winner cannot identify this player's outcome.
const OUTCOME: &str = "CASE
    WHEN json_type(p.value,'$.victory') IN ('true','false') THEN json_extract(p.value,'$.victory')
    WHEN json_type(p.value,'$.victory')='integer' AND json_extract(p.value,'$.victory') IN (0,1)
        THEN json_extract(p.value,'$.victory')
    WHEN json_type(p.value,'$.team')='integer' AND json_extract(p.value,'$.team') IN (0,1)
         AND json_type(e.body,'$.result.winner')='integer' AND json_extract(e.body,'$.result.winner') IN (0,1)
        THEN json_extract(p.value,'$.team')=json_extract(e.body,'$.result.winner')
    END";

pub(crate) fn initialize(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS ce_migrations(name TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS ce_completed(
             player TEXT NOT NULL, room TEXT NOT NULL, won INTEGER, hero TEXT, mode TEXT, day INTEGER,
             PRIMARY KEY(player,room));
         CREATE INDEX IF NOT EXISTS ce_federated_match ON federated_queue(match_id);
         CREATE TABLE IF NOT EXISTS ce_player_matches(
             player TEXT PRIMARY KEY, completed INTEGER NOT NULL, wins INTEGER NOT NULL,
             non_tutorial INTEGER NOT NULL, aral INTEGER NOT NULL, blitz INTEGER NOT NULL,
             blitz_rounds_casual INTEGER NOT NULL, casual INTEGER NOT NULL, ranked INTEGER NOT NULL,
             last_won INTEGER, streak INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS ce_player_matches_ranking ON ce_player_matches(completed DESC,player);
         CREATE TABLE IF NOT EXISTS ce_daily_wins(
             player TEXT NOT NULL, day INTEGER NOT NULL, wins INTEGER NOT NULL, PRIMARY KEY(player,day));
         CREATE TABLE IF NOT EXISTS ce_hero_matches(
             player TEXT NOT NULL, hero TEXT NOT NULL, completed INTEGER NOT NULL, wins INTEGER NOT NULL,
             PRIMARY KEY(player,hero));",
    )?;
    if db.execute(
        "INSERT OR IGNORE INTO ce_migrations VALUES('nullable_outcome_and_match_totals')",
        [],
    )? == 0
    {
        return Ok(());
    }
    db.execute_batch(
        "DROP TRIGGER IF EXISTS ce_match_quests;
         DROP TRIGGER IF EXISTS ce_result_event;",
    )?;
    let mut columns = db.prepare("SELECT name FROM pragma_table_info('ce_completed')")?;
    let columns = columns
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let old_mode = if columns.iter().any(|column| column == "mode") {
        "old.mode"
    } else {
        "NULL"
    };
    let old_day = if columns.iter().any(|column| column == "day") {
        "old.day"
    } else {
        "NULL"
    };
    db.execute_batch(&format!(
        "ALTER TABLE ce_completed RENAME TO ce_completed_legacy;
         CREATE TABLE ce_completed(
             player TEXT NOT NULL, room TEXT NOT NULL, won INTEGER, hero TEXT, mode TEXT, day INTEGER,
             PRIMARY KEY(player,room));
         {TOTALS_TRIGGER}
         CREATE TEMP TABLE ce_result_evidence(
             player TEXT NOT NULL, room TEXT NOT NULL, completed INTEGER NOT NULL,
             won INTEGER, hero TEXT, cursor INTEGER NOT NULL, PRIMARY KEY(player,room));
         INSERT OR IGNORE INTO ce_result_evidence
         SELECT e.player,json_extract(e.body,'$.matchId'),
            coalesce(json_extract(e.body,'$.result.reason'),'completed')='completed',
            {OUTCOME},json_extract(p.value,'$.hero'),e.cursor
         FROM events e LEFT JOIN json_each(e.body,'$.result.players') p ON p.key=e.player AND p.type='object'
         WHERE json_extract(e.body,'$.type')='match_finished'
           AND json_type(e.body,'$.matchId')='text'
         ORDER BY 3 DESC,e.cursor;
         INSERT INTO ce_completed(rowid,player,room,won,hero,mode,day)
         SELECT old.rowid,old.player,old.room,
             CASE WHEN known.cursor IS NULL THEN old.won ELSE known.won END,
             coalesce(known.hero,old.hero),
             coalesce({old_mode},(SELECT mode FROM matches WHERE id=old.room),
                      (SELECT mode FROM federated_queue WHERE match_id=old.room LIMIT 1)),
             {old_day}
         FROM ce_completed_legacy old LEFT JOIN ce_result_evidence known USING(player,room)
         WHERE known.cursor IS NULL OR known.completed=1 ORDER BY old.rowid;
         INSERT INTO ce_completed(player,room,won,hero,mode,day)
         SELECT known.player,known.room,known.won,known.hero,
             coalesce((SELECT mode FROM matches WHERE id=known.room),
                      (SELECT mode FROM federated_queue WHERE match_id=known.room LIMIT 1)),NULL
         FROM ce_result_evidence known
         WHERE known.completed=1 AND NOT EXISTS (
             SELECT 1 FROM ce_completed_legacy old WHERE old.player=known.player AND old.room=known.room
         ) ORDER BY known.cursor;
         DROP TABLE ce_result_evidence;
         DROP TABLE ce_completed_legacy;
         CREATE TRIGGER ce_result_event AFTER INSERT ON events
         WHEN json_extract(NEW.body,'$.type')='match_finished'
           AND coalesce(json_extract(NEW.body,'$.result.reason'),'completed')='completed'
           AND json_type(NEW.body,'$.matchId')='text'
         BEGIN
             INSERT OR IGNORE INTO ce_completed(player,room,won,hero,mode,day)
             SELECT e.player,json_extract(e.body,'$.matchId'),{OUTCOME},json_extract(p.value,'$.hero'),
                 coalesce((SELECT mode FROM matches WHERE id=json_extract(e.body,'$.matchId')),
                          (SELECT mode FROM federated_queue WHERE match_id=json_extract(e.body,'$.matchId') LIMIT 1)),
                 unixepoch()/86400
             FROM (SELECT NEW.player AS player, NEW.body AS body) e
             LEFT JOIN json_each(e.body,'$.result.players') p ON p.key=e.player AND p.type='object';
         END;
         {QUEST_TRIGGER}"
    ))?;
    Ok(())
}

const TOTALS_TRIGGER: &str = "
CREATE TRIGGER ce_match_totals AFTER INSERT ON ce_completed
BEGIN
    INSERT INTO ce_player_matches VALUES (NEW.player,1,NEW.won IS 1,
        coalesce(NEW.mode NOT LIKE '%tutorial%',0),
        NEW.won IS 1 AND coalesce(NEW.mode LIKE '%aral%',0),
        NEW.won IS 1 AND coalesce(NEW.mode LIKE '%blitz%',0),
        NEW.won IS 1 AND coalesce(NEW.mode='blitz_rounds_casual',0),
        NEW.won IS 1 AND coalesce(NEW.mode LIKE '%casual%',0),
        NEW.won IS 1 AND coalesce(NEW.mode LIKE '%ranked%',0),
        NEW.won,NEW.won IS NOT NULL)
    ON CONFLICT(player) DO UPDATE SET
        completed=completed+1,wins=wins+excluded.wins,non_tutorial=non_tutorial+excluded.non_tutorial,
        aral=aral+excluded.aral,blitz=blitz+excluded.blitz,
        blitz_rounds_casual=blitz_rounds_casual+excluded.blitz_rounds_casual,
        casual=casual+excluded.casual,ranked=ranked+excluded.ranked,
        streak=CASE WHEN excluded.last_won IS NULL THEN 0
                    WHEN excluded.last_won=last_won THEN streak+1 ELSE 1 END,
        last_won=excluded.last_won;
    INSERT INTO ce_daily_wins SELECT NEW.player,NEW.day,1 WHERE NEW.won=1 AND NEW.day IS NOT NULL
    ON CONFLICT(player,day) DO UPDATE SET wins=wins+1;
    INSERT INTO ce_hero_matches SELECT NEW.player,NEW.hero,1,NEW.won IS 1 WHERE NEW.hero IS NOT NULL
    ON CONFLICT(player,hero) DO UPDATE SET completed=completed+1,wins=wins+excluded.wins;
END;";

const QUEST_TRIGGER: &str = "
CREATE TRIGGER ce_match_quests AFTER INSERT ON ce_completed
WHEN EXISTS(SELECT 1 FROM community_players WHERE id=NEW.player)
BEGIN
    INSERT INTO community_quest_states(player,quest,progress)
    SELECT NEW.player,id,1 FROM community_quests
        WHERE category='complete_match' OR (category='win_match' AND NEW.won=1)
    ON CONFLICT(player,quest) DO UPDATE SET progress=min(progress+1,
        (SELECT goal FROM community_quests WHERE id=excluded.quest))
        WHERE state='active';
END;";

pub(crate) fn read_player_matches(
    tx: &Transaction<'_>,
    player: &str,
) -> Result<PlayerMatches, Error> {
    let fields = tx.query_row(
        "SELECT coalesce(p.completed,0),coalesce(p.wins,0),coalesce(p.non_tutorial,0),coalesce(d.wins,0),
                coalesce(p.aral,0),coalesce(p.blitz,0),coalesce(p.blitz_rounds_casual,0),
                coalesce(p.casual,0),coalesce(p.ranked,0),p.last_won,coalesce(p.streak,0)
         FROM (SELECT ?1 AS id) q
         LEFT JOIN ce_player_matches p ON p.player=q.id
         LEFT JOIN ce_daily_wins d ON d.player=q.id AND d.day=unixepoch()/86400",
        params![player],
        |row| {
            let streak = row.get::<_, u64>(10)?;
            let won = row.get::<_, Option<bool>>(9)?;
            Ok(PlayerMatches {
                completed: row.get(0)?,
                wins: row.get(1)?,
                completed_non_tutorial: row.get(2)?,
                wins_today: row.get(3)?,
                wins_aral: row.get(4)?,
                wins_blitz: row.get(5)?,
                wins_blitz_rounds_casual: row.get(6)?,
                wins_casual: row.get(7)?,
                wins_ranked: row.get(8)?,
                win_streak: won.filter(|won| *won).map(|_| streak),
                loss_streak: won.filter(|won| !*won).map(|_| streak),
            })
        },
    )?;
    Ok(fields)
}

#[derive(Default, serde::Serialize)]
pub(crate) struct PlayerMatches {
    completed: u64,
    wins: u64,
    completed_non_tutorial: u64,
    #[serde(rename = "winsToday")]
    wins_today: u64,
    wins_aral: u64,
    wins_blitz: u64,
    wins_blitz_rounds_casual: u64,
    wins_casual: u64,
    wins_ranked: u64,
    #[serde(rename = "winStreak", skip_serializing_if = "Option::is_none")]
    win_streak: Option<u64>,
    #[serde(rename = "lossStreak", skip_serializing_if = "Option::is_none")]
    loss_streak: Option<u64>,
}

impl PlayerMatches {
    pub(crate) fn with_streak_defaults(mut self) -> Self {
        self.win_streak.get_or_insert(0);
        self.loss_streak.get_or_insert(0);
        self
    }
}

#[cfg(test)]
mod tests;
