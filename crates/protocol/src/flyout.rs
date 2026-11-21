use crate::{Message, Opcode, fixed_body};
use binrw::{BinRead, BinWrite};

const FLYOUT_PREFIX_LEN: usize = 10;

/// Native 147219 flyout text selectors, in client table order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum FlyoutText {
    Slowed,
    Feared,
    Charmed,
    Panicked,
    Blinded,
    Missed,
    Dodged,
    Blocked,
    Invulnerable,
    Resisted,
    Rooted,
    Stasis,
    Taunted,
    Cleansed,
    Rekt,
    Fulfilled,
    Reloading,
    Eaten,
    Om,
    Nom,
    GameOver,
    OnePoint,
    TwoPoints,
    ThreePoints,
    Executed,
}

impl FlyoutText {
    const TABLE: [Self; 25] = [
        Self::Slowed,
        Self::Feared,
        Self::Charmed,
        Self::Panicked,
        Self::Blinded,
        Self::Missed,
        Self::Dodged,
        Self::Blocked,
        Self::Invulnerable,
        Self::Resisted,
        Self::Rooted,
        Self::Stasis,
        Self::Taunted,
        Self::Cleansed,
        Self::Rekt,
        Self::Fulfilled,
        Self::Reloading,
        Self::Eaten,
        Self::Om,
        Self::Nom,
        Self::GameOver,
        Self::OnePoint,
        Self::TwoPoints,
        Self::ThreePoints,
        Self::Executed,
    ];
    pub fn from_selector(selector: u16) -> Option<Self> {
        Self::TABLE.get(usize::from(selector)).copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, BinRead, BinWrite)]
#[brw(big)]
struct FlyoutWire {
    actor: u32,
    selector: u16,
    parameter: u32,
}

/// Keeps an unknown selector, parameter and any decrypted padding for replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Flyout {
    pub actor: u32,
    pub selector: u16,
    pub parameter: u32,
    pub padding: Option<[u8; 4]>,
}

impl Flyout {
    pub fn new(actor: u32, text: FlyoutText, parameter: u32) -> Self {
        Self {
            actor,
            selector: text as u16,
            parameter,
            padding: None,
        }
    }

    pub fn text(self) -> Option<FlyoutText> {
        FlyoutText::from_selector(self.selector)
    }

    pub(super) fn decode(body: &[u8]) -> Option<Self> {
        let (wire, tail): (FlyoutWire, _) =
            fixed_body::read_with_padding(body, FLYOUT_PREFIX_LEN).ok()?;
        let padding = match tail {
            [] => None,
            tail => Some(tail.try_into().ok()?),
        };
        Some(Self {
            actor: wire.actor,
            selector: wire.selector,
            parameter: wire.parameter,
            padding,
        })
    }

    pub fn to_message(self) -> Message {
        let wire = FlyoutWire {
            actor: self.actor,
            selector: self.selector,
            parameter: self.parameter,
        };
        let suffix = self.padding.as_ref().map_or(&[][..], |pad| &pad[..]);
        fixed_body::message_with_padding(Opcode::ShowFlyoutText, &wire, FLYOUT_PREFIX_LEN, suffix)
            .expect("flyout padding is 0 or 4 bytes")
    }
}

impl Message {
    pub fn flyout(&self) -> Option<Flyout> {
        self.is(Opcode::ShowFlyoutText)
            .then(|| Flyout::decode(&self.body))
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyout_word_roundtrips_known_and_unknown_selectors_and_framing() {
        for selector in 0..=25 {
            let message = Flyout {
                actor: 1500,
                selector,
                parameter: u32::MAX,
                padding: Some([0, 1, 2, 3]),
            }
            .to_message();
            let decoded = message.flyout().unwrap();
            assert_eq!(
                decoded.text().map(|text| text as u16),
                (selector < 25).then_some(selector)
            );
            assert_eq!(decoded.to_message(), message);
        }
        let cleansed = Flyout::new(1500, FlyoutText::Cleansed, u32::MAX).to_message();
        assert_eq!(&cleansed.body, &[0, 0, 5, 220, 0, 13, 255, 255, 255, 255]);
    }
    #[test]
    fn supplied_flyout_fixtures_roundtrip() {
        let Some(root) = std::env::var_os("HALCYON_PROTOCOL_SPEC") else {
            return;
        };
        let file = std::path::PathBuf::from(root).join("fixtures/03_buffs_items_flyout.json");
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let mut checked = 0;
        for row in rows.iter().filter(|r| r["opcode"] == "0x0440") {
            let bytes = row["payload_hex"]
                .as_str()
                .unwrap()
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            let m = Message::new(
                u16::from_be_bytes(bytes[..2].try_into().unwrap()),
                bytes[2..].to_vec(),
            );
            let decoded = m.flyout().unwrap();
            let expected = &row["expected_decoded"];
            assert_eq!(
                u64::from(decoded.actor),
                expected["actor_id"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(decoded.selector),
                expected["selector"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(decoded.parameter),
                expected["param"].as_u64().unwrap()
            );
            assert!(decoded.text().is_some());
            assert_eq!(decoded.to_message(), m);
            checked += 1;
        }
        assert!(checked >= 7);
    }
}
