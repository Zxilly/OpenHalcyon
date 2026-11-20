-- The platform already owns and initializes players and sessions during this migration phase.
-- Both new and legacy installs keep the same SQLite table without rewriting stored credentials.
CREATE TABLE IF NOT EXISTS local_accounts (
    login TEXT PRIMARY KEY,
    player TEXT NOT NULL UNIQUE REFERENCES players(id),
    hash TEXT NOT NULL
);
