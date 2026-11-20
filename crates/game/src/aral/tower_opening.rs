use crate::{Message, Opcode};
use halcyon_protocol::{BuffApplication, BuffExtras, BuffPrefix, Fields};

// Permanent structure declarations observed only at initial ARAL world sync.
const DEFINITIONS: [u16; 3] = [194, 198, 204];
const DURATION: u16 = 0xbc00;
const EXTRAS: BuffExtras = BuffExtras {
    stacks: 1,
    words: [0; 4],
};

fn prefix(slot: u8, operand: usize) -> Option<BuffPrefix> {
    let (side, local) = (slot / 8, slot % 8);
    if side > 1 || !(3..=7).contains(&local) {
        return None;
    }
    let actor = 2000 + u32::from(slot);
    Some(BuffPrefix {
        target: actor,
        source: actor,
        duration_bits: DURATION,
        instance: actor + 19 + 5 * u32::from(local - 3) + 27 * u32::from(side) + operand as u32,
        definition: *DEFINITIONS.get(operand)?,
    })
}

pub(super) fn messages(slot: u8) -> impl Iterator<Item = Message> {
    (0..DEFINITIONS.len()).filter_map(move |i| {
        Some(
            BuffApplication {
                prefix: prefix(slot, i)?,
                extras: Some(EXTRAS),
                suffix: &[],
            }
            .to_message()
            .expect("valid ARAL opening profile"),
        )
    })
}

pub(crate) fn recordable(message: &Message) -> bool {
    if !message.is(Opcode::EntityData) {
        return false;
    }
    let Ok(Fields::BuffApplication(effect)) = message.fields() else {
        return false;
    };
    if effect.extras != Some(EXTRAS) || !matches!(effect.suffix, [] | [0, 0, 0, 0]) {
        return false;
    }
    let Some(slot) = effect
        .prefix
        .target
        .checked_sub(2000)
        .and_then(|s| u8::try_from(s).ok())
    else {
        return false;
    };
    DEFINITIONS
        .iter()
        .position(|&id| id == effect.prefix.definition)
        .and_then(|ordinal| prefix(slot, ordinal))
        == Some(effect.prefix)
}
