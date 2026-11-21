use halcyon_protocol::{Fields, LiveFields, Message, Opcode, ZoneAction};
use halcyon_sim::{AbilitySlot, CastTarget, Command, Event, Position, TICK_HZ};

use crate::{Room, messages::wire_id, native_abilities};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatalogInputKind {
    Point,
    TargetActor,
    AllyOrSelf,
    SelfOnly,
    PointOrSelf,
    PointOrTarget,
}

impl CatalogInputKind {
    const fn accepts_point(self) -> bool {
        matches!(self, Self::Point | Self::PointOrSelf | Self::PointOrTarget)
    }

    const fn accepts_target(self) -> bool {
        !matches!(self, Self::Point)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatalogSlotRole {
    Primary(AbilitySlot),
    Recast(AbilitySlot),
    Cancel(AbilitySlot),
}

impl CatalogSlotRole {
    pub(super) const fn slot(self) -> AbilitySlot {
        match self {
            Self::Primary(slot) | Self::Recast(slot) | Self::Cancel(slot) => slot,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectorSpec {
    pub selector: u8,
    pub role: CatalogSlotRole,
    pub input: CatalogInputKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HeroCatalogSpec {
    pub type_id: u16,
    pub hero_id: &'static str,
    pub learn_slots: [u8; 3],
    pub selectors: &'static [SelectorSpec],
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PendingCatalogCast {
    pub source: u32,
    pub slot: AbilitySlot,
    pub reply: Message,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum CatalogCastIntent {
    Cast {
        slot: AbilitySlot,
        target: CastTarget,
        reply: Message,
    },
    Cancel {
        reply: Message,
    },
}

/// Oriented 2D line-segment barrier emitted over `0x0422` (`CreateRectZoneOfControl`)
/// and torn down via `0x0423` (`DestroyZoneOfControl`), matching Silvernail A tripwires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SegmentBarrier {
    pub zone_id: u32,
    pub owner_wire: u32,
    pub p0: Position,
    pub p1: Position,
}

impl SegmentBarrier {
    pub(super) fn to_create_message(self) -> Option<Message> {
        ZoneAction::Rectangle {
            id: self.zone_id,
            owner: self.owner_wire,
            p0_xz_bits: [self.p0.x.to_bits(), self.p0.y.to_bits()],
            p1_xz_bits: [self.p1.x.to_bits(), self.p1.y.to_bits()],
            suffix: &[],
        }
        .to_message()
        .ok()
    }

    pub(super) fn to_destroy_message(zone_id: u32) -> Option<Message> {
        ZoneAction::Destroy {
            id: zone_id,
            suffix: &[],
        }
        .to_message()
        .ok()
    }
}

#[derive(Default)]
pub(super) struct TripwireWireTracker {
    next_seq: u32,
    stakes: Vec<(Position, u64)>,
    wires: Vec<(u32, u64)>,
}

const MAX_TRIPWIRE_STAKES: usize = 3;
const MAX_TRIPWIRE_ACTIVE_SPAN: f32 = 16.0;
const SILVERNAIL_TYPE_ID: u16 = 432;
const SILVERNAIL_TRIPWIRE_DURATIONS: [f32; 5] = [4.0, 4.5, 5.0, 5.5, 7.0];

fn emit_zone_message(room: &mut Room, source: u32, seat_idx: usize, msg: &Message) {
    if let Some(peer) = room
        .seats
        .get(seat_idx)
        .and_then(crate::Seat::gameplay_peer)
    {
        peer.send(crate::transport::Delivery::Private(vec![msg.clone()]));
    }
    native_abilities::witness(room, source, None, msg, None);
}

pub(super) fn tick_tripwires(room: &mut Room) {
    let tick = room.world.tick;
    let mut expired = Vec::new();
    for (seat_idx, seat) in room.seats.iter_mut().enumerate() {
        let tracker = &mut seat.native_abilities.tripwires;
        if tracker.stakes.is_empty() && tracker.wires.is_empty() {
            continue;
        }
        tracker.stakes.retain(|(_, expires)| *expires > tick);
        let mut keep_wires = Vec::with_capacity(tracker.wires.len());
        for (zone_id, expires) in tracker.wires.drain(..) {
            if expires <= tick {
                if let Some(source) = room.world.entities.get(seat_idx).map(|e| e.id) {
                    expired.push((source, seat_idx, zone_id));
                }
            } else {
                keep_wires.push((zone_id, expires));
            }
        }
        tracker.wires = keep_wires;
    }
    for (source, seat_idx, zone_id) in expired {
        if let Some(msg) = SegmentBarrier::to_destroy_message(zone_id) {
            emit_zone_message(room, source, seat_idx, &msg);
        }
    }
}

fn handle_silvernail_stake_event(
    room: &mut Room,
    idx: usize,
    source: u32,
    actor_wire: u32,
    target: CastTarget,
) {
    let caster_pos = room.world.entities[idx].position;
    let stake_pos = match target {
        CastTarget::Point(p) => p,
        CastTarget::Entity(id) => room
            .world
            .entities
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.position)
            .unwrap_or(caster_pos),
        CastTarget::SelfCast => caster_pos,
    };
    let rank_idx = usize::from(room.world.entities[idx].ability_ranks[0].saturating_sub(1)).min(4);
    let dur_ticks = (SILVERNAIL_TRIPWIRE_DURATIONS[rank_idx] * TICK_HZ as f32).round() as u64;
    let tick = room.world.tick;
    let expires_at = tick + dur_ticks.max(1);
    let mut destroyed = Vec::new();
    let created = {
        let Some(seat) = room.seats.get_mut(idx) else {
            return;
        };
        let tracker = &mut seat.native_abilities.tripwires;
        tracker.stakes.retain(|(_, exp)| *exp > tick);
        tracker.wires.retain(|(id, exp)| {
            if *exp <= tick {
                destroyed.push(*id);
                false
            } else {
                true
            }
        });
        while tracker.stakes.len() >= MAX_TRIPWIRE_STAKES {
            tracker.stakes.remove(0);
            if tracker.wires.len() >= MAX_TRIPWIRE_STAKES - 1 {
                destroyed.push(tracker.wires.remove(0).0);
            }
        }
        let prev = tracker
            .stakes
            .iter()
            .rev()
            .find(|(p, _)| (p.x - stake_pos.x).hypot(p.y - stake_pos.y) <= MAX_TRIPWIRE_ACTIVE_SPAN)
            .map(|(p, _)| *p);
        tracker.stakes.push((stake_pos, expires_at));
        if let Some(p0) = prev {
            while tracker.wires.len() >= MAX_TRIPWIRE_STAKES {
                destroyed.push(tracker.wires.remove(0).0);
            }
            tracker.next_seq = tracker.next_seq.wrapping_add(1).max(1);
            let zone_id = actor_wire.saturating_mul(1000) + 200 + (tracker.next_seq % 800);
            tracker.wires.push((zone_id, expires_at));
            Some(SegmentBarrier {
                zone_id,
                owner_wire: actor_wire,
                p0,
                p1: stake_pos,
            })
        } else {
            None
        }
    };
    for zone_id in destroyed {
        if let Some(msg) = SegmentBarrier::to_destroy_message(zone_id) {
            emit_zone_message(room, source, idx, &msg);
        }
    }
    if let Some(barrier) = created
        && let Some(msg) = barrier.to_create_message()
    {
        emit_zone_message(room, source, idx, &msg);
    }
}

/// Closed 2D segment-vs-segment intersection (`t, u in [0, 1]`), matching client `0x18bb8ec`.
#[allow(dead_code)]
pub(super) fn segments_intersect(a0: Position, a1: Position, b0: Position, b1: Position) -> bool {
    let rx = a1.x - a0.x;
    let ry = a1.y - a0.y;
    let sx = b1.x - b0.x;
    let sy = b1.y - b0.y;
    let qpx = b0.x - a0.x;
    let qpy = b0.y - a0.y;
    let denom = rx * sy - ry * sx;
    const EPS: f32 = 1e-6;
    if denom.abs() <= EPS {
        if (qpx * ry - qpy * rx).abs() > EPS {
            return false;
        }
        let r_len_sq = rx * rx + ry * ry;
        if r_len_sq <= EPS {
            return (a0.x - b0.x).hypot(a0.y - b0.y) <= EPS
                || (a0.x - b1.x).hypot(a0.y - b1.y) <= EPS;
        }
        let t0 = (qpx * rx + qpy * ry) / r_len_sq;
        let t1 = ((b1.x - a0.x) * rx + (b1.y - a0.y) * ry) / r_len_sq;
        let (lo, hi) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        return hi >= -EPS && lo <= 1.0 + EPS;
    }
    let t = (qpx * sy - qpy * sx) / denom;
    let u = (qpx * ry - qpy * rx) / denom;
    (-EPS..=1.0 + EPS).contains(&t) && (-EPS..=1.0 + EPS).contains(&u)
}

const fn p(selector: u8, slot: AbilitySlot, input: CatalogInputKind) -> SelectorSpec {
    SelectorSpec {
        selector,
        role: CatalogSlotRole::Primary(slot),
        input,
    }
}

const fn r(selector: u8, slot: AbilitySlot, input: CatalogInputKind) -> SelectorSpec {
    SelectorSpec {
        selector,
        role: CatalogSlotRole::Recast(slot),
        input,
    }
}

const fn c(selector: u8, slot: AbilitySlot) -> SelectorSpec {
    SelectorSpec {
        selector,
        role: CatalogSlotRole::Cancel(slot),
        input: CatalogInputKind::SelfOnly,
    }
}

const SEL_PPP_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::Point),
    p(1, AbilitySlot::B, CatalogInputKind::Point),
    p(2, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_PPP_123: &[SelectorSpec] = &[
    p(1, AbilitySlot::A, CatalogInputKind::Point),
    p(2, AbilitySlot::B, CatalogInputKind::Point),
    p(3, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_PPP_234: &[SelectorSpec] = &[
    p(2, AbilitySlot::A, CatalogInputKind::Point),
    p(3, AbilitySlot::B, CatalogInputKind::Point),
    p(4, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_PSP_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::Point),
    p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
    p(2, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_PSS_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::Point),
    p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
    p(2, AbilitySlot::C, CatalogInputKind::SelfOnly),
];

const SEL_PTP_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::Point),
    p(1, AbilitySlot::B, CatalogInputKind::TargetActor),
    p(2, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_TPP_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::TargetActor),
    p(1, AbilitySlot::B, CatalogInputKind::Point),
    p(2, AbilitySlot::C, CatalogInputKind::Point),
];

const SEL_TST_012: &[SelectorSpec] = &[
    p(0, AbilitySlot::A, CatalogInputKind::TargetActor),
    p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
    p(2, AbilitySlot::C, CatalogInputKind::TargetActor),
];

/// Verified 61-hero CFF0 `AbilitySet` slot catalog matching `server_data_extra/abilities.json`.
static HERO_SLOT_PROFILES: [HeroCatalogSpec; 61] = [
    HeroCatalogSpec {
        type_id: 925,
        hero_id: "Amael",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            c(3, AbilitySlot::A),
        ],
    },
    HeroCatalogSpec {
        type_id: 244,
        hero_id: "Adagio",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::AllyOrSelf),
            p(1, AbilitySlot::B, CatalogInputKind::AllyOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 268,
        hero_id: "Alpha",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::TargetActor),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
            r(3, AbilitySlot::A, CatalogInputKind::Point),
            r(4, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 429,
        hero_id: "Anka",
        learn_slots: [0, 2, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::B, CatalogInputKind::Point),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 257,
        hero_id: "Ardan",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::AllyOrSelf),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrTarget),
        ],
    },
    HeroCatalogSpec {
        type_id: 399,
        hero_id: "Baptiste",
        learn_slots: [0, 1, 2],
        selectors: SEL_PTP_012,
    },
    HeroCatalogSpec {
        type_id: 261,
        hero_id: "Baron",
        learn_slots: [1, 2, 3],
        selectors: SEL_PPP_123,
    },
    HeroCatalogSpec {
        type_id: 280,
        hero_id: "Blackfeather",
        learn_slots: [0, 1, 2],
        selectors: SEL_TPP_012,
    },
    HeroCatalogSpec {
        type_id: 915,
        hero_id: "Caine",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 242,
        hero_id: "Catherine",
        learn_slots: [1, 2, 3],
        selectors: &[
            p(1, AbilitySlot::A, CatalogInputKind::SelfOnly),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 285,
        hero_id: "Celeste",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 408,
        hero_id: "Churnwalker",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSP_012,
    },
    HeroCatalogSpec {
        type_id: 393,
        hero_id: "Flicker",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSS_012,
    },
    HeroCatalogSpec {
        type_id: 259,
        hero_id: "Fortress",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrTarget),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            p(2, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 249,
        hero_id: "Glaive",
        learn_slots: [0, 2, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(2, AbilitySlot::B, CatalogInputKind::TargetActor),
            p(3, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 406,
        hero_id: "Grace",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::TargetActor),
            p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(2, AbilitySlot::C, CatalogInputKind::AllyOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 396,
        hero_id: "Grumpjaw",
        learn_slots: [1, 2, 3],
        selectors: &[
            p(1, AbilitySlot::A, CatalogInputKind::Point),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(3, AbilitySlot::C, CatalogInputKind::TargetActor),
            r(4, AbilitySlot::C, CatalogInputKind::TargetActor),
            r(5, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 395,
        hero_id: "Gwen",
        learn_slots: [1, 2, 3],
        selectors: &[
            p(1, AbilitySlot::A, CatalogInputKind::Point),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 273,
        hero_id: "Idris",
        learn_slots: [0, 3, 4],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::Point),
            r(2, AbilitySlot::A, CatalogInputKind::Point),
            p(3, AbilitySlot::B, CatalogInputKind::Point),
            p(4, AbilitySlot::C, CatalogInputKind::PointOrTarget),
            r(5, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 440,
        hero_id: "Inara",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::SelfOnly),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 922,
        hero_id: "Ishtar",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSS_012,
    },
    HeroCatalogSpec {
        type_id: 253,
        hero_id: "Joule",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 926,
        hero_id: "Karas",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            c(3, AbilitySlot::A),
        ],
    },
    HeroCatalogSpec {
        type_id: 412,
        hero_id: "Kensei",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrTarget),
        ],
    },
    HeroCatalogSpec {
        type_id: 267,
        hero_id: "Kestrel",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSP_012,
    },
    HeroCatalogSpec {
        type_id: 420,
        hero_id: "Kinetic",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 245,
        hero_id: "Koshka",
        learn_slots: [0, 1, 2],
        selectors: SEL_TST_012,
    },
    HeroCatalogSpec {
        type_id: 254,
        hero_id: "Hero009",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::TargetActor),
            p(1, AbilitySlot::B, CatalogInputKind::TargetActor),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 275,
        hero_id: "Lance",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 913,
        hero_id: "Leo",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            r(3, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 409,
        hero_id: "Lorelai",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::AllyOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 269,
        hero_id: "Lyra",
        learn_slots: [0, 2, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 418,
        hero_id: "Magnus",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 281,
        hero_id: "Malene",
        learn_slots: [0, 2, 4],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::Point),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            r(3, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(4, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(5, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 919,
        hero_id: "Miho",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            r(3, AbilitySlot::B, CatalogInputKind::Point),
            c(4, AbilitySlot::B),
        ],
    },
    HeroCatalogSpec {
        type_id: 274,
        hero_id: "Ozo",
        learn_slots: [0, 3, 8],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrTarget),
            r(1, AbilitySlot::A, CatalogInputKind::PointOrTarget),
            r(2, AbilitySlot::A, CatalogInputKind::PointOrTarget),
            p(3, AbilitySlot::B, CatalogInputKind::TargetActor),
            r(4, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            r(5, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            r(6, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            r(7, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(8, AbilitySlot::C, CatalogInputKind::TargetActor),
            r(9, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 246,
        hero_id: "Petal",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 279,
        hero_id: "Phinn",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSP_012,
    },
    HeroCatalogSpec {
        type_id: 266,
        hero_id: "Reim",
        learn_slots: [0, 1, 2],
        selectors: SEL_PSP_012,
    },
    HeroCatalogSpec {
        type_id: 403,
        hero_id: "Reza",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 243,
        hero_id: "Ringo",
        learn_slots: [0, 1, 2],
        selectors: SEL_TST_012,
    },
    HeroCatalogSpec {
        type_id: 260,
        hero_id: "Hero016",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::TargetActor),
            p(2, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(4, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            c(5, AbilitySlot::C),
        ],
    },
    HeroCatalogSpec {
        type_id: 276,
        hero_id: "Samuel",
        learn_slots: [1, 3, 4],
        selectors: &[
            p(1, AbilitySlot::A, CatalogInputKind::Point),
            r(2, AbilitySlot::A, CatalogInputKind::Point),
            p(3, AbilitySlot::B, CatalogInputKind::Point),
            p(4, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 446,
        hero_id: "Sanfeng",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 250,
        hero_id: "SAW",
        learn_slots: [1, 2, 3],
        selectors: &[
            p(1, AbilitySlot::A, CatalogInputKind::SelfOnly),
            p(2, AbilitySlot::B, CatalogInputKind::Point),
            p(3, AbilitySlot::C, CatalogInputKind::SelfOnly),
            c(4, AbilitySlot::B),
        ],
    },
    HeroCatalogSpec {
        type_id: 927,
        hero_id: "Shin",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            r(3, AbilitySlot::A, CatalogInputKind::Point),
            r(4, AbilitySlot::B, CatalogInputKind::Point),
            c(5, AbilitySlot::C),
            r(6, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 432,
        hero_id: "Silvernail",
        learn_slots: [2, 3, 4],
        selectors: SEL_PPP_234,
    },
    HeroCatalogSpec {
        type_id: 255,
        hero_id: "Hero010",
        learn_slots: [0, 1, 2],
        selectors: SEL_PPP_012,
    },
    HeroCatalogSpec {
        type_id: 265,
        hero_id: "Skye",
        learn_slots: [0, 2, 4],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            c(1, AbilitySlot::A),
            p(2, AbilitySlot::B, CatalogInputKind::PointOrTarget),
            p(4, AbilitySlot::C, CatalogInputKind::PointOrTarget),
        ],
    },
    HeroCatalogSpec {
        type_id: 256,
        hero_id: "Sayoc",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrTarget),
            p(1, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(2, AbilitySlot::C, CatalogInputKind::TargetActor),
        ],
    },
    HeroCatalogSpec {
        type_id: 397,
        hero_id: "Tony",
        learn_slots: [0, 4, 5],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            r(1, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            r(2, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            r(3, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(4, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(5, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 413,
        hero_id: "Varya",
        learn_slots: [0, 4, 8],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::Point),
            r(2, AbilitySlot::A, CatalogInputKind::Point),
            r(3, AbilitySlot::A, CatalogInputKind::Point),
            p(4, AbilitySlot::B, CatalogInputKind::Point),
            r(5, AbilitySlot::B, CatalogInputKind::Point),
            r(6, AbilitySlot::B, CatalogInputKind::Point),
            r(7, AbilitySlot::B, CatalogInputKind::Point),
            p(8, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 924,
        hero_id: "Viola",
        learn_slots: [0, 1, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(3, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(4, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(5, AbilitySlot::C, CatalogInputKind::SelfOnly),
            r(6, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 258,
        hero_id: "Vox",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 916,
        hero_id: "Warhawk",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            r(3, AbilitySlot::B, CatalogInputKind::SelfOnly),
            r(5, AbilitySlot::C, CatalogInputKind::Point),
            r(6, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 439,
        hero_id: "Yates",
        learn_slots: [0, 2, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            r(1, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::B, CatalogInputKind::SelfOnly),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 436,
        hero_id: "Ylva",
        learn_slots: [0, 2, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::PointOrSelf),
            c(1, AbilitySlot::A),
            p(2, AbilitySlot::B, CatalogInputKind::TargetActor),
            p(3, AbilitySlot::C, CatalogInputKind::Point),
        ],
    },
    HeroCatalogSpec {
        type_id: 435,
        hero_id: "Hero057",
        learn_slots: [0, 1, 3],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::AllyOrSelf),
            r(2, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(3, AbilitySlot::C, CatalogInputKind::SelfOnly),
        ],
    },
    HeroCatalogSpec {
        type_id: 912,
        hero_id: "Hero062",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 914,
        hero_id: "Hero999",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::PointOrSelf),
            p(2, AbilitySlot::C, CatalogInputKind::PointOrSelf),
        ],
    },
    HeroCatalogSpec {
        type_id: 443,
        hero_id: "LanceBall_Lance",
        learn_slots: [0, 1, 2],
        selectors: &[
            p(0, AbilitySlot::A, CatalogInputKind::Point),
            p(1, AbilitySlot::B, CatalogInputKind::Point),
            p(2, AbilitySlot::C, CatalogInputKind::Point),
            r(3, AbilitySlot::A, CatalogInputKind::Point),
        ],
    },
];

pub(super) fn hero_profile(type_id: u16) -> Option<&'static HeroCatalogSpec> {
    HERO_SLOT_PROFILES.iter().find(|h| h.type_id == type_id)
}

#[allow(dead_code)]
pub(super) const fn all_heroes() -> &'static [HeroCatalogSpec; 61] {
    &HERO_SLOT_PROFILES
}

pub(super) const fn slot_index(slot: AbilitySlot) -> usize {
    match slot {
        AbilitySlot::A => 0,
        AbilitySlot::B => 1,
        AbilitySlot::C => 2,
    }
}

pub(super) fn learn_request(type_id: u16, index: u8) -> Option<(AbilitySlot, u8)> {
    let slot = match index {
        0 => AbilitySlot::A,
        1 => AbilitySlot::B,
        2 => AbilitySlot::C,
        _ => return None,
    };
    learn_operand(type_id, slot).map(|operand| (slot, operand))
}

pub(super) fn learn_operand(type_id: u16, slot: AbilitySlot) -> Option<u8> {
    hero_profile(type_id).map(|profile| profile.learn_slots[slot_index(slot)])
}

/// Secondary form grant operands (`[Light, Dark]`) for Malene's 6-slot kit (`281`).
#[allow(dead_code)]
pub(super) const fn malene_form_operands(slot: AbilitySlot) -> [u8; 2] {
    match slot {
        AbilitySlot::A => [0, 1],
        AbilitySlot::B => [2, 3],
        AbilitySlot::C => [4, 5],
    }
}

pub(super) fn selector_spec(type_id: u16, selector: u8) -> Option<&'static SelectorSpec> {
    hero_profile(type_id)?
        .selectors
        .iter()
        .find(|entry| entry.selector == selector)
}

pub(super) fn cancel_message(actor_wire: u32, selector: u8, cancel_shared: bool) -> Message {
    let mut body = [0u8; 14];
    body[0..4].copy_from_slice(&actor_wire.to_be_bytes());
    body[4..8].copy_from_slice(&u32::from(selector).to_be_bytes());
    body[9] = u8::from(cancel_shared);
    Message::known(Opcode::CancelAbility, body)
}

pub(super) fn default_cast_message(
    actor_wire: u32,
    type_id: u16,
    slot: AbilitySlot,
    target: CastTarget,
) -> Option<Message> {
    let profile = hero_profile(type_id)?;
    let selector = profile.learn_slots[slot_index(slot)];
    match target {
        CastTarget::Point(point) => Some(Message::ability_point_used(
            actor_wire,
            [point.x, 0.0, point.y],
            selector,
        )),
        CastTarget::SelfCast => Some(Message::ability_target_used(actor_wire, u32::MAX, selector)),
        CastTarget::Entity(target_id) => Some(Message::ability_target_used(
            actor_wire,
            u32::from(wire_id(target_id)),
            selector,
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActorTeamRef {
    pub id: u32,
    pub team: u8,
    pub pos: Position,
    pub alive: bool,
}

fn dist_2d(a: Position, b: Position) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn clamp_point_to_range(origin: Position, target: Position, max_range: f32) -> Position {
    let d = dist_2d(origin, target);
    if d <= max_range || d <= 1e-6 {
        return target;
    }
    let scale = max_range / d;
    Position {
        x: origin.x + (target.x - origin.x) * scale,
        y: origin.y + (target.y - origin.y) * scale,
    }
}

fn nearest_actor(
    actors: &[ActorTeamRef],
    caster_id: u32,
    anchor: Position,
    max_dist: f32,
    want_team: u8,
    exclude_self: bool,
) -> Option<&ActorTeamRef> {
    let mut best: Option<(f32, &ActorTeamRef)> = None;
    for actor in actors {
        if !actor.alive || actor.team != want_team || (exclude_self && actor.id == caster_id) {
            continue;
        }
        let d = dist_2d(anchor, actor.pos);
        if d <= max_dist && best.is_none_or(|(bd, _)| (d, actor.id) < (bd, actor.id)) {
            best = Some((d, actor));
        }
    }
    best.map(|(_, actor)| actor)
}

fn resolve_target_entity(
    caster_id: u32,
    team: u8,
    input: CatalogInputKind,
    target: u32,
    actors: &[ActorTeamRef],
) -> Option<(CastTarget, u32)> {
    if target == u32::MAX {
        if !matches!(
            input,
            CatalogInputKind::SelfOnly
                | CatalogInputKind::AllyOrSelf
                | CatalogInputKind::PointOrSelf
                | CatalogInputKind::PointOrTarget
        ) {
            return None;
        }
        return Some((CastTarget::SelfCast, u32::MAX));
    }
    let target_id = target.checked_sub(1499)?;
    let target_entity = actors.iter().find(|e| e.id == target_id)?;
    match input {
        CatalogInputKind::SelfOnly => {
            (target_entity.id == caster_id).then_some((CastTarget::SelfCast, u32::MAX))
        }
        CatalogInputKind::AllyOrSelf => {
            if target_entity.team != team {
                return None;
            }
            if target_entity.id == caster_id {
                Some((CastTarget::SelfCast, u32::MAX))
            } else {
                Some((CastTarget::Entity(target_entity.id), target))
            }
        }
        CatalogInputKind::TargetActor => (target_entity.id != caster_id)
            .then_some((CastTarget::Entity(target_entity.id), target)),
        CatalogInputKind::PointOrSelf | CatalogInputKind::PointOrTarget => {
            if target_entity.id == caster_id {
                Some((CastTarget::SelfCast, u32::MAX))
            } else {
                Some((CastTarget::Entity(target_entity.id), target))
            }
        }
        CatalogInputKind::Point => None,
    }
}

fn quick_point_for_target(
    caster_id: u32,
    team: u8,
    caster_pos: Position,
    target: u32,
    actors: &[ActorTeamRef],
) -> Option<Position> {
    if target != u32::MAX {
        let target_id = target.checked_sub(1499)?;
        return actors.iter().find(|e| e.id == target_id).map(|e| e.pos);
    }
    if let Some(enemy) = nearest_actor(actors, caster_id, caster_pos, 14.0, 1 - team, true) {
        return Some(enemy.pos);
    }
    let dir = if team == 0 { 2.0 } else { -2.0 };
    Some(Position {
        x: caster_pos.x + dir,
        y: caster_pos.y,
    })
}

pub(super) fn decode_cast_intent(
    hero: u16,
    caster_id: u32,
    team: u8,
    actors: &[ActorTeamRef],
    m: &Message,
) -> Option<CatalogCastIntent> {
    let wire = u32::from(wire_id(caster_id));
    let caster_pos = actors
        .iter()
        .find(|e| e.id == caster_id)
        .map_or(Position::default(), |e| e.pos);
    match m.known_opcode() {
        Some(Opcode::RequestPointAbility) => {
            let Ok(Fields::Live(LiveFields::AbilityPoint { vector, selector })) = m.fields() else {
                return None;
            };
            if !vector.iter().all(|v| v.is_finite()) {
                return None;
            }
            let spec = selector_spec(hero, selector)?;
            if matches!(spec.role, CatalogSlotRole::Cancel(_)) {
                return None;
            }
            let tapped = Position {
                x: vector[0],
                y: vector[2],
            };
            if spec.input.accepts_point() {
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: CastTarget::Point(tapped),
                    reply: Message::ability_point_used(wire, vector, selector),
                });
            }
            let (cast_target, reply_target) = match spec.input {
                CatalogInputKind::SelfOnly => (CastTarget::SelfCast, u32::MAX),
                CatalogInputKind::AllyOrSelf => {
                    if let Some(ally) = nearest_actor(actors, caster_id, tapped, 4.5, team, true) {
                        (CastTarget::Entity(ally.id), u32::from(wire_id(ally.id)))
                    } else {
                        (CastTarget::SelfCast, u32::MAX)
                    }
                }
                CatalogInputKind::TargetActor => {
                    let enemy = nearest_actor(actors, caster_id, tapped, 4.5, 1 - team, true)?;
                    (CastTarget::Entity(enemy.id), u32::from(wire_id(enemy.id)))
                }
                CatalogInputKind::Point
                | CatalogInputKind::PointOrSelf
                | CatalogInputKind::PointOrTarget => unreachable!(),
            };
            Some(CatalogCastIntent::Cast {
                slot: spec.role.slot(),
                target: cast_target,
                reply: Message::ability_target_used(wire, reply_target, selector),
            })
        }
        Some(Opcode::RequestTargetAbility) => {
            let Ok(Fields::Live(LiveFields::AbilityTarget { target, selector })) = m.fields()
            else {
                return None;
            };
            let spec = selector_spec(hero, selector)?;
            if matches!(spec.role, CatalogSlotRole::Cancel(_)) {
                return Some(CatalogCastIntent::Cancel {
                    reply: cancel_message(wire, selector, true),
                });
            }
            if !spec.input.accepts_target() {
                let pt = quick_point_for_target(caster_id, team, caster_pos, target, actors)?;
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: CastTarget::Point(pt),
                    reply: Message::ability_point_used(wire, [pt.x, 0.0, pt.y], selector),
                });
            }
            let (cast_target, reply_target) =
                resolve_target_entity(caster_id, team, spec.input, target, actors)?;
            Some(CatalogCastIntent::Cast {
                slot: spec.role.slot(),
                target: cast_target,
                reply: Message::ability_target_used(wire, reply_target, selector),
            })
        }
        Some(Opcode::RequestActivateAbilityUnseen) => {
            let Ok(Fields::Outbound(halcyon_protocol::OutboundAction {
                operands:
                    halcyon_protocol::OutboundOperands::AbilityAlternate {
                        words: [target, x_bits, y_bits, z_bits],
                        byte: selector,
                    },
                ..
            })) = m.fields()
            else {
                return None;
            };
            let spec = selector_spec(hero, selector)?;
            if matches!(spec.role, CatalogSlotRole::Cancel(_)) {
                return Some(CatalogCastIntent::Cancel {
                    reply: cancel_message(wire, selector, true),
                });
            }
            let vector = [
                f32::from_bits(x_bits),
                f32::from_bits(y_bits),
                f32::from_bits(z_bits),
            ];
            let has_finite_point =
                vector.iter().all(|v| v.is_finite()) && (vector[0] != 0.0 || vector[2] != 0.0);
            if spec.input.accepts_target()
                && target != 0
                && target != u32::MAX
                && let Some((cast_target, reply_target)) =
                    resolve_target_entity(caster_id, team, spec.input, target, actors)
            {
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: cast_target,
                    reply: Message::ability_target_used(wire, reply_target, selector),
                });
            }
            if spec.input.accepts_point() && has_finite_point {
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: CastTarget::Point(Position {
                        x: vector[0],
                        y: vector[2],
                    }),
                    reply: Message::ability_point_used(wire, vector, selector),
                });
            }
            if spec.input.accepts_target()
                && let Some((cast_target, reply_target)) =
                    resolve_target_entity(caster_id, team, spec.input, target, actors)
            {
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: cast_target,
                    reply: Message::ability_target_used(wire, reply_target, selector),
                });
            }
            if spec.input == CatalogInputKind::Point {
                let pt = quick_point_for_target(caster_id, team, caster_pos, target, actors)?;
                return Some(CatalogCastIntent::Cast {
                    slot: spec.role.slot(),
                    target: CastTarget::Point(pt),
                    reply: Message::ability_point_used(wire, [pt.x, 0.0, pt.y], selector),
                });
            }
            None
        }
        _ => None,
    }
}

pub(super) fn receive(room: &mut Room, seat: usize, m: Message) -> Vec<Message> {
    let id = room.world.entities[seat].id;
    let hero = room.world.entities[seat].type_id;
    let team = room.world.entities[seat].team;
    let wire = u32::from(wire_id(id));

    match m.known_opcode() {
        Some(Opcode::RequestCancelAbility) => {
            let selector = room.seats[seat]
                .native_abilities
                .catalog_cast
                .take()
                .and_then(|pending| learn_operand(hero, pending.slot))
                .or_else(|| {
                    room.seats[seat]
                        .native_abilities
                        .take_legacy_cast_slot()
                        .and_then(|slot| learn_operand(hero, slot))
                })
                .unwrap_or(0);
            let cancel = cancel_message(wire, selector, false);
            native_abilities::witness(room, id, None, &cancel, None);
            vec![m, cancel]
        }
        Some(Opcode::RequestUpgradeAbility) => {
            let Ok(Fields::Live(LiveFields::LearnSelection(index))) = m.fields() else {
                return Vec::new();
            };
            if m.body.len() == 6 && m.body[1..] != [0; 5] {
                return Vec::new();
            }
            let Some((slot, operand)) = learn_request(hero, index) else {
                return Vec::new();
            };
            if room.seats[seat].native_abilities.learned_at == Some(room.world.tick)
                || room.world.command(id, Command::Learn(slot)).is_err()
            {
                return Vec::new();
            }
            room.seats[seat].native_abilities.learned_at = Some(room.world.tick);
            let grant = Message::ability_learned(wire, u32::from(operand));
            native_abilities::witness(room, id, None, &grant, None);
            vec![m, grant]
        }
        Some(
            Opcode::RequestPointAbility
            | Opcode::RequestTargetAbility
            | Opcode::RequestActivateAbilityUnseen,
        ) => {
            let actors: Vec<ActorTeamRef> = room
                .world
                .entities
                .iter()
                .map(|e| ActorTeamRef {
                    id: e.id,
                    team: e.team,
                    pos: e.position,
                    alive: e.hp > 0.0,
                })
                .collect();
            let Some(intent) = decode_cast_intent(hero, id, team, &actors, &m) else {
                return Vec::new();
            };
            match intent {
                CatalogCastIntent::Cancel { reply } => {
                    room.seats[seat].native_abilities.catalog_cast = None;
                    native_abilities::witness(room, id, None, &reply, None);
                    vec![m, reply]
                }
                CatalogCastIntent::Cast {
                    slot,
                    mut target,
                    mut reply,
                } => {
                    if let CastTarget::Point(p) = target
                        && let Some(range) = room.world.effective_cast_range(id, slot)
                        && range < 180.0
                    {
                        let origin = room.world.entities[seat].position;
                        if dist_2d(origin, p) > range {
                            let clamped = clamp_point_to_range(origin, p, (range - 0.1).max(0.5));
                            target = CastTarget::Point(clamped);
                            if reply.is(Opcode::PlayPointAbility) && reply.body.len() >= 17 {
                                let height = f32::from_be_bytes(
                                    reply.body[8..12].try_into().unwrap_or([0; 4]),
                                );
                                let selector = reply.body[16];
                                reply = Message::ability_point_used(
                                    wire,
                                    [clamped.x, height, clamped.y],
                                    selector,
                                );
                            }
                        }
                    }
                    let effective_target = if room
                        .world
                        .command(id, Command::Cast { slot, target })
                        .is_ok()
                    {
                        target
                    } else if target == CastTarget::SelfCast
                        && room
                            .world
                            .command(
                                id,
                                Command::Cast {
                                    slot,
                                    target: CastTarget::Entity(id),
                                },
                            )
                            .is_ok()
                    {
                        if let Some(selector) = m.body.get(4).copied()
                            && m.known_opcode() == Some(Opcode::RequestTargetAbility)
                        {
                            reply = Message::ability_target_used(wire, u32::MAX, selector);
                        }
                        CastTarget::Entity(id)
                    } else {
                        return Vec::new();
                    };
                    let _ = effective_target;
                    room.seats[seat].native_abilities.catalog_cast = Some(PendingCatalogCast {
                        source: id,
                        slot,
                        reply,
                    });
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    }
}

pub(super) fn publish_event(room: &mut Room, event: &Event) {
    match *event {
        Event::Learned { source, slot } => {
            let Some(hero) = room
                .world
                .entities
                .iter()
                .find(|e| e.id == source)
                .map(|e| e.type_id)
            else {
                return;
            };
            let Some(operand) = learn_operand(hero, slot) else {
                return;
            };
            let actor_wire = u32::from(wire_id(source));
            let grant = Message::ability_learned(actor_wire, u32::from(operand));
            native_abilities::witness(room, source, None, &grant, None);
        }
        Event::Cast {
            source,
            slot,
            target,
        } => {
            let Some(idx) = room.world.entities.iter().position(|e| e.id == source) else {
                return;
            };
            let hero = room.world.entities[idx].type_id;
            let pending_reply = room.seats.get_mut(idx).and_then(|seat| {
                let matches_cast = seat
                    .native_abilities
                    .catalog_cast
                    .as_ref()
                    .is_some_and(|pending| pending.source == source && pending.slot == slot);
                if matches_cast {
                    seat.native_abilities.catalog_cast.take().map(|p| p.reply)
                } else {
                    None
                }
            });
            let actor_wire = u32::from(wire_id(source));
            let is_pending = pending_reply.is_some();
            let Some(reply) =
                pending_reply.or_else(|| default_cast_message(actor_wire, hero, slot, target))
            else {
                return;
            };
            let target_id = match target {
                CastTarget::Entity(id) => Some(id),
                CastTarget::SelfCast | CastTarget::Point(_) => None,
            };
            if is_pending
                && let Some(peer) = room.seats.get(idx).and_then(crate::Seat::gameplay_peer)
            {
                peer.send(crate::transport::Delivery::Private(vec![reply.clone()]));
            }
            native_abilities::witness(room, source, target_id, &reply, None);
            if hero == SILVERNAIL_TYPE_ID && slot == AbilitySlot::A {
                handle_silvernail_stake_event(room, idx, source, actor_wire, target);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Peer, Reservation, Rooms};
    use halcyon_platform::MatchHost;
    use halcyon_protocol::ce::KnownQueue;
    use std::collections::BTreeSet;
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    fn drain_deliveries(rx: &mut mpsc::Receiver<crate::transport::Delivery>) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(delivery) = rx.try_recv() {
            match delivery {
                crate::transport::Delivery::Private(batch) => out.extend(batch),
                crate::transport::Delivery::Team(batch) => out.extend_from_slice(batch.messages()),
            }
        }
        out
    }

    #[test]
    fn catalog_covers_all_61_heroes_with_verified_cff0_learn_slots() {
        assert_eq!(HERO_SLOT_PROFILES.len(), 61);
        let mut seen_tids = BTreeSet::new();
        let mut seen_ids = BTreeSet::new();
        for profile in all_heroes() {
            assert!(
                seen_tids.insert(profile.type_id),
                "duplicate tid {} for {}",
                profile.type_id,
                profile.hero_id
            );
            assert!(
                seen_ids.insert(profile.hero_id),
                "duplicate hero_id {}",
                profile.hero_id
            );
            for (index, slot) in [
                (0, AbilitySlot::A),
                (1, AbilitySlot::B),
                (2, AbilitySlot::C),
            ] {
                let (mapped_slot, operand) = learn_request(profile.type_id, index).unwrap();
                assert_eq!(mapped_slot, slot);
                assert_eq!(operand, profile.learn_slots[usize::from(index)]);
                assert_eq!(learn_operand(profile.type_id, slot), Some(operand));
                let spec = selector_spec(profile.type_id, operand)
                    .unwrap_or_else(|| panic!("missing primary selector for {}", profile.hero_id));
                assert_eq!(spec.role, CatalogSlotRole::Primary(slot));
            }
        }
        assert!(learn_request(242, 3).is_none());
        assert_eq!(learn_request(242, 0), Some((AbilitySlot::A, 1)));
        assert_eq!(learn_request(242, 1), Some((AbilitySlot::B, 2)));
        assert_eq!(learn_request(242, 2), Some((AbilitySlot::C, 3)));
        assert_eq!(learn_request(395, 0), Some((AbilitySlot::A, 1)));
        assert_eq!(learn_request(269, 1), Some((AbilitySlot::B, 2)));
        assert_eq!(learn_request(250, 0), Some((AbilitySlot::A, 1)));
        assert_eq!(learn_request(439, 2), Some((AbilitySlot::C, 3)));
        assert_eq!(learn_request(273, 1), Some((AbilitySlot::B, 3)));
        assert_eq!(learn_request(273, 2), Some((AbilitySlot::C, 4)));
        assert_eq!(learn_request(274, 2), Some((AbilitySlot::C, 8)));
        assert_eq!(learn_request(413, 2), Some((AbilitySlot::C, 8)));
        assert_eq!(learn_request(432, 0), Some((AbilitySlot::A, 2)));
        assert_eq!(learn_request(924, 2), Some((AbilitySlot::C, 3)));
        assert_eq!(learn_request(435, 2), Some((AbilitySlot::C, 3)));
        assert_eq!(malene_form_operands(AbilitySlot::A), [0, 1]);
        assert_eq!(malene_form_operands(AbilitySlot::B), [2, 3]);
        assert_eq!(malene_form_operands(AbilitySlot::C), [4, 5]);
    }

    #[test]
    fn all_61_heroes_wire_selectors_convert_0411_0412_to_0415_0416_043c() {
        let entities = [
            ActorTeamRef {
                id: 1,
                team: 0,
                pos: Position { x: 0.0, y: 0.0 },
                alive: true,
            },
            ActorTeamRef {
                id: 2,
                team: 0,
                pos: Position { x: 2.0, y: 0.0 },
                alive: true,
            },
            ActorTeamRef {
                id: 3,
                team: 1,
                pos: Position { x: 4.0, y: 0.0 },
                alive: true,
            },
        ];
        let enemy_wire = u32::from(wire_id(3));
        let ally_wire = u32::from(wire_id(2));

        for profile in all_heroes() {
            for entry in profile.selectors {
                if matches!(entry.role, CatalogSlotRole::Cancel(_)) {
                    let mut body = [0u8; 6];
                    body[..4].copy_from_slice(&u32::MAX.to_be_bytes());
                    body[4] = entry.selector;
                    let req = Message::known(Opcode::RequestTargetAbility, body);
                    let intent =
                        decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                    assert_eq!(
                        intent,
                        CatalogCastIntent::Cancel {
                            reply: cancel_message(1500, entry.selector, true),
                        },
                        "hero {} cancel selector {}",
                        profile.hero_id,
                        entry.selector
                    );
                    continue;
                }
                if entry.input.accepts_point() {
                    let mut body = [0u8; 14];
                    body[0..4].copy_from_slice(&(-0.909_949f32).to_be_bytes());
                    body[4..8].copy_from_slice(&0.005_715f32.to_be_bytes());
                    body[8..12].copy_from_slice(&4.366_973f32.to_be_bytes());
                    body[12] = entry.selector;
                    let req = Message::known(Opcode::RequestPointAbility, body);
                    let intent =
                        decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                    assert_eq!(
                        intent,
                        CatalogCastIntent::Cast {
                            slot: entry.role.slot(),
                            target: CastTarget::Point(Position {
                                x: -0.909_949,
                                y: 4.366_973,
                            }),
                            reply: Message::ability_point_used(
                                1500,
                                [-0.909_949, 0.005_715, 4.366_973],
                                entry.selector,
                            ),
                        },
                        "hero {} point selector {}",
                        profile.hero_id,
                        entry.selector
                    );
                } else {
                    let (target_wire, expected_cast_target, expected_reply_target) =
                        match entry.input {
                            CatalogInputKind::TargetActor => {
                                (enemy_wire, CastTarget::Entity(3), enemy_wire)
                            }
                            CatalogInputKind::AllyOrSelf => {
                                (ally_wire, CastTarget::Entity(2), ally_wire)
                            }
                            CatalogInputKind::SelfOnly
                            | CatalogInputKind::PointOrSelf
                            | CatalogInputKind::PointOrTarget => {
                                (u32::MAX, CastTarget::SelfCast, u32::MAX)
                            }
                            CatalogInputKind::Point => unreachable!(),
                        };
                    let mut body = [0u8; 6];
                    body[..4].copy_from_slice(&target_wire.to_be_bytes());
                    body[4] = entry.selector;
                    let req = Message::known(Opcode::RequestTargetAbility, body);
                    let intent =
                        decode_cast_intent(profile.type_id, 1, 0, &entities, &req).unwrap();
                    assert_eq!(
                        intent,
                        CatalogCastIntent::Cast {
                            slot: entry.role.slot(),
                            target: expected_cast_target,
                            reply: Message::ability_target_used(
                                1500,
                                expected_reply_target,
                                entry.selector,
                            ),
                        },
                        "hero {} target selector {}",
                        profile.hero_id,
                        entry.selector
                    );
                }
            }
        }
    }

    #[test]
    fn segment_zone_barrier_matches_0422_and_0423_wire_and_intersection() {
        let barrier = SegmentBarrier {
            zone_id: 9001,
            owner_wire: 1500,
            p0: Position { x: -4.0, y: 2.0 },
            p1: Position { x: 4.0, y: 2.0 },
        };
        let create = barrier.to_create_message().unwrap();
        assert_eq!(create.known_opcode(), Some(Opcode::CreateRectZoneOfControl));
        assert_eq!(create.body.len(), 24);
        assert_eq!(
            create.zone_action(),
            Some(ZoneAction::Rectangle {
                id: 9001,
                owner: 1500,
                p0_xz_bits: [(-4.0f32).to_bits(), 2.0f32.to_bits()],
                p1_xz_bits: [4.0f32.to_bits(), 2.0f32.to_bits()],
                suffix: &[],
            })
        );
        let destroy = SegmentBarrier::to_destroy_message(9001).unwrap();
        assert_eq!(destroy.known_opcode(), Some(Opcode::DestroyZoneOfControl));
        assert_eq!(
            destroy.zone_action(),
            Some(ZoneAction::Destroy {
                id: 9001,
                suffix: &[]
            })
        );

        assert!(segments_intersect(
            barrier.p0,
            barrier.p1,
            Position { x: 0.0, y: -1.0 },
            Position { x: 0.0, y: 5.0 }
        ));
        assert!(!segments_intersect(
            barrier.p0,
            barrier.p1,
            Position { x: 0.0, y: 3.0 },
            Position { x: 0.0, y: 7.0 }
        ));
        assert!(segments_intersect(
            barrier.p0,
            barrier.p1,
            Position { x: 2.0, y: 2.0 },
            Position { x: 6.0, y: 2.0 }
        ));
    }

    #[tokio::test]
    async fn live_5v5_channel_1_silvernail_tripwires_emit_0422_and_0423() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4(), Uuid::new_v4()],
                teams: vec![0, 1],
                talent_levels: Vec::new(),
                difficulty: None,
                mode: KnownQueue::CasualFive.as_str().into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let (tx0, mut rx0) = mpsc::channel(32);
        let (tx1, mut rx1) = mpsc::channel(32);
        {
            let mut room = shared.lock().unwrap();
            room.phase(crate::RoomPhase::Running);
            for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
                room.seats[seat_idx].peers[1] = Some(Peer {
                    generation: 1,
                    tx,
                    stalled: watch::channel(false).0,
                });
            }
            room.world.entities[0].type_id = SILVERNAIL_TYPE_ID;
            room.world.entities[0].level = 2;
            room.world.entities[0].skill_points = 1;
            room.world.entities[0].position = Position { x: 0.0, y: 0.0 };
            room.world.entities[1].position = Position { x: 3.0, y: 0.0 };
            let caster_decl = crate::messages::spawn(&room.world.entities[0], &room.picks, None);
            let enemy_decl = crate::messages::spawn(&room.world.entities[1], &room.picks, None);
            for seat in &mut room.scene_views {
                seat.declarations(&[caster_decl.clone(), enemy_decl.clone()]);
            }
            let hero_id = room.world.entities[0].id;
            let enemy_id = room.world.entities[1].id;
            room.visible[0].insert(enemy_id);
            room.visible[1].insert(hero_id);

            // Verify Silvernail learn mapping (index 0 -> AbilitySlot::A -> CFF0 selector 2)
            // and 0x043A grant + 0x0422/0x0423 tripwire lifecycle on Event::Cast.
            assert_eq!(
                learn_request(SILVERNAIL_TYPE_ID, 0),
                Some((AbilitySlot::A, 2))
            );
            room.world.entities[0].ability_ranks[0] = 1;
            publish_event(
                &mut room,
                &Event::Learned {
                    source: hero_id,
                    slot: AbilitySlot::A,
                },
            );
            assert_eq!(
                drain_deliveries(&mut rx1),
                vec![Message::ability_learned(1500, 2)]
            );

            // Stake 1: no wire until paired with stake 2.
            publish_event(
                &mut room,
                &Event::Cast {
                    source: hero_id,
                    slot: AbilitySlot::A,
                    target: CastTarget::Point(Position { x: 1.0, y: 2.0 }),
                },
            );
            assert!(
                drain_deliveries(&mut rx0)
                    .iter()
                    .all(|m| m.known_opcode() != Some(Opcode::CreateRectZoneOfControl))
            );
            let _ = drain_deliveries(&mut rx1);

            // Stake 2 within 16u: emits 0x0422 CreateRectZoneOfControl to caster & witness.
            publish_event(
                &mut room,
                &Event::Cast {
                    source: hero_id,
                    slot: AbilitySlot::A,
                    target: CastTarget::Point(Position { x: 5.0, y: 2.0 }),
                },
            );
            let caster_msgs = drain_deliveries(&mut rx0);
            let witness_msgs = drain_deliveries(&mut rx1);
            let zone1 = caster_msgs
                .iter()
                .find(|m| m.known_opcode() == Some(Opcode::CreateRectZoneOfControl))
                .expect("expected 0x0422 zone creation for caster");
            assert!(
                witness_msgs.contains(zone1),
                "expected identical 0x0422 for visible enemy peer"
            );
            let zone1_id = match zone1.zone_action() {
                Some(ZoneAction::Rectangle {
                    id,
                    owner,
                    p0_xz_bits,
                    p1_xz_bits,
                    ..
                }) => {
                    assert_eq!(owner, 1500);
                    assert_eq!(p0_xz_bits, [1.0f32.to_bits(), 2.0f32.to_bits()]);
                    assert_eq!(p1_xz_bits, [5.0f32.to_bits(), 2.0f32.to_bits()]);
                    id
                }
                other => panic!("unexpected zone1 packet: {other:?}"),
            };

            // Stake 3 & 4 prune oldest wire -> 0x0423 DestroyZoneOfControl + new 0x0422.
            publish_event(
                &mut room,
                &Event::Cast {
                    source: hero_id,
                    slot: AbilitySlot::A,
                    target: CastTarget::Point(Position { x: 6.0, y: -1.0 }),
                },
            );
            let _ = drain_deliveries(&mut rx0);
            let _ = drain_deliveries(&mut rx1);
            publish_event(
                &mut room,
                &Event::Cast {
                    source: hero_id,
                    slot: AbilitySlot::A,
                    target: CastTarget::Point(Position { x: 8.0, y: -1.0 }),
                },
            );
            let prune_msgs = drain_deliveries(&mut rx0);
            assert!(prune_msgs.iter().any(|m| {
                m.known_opcode() == Some(Opcode::DestroyZoneOfControl)
                    && matches!(m.zone_action(), Some(ZoneAction::Destroy { id, .. }) if id == zone1_id)
            }));
            let _ = drain_deliveries(&mut rx1);

            // Expiry tick cleanup emits 0x0423 for remaining active wires.
            room.world.tick += 200;
            tick_tripwires(&mut room);
            let expired_msgs = drain_deliveries(&mut rx0);
            assert!(
                expired_msgs
                    .iter()
                    .any(|m| m.known_opcode() == Some(Opcode::DestroyZoneOfControl))
            );
            assert!(
                drain_deliveries(&mut rx1)
                    .iter()
                    .any(|m| m.known_opcode() == Some(Opcode::DestroyZoneOfControl))
            );
        }
    }

    #[tokio::test]
    async fn all_61_heroes_channel_1_learn_round_trip_covers_every_cff0_slot() {
        for profile in HERO_SLOT_PROFILES {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
                    match_id: id,
                    players: vec![Uuid::new_v4(), Uuid::new_v4()],
                    teams: vec![0, 1],
                    talent_levels: Vec::new(),
                    difficulty: None,
                    mode: KnownQueue::CasualFive.as_str().into(),
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let (tx0, _rx0) = mpsc::channel(32);
            let (tx1, mut rx1) = mpsc::channel(32);
            let mut room = shared.lock().unwrap();
            room.phase(crate::RoomPhase::Running);
            for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
                room.seats[seat_idx].peers[1] = Some(Peer {
                    generation: 1,
                    tx,
                    stalled: watch::channel(false).0,
                });
            }
            room.world.entities[0].type_id = profile.type_id;
            room.world.entities[0].level = 6;
            room.world.entities[0].skill_points = 3;
            let mut caster_decl =
                crate::messages::spawn(&room.world.entities[0], &room.picks, None);
            caster_decl.body[745] = 1;
            for seat in &mut room.scene_views {
                seat.declarations(std::slice::from_ref(&caster_decl));
            }
            let hero_id = room.world.entities[0].id;
            let enemy_id = room.world.entities[1].id;
            room.visible[0].insert(hero_id);
            room.visible[0].insert(enemy_id);
            room.visible[1].insert(hero_id);
            room.visible[1].insert(enemy_id);

            for (slot_idx, &learn_index) in profile.learn_slots.iter().enumerate() {
                let slot = match slot_idx {
                    0 => AbilitySlot::A,
                    1 => AbilitySlot::B,
                    _ => AbilitySlot::C,
                };
                let primary = profile
                    .selectors
                    .iter()
                    .find(|s| s.role == CatalogSlotRole::Primary(slot))
                    .unwrap_or_else(|| panic!("{} missing primary selector", profile.hero_id));
                assert_eq!(
                    learn_request(profile.type_id, slot_idx as u8),
                    Some((slot, primary.selector)),
                    "hero {} learn slot_idx {} -> CFF0 {}",
                    profile.hero_id,
                    slot_idx,
                    learn_index
                );
                publish_event(
                    &mut room,
                    &Event::Learned {
                        source: hero_id,
                        slot,
                    },
                );
                assert_eq!(
                    drain_deliveries(&mut rx1),
                    vec![Message::ability_learned(1500, u32::from(primary.selector))],
                    "witness learn failed for {}",
                    profile.hero_id
                );

                let target = if primary.input.accepts_point() {
                    CastTarget::Point(Position { x: 2.5, y: -1.0 })
                } else if matches!(
                    primary.input,
                    CatalogInputKind::AllyOrSelf | CatalogInputKind::SelfOnly
                ) {
                    CastTarget::SelfCast
                } else {
                    CastTarget::Entity(enemy_id)
                };
                publish_event(
                    &mut room,
                    &Event::Cast {
                        source: hero_id,
                        slot,
                        target,
                    },
                );
                let msgs = drain_deliveries(&mut rx1);
                let expected = default_cast_message(1500, profile.type_id, slot, target)
                    .expect("default cast packet");
                assert!(
                    msgs.contains(&expected),
                    "witness cast failed for {} {:?}",
                    profile.hero_id,
                    slot
                );
            }
        }
    }

    #[tokio::test]
    async fn live_5v5_channel_1_learns_and_casts_non_standard_cff0_heroes() {
        for (type_id, index, selector, target_kind) in [
            (269u16, 0u8, 0u8, CatalogInputKind::Point),
            (273u16, 1u8, 3u8, CatalogInputKind::Point),
            (409u16, 0u8, 0u8, CatalogInputKind::Point),
            (409u16, 1u8, 1u8, CatalogInputKind::Point),
            (409u16, 2u8, 2u8, CatalogInputKind::AllyOrSelf),
            (243u16, 1u8, 1u8, CatalogInputKind::SelfOnly),
            (244u16, 0u8, 0u8, CatalogInputKind::AllyOrSelf),
        ] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let id = Uuid::new_v4();
            rooms
                .reserve(Reservation {
                    match_id: id,
                    players: vec![Uuid::new_v4(), Uuid::new_v4()],
                    teams: vec![0, 1],
                    talent_levels: Vec::new(),
                    difficulty: None,
                    mode: KnownQueue::CasualFive.as_str().into(),
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let (tx0, mut rx0) = mpsc::channel(32);
            let (tx1, mut rx1) = mpsc::channel(32);
            {
                let mut room = shared.lock().unwrap();
                room.phase(crate::RoomPhase::Running);
                for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
                    room.seats[seat_idx].peers[1] = Some(Peer {
                        generation: 1,
                        tx,
                        stalled: watch::channel(false).0,
                    });
                }
                room.world.entities[0].type_id = type_id;
                room.world.entities[0].level = 6;
                room.world.entities[0].skill_points = 1;
                room.world.entities[0].energy = 1000.0;
                room.world.entities[0].max_energy = 1000.0;
                room.world.entities[0].position = Position { x: 0.0, y: 0.0 };
                room.world.entities[1].position = Position { x: 3.0, y: 0.0 };
                let caster_decl =
                    crate::messages::spawn(&room.world.entities[0], &room.picks, None);
                let enemy_decl = crate::messages::spawn(&room.world.entities[1], &room.picks, None);
                for seat in &mut room.scene_views {
                    seat.declarations(&[caster_decl.clone(), enemy_decl.clone()]);
                }
                let hero_id = room.world.entities[0].id;
                let enemy_id = room.world.entities[1].id;
                room.visible[0].insert(enemy_id);
                room.visible[1].insert(hero_id);

                let learn = Message::known(Opcode::RequestUpgradeAbility, [index, 0, 0, 0, 0, 0]);
                let out = room.receive(0, 1, learn.clone()).unwrap();
                assert_eq!(
                    out,
                    vec![learn, Message::ability_learned(1500, u32::from(selector))]
                );
                assert_eq!(
                    drain_deliveries(&mut rx1),
                    vec![Message::ability_learned(1500, u32::from(selector))]
                );

                let (req, expected) = match target_kind {
                    CatalogInputKind::Point => {
                        let mut body = [0u8; 14];
                        body[0..4].copy_from_slice(&3.5f32.to_be_bytes());
                        body[4..8].copy_from_slice(&0.005_715f32.to_be_bytes());
                        body[8..12].copy_from_slice(&(-1.25f32).to_be_bytes());
                        body[12] = selector;
                        (
                            Message::known(Opcode::RequestPointAbility, body),
                            Message::ability_point_used(1500, [3.5, 0.005_715, -1.25], selector),
                        )
                    }
                    CatalogInputKind::TargetActor => {
                        let enemy_wire = u32::from(wire_id(enemy_id));
                        let mut body = [0u8; 6];
                        body[..4].copy_from_slice(&enemy_wire.to_be_bytes());
                        body[4] = selector;
                        (
                            Message::known(Opcode::RequestTargetAbility, body),
                            Message::ability_target_used(1500, enemy_wire, selector),
                        )
                    }
                    _ => {
                        let mut body = [0u8; 6];
                        body[..4].copy_from_slice(&u32::MAX.to_be_bytes());
                        body[4] = selector;
                        (
                            Message::known(Opcode::RequestTargetAbility, body),
                            Message::ability_target_used(1500, u32::MAX, selector),
                        )
                    }
                };
                assert!(room.receive(0, 1, req).unwrap().is_empty());
                assert!(
                    room.seats[0].native_abilities.catalog_cast.is_some(),
                    "cast failed for hero {type_id} selector {selector}"
                );

                let mut emitted = Vec::new();
                for _ in 0..40 {
                    room.tick();
                    emitted.extend(drain_deliveries(&mut rx0));
                    if emitted.contains(&expected) {
                        break;
                    }
                }
                assert!(
                    emitted.contains(&expected),
                    "hero {type_id} selector {selector} missing {expected:?} in {emitted:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn baron_out_of_range_0412_clamps_and_0411_0413_quick_cast_emit_valid_replies() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let id = Uuid::new_v4();
        rooms
            .reserve(Reservation {
                match_id: id,
                players: vec![Uuid::new_v4(), Uuid::new_v4()],
                teams: vec![0, 1],
                talent_levels: Vec::new(),
                difficulty: None,
                mode: KnownQueue::CasualFive.as_str().into(),
            })
            .unwrap();
        let shared = rooms.room(id).unwrap();
        let (tx0, mut rx0) = mpsc::channel(64);
        let (tx1, _rx1) = mpsc::channel(64);
        {
            let mut room = shared.lock().unwrap();
            room.phase(crate::RoomPhase::Running);
            for (seat_idx, tx) in [tx0, tx1].into_iter().enumerate() {
                room.seats[seat_idx].peers[1] = Some(Peer {
                    generation: 1,
                    tx,
                    stalled: watch::channel(false).0,
                });
            }
            room.world.enable_builtin_hero_kits();
            room.world.entities[0].type_id = 261; // Baron (selectors 1, 2, 3)
            room.world.entities[0].level = 9;
            room.world.entities[0].skill_points = 9;
            room.world.entities[0].energy = 1000.0;
            room.world.entities[0].max_energy = 1000.0;
            room.world.entities[0].position = Position { x: 20.0, y: 0.0 };
            room.world.entities[1].position = Position { x: 25.0, y: 0.0 };
            let hero_id = room.world.entities[0].id;
            let enemy_id = room.world.entities[1].id;
            room.visible[0].insert(enemy_id);
            room.visible[1].insert(hero_id);

            // Learn Baron A (learn index 0 -> selector 1).
            room.receive(
                0,
                1,
                Message::known(Opcode::RequestUpgradeAbility, [0, 0, 0, 0, 0, 0]),
            )
            .unwrap();
            let _ = drain_deliveries(&mut rx0);

            // 1. Out-of-range 0x0412 (25m away) clamps inside Baron A's 8m range and survives 0x03F4 moves!
            let mut pt_body = [0u8; 14];
            pt_body[0..4].copy_from_slice(&(45.0f32).to_be_bytes());
            pt_body[4..8].copy_from_slice(&0.0f32.to_be_bytes());
            pt_body[8..12].copy_from_slice(&(0.0f32).to_be_bytes());
            pt_body[12] = 1;
            room.receive(0, 1, Message::known(Opcode::RequestPointAbility, pt_body))
                .unwrap();

            let mut emitted = Vec::new();
            for _ in 0..8 {
                let _ = room.receive(0, 1, Message::movement(15.0, 0.0));
                room.tick();
                emitted.extend(drain_deliveries(&mut rx0));
            }
            let clamped_reply = emitted
                .iter()
                .find(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1))
                .expect("Baron A out-of-range + joystick move must emit 0x0416");
            let x = f32::from_be_bytes(clamped_reply.body[4..8].try_into().unwrap());
            assert!(
                (27.0..=28.0).contains(&x),
                "expected clamped Baron A X in [27.0, 28.0], got {x}"
            );

            // 2. Joystick quick-cast 0x0411 targeting enemy actor 1501 converts cleanly to Baron A point cast.
            let mut tgt_body = [0u8; 6];
            tgt_body[0..4].copy_from_slice(&1501u32.to_be_bytes());
            tgt_body[4] = 1;
            room.receive(0, 1, Message::known(Opcode::RequestTargetAbility, tgt_body))
                .unwrap();
            let mut quick_emitted = Vec::new();
            for _ in 0..8 {
                room.tick();
                quick_emitted.extend(drain_deliveries(&mut rx0));
            }
            assert!(
                quick_emitted
                    .iter()
                    .any(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1)),
                "0x0411 quick-cast on Point ability must emit 0x0416"
            );

            // Allow a charge to regenerate before testing 0x0413 on Baron A.
            for _ in 0..105 {
                room.tick();
            }
            let _ = drain_deliveries(&mut rx0);

            // 3. 0x0413 vector/unseen ability packet decodes & emits PlayPointAbility for Baron A.
            let mut vec_body = Vec::with_capacity(17);
            vec_body.extend_from_slice(&u32::MAX.to_be_bytes());
            vec_body.extend_from_slice(&(23.0f32).to_be_bytes());
            vec_body.extend_from_slice(&0.0f32.to_be_bytes());
            vec_body.extend_from_slice(&(1.0f32).to_be_bytes());
            vec_body.push(1);
            room.receive(
                0,
                1,
                Message::known(Opcode::RequestActivateAbilityUnseen, vec_body),
            )
            .unwrap();
            let mut unseen_emitted = Vec::new();
            for _ in 0..8 {
                room.tick();
                unseen_emitted.extend(drain_deliveries(&mut rx0));
            }
            assert!(
                unseen_emitted
                    .iter()
                    .any(|m| m.is(Opcode::PlayPointAbility) && m.body.get(16) == Some(&1)),
                "0x0413 packet must emit 0x0416"
            );
        }
    }
}
