use crate::{Result, ce_stats, schema::ce_migrations};
use diesel::{
    connection::SimpleConnection,
    prelude::*,
    sql_types::{Bool, Text},
    sqlite::SqliteConnection,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[derive(QueryableByName)]
struct BoolValue {
    #[diesel(sql_type = Bool)]
    value: bool,
}

#[derive(QueryableByName)]
struct ColumnName {
    #[diesel(sql_type = Text)]
    name: String,
}

/// Apply all unified storage migrations and idempotently adopt any legacy SQLite layout.
pub fn ensure_schema(db: &mut SqliteConnection) -> Result<()> {
    db.batch_execute("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    db.immediate_transaction(|db| {
        let had_notifications = table_exists(db, "notification_events")?;
        let had_settlements = table_exists(db, "native_settlements")?;
        db.run_pending_migrations(MIGRATIONS)
            .map_err(crate::Error::Migration)?;
        adopt_legacy_platform_schema(db, had_notifications, had_settlements)?;
        adopt_legacy_ce_match_stats(db)?;
        Ok(())
    })
}

/// Adopt legacy pre-consolidation or downgraded platform tables inside an active write transaction.
pub fn adopt_legacy_platform_schema(
    db: &mut SqliteConnection,
    had_notifications: bool,
    had_settlements: bool,
) -> Result<()> {
    // Reassert unified DDL & intra-platform triggers cleanly in case an older database
    // or compatibility test dropped tables or downgraded columns after initial migration.
    db.batch_execute(include_str!(
        "../migrations/2026-11-21-000006_consolidate_platform_and_ce/up.sql"
    ))?;

    for (column, definition) in [
        ("batch", "TEXT"),
        ("seat_limit", "INTEGER"),
        ("difficulty", "TEXT"),
        ("queued_at_ms", "INTEGER"),
    ] {
        if !has_column(db, "tickets", column)? {
            db.batch_execute(&format!(
                "ALTER TABLE tickets ADD COLUMN {column} {definition}"
            ))?;
            if column == "queued_at_ms" {
                db.batch_execute(
                    "UPDATE tickets SET queued_at_ms = CAST(unixepoch('subsec') * 1000 AS INTEGER)
                     WHERE match_id IS NULL",
                )?;
            }
        }
    }

    for (column, definition) in [("difficulty", "TEXT"), ("talent_levels", "TEXT")] {
        if !has_column(db, "matches", column)? {
            db.batch_execute(&format!(
                "ALTER TABLE matches ADD COLUMN {column} {definition}"
            ))?;
        }
    }

    for (column, definition) in [
        ("enqueue_schema", "INTEGER NOT NULL DEFAULT 0"),
        ("difficulty", "TEXT"),
        ("queued_at_ms", "INTEGER"),
    ] {
        if !has_column(db, "federated_queue", column)? {
            db.batch_execute(&format!(
                "ALTER TABLE federated_queue ADD COLUMN {column} {definition}"
            ))?;
            if column == "queued_at_ms" {
                db.batch_execute(
                    "UPDATE federated_queue SET queued_at_ms = CAST(unixepoch('subsec') * 1000 AS INTEGER)
                     WHERE phase IN ('pending', 'queued')",
                )?;
            }
        }
    }

    if table_exists(db, "federated_intents")? {
        db.batch_execute(
            "INSERT INTO federated_queue(ticket, player, pool, mode, seats, accepted_match, cancel, phase, match_id, address, result)
             SELECT ticket, player, pool, mode, seats, accepted_match, cancel, phase, match_id, address, result
             FROM federated_intents;
             INSERT INTO federated_queue_members(ticket, player, pos, accepted_match)
             SELECT ticket, player, 0, accepted_match FROM federated_intents;
             DROP TABLE federated_intents;",
        )?;
    }

    if !had_settlements {
        db.batch_execute(
            "INSERT OR IGNORE INTO native_settlements
             SELECT room, CAST(unixepoch('subsec') * 1000 AS INTEGER) FROM (
               SELECT DISTINCT t.match_id AS room FROM tickets t JOIN matches m ON m.id = t.match_id WHERE m.phase = 'finished'
               UNION SELECT match_id FROM federated_queue WHERE phase = 'finished' AND match_id IS NOT NULL
             );",
        )?;
    }

    db.batch_execute(
        "INSERT OR IGNORE INTO native_offer_seats
         SELECT match_id, player, coalesce(batch, player), serial, accepted
         FROM tickets WHERE match_id IS NOT NULL;",
    )?;

    if !had_notifications {
        db.batch_execute(
            "INSERT INTO notification_events(player, ordinal, event_cursor)
             SELECT player, row_number() OVER (PARTITION BY player ORDER BY cursor), cursor
             FROM events WHERE json_extract(body, '$.type') <> 'chat';",
        )?;
    }

    db.batch_execute("DELETE FROM spectator_sessions WHERE spectator_id IS NULL;")?;
    Ok(())
}

/// Rebuild legacy CE match statistics and clear deprecated SQLite triggers once per database.
pub fn adopt_legacy_ce_match_stats(db: &mut SqliteConnection) -> Result<()> {
    let inserted = diesel::insert_into(ce_migrations::table)
        .values(ce_migrations::name.eq("nullable_outcome_and_match_totals"))
        .on_conflict_do_nothing()
        .execute(db)?;
    if inserted == 0 {
        return Ok(());
    }
    db.batch_execute(
        "DROP TRIGGER IF EXISTS ce_snapshot_match_handle;
         DROP TRIGGER IF EXISTS ce_join_time;
         DROP TRIGGER IF EXISTS ce_offer_time;
         DROP TRIGGER IF EXISTS ce_result_event;
         DROP TRIGGER IF EXISTS ce_match_totals;
         DROP TRIGGER IF EXISTS ce_match_quests;",
    )?;
    let has_mode = has_column(db, "ce_completed", "mode")?;
    let has_day = has_column(db, "ce_completed", "day")?;
    ce_stats::migrate_legacy_completed(db, has_mode, has_day)?;
    Ok(())
}

fn table_exists(db: &mut SqliteConnection, table: &str) -> Result<bool> {
    let row = diesel::sql_query(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1) AS value",
    )
    .bind::<Text, _>(table)
    .get_result::<BoolValue>(db)?;
    Ok(row.value)
}

fn has_column(db: &mut SqliteConnection, table: &str, column: &str) -> Result<bool> {
    let cols = diesel::sql_query(format!("SELECT name FROM pragma_table_info('{table}')"))
        .load::<ColumnName>(db)?;
    Ok(cols.iter().any(|col| col.name == column))
}
