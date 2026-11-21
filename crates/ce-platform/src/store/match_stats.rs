use diesel::sqlite::SqliteConnection;

use crate::Error;

pub(crate) use halcyon_storage::ce_stats::PlayerMatchStats as PlayerMatches;

pub(crate) fn read_player_matches(
    db: &mut SqliteConnection,
    player: &str,
) -> Result<PlayerMatches, Error> {
    Ok(halcyon_storage::ce_stats::read_player_matches(db, player)?)
}

#[cfg(test)]
mod tests;
