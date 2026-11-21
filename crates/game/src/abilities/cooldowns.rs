use crate::Message;
use crate::replication::{Batch, messages::wire_id};
use halcyon_data::EntityTemplate;
use halcyon_protocol::ce::NativeMode;
use halcyon_sim::{Entity, Kind};
use std::{collections::HashSet, sync::OnceLock};

mod profiles;

enum ModeSet {
    Aral,
    Solo,
    Both,
}

impl ModeSet {
    fn includes(&self, mode: NativeMode) -> bool {
        matches!(
            (self, mode),
            (Self::Aral | Self::Both, NativeMode::Aral)
                | (Self::Solo | Self::Both, NativeMode::SoloBots)
        )
    }
}

struct Opening {
    template: EntityTemplate,
    modes: ModeSet,
    rows: &'static [Row],
}

#[derive(Clone, Copy)]
enum Preset {
    EmoteFive,
    EmoteHalf,
    Ability,
    Ultimate,
    Internal,
    AlternateOne,
    AlternateZero,
    MovementVariant,
    SharedHalf,
    SharedSixTenths,
    SharedSevenTenths,
    SharedEightTenths,
}

impl Preset {
    fn operands(self) -> (f32, [u8; 6]) {
        use Preset::*;
        const SHARED: [u8; 6] = [1, 1, 0, 0, 0, 1];
        match self {
            EmoteFive => (5.0, [1, 1, 1, 0, 1, 0]),
            EmoteHalf => (0.5, [1, 1, 1, 0, 1, 0]),
            Ability => (0.0, [0, 0, 1, 0, 1, 0]),
            Ultimate => (0.0, [0, 0, 1, 1, 1, 0]),
            Internal => (0.0, [1, 1, 1, 1, 0, 0]),
            AlternateOne => (1.0, [1, 1, 1, 0, 1, 1]),
            AlternateZero => (0.0, [1, 1, 1, 0, 0, 0]),
            MovementVariant => (0.0, [0, 0, 1, 0, 0, 0]),
            SharedHalf => (0.5, SHARED),
            SharedSixTenths => (0.6, SHARED),
            SharedSevenTenths => (0.7, SHARED),
            SharedEightTenths => (0.8, SHARED),
        }
    }
}

#[derive(Clone, Copy)]
struct Row {
    key: u32,
    preset: Preset,
}

impl Row {
    const fn raw(key: u32, preset: Preset) -> Self {
        Self { key, preset }
    }

    fn payload(self) -> [u8; 18] {
        let (configured, flags) = self.preset.operands();
        // The opening value at +4 is positive zero for every recorded hero.
        halcyon_protocol::InitCooldownBody {
            owner: 0,
            key: self.key,
            value: 0.0,
            config_value: configured,
            flags,
        }
        .encode_payload()
    }
}

/// FNV-1a 32-bit hash over a case-sensitive Android definition name.
const fn fnv1a_32(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash = 0x811c9dc5u32;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u32).wrapping_mul(0x01000193);
        i += 1;
    }
    hash
}

const fn row(name: &str, preset: Preset) -> Row {
    Row::raw(fnv1a_32(name), preset)
}

/// Called immediately after a hero declaration on the initial native scene.
pub(crate) fn opening(batch: &mut Batch, actor: &Entity, mode: NativeMode) {
    if actor.kind != Kind::Hero {
        return;
    }
    let Some(profile) = profiles::ALL
        .iter()
        .find(|p| p.template.as_raw() == actor.type_id && p.modes.includes(mode))
    else {
        return;
    };
    let owner = u32::from(wire_id(actor.id));
    batch.reserve(profile.rows.len());
    for row in profile.rows {
        batch.push(halcyon_protocol::InitCooldownBody::from_payload(owner, row.payload()).encode());
    }
}

pub(crate) fn recordable(msg: &Message) -> bool {
    let Ok(body) = halcyon_protocol::InitCooldownBody::read_exact(&msg.body) else {
        return false;
    };
    if !(1500..=1509).contains(&body.owner) {
        return false;
    }
    static VERIFIED: OnceLock<HashSet<[u8; 18]>> = OnceLock::new();
    VERIFIED
        .get_or_init(|| {
            profiles::ALL
                .iter()
                .flat_map(|p| p.rows.iter().map(|r| r.payload()))
                .collect()
        })
        .contains(&body.encode_payload())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn native_initializers_match_original_aral_and_first_solo_chunk() {
        let mut world = halcyon_sim::World::new(1, 10);
        let hero = &mut world.entities[0];
        for (mode, templates, rows, expected) in [
            (
                NativeMode::Aral,
                30,
                228,
                "7f7c078337111ee4803bac3b030dcd18884f1443353db2e7db08db48c1a239bd",
            ),
            (
                NativeMode::SoloBots,
                9,
                72,
                "ca7626305d8c7a483cdb0acf6bd448eb5398fa581e300fbbf12ea592953dd05b",
            ),
        ] {
            let mut digest = Sha256::new();
            let mut counts = (0, 0);
            for profile in profiles::ALL {
                hero.type_id = profile.template.as_raw();
                let mut batch = Vec::new();
                opening(&mut batch, hero, mode);
                if batch.is_empty() {
                    continue;
                }
                counts.0 += 1;
                counts.1 += batch.len();
                digest.update(hero.type_id.to_be_bytes());
                digest.update([u8::try_from(batch.len()).unwrap()]);
                for mut msg in batch {
                    assert!(msg.is(crate::Opcode::InitCooldown));
                    assert_eq!(msg.body[..4], 1500u32.to_be_bytes());
                    assert!(recordable(&msg));
                    digest.update(&msg.body[4..]);
                    msg.body[8] = 1;
                    assert!(!recordable(&msg));
                }
            }
            assert_eq!(counts, (templates, rows));
            assert_eq!(format!("{:x}", digest.finalize()), expected);
        }
    }

    #[test]
    fn resolved_item_cooldown_hashes_match_cff0_names() {
        assert_eq!(fnv1a_32("Ability__Item__HealingFlask"), 0x1b184adf);
        assert_eq!(fnv1a_32("StormguardBanner"), 0xdb161949);
    }
}
