CREATE TABLE IF NOT EXISTS community_players (
    id TEXT PRIMARY KEY, handle TEXT NOT NULL, online INTEGER NOT NULL,
    invisible INTEGER NOT NULL DEFAULT 0, receive_presence INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS community_handles ON community_players(handle COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS community_friend_requests (
    requester TEXT NOT NULL REFERENCES community_players(id),
    recipient TEXT NOT NULL REFERENCES community_players(id),
    created INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(requester, recipient)
);
CREATE TABLE IF NOT EXISTS community_friends (
    player TEXT NOT NULL REFERENCES community_players(id),
    friend TEXT NOT NULL REFERENCES community_players(id),
    favorite INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(player, friend)
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
CREATE TABLE IF NOT EXISTS community_invites (
    group_id TEXT NOT NULL REFERENCES community_groups(id) ON DELETE CASCADE,
    player TEXT NOT NULL REFERENCES community_players(id),
    inviter TEXT NOT NULL REFERENCES community_players(id),
    created INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(group_id, player)
);
CREATE TABLE IF NOT EXISTS community_items (
    id TEXT PRIMARY KEY, slot TEXT NOT NULL, name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS community_inventory (
    player TEXT NOT NULL REFERENCES community_players(id),
    item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL CHECK(count > 0),
    PRIMARY KEY(player, item)
);
CREATE TABLE IF NOT EXISTS community_equipment (
    player TEXT NOT NULL REFERENCES community_players(id),
    slot TEXT NOT NULL, item TEXT NOT NULL REFERENCES community_items(id),
    PRIMARY KEY(player, slot)
);
CREATE TABLE IF NOT EXISTS community_item_grants (
    id TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES community_players(id),
    item TEXT NOT NULL REFERENCES community_items(id), count INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS community_quests (
    id TEXT PRIMARY KEY, category TEXT NOT NULL, title TEXT NOT NULL,
    goal INTEGER NOT NULL CHECK(goal > 0), skippable INTEGER NOT NULL,
    reward_item TEXT REFERENCES community_items(id),
    reward_count INTEGER
);
CREATE TABLE IF NOT EXISTS community_quest_states (
    player TEXT NOT NULL REFERENCES community_players(id),
    quest TEXT NOT NULL REFERENCES community_quests(id), progress INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'active', PRIMARY KEY(player, quest)
);
CREATE TABLE IF NOT EXISTS community_quest_progress (
    id TEXT PRIMARY KEY, player TEXT NOT NULL REFERENCES community_players(id),
    quest TEXT NOT NULL REFERENCES community_quests(id), amount INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS community_party_queues (
    party TEXT PRIMARY KEY, ticket TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS community_redemptions(
    player TEXT NOT NULL REFERENCES community_players(id),
    operation TEXT NOT NULL, sku TEXT NOT NULL, request BLOB NOT NULL,
    PRIMARY KEY(player, request));
CREATE INDEX IF NOT EXISTS redemptions_by_offer ON community_redemptions(player,operation,sku);
