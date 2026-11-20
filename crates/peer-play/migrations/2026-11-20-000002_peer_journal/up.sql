CREATE TABLE IF NOT EXISTS peer_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    pool TEXT NOT NULL,
    node TEXT NOT NULL,
    sequence INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS hosted (
    id TEXT PRIMARY KEY,
    address TEXT NOT NULL,
    result TEXT,
    delivered INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS hosted_outbox ON hosted(id)
    WHERE result IS NOT NULL AND delivered = 0;
CREATE INDEX IF NOT EXISTS hosted_unsettled ON hosted(id)
    WHERE result IS NULL;
CREATE TABLE IF NOT EXISTS outgoing_chat (
    id TEXT NOT NULL,
    home TEXT NOT NULL,
    body TEXT NOT NULL,
    expires INTEGER NOT NULL DEFAULT (unixepoch() + 120),
    next_attempt INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (id, home)
);
CREATE INDEX IF NOT EXISTS outgoing_chat_due ON outgoing_chat(next_attempt);
CREATE TABLE IF NOT EXISTS enqueue_payloads (
    id TEXT PRIMARY KEY,
    payload TEXT NOT NULL
);
