-- Populated tables adopted from the pre-Diesel community store.
CREATE TABLE IF NOT EXISTS community_players (
   id TEXT PRIMARY KEY, handle TEXT NOT NULL, online INTEGER NOT NULL,
   invisible INTEGER NOT NULL DEFAULT 0, receive_presence INTEGER NOT NULL DEFAULT 1,
   status TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE IF NOT EXISTS community_groups (
   id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
   tag TEXT NOT NULL DEFAULT '', motto TEXT NOT NULL DEFAULT '',
   mode TEXT NOT NULL DEFAULT '', invite_only INTEGER NOT NULL DEFAULT 1,
   lobby_open INTEGER NOT NULL DEFAULT 0, code TEXT UNIQUE,
   created INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE IF NOT EXISTS community_members (
   group_id TEXT NOT NULL REFERENCES community_groups(id) ON DELETE CASCADE,
   kind TEXT NOT NULL, player TEXT NOT NULL REFERENCES community_players(id),
   role TEXT NOT NULL, side INTEGER NOT NULL DEFAULT 0,
   PRIMARY KEY(group_id, player), UNIQUE(kind, player)
);
CREATE TABLE IF NOT EXISTS community_items (
   id TEXT PRIMARY KEY, slot TEXT NOT NULL, name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS community_inventory (
   player TEXT NOT NULL REFERENCES community_players(id),
   item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL CHECK(count > 0),
   PRIMARY KEY(player, item)
);
CREATE TABLE IF NOT EXISTS community_item_grants (
   id TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES community_players(id),
   item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL
);
