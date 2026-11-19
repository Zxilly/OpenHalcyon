//! SQLite roster operations shared by the party and platform services.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{CommunityError, Result};

pub struct PartyRoster {
    pub id: String,
    pub players: Vec<String>,
}

pub fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS community_party_queues (
            party TEXT PRIMARY KEY, ticket TEXT NOT NULL UNIQUE
         );",
    )?;
    Ok(())
}

pub fn roster(tx: &Transaction<'_>, actor: &str, mode: &str) -> Result<Option<PartyRoster>> {
    let party = tx
        .query_row(
            "SELECT m.group_id, m.role, g.mode FROM community_members m
         JOIN community_groups g ON g.id=m.group_id
         WHERE m.player=? AND m.kind='party'",
            [actor],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((id, role, configured_mode)) = party else {
        return Ok(None);
    };
    if role != "owner" {
        return Err(CommunityError::Forbidden);
    }
    if !configured_mode.is_empty() && configured_mode != mode {
        return Err(CommunityError::Conflict(
            "requested queue differs from the party mode",
        ));
    }
    let mut query = tx.prepare(
        "SELECT player FROM community_members WHERE group_id=?
         ORDER BY role='owner' DESC, rowid",
    )?;
    let players = query
        .query_map([&id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(PartyRoster { id, players }))
}

pub fn claim(tx: &Transaction<'_>, party: &str, ticket: &str) -> Result<()> {
    ensure_unlocked(tx, party)?;
    tx.execute(
        "INSERT INTO community_party_queues(party,ticket) VALUES(?,?)",
        params![party, ticket],
    )?;
    Ok(())
}

pub fn release(tx: &Transaction<'_>, ticket: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM community_party_queues WHERE ticket=?",
        [ticket],
    )?;
    Ok(())
}

pub(crate) fn ensure_unlocked(tx: &Transaction<'_>, party: &str) -> Result<()> {
    let claimed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM community_party_queues WHERE party=?)",
        [party],
        |row| row.get(0),
    )?;
    if claimed {
        return Err(CommunityError::Conflict(
            "party is already queued or playing",
        ));
    }
    Ok(())
}
