mod accounts;
mod schema;

use diesel::{Connection, connection::SimpleConnection, sqlite::SqliteConnection};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub use accounts::{AccountStore, PasswordAccount};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid SQLite path")]
    Path,
    #[error("could not connect to SQLite: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("SQLite operation failed: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("SQLite migration failed: {0}")]
    Migration(Box<dyn std::error::Error + Send + Sync>),
    #[error("account or session not found")]
    Unauthorized,
    #[error("invalid persisted player identifier")]
    InvalidPlayerId,
    #[error("account already linked")]
    AlreadyLinked,
}

pub type Result<T> = std::result::Result<T, Error>;

/// An address reusable by Diesel and the remaining SQLite callers. A plain
/// in-memory database is private to its platform instance, including when that
/// instance uses multiple connections.
pub struct DatabaseAddress(PathBuf);

impl DatabaseAddress {
    pub fn new(path: &Path) -> Self {
        if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            // The leading slash names a shared memdb VFS store. Its connections
            // get ordinary database locks, so SQLite's busy timeout applies.
            Self(format!("file:/halcyon-{}?vfs=memdb", Uuid::new_v4()).into())
        } else {
            Self(path.to_owned())
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn connect(&self) -> Result<SqliteConnection> {
        let address = sqlite_url(&self.0)?;
        let mut connection = SqliteConnection::establish(&address)?;
        connection.batch_execute("PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")?;
        Ok(connection)
    }
}

fn sqlite_url(path: &Path) -> Result<Cow<'_, str>> {
    if let Some(path) = path.to_str() {
        return Ok(Cow::Borrowed(path));
    }
    #[cfg(unix)]
    {
        use std::{fmt::Write, os::unix::ffi::OsStrExt};
        let mut uri = String::from("file:");
        for &byte in path.as_os_str().as_bytes() {
            match byte {
                0 => return Err(Error::Path),
                b'/' => uri.push('/'),
                value => write!(uri, "%{value:02x}").expect("writing to a String"),
            }
        }
        Ok(Cow::Owned(uri))
    }
    #[cfg(not(unix))]
    Err(Error::Path)
}
