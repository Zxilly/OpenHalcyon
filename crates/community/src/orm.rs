use crate::{CommunityError, Result, schema::community_players};
use diesel::{dsl::exists, prelude::*, sql_types::Text};
use diesel_migrations::{EmbeddedMigrations, embed_migrations};

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

diesel::define_sql_function!(fn unixepoch() -> BigInt);
diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_after(base: Text, modifier: Text) -> BigInt);
pub(crate) fn nocase_handle() -> diesel::expression::SqlLiteral<Text> {
    diesel::dsl::sql("community_players.handle COLLATE NOCASE")
}

pub(crate) fn ensure_player(db: &mut SqliteConnection, actor: &str) -> Result<()> {
    if diesel::select(exists(community_players::table.find(actor))).get_result(db)? {
        Ok(())
    } else {
        Err(CommunityError::Missing("player does not exist"))
    }
}

pub(crate) fn connection_error(error: impl std::fmt::Display) -> CommunityError {
    tracing::error!(%error, "community ORM storage failed");
    CommunityError::WorkerUnavailable
}

impl From<diesel::result::Error> for CommunityError {
    fn from(error: diesel::result::Error) -> Self {
        connection_error(error)
    }
}
