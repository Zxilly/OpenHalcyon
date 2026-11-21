//! In-process room hosting with CE control and game TCP connections.

mod abilities;
mod host;
mod modes;
mod replication;
mod room;
mod transport;

#[cfg(test)]
mod tests;

pub use host::admin_views::{
    RoomEndReason, RoomParticipant, RoomPhase, RoomSummary, TickLatencyReport,
};
pub use host::admission::{AdmissionPolicy, PortRange, ResourceMetrics};
pub use host::manager::{Rooms, TerminateError};

pub(crate) use halcyon_protocol::{Message, Opcode, wire_opcode};
pub(crate) use host::manager::Shared;
pub(crate) use replication::Batch;
#[cfg(test)]
pub(crate) use room::Peer;
pub(crate) use room::{Pick, Room, Seat};
