use crate::{
    DatabaseAddress, Error, Result,
    schema::{local_accounts, players, sessions},
};
use diesel::sql_types::Text;
use diesel::{dsl::exists, prelude::*, sqlite::SqliteConnection, upsert::excluded};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

diesel::define_sql_function!(fn unixepoch() -> BigInt);
diesel::define_sql_function!(#[sql_name = "unixepoch"] fn unixepoch_modified(base: Text, modifier: Text) -> BigInt);

#[derive(Queryable, Selectable)]
#[diesel(table_name = local_accounts)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct PasswordAccount {
    pub login: String,
    pub player: String,
    pub hash: String,
}

pub struct AccountStore(SqliteConnection);

impl AccountStore {
    pub fn open(address: &DatabaseAddress) -> Result<Self> {
        let mut db = address.connect()?;
        db.immediate_transaction(|db| {
            db.run_pending_migrations(MIGRATIONS)
                .map_err(Error::Migration)?;
            Ok::<(), Error>(())
        })?;
        Ok(Self(db))
    }

    pub fn link(&mut self, session_key: &[u8], login: &str, hash: &str) -> Result<()> {
        self.0.immediate_transaction(|db| {
            let player = sessions::table
                .find(session_key)
                .select(sessions::player)
                .first::<String>(db)
                .optional()?
                .ok_or(Error::Unauthorized)?;
            diesel::update(
                players::table
                    .find(&player)
                    .filter(players::last_seen.lt(unixepoch_modified("now", "-15 seconds"))),
            )
            .set(players::last_seen.eq(unixepoch()))
            .execute(db)?;
            let count = diesel::insert_into(local_accounts::table)
                .values((
                    local_accounts::login.eq(login),
                    local_accounts::player.eq(&player),
                    local_accounts::hash.eq(hash),
                ))
                .on_conflict_do_nothing()
                .execute(db)?;
            if count == 1 {
                Ok(())
            } else {
                Err(Error::AlreadyLinked)
            }
        })
    }

    pub fn password(&mut self, login: &str) -> Result<PasswordAccount> {
        local_accounts::table
            .find(login)
            .select(PasswordAccount::as_select())
            .first(&mut self.0)
            .optional()?
            .ok_or(Error::Unauthorized)
    }

    /// Rotate both credentials in one transaction, conditioned on the same
    /// password record that the caller verified outside the database lock.
    pub fn rotate(
        &mut self,
        account: &PasswordAccount,
        recovery_key: &[u8],
        session_key: &[u8],
    ) -> Result<()> {
        self.0.immediate_transaction(|db| {
            let verified = diesel::select(exists(
                local_accounts::table
                    .find(&account.login)
                    .filter(local_accounts::player.eq(&account.player))
                    .filter(local_accounts::hash.eq(&account.hash)),
            ))
            .get_result::<bool>(db)?;
            if !verified {
                return Err(Error::Unauthorized);
            }
            let count = diesel::update(players::table.find(&account.player))
                .set((
                    players::recovery.eq(recovery_key),
                    players::last_seen.eq(unixepoch()),
                ))
                .execute(db)?;
            if count != 1 {
                return Err(Error::Unauthorized);
            }
            diesel::insert_into(sessions::table)
                .values((
                    sessions::key.eq(session_key),
                    sessions::player.eq(&account.player),
                ))
                .on_conflict(sessions::player)
                .do_update()
                .set(sessions::key.eq(excluded(sessions::key)))
                .execute(db)?;
            Ok(())
        })
    }
}
