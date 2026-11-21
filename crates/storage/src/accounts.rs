use crate::{
    DatabaseAddress, Error, Result,
    migrations::ensure_schema,
    schema::{events, local_accounts, notification_events, players, sessions},
};
use diesel::sql_types::Text;
use diesel::{dsl::exists, prelude::*, sqlite::SqliteConnection, upsert::excluded};
use uuid::Uuid;

diesel::define_sql_function!(fn total_changes() -> diesel::sql_types::Integer);
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
        ensure_schema(&mut db)?;
        Ok(Self(db))
    }

    /// Read the next notification in the authenticated player's cursor space.
    /// The session and both cursor reads share one SQLite read snapshot.
    pub fn next_notification(
        &mut self,
        session_key: &[u8],
        after: i64,
        initial: bool,
    ) -> Result<Option<(i64, String)>> {
        self.0.transaction(|db| {
            let player = session_owner(db, session_key)?;
            let latest = notification_events::table
                .filter(notification_events::player.eq(&player))
                .select(diesel::dsl::max(notification_events::ordinal))
                .get_result::<Option<i64>>(db)?
                .unwrap_or(0);
            // Initial loads and cursors from an older deployment read its newest notification.
            let after = if initial || after > latest {
                latest.saturating_sub(1)
            } else {
                after
            };
            notification_events::table
                .inner_join(events::table)
                .filter(notification_events::player.eq(&player))
                .filter(notification_events::ordinal.gt(after))
                .order(notification_events::ordinal)
                .select((notification_events::ordinal, events::body))
                .first(db)
                .optional()
                .map_err(Into::into)
        })
    }

    /// Run a deferred read on one SQLite snapshot.
    pub fn read<T, E>(
        &mut self,
        read: impl FnOnce(&mut SqliteConnection) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        self.0.transaction(read)
    }

    /// Run an unauthenticated platform mutation in an immediate SQLite transaction.
    pub fn write<T, E>(
        &mut self,
        apply: impl FnOnce(&mut SqliteConnection) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        self.0.immediate_transaction(apply)
    }

    /// Run an unauthenticated mutation and report whether any SQLite rows changed.
    pub fn write_observing_changes<T, E>(
        &mut self,
        apply: impl FnOnce(&mut SqliteConnection) -> std::result::Result<T, E>,
    ) -> std::result::Result<(T, bool), E>
    where
        E: From<diesel::result::Error>,
    {
        let before = diesel::select(total_changes()).get_result::<i32>(&mut self.0)?;
        let value = self.0.immediate_transaction(apply)?;
        let after = diesel::select(total_changes()).get_result::<i32>(&mut self.0)?;
        Ok((value, after != before))
    }

    /// Run a deferred read on the same SQLite snapshot as session validation.
    pub fn read_as<T, E>(
        &mut self,
        session_key: &[u8],
        read: impl FnOnce(&mut SqliteConnection, &str) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: From<Error> + From<diesel::result::Error>,
    {
        self.read(|db| {
            let player = session_owner(db, session_key)?;
            read(db, &player)
        })
    }

    /// Run a platform mutation on the same connection as its session check.
    pub fn write_as<T, E>(
        &mut self,
        session_key: &[u8],
        apply: impl FnOnce(&mut SqliteConnection, &str) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: From<Error> + From<diesel::result::Error>,
    {
        self.write(|db| {
            let player = session_owner(db, session_key)?;
            apply(db, &player)
        })
    }

    pub fn link(&mut self, session_key: &[u8], login: &str, hash: &str) -> Result<()> {
        self.0.immediate_transaction(|db| {
            let player = session_owner(db, session_key)?;
            touch_presence(db, &player)?;
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
                .set(players::recovery.eq(recovery_key))
                .execute(db)?;
            if count != 1 {
                return Err(Error::Unauthorized);
            }
            issue_session(db, &account.player, session_key)
        })
    }

    pub fn create_guest(
        &mut self,
        player: Uuid,
        name: &str,
        recovery_key: &[u8],
        session_key: &[u8],
    ) -> Result<()> {
        let player = player.to_string();
        self.0.immediate_transaction(|db| {
            diesel::insert_into(players::table)
                .values((
                    players::id.eq(&player),
                    players::name.eq(name),
                    players::recovery.eq(recovery_key),
                ))
                .execute(db)?;
            issue_session(db, &player, session_key)
        })
    }

    pub fn resume_guest(&mut self, recovery_key: &[u8], session_key: &[u8]) -> Result<Uuid> {
        self.0.immediate_transaction(|db| {
            let player = players::table
                .filter(players::recovery.eq(recovery_key))
                .select(players::id)
                .first::<String>(db)
                .optional()?
                .ok_or(Error::Unauthorized)?;
            let id = Uuid::parse_str(&player).map_err(|_| Error::InvalidPlayerId)?;
            issue_session(db, &player, session_key)?;
            Ok(id)
        })
    }

    pub fn rename(&mut self, session_key: &[u8], name: &str) -> Result<()> {
        self.0.immediate_transaction(|db| {
            let player = session_owner(db, session_key)?;
            touch_presence(db, &player)?;
            diesel::update(players::table.find(&player))
                .set(players::name.eq(name))
                .execute(db)?;
            Ok(())
        })
    }

    /// Returns whether a live session was actually removed.
    pub fn end_session(&mut self, session_key: &[u8]) -> Result<bool> {
        let count = diesel::delete(sessions::table.find(session_key)).execute(&mut self.0)?;
        Ok(count != 0)
    }
}

pub fn session_owner(db: &mut SqliteConnection, key: &[u8]) -> Result<String> {
    sessions::table
        .find(key)
        .select(sessions::player)
        .first::<String>(db)
        .optional()?
        .ok_or(Error::Unauthorized)
}

pub fn touch_presence(db: &mut SqliteConnection, player: &str) -> Result<()> {
    diesel::update(
        players::table
            .find(player)
            .filter(players::last_seen.lt(unixepoch_modified("now", "-15 seconds"))),
    )
    .set(players::last_seen.eq(unixepoch()))
    .execute(db)?;
    Ok(())
}

fn issue_session(db: &mut SqliteConnection, player: &str, key: &[u8]) -> Result<()> {
    diesel::insert_into(sessions::table)
        .values((sessions::key.eq(key), sessions::player.eq(player)))
        .on_conflict(sessions::player)
        .do_update()
        .set(sessions::key.eq(excluded(sessions::key)))
        .execute(db)?;
    diesel::update(players::table.find(player))
        .set(players::last_seen.eq(unixepoch()))
        .execute(db)?;
    Ok(())
}
