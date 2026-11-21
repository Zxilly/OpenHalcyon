use std::{path::Path, sync::Arc};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use halcyon_platform::{
    AppState, HostResult, HostedMatch, MatchHost, PlatformConfig, PlatformError, Reservation,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PASSWORD: &str = "local fixture password";
static HASH_CASES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn mixed_in_memory_writers_use_sqlites_retryable_database_lock() {
    let address = halcyon_storage::DatabaseAddress::new(Path::new(":memory:"));
    let mut first = Connection::open(address.path()).unwrap();
    let second = Connection::open(address.path()).unwrap();
    second.busy_timeout(std::time::Duration::ZERO).unwrap();
    let _orm = halcyon_storage::AccountStore::open(&address).unwrap();
    let writer = first
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let error = second.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    writer.commit().unwrap();
    second.execute_batch("BEGIN IMMEDIATE; COMMIT").unwrap();
}

struct NoHost;
impl MatchHost for NoHost {
    fn reserve(&self, _: Reservation) -> HostResult<HostedMatch> {
        panic!("account storage does not allocate matches")
    }
}

fn app(path: &Path) -> AppState {
    AppState::open(
        PlatformConfig {
            database_path: path.into(),
            public_address: "localhost".into(),
            seats_per_match: 1,
        },
        Arc::new(NoHost),
    )
    .unwrap()
}

fn digest(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}

#[tokio::test]
async fn accounts_adopt_existing_rows_rotate_credentials_and_share_uniqueness() {
    let _exclusive = HASH_CASES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("platform.sqlite");
    let old = Connection::open(&path).unwrap();
    old.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE players (id TEXT PRIMARY KEY, name TEXT NOT NULL,
           recovery BLOB NOT NULL UNIQUE, last_seen INTEGER NOT NULL DEFAULT (unixepoch()));
         CREATE TABLE sessions (key BLOB PRIMARY KEY, player TEXT NOT NULL UNIQUE REFERENCES players(id));
         CREATE TABLE local_accounts (login TEXT PRIMARY KEY,
           player TEXT NOT NULL UNIQUE REFERENCES players(id), hash TEXT NOT NULL);"
    ).unwrap();
    let id = Uuid::new_v4();
    let hash = Argon2::default()
        .hash_password(
            PASSWORD.as_bytes(),
            &SaltString::encode_b64(b"legacy-account-fixture").unwrap(),
        )
        .unwrap()
        .to_string();
    let session = "legacy-session";
    let recovery = "legacy-recovery";
    old.execute(
        "INSERT INTO players (id,name,recovery) VALUES (?, ?, ?)",
        params![id.to_string(), "Legacy", digest(recovery)],
    )
    .unwrap();
    old.execute(
        "INSERT INTO sessions VALUES (?, ?)",
        params![digest(session), id.to_string()],
    )
    .unwrap();
    old.execute(
        "INSERT INTO local_accounts VALUES (?, ?, ?)",
        params!["legacy", id.to_string(), hash],
    )
    .unwrap();

    let first = app(&path);
    let second = app(&path);
    assert_eq!(first.profile(session).await.unwrap().player_id, id);
    assert!(matches!(
        first
            .login_account("legacy", "incorrect password".into())
            .await,
        Err(PlatformError::Unauthorized)
    ));
    assert_eq!(second.profile(session).await.unwrap().player_id, id);
    let restored = second.resume_guest(recovery.into()).await.unwrap();
    assert_eq!(restored.player_id, id);
    assert!(matches!(
        first.profile(session).await,
        Err(PlatformError::Unauthorized)
    ));
    let newer = first
        .login_account(" LEGACY ", PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(newer.player_id, id);
    assert!(matches!(
        second.profile(&restored.session_token).await,
        Err(PlatformError::Unauthorized)
    ));
    assert!(matches!(
        second.resume_guest(recovery.into()).await,
        Err(PlatformError::Unauthorized)
    ));
    assert_eq!(
        second
            .profile(&newer.session_token)
            .await
            .unwrap()
            .player_id,
        id
    );
    assert_eq!(
        second
            .resume_guest(newer.recovery_token.unwrap())
            .await
            .unwrap()
            .player_id,
        id
    );
    assert_eq!(
        old.query_row(
            "SELECT hash FROM local_accounts WHERE login='legacy'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        hash
    );
    assert_eq!(
        old.query_row("SELECT count(*) FROM __diesel_schema_migrations", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );

    let participant_a = first.create_guest("First".into()).await.unwrap();
    let participant_b = second.create_guest("Second".into()).await.unwrap();
    let (a, b) = tokio::join!(
        first.link_account(&participant_a.session_token, " shared ", PASSWORD.into()),
        second.link_account(&participant_b.session_token, "SHARED", PASSWORD.into())
    );
    let winner = match (a, b) {
        (Ok(()), Err(PlatformError::Conflict(_))) => participant_a,
        (Err(PlatformError::Conflict(_)), Ok(())) => participant_b,
        other => panic!("expected one account winner, got {other:?}"),
    };
    assert!(matches!(
        second
            .link_account(&winner.session_token, "another", PASSWORD.into())
            .await,
        Err(PlatformError::Conflict(_))
    ));
    assert_eq!(
        first
            .login_account("shared", PASSWORD.into())
            .await
            .unwrap()
            .player_id,
        winner.player_id
    );
}

#[tokio::test]
async fn separate_in_memory_instances_share_their_own_orm_tables_only() {
    let _exclusive = HASH_CASES.lock().await;
    let path = Path::new(":memory:");
    let a = app(path);
    let b = app(path);
    let guest = a.create_guest("isolated".into()).await.unwrap();
    let passwd = "separate isolation password";
    a.link_account(&guest.session_token, "isolated", passwd.into())
        .await
        .unwrap();
    assert!(matches!(
        b.login_account("isolated", passwd.into()).await,
        Err(PlatformError::Unauthorized)
    ));
    let confirmed = a.login_account("isolated", passwd.into()).await.unwrap();
    assert_eq!(confirmed.player_id, guest.player_id);
    assert_eq!(
        a.profile(&confirmed.session_token).await.unwrap().player_id,
        guest.player_id
    );
}

#[tokio::test]
async fn credential_rotation_rolls_back_recovery_on_session_collision() {
    let _exclusive = HASH_CASES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollback.sqlite");
    let app = app(&path);
    let a = app.create_guest("First".into()).await.unwrap();
    let b = app.create_guest("Second".into()).await.unwrap();
    app.link_account(&a.session_token, "rollback", PASSWORD.into())
        .await
        .unwrap();
    let mut account =
        halcyon_storage::AccountStore::open(&halcyon_storage::DatabaseAddress::new(&path)).unwrap();
    let stored = account.password("rollback").unwrap();
    assert!(matches!(
        account.rotate(
            &stored,
            &digest("new but uncommitted"),
            &digest(&b.session_token)
        ),
        Err(halcyon_storage::Error::Query(_))
    ));
    assert_eq!(
        app.profile(&a.session_token).await.unwrap().player_id,
        a.player_id
    );
    assert_eq!(
        app.profile(&b.session_token).await.unwrap().player_id,
        b.player_id
    );
    assert_eq!(
        app.resume_guest(a.recovery_token.unwrap())
            .await
            .unwrap()
            .player_id,
        a.player_id
    );
}

#[tokio::test]
async fn guest_recovery_retries_keep_one_identity_and_only_the_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guest.sqlite");
    let created = app(&path).create_guest("Fresh".into()).await.unwrap();
    let recovery = created.recovery_token.unwrap();
    let first = app(&path);
    let second = app(&path);
    let (a, b) = tokio::join!(
        first.resume_guest(recovery.clone()),
        second.resume_guest(recovery.clone()),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(
        (a.player_id, b.player_id),
        (created.player_id, created.player_id)
    );
    assert!(a.recovery_token.is_none() && b.recovery_token.is_none());
    let current_a = first.profile(&a.session_token).await.is_ok();
    let current_b = second.profile(&b.session_token).await.is_ok();
    assert_ne!(current_a, current_b);
    assert!(matches!(
        first.profile(&created.session_token).await,
        Err(PlatformError::Unauthorized)
    ));

    let active = if current_a {
        a.session_token
    } else {
        b.session_token
    };
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM players", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM sessions", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    db.execute("UPDATE players SET last_seen=1", []).unwrap();
    assert!(
        !first
            .player_activity(created.player_id)
            .await
            .unwrap()
            .unwrap()
            .online
    );
    first.rename(&active, "Renamed".into()).await.unwrap();
    let active_view = second
        .player_activity(created.player_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active_view.profile.name, "Renamed");
    assert!(active_view.online && active_view.last_seen > 1);
    first.end_session(&active).await.unwrap();
    first.end_session(&active).await.unwrap();
    assert!(
        !second
            .player_activity(created.player_id)
            .await
            .unwrap()
            .unwrap()
            .online
    );
    let retry = second.resume_guest(recovery).await.unwrap();
    assert_eq!(retry.player_id, created.player_id);
    assert_eq!(
        first.profile(&retry.session_token).await.unwrap().name,
        "Renamed"
    );
}

#[tokio::test]
async fn guest_creation_rolls_back_identity_if_session_key_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guest-collision.sqlite");
    let state = app(&path);
    let existing = state.create_guest("Existing".into()).await.unwrap();
    let mut storage =
        halcyon_storage::AccountStore::open(&halcyon_storage::DatabaseAddress::new(&path)).unwrap();
    let id = Uuid::new_v4();
    assert!(matches!(
        storage.create_guest(
            id,
            "Second",
            &digest("uncommitted"),
            &digest(&existing.session_token)
        ),
        Err(halcyon_storage::Error::Query(_))
    ));
    assert!(state.player_activity(id).await.unwrap().is_none());
    assert!(matches!(
        state.resume_guest("uncommitted".into()).await,
        Err(PlatformError::Unauthorized)
    ));
    assert_eq!(
        state
            .profile(&existing.session_token)
            .await
            .unwrap()
            .player_id,
        existing.player_id
    );
}
