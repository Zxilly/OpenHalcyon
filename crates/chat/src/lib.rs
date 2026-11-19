//! Local XMPP client streams using STARTTLS, SASL PLAIN and resource binding.
use async_trait::async_trait;
use std::io;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Contact {
    pub id: Uuid,
    pub name: String,
}
#[async_trait]
pub trait Directory: Send + Sync + 'static {
    async fn authenticate(&self, user: Uuid, pass: &str) -> Option<String>;
    /// None signals an invalid session. Only authorized contacts belong here.
    async fn contacts(&self, session: &str) -> Option<Vec<Contact>>;
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid XMPP stream")
}
mod server;
mod xml;
pub use server::Server;
