-- Consolidate platform, matchmaking, spectator, and CE persistence tables cleanly into crates/storage.
-- Eliminate cross-crate CE SQLite triggers; Rust application hooks now maintain CE state transactionally.
DROP TRIGGER IF EXISTS ce_snapshot_match_handle;
DROP TRIGGER IF EXISTS ce_join_time;
DROP TRIGGER IF EXISTS ce_offer_time;
DROP TRIGGER IF EXISTS ce_result_event;
DROP TRIGGER IF EXISTS ce_match_totals;
DROP TRIGGER IF EXISTS ce_match_quests;

CREATE TABLE IF NOT EXISTS players (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    recovery BLOB NOT NULL UNIQUE,
    last_seen INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS sessions (
    key BLOB PRIMARY KEY,
    player TEXT NOT NULL UNIQUE REFERENCES players(id)
);

CREATE TABLE IF NOT EXISTS matches (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL,
    phase TEXT NOT NULL,
    address TEXT,
    result TEXT,
    offered_at INTEGER NOT NULL DEFAULT (unixepoch()),
    difficulty TEXT,
    talent_levels TEXT
);

CREATE TABLE IF NOT EXISTS tickets (
    serial INTEGER PRIMARY KEY AUTOINCREMENT,
    player TEXT NOT NULL UNIQUE REFERENCES players(id),
    mode TEXT NOT NULL,
    match_id TEXT REFERENCES matches(id),
    accepted INTEGER NOT NULL DEFAULT 0,
    batch TEXT,
    seat_limit INTEGER,
    difficulty TEXT,
    queued_at_ms INTEGER
);

CREATE TABLE IF NOT EXISTS events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    player TEXT NOT NULL REFERENCES players(id),
    body TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS events_by_player ON events(player, cursor);
CREATE INDEX IF NOT EXISTS ce_event_kind ON events(player, json_extract(body, '$.type'));
CREATE INDEX IF NOT EXISTS pending_party_notifications
    ON events(json_extract(body, '$.party'))
    WHERE json_extract(body, '$.party') IS NOT NULL;
CREATE INDEX IF NOT EXISTS tickets_by_mode ON tickets(mode, match_id, serial);
CREATE INDEX IF NOT EXISTS tickets_by_match ON tickets(match_id);

CREATE TABLE IF NOT EXISTS notification_events (
    player TEXT NOT NULL REFERENCES players(id),
    ordinal INTEGER NOT NULL,
    event_cursor INTEGER NOT NULL UNIQUE REFERENCES events(cursor) ON DELETE CASCADE,
    PRIMARY KEY(player, ordinal)
);

CREATE TRIGGER IF NOT EXISTS event_notifications AFTER INSERT ON events
WHEN json_extract(NEW.body, '$.type') <> 'chat'
BEGIN
    INSERT INTO notification_events(player, ordinal, event_cursor)
    SELECT NEW.player, coalesce(max(ordinal), 0) + 1, NEW.cursor
    FROM notification_events WHERE player = NEW.player;
END;

CREATE TABLE IF NOT EXISTS federated_chat_receipts (
    pool TEXT NOT NULL,
    room TEXT NOT NULL,
    id TEXT NOT NULL,
    seen INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(pool, room, id)
);
CREATE INDEX IF NOT EXISTS federated_chat_receipt_expiry ON federated_chat_receipts(seen);

CREATE TABLE IF NOT EXISTS federated_queue (
    ticket TEXT PRIMARY KEY,
    player TEXT NOT NULL REFERENCES players(id),
    pool TEXT NOT NULL,
    mode TEXT NOT NULL,
    seats INTEGER NOT NULL,
    accepted_match TEXT,
    cancel INTEGER NOT NULL DEFAULT 0,
    phase TEXT NOT NULL DEFAULT 'pending',
    match_id TEXT,
    address TEXT,
    result TEXT,
    enqueue_schema INTEGER NOT NULL DEFAULT 0,
    difficulty TEXT,
    queued_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS federated_queue_by_pool ON federated_queue(pool);
CREATE INDEX IF NOT EXISTS federated_queue_pending ON federated_queue(pool, ticket)
    WHERE phase <> 'finished';
CREATE INDEX IF NOT EXISTS ce_federated_match ON federated_queue(match_id);

CREATE TABLE IF NOT EXISTS federated_site_reports (
    ticket TEXT PRIMARY KEY REFERENCES federated_queue(ticket) ON DELETE CASCADE,
    estimates TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS federated_queue_members (
    ticket TEXT NOT NULL REFERENCES federated_queue(ticket) ON DELETE CASCADE,
    player TEXT NOT NULL REFERENCES players(id),
    pos INTEGER NOT NULL,
    accepted_match TEXT,
    released INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(ticket, player),
    UNIQUE(ticket, pos)
);
CREATE UNIQUE INDEX IF NOT EXISTS federated_one_active_ticket
    ON federated_queue_members(player) WHERE released = 0;

CREATE TABLE IF NOT EXISTS federated_talent_levels (
    ticket TEXT PRIMARY KEY REFERENCES federated_queue(ticket) ON DELETE CASCADE,
    levels TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS native_rejections (
    player TEXT NOT NULL,
    room TEXT NOT NULL,
    ticket TEXT,
    PRIMARY KEY(player, room)
);

CREATE TABLE IF NOT EXISTS native_settlements (
    room TEXT PRIMARY KEY,
    at_ms INTEGER NOT NULL
);

CREATE TRIGGER IF NOT EXISTS native_local_settlement AFTER UPDATE OF phase ON matches
WHEN NEW.phase = 'finished' AND OLD.phase <> 'finished'
BEGIN
    INSERT OR IGNORE INTO native_settlements
    VALUES(NEW.id, CAST(unixepoch('subsec') * 1000 AS INTEGER));
END;

CREATE TRIGGER IF NOT EXISTS native_peer_settlement AFTER UPDATE OF phase ON federated_queue
WHEN NEW.phase = 'finished' AND OLD.phase <> 'finished' AND NEW.match_id IS NOT NULL
BEGIN
    INSERT OR IGNORE INTO native_settlements
    VALUES(NEW.match_id, CAST(unixepoch('subsec') * 1000 AS INTEGER));
END;

CREATE TABLE IF NOT EXISTS native_offer_seats (
    room TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    player TEXT NOT NULL,
    entry TEXT NOT NULL,
    position INTEGER NOT NULL,
    accepted INTEGER NOT NULL,
    PRIMARY KEY(room, player)
);

CREATE TRIGGER IF NOT EXISTS native_offer_assignment AFTER UPDATE OF match_id ON tickets
WHEN NEW.match_id IS NOT NULL
BEGIN
    INSERT INTO native_offer_seats
    VALUES(NEW.match_id, NEW.player, coalesce(NEW.batch, NEW.player), NEW.serial, NEW.accepted);
END;

CREATE TRIGGER IF NOT EXISTS native_offer_accept AFTER UPDATE OF accepted ON tickets
WHEN NEW.match_id IS NOT NULL
BEGIN
    UPDATE native_offer_seats SET accepted = NEW.accepted
    WHERE room = NEW.match_id AND player = NEW.player;
END;

CREATE TABLE IF NOT EXISTS spectator_sessions (
    player TEXT PRIMARY KEY REFERENCES players(id),
    request TEXT NOT NULL UNIQUE,
    match_id TEXT NOT NULL REFERENCES matches(id),
    spectator_id TEXT UNIQUE,
    address TEXT,
    cancelled INTEGER NOT NULL DEFAULT 0,
    CHECK ((spectator_id IS NULL) = (address IS NULL))
);
CREATE INDEX IF NOT EXISTS spectators_by_match ON spectator_sessions(match_id);

CREATE TABLE IF NOT EXISTS pooled_spectators (
    player TEXT PRIMARY KEY REFERENCES players(id),
    friend TEXT NOT NULL REFERENCES players(id),
    pool TEXT NOT NULL,
    host TEXT NOT NULL,
    match_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    request TEXT NOT NULL UNIQUE,
    address TEXT,
    spectator_id TEXT,
    cancelled INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS pooled_spectators_by_pool ON pooled_spectators(pool);

CREATE TABLE IF NOT EXISTS ce_preferences (
    player TEXT NOT NULL,
    category TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(player, category)
);

CREATE TABLE IF NOT EXISTS ce_metrics (
    player TEXT NOT NULL,
    match_id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(player, match_id)
);

CREATE TABLE IF NOT EXISTS ce_scores (
    player TEXT NOT NULL,
    achievement TEXT NOT NULL,
    score INTEGER NOT NULL,
    PRIMARY KEY(player, achievement)
);

CREATE TABLE IF NOT EXISTS ce_messages (
    id TEXT PRIMARY KEY,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    body TEXT NOT NULL,
    created INTEGER DEFAULT (unixepoch()),
    dismissed INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS ce_recipient ON ce_messages(recipient, dismissed, created);

CREATE TABLE IF NOT EXISTS ce_match_handles (
    room TEXT NOT NULL,
    player TEXT NOT NULL,
    handle TEXT NOT NULL,
    PRIMARY KEY(room, player)
);

CREATE TABLE IF NOT EXISTS ce_honor (
    match_id TEXT NOT NULL,
    source TEXT NOT NULL,
    target TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY(match_id, source, target)
);

CREATE TABLE IF NOT EXISTS ce_pending_gifts (
    player TEXT NOT NULL,
    target TEXT NOT NULL,
    sku TEXT NOT NULL,
    PRIMARY KEY(player, target, sku)
);

CREATE TABLE IF NOT EXISTS ce_wait (
    player TEXT PRIMARY KEY,
    mode TEXT NOT NULL,
    joined INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ce_wait_history (
    mode TEXT PRIMARY KEY,
    seconds REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS ce_provider_orders (
    id TEXT PRIMARY KEY,
    player TEXT NOT NULL,
    request TEXT NOT NULL,
    product TEXT NOT NULL,
    item TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    receipt BLOB UNIQUE,
    UNIQUE(player, request)
);

CREATE TABLE IF NOT EXISTS ce_provider_identity (
    player TEXT PRIMARY KEY,
    valid_until INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ce_migrations (
    name TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS ce_completed (
    player TEXT NOT NULL,
    room TEXT NOT NULL,
    won INTEGER,
    hero TEXT,
    mode TEXT,
    day INTEGER,
    PRIMARY KEY(player, room)
);

CREATE TABLE IF NOT EXISTS ce_player_matches (
    player TEXT PRIMARY KEY,
    completed INTEGER NOT NULL,
    wins INTEGER NOT NULL,
    non_tutorial INTEGER NOT NULL,
    aral INTEGER NOT NULL,
    blitz INTEGER NOT NULL,
    blitz_rounds_casual INTEGER NOT NULL,
    casual INTEGER NOT NULL,
    ranked INTEGER NOT NULL,
    last_won INTEGER,
    streak INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ce_player_matches_ranking
    ON ce_player_matches(completed DESC, player);

CREATE TABLE IF NOT EXISTS ce_daily_wins (
    player TEXT NOT NULL,
    day INTEGER NOT NULL,
    wins INTEGER NOT NULL,
    PRIMARY KEY(player, day)
);

CREATE TABLE IF NOT EXISTS ce_hero_matches (
    player TEXT NOT NULL,
    hero TEXT NOT NULL,
    completed INTEGER NOT NULL,
    wins INTEGER NOT NULL,
    PRIMARY KEY(player, hero)
);
