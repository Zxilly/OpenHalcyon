//! Scoreboard AFK and deserter flags. Reconnection clears both flags.
use crate::{Message, Opcode, fixed_body};
use binrw::{BinRead, BinWrite};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorFlagChange {
    Afk,
    Deserter,
    Reconnected,
}

impl ActorFlagChange {
    pub const fn opcode(self) -> Opcode {
        match self {
            Self::Afk => Opcode::PlayerAfk,
            Self::Deserter => Opcode::PlayerDeserter,
            Self::Reconnected => Opcode::PlayerReconnected,
        }
    }

    pub const fn from_opcode(opcode: Opcode) -> Option<Self> {
        match opcode {
            Opcode::PlayerAfk => Some(Self::Afk),
            Opcode::PlayerDeserter => Some(Self::Deserter),
            Opcode::PlayerReconnected => Some(Self::Reconnected),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)]
struct ActorFlagWire {
    actor: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFlags<'a> {
    pub actor: u32,
    pub change: ActorFlagChange,
    padding: &'a [u8],
}

impl Message {
    pub fn actor_flags(&self) -> Option<ActorFlags<'_>> {
        let change = ActorFlagChange::from_opcode(self.known_opcode()?)?;
        let (wire, padding): (ActorFlagWire, _) =
            fixed_body::read_with_padding(&self.body, 4).ok()?;
        Some(ActorFlags {
            actor: wire.actor,
            change,
            padding,
        })
    }
}

impl ActorFlags<'_> {
    pub fn new(actor: u32, change: ActorFlagChange) -> Self {
        Self {
            actor,
            change,
            padding: &[],
        }
    }

    pub fn to_message(self) -> Message {
        fixed_body::message_with_padding(
            self.change.opcode(),
            &ActorFlagWire { actor: self.actor },
            4,
            self.padding,
        )
        .expect("actor flag padding validated at decode")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_both_setters_and_the_shared_clear() {
        for (opcode, change) in [
            (crate::wire_opcode::PlayerAfk, ActorFlagChange::Afk),
            (
                crate::wire_opcode::PlayerDeserter,
                ActorFlagChange::Deserter,
            ),
            (
                crate::wire_opcode::PlayerReconnected,
                ActorFlagChange::Reconnected,
            ),
        ] {
            for padding in [&[][..], &[0x35, 0x78]] {
                let message = Message::new(opcode, [&1502u32.to_be_bytes()[..], padding].concat());
                let flags = message.actor_flags().unwrap();
                assert_eq!((flags.actor, flags.change), (1502, change));
                assert_eq!(flags.to_message(), message);
            }
        }
        assert!(
            Message::known(crate::Opcode::PlayerReconnected, [0; 5])
                .actor_flags()
                .is_none()
        );
        assert!(
            Message::known(crate::Opcode::PlayerLoadedScene, [0; 6])
                .actor_flags()
                .is_none()
        );
    }
}
