//! Big-endian `binrw` wire definitions for Lobby, Draft, Scene, Roster, and PlayerInfo packets.

pub mod draft_5v5;
mod roster;
mod session;
mod settings;

pub use roster::{
    AralRerollStateBody, AralRoster, DraftPreferenceFlags, DraftRoster, DraftRosterBody, DraftSlot,
    DraftSlotBody, SceneReadinessRow, SceneReadinessRowsBody,
};
pub use session::{
    ConfirmSceneOwnerBody, PlayerInfoBody, PlayerInfoPrefix, PlayerInfoSummaryPrefix,
    PlayerLoadedSceneBody, RegisterPlayerUuidBody,
};
pub use settings::{
    DraftCatalogEntryBody, DraftModeAnnouncementBody, MatchSettingsBody, MatchSettingsPrefix,
    SceneSettingsBody,
};

use crate::codec::invalid;
use crate::{Message, Opcode};
use binrw::{BinRead, BinWrite};
use std::io::{self, Cursor};

pub(super) fn read_bin<T>(body: &[u8], expected_len: usize, reason: &'static str) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    if body.len() != expected_len {
        return Err(invalid(reason));
    }
    let mut cursor = Cursor::new(body);
    let value = T::read_be(&mut cursor).map_err(|_| invalid(reason))?;
    debug_assert_eq!(cursor.position(), expected_len as u64);
    Ok(value)
}

pub(super) fn read_bin_variable<T>(body: &[u8], reason: &'static str) -> io::Result<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    T::read_be(&mut Cursor::new(body)).map_err(|_| invalid(reason))
}

pub(super) fn write_bin<T>(value: &T, dst: &mut [u8], len: usize) -> io::Result<usize>
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let out = dst
        .get_mut(..len)
        .ok_or_else(|| invalid("lobby wire output buffer is too small"))?;
    let mut writer = Cursor::new(out);
    value
        .write_be(&mut writer)
        .map_err(|_| invalid("invalid lobby wire body size"))?;
    debug_assert_eq!(writer.position(), len as u64);
    Ok(len)
}

pub(super) fn encode_bin<T>(opcode: Opcode, value: &T, len: usize) -> Message
where
    T: for<'a> BinWrite<Args<'a> = ()>,
{
    let mut body = vec![0u8; len];
    write_bin(value, &mut body, len).expect("fixed lobby message fits allocated body");
    Message::known(opcode, body)
}

#[cfg(test)]
pub(super) fn assert_rejects<T>(len: usize, f: impl Fn(&[u8]) -> io::Result<T>) {
    assert!(f(&vec![0u8; len.saturating_sub(1)]).is_err());
    assert!(f(&vec![0u8; len + 1]).is_err());
}
