use crate::{
    Result,
    schema::{
        community_friends, community_groups, community_members, community_party_queues,
        community_players,
    },
};
use diesel::{
    dsl::{exists, sql},
    prelude::*,
    sql_types::Bool,
    sqlite::SqliteConnection,
};

pub struct PartyRoster {
    pub id: String,
    pub players: Vec<String>,
}

pub enum RosterStatus {
    Ok(Option<PartyRoster>),
    NotOwner,
    ModeMismatch,
}

pub fn roster(db: &mut SqliteConnection, actor: &str, mode: &str) -> Result<RosterStatus> {
    let party = community_members::table
        .inner_join(community_groups::table)
        .filter(community_members::player.eq(actor))
        .filter(community_members::kind.eq("party"))
        .select((
            community_members::group_id,
            community_members::role,
            community_groups::mode,
        ))
        .first::<(String, String, String)>(db)
        .optional()?;
    let Some((id, role, configured_mode)) = party else {
        return Ok(RosterStatus::Ok(None));
    };
    if role != "owner" {
        return Ok(RosterStatus::NotOwner);
    }
    if !configured_mode.is_empty() && configured_mode != mode {
        return Ok(RosterStatus::ModeMismatch);
    }
    let players = community_members::table
        .filter(community_members::group_id.eq(&id))
        .order((
            sql::<Bool>("community_members.role = 'owner'").desc(),
            sql::<diesel::sql_types::BigInt>("community_members.rowid"),
        ))
        .select(community_members::player)
        .load::<String>(db)?;
    Ok(RosterStatus::Ok(Some(PartyRoster { id, players })))
}

pub fn is_claimed(db: &mut SqliteConnection, party: &str) -> Result<bool> {
    Ok(diesel::select(exists(community_party_queues::table.find(party))).get_result(db)?)
}

pub fn claim(db: &mut SqliteConnection, party: &str, ticket: &str) -> Result<bool> {
    if is_claimed(db, party)? {
        return Ok(false);
    }
    diesel::insert_into(community_party_queues::table)
        .values((
            community_party_queues::party.eq(party),
            community_party_queues::ticket.eq(ticket),
        ))
        .execute(db)?;
    Ok(true)
}

pub fn release(db: &mut SqliteConnection, ticket: &str) -> Result<()> {
    diesel::delete(community_party_queues::table.filter(community_party_queues::ticket.eq(ticket)))
        .execute(db)?;
    Ok(())
}

pub fn may_spectate(db: &mut SqliteConnection, viewer: &str, friend: &str) -> Result<bool> {
    Ok(diesel::select(exists(
        community_friends::table
            .inner_join(community_players::table)
            .filter(community_friends::player.eq(viewer))
            .filter(community_friends::friend.eq(friend))
            .filter(community_players::invisible.eq(false)),
    ))
    .get_result(db)?)
}
