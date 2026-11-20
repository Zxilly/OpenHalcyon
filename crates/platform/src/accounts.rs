//! Password accounts belong to one instance. Linking requires a live session.
use super::{AppState, Guest, PlatformError, Result, credential, digest, parse_id};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use halcyon_storage::Error as StorageError;
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

pub(super) fn storage_error(error: StorageError) -> PlatformError {
    match error {
        StorageError::Unauthorized => PlatformError::Unauthorized,
        StorageError::AlreadyLinked => PlatformError::Conflict("account already linked"),
        independent => {
            tracing::error!(error = %independent, "account storage failed");
            PlatformError::WorkerUnavailable
        }
    }
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
            s.accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .link(&session, &login, &hash)
                .map_err(storage_error)?;
            s.changes.send_replace(());
            Ok(())
        })
        .await
    }

    /// Issue new session and device recovery credentials after password verification.
    pub async fn login_account(&self, name: &str, password: String) -> Result<Guest> {
        let login = credentials(name, &password)?;
        let account = self
            .run(move |s| {
                s.accounts
                    .lock()
                    .map_err(|_| PlatformError::WorkerUnavailable)?
                    .password(&login)
                    .map_err(storage_error)
            })
            .await?;
        let permit = HASH_SLOTS
            .try_acquire()
            .map_err(|_| PlatformError::WorkerUnavailable)?;
        let stored = account.hash.clone();
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
            let player_id = parse_id(&account.player)?;
            let recovery = credential();
            let session = credential();
            s.accounts
                .lock()
                .map_err(|_| PlatformError::WorkerUnavailable)?
                .rotate(&account, &digest(&recovery), &digest(&session))
                .map_err(storage_error)?;
            s.changes.send_replace(());
            Ok(Guest {
                player_id,
                recovery_token: Some(recovery),
                session_token: session,
            })
        })
        .await
    }
}
