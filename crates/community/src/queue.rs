//! SQLite roster operations shared by the party and platform services.

use diesel::sqlite::SqliteConnection;
use halcyon_storage::community_queues::{self, RosterStatus};

use crate::{CommunityError, Result};

pub use halcyon_storage::community_queues::PartyRoster;

pub fn roster(db: &mut SqliteConnection, actor: &str, mode: &str) -> Result<Option<PartyRoster>> {
    match community_queues::roster(db, actor, mode)? {
        RosterStatus::Ok(roster) => Ok(roster),
        RosterStatus::NotOwner => Err(CommunityError::Forbidden),
        RosterStatus::ModeMismatch => Err(CommunityError::Conflict(
            "requested queue differs from the party mode",
        )),
    }
}

pub fn claim(db: &mut SqliteConnection, party: &str, ticket: &str) -> Result<()> {
    if !community_queues::claim(db, party, ticket)? {
        return Err(CommunityError::Conflict(
            "party is already queued or playing",
        ));
    }
    Ok(())
}

pub fn release(db: &mut SqliteConnection, ticket: &str) -> Result<()> {
    community_queues::release(db, ticket)?;
    Ok(())
}
