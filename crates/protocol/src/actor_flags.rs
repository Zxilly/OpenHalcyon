//! The native receiver exposes two independent actor flags. Their gameplay meanings are unknown.
use crate::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorFlagChange {
    SetFirst,
    SetSecond,
    ClearBoth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFlags<'a> {
    pub actor: u32,
    pub change: ActorFlagChange,
    padding: &'a [u8],
}

impl Message {
    pub fn actor_flags(&self) -> Option<ActorFlags<'_>> {
        let change = match self.opcode {
            0x03ea => ActorFlagChange::SetFirst,
            0x03eb => ActorFlagChange::SetSecond,
            0x03ec => ActorFlagChange::ClearBoth,
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
    pub fn to_message(self) -> Message {
        let opcode = match self.change {
            ActorFlagChange::SetFirst => 0x03ea,
            ActorFlagChange::SetSecond => 0x03eb,
            ActorFlagChange::ClearBoth => 0x03ec,
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
            (0x03ea, ActorFlagChange::SetFirst),
            (0x03eb, ActorFlagChange::SetSecond),
            (0x03ec, ActorFlagChange::ClearBoth),
        ] {
            for padding in [&[][..], &[0x35, 0x78]] {
                let message = Message::new(opcode, [&1502u32.to_be_bytes()[..], padding].concat());
                let flags = message.actor_flags().unwrap();
                assert_eq!((flags.actor, flags.change), (1502, change));
                assert_eq!(flags.to_message(), message);
            }
        }
        assert!(Message::new(0x03ec, [0; 5]).actor_flags().is_none());
        assert!(Message::new(0x03ed, [0; 6]).actor_flags().is_none());
    }
}
