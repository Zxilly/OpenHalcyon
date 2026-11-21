//! Asynchronous port trait decoupling `halcyon-ce-platform` from `halcyon-peer-play`.
use std::{collections::BTreeMap, future::Future, pin::Pin};
use uuid::Uuid;

use crate::{
    BotDifficulty, HostResult, NativeOffer,
    player_actions::{ActionCall, ActionReply},
};

/// Object-safe boxed future used by `PeerPlayPort`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port for federated matchmaking, chat, offer, action, and observation operations.
pub trait PeerPlayPort: Send + Sync + 'static {
    fn player_action<'a>(
        &'a self,
        session: &'a str,
        room: Uuid,
        call: ActionCall,
    ) -> BoxFuture<'a, HostResult<ActionReply>>;

    fn join_party_with_site_report<'a>(
        &'a self,
        token: &'a str,
        mode: String,
        seats: u8,
        difficulty: Option<BotDifficulty>,
        estimates: BTreeMap<Uuid, u32>,
    ) -> BoxFuture<'a, HostResult<()>>;

    fn native_offer<'a>(
        &'a self,
        session: &'a str,
    ) -> BoxFuture<'a, HostResult<Option<NativeOffer>>>;

    fn accept<'a>(&'a self, token: &'a str, room: Uuid) -> BoxFuture<'a, HostResult<()>>;

    fn reject_offer<'a>(&'a self, token: &'a str, room: Uuid) -> BoxFuture<'a, HostResult<bool>>;

    fn leave<'a>(&'a self, token: &'a str) -> BoxFuture<'a, HostResult<()>>;

    fn send_chat<'a>(
        &'a self,
        token: &'a str,
        room: Uuid,
        message: String,
    ) -> BoxFuture<'a, HostResult<()>>;

    fn spectate_friend<'a>(
        &'a self,
        token: &'a str,
        friend: Uuid,
    ) -> BoxFuture<'a, HostResult<bool>>;

    fn leave_spectating<'a>(&'a self, token: &'a str) -> BoxFuture<'a, HostResult<bool>>;
}
