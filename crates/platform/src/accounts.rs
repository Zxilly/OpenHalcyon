//! Password accounts belong to one instance. Linking requires a live session.
use super::{
    AppState, Guest, PlatformError, Result, credential, digest, issue_session, parse_id, player,
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rusqlite::{OptionalExtension, params};
use tokio::sync::Semaphore;
use uuid::Uuid;

// Bound concurrent memory-hard hashes independently of the I/O worker pool.
static HASH_SLOTS: Semaphore = Semaphore::const_new(2);

fn credentials(name: &str, password: &str) -> Result<String> {
    let name = name.trim().to_ascii_lowercase();
    if !(3..=128).contains(&name.len())
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
        || !(8..=256).contains(&password.len())
    {
        return Err(PlatformError::Invalid(
            "invalid account name or password length",
        ));
    }
    Ok(name)
}

pub(super) fn initialize(db: &rusqlite::Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS local_accounts(
        login TEXT PRIMARY KEY, player TEXT NOT NULL UNIQUE REFERENCES players(id), hash TEXT NOT NULL)")?;
    Ok(())
}

impl AppState {
    pub async fn link_account(&self, session: &str, name: &str, password: String) -> Result<()> {
        let login = credentials(name, &password)?;
        let session = digest(session);
        let permit = HASH_SLOTS
            .try_acquire()
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        let hash = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
                .map_err(|_| PlatformError::WorkerUnavailable)?;
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|_| PlatformError::WorkerUnavailable)
        })
        .await
        .map_err(|_| PlatformError::WorkerUnavailable)??;
        self.run(move |s| {
            s.transaction(|tx| {
                let id = player(tx, &session)?;
                let inserted = tx.execute(
                    "INSERT INTO local_accounts VALUES(?,?,?) ON CONFLICT DO NOTHING",
                    params![login, id, hash],
                )?;
                if inserted == 0 {
                    return Err(PlatformError::Conflict("account already linked"));
                }
                Ok(())
            })
        })
        .await
    }

    /// Issue new session and device recovery credentials after password verification.
    pub async fn login_account(&self, name: &str, password: String) -> Result<Guest> {
        let login = credentials(name, &password)?;
        let (id, stored): (String, String) = self
            .run(move |s| {
                let db = s.db.lock().map_err(|_| PlatformError::WorkerUnavailable)?;
                db.query_row(
                    "SELECT player,hash FROM local_accounts WHERE login=?",
                    [login],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(PlatformError::Unauthorized)
            })
            .await?;
        let permit = HASH_SLOTS
            .try_acquire()
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let hash = PasswordHash::new(&stored).map_err(|_| PlatformError::WorkerUnavailable)?;
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .map_err(|_| PlatformError::Unauthorized)
        })
        .await
        .map_err(|_| PlatformError::WorkerUnavailable)??;
        self.run(move |s| {
            s.transaction(|tx| {
                let recovery = credential();
                tx.execute(
                    "UPDATE players SET recovery=? WHERE id=?",
                    params![digest(&recovery), id],
                )?;
                Ok(Guest {
                    player_id: parse_id(&id)?,
                    recovery_token: Some(recovery),
                    session_token: issue_session(tx, &id)?,
                })
            })
        })
        .await
    }
}
