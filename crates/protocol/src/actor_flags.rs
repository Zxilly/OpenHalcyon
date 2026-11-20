//! Scoreboard AFK and deserter flags. Reconnection clears both flags.
use crate::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorFlagChange {
    Afk,
    Deserter,
    Reconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFlags<'a> {
    pub actor: u32,
    pub change: ActorFlagChange,
    padding: &'a [u8],
}

impl Message {
    pub fn actor_flags(&self) -> Option<ActorFlags<'_>> {
        let change = match self.known_opcode() {
            Some(crate::Opcode::PlayerAfk) => ActorFlagChange::Afk,
            Some(crate::Opcode::PlayerDeserter) => ActorFlagChange::Deserter,
            Some(crate::Opcode::PlayerReconnected) => ActorFlagChange::Reconnected,
            _ => return None,
        };
        let (actor, padding) = self.body.split_first_chunk::<4>()?;
        matches!(padding.len(), 0 | 2).then(|| ActorFlags {
            actor: u32::from_be_bytes(*actor),
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
        let opcode = match self.change {
            ActorFlagChange::Afk => crate::wire_opcode::PlayerAfk,
            ActorFlagChange::Deserter => crate::wire_opcode::PlayerDeserter,
            ActorFlagChange::Reconnected => crate::wire_opcode::PlayerReconnected,
        };
        let mut body = Vec::with_capacity(4 + self.padding.len());
        body.extend_from_slice(&self.actor.to_be_bytes());
        body.extend_from_slice(self.padding);
        Message::new(opcode, body)
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
            Message::known(crate::Opcode::PlayerHandle, [0; 6])
                .actor_flags()
                .is_none()
        );
    }
}
