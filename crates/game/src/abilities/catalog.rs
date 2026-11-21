use halcyon_protocol::{Fields, LiveFields, Message, Opcode, ZoneAction};
use halcyon_sim::{AbilitySlot, CastTarget, Command, Event, Position, TICK_HZ};

use crate::Room;
use crate::abilities as native_abilities;
use crate::replication::messages::wire_id;

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
pub(crate) struct PendingCatalogCast {
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
pub(crate) struct TripwireWireTracker {
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

pub(crate) fn tick_tripwires(room: &mut Room) {
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

pub(crate) fn learn_operand(type_id: u16, slot: AbilitySlot) -> Option<u8> {
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
    halcyon_protocol::CancelAbilityBody {
        actor: actor_wire,
        reference: u32::from(selector),
        is_item: 0,
        cancel_shared: u8::from(cancel_shared),
    }
    .encode_padded()
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

pub(crate) fn receive(room: &mut Room, seat: usize, m: Message) -> Vec<Message> {
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
            let Ok((req, tail)) =
                halcyon_protocol::RequestUpgradeAbilityBody::read_with_padding(&m.body)
            else {
                return Vec::new();
            };
            if !tail.iter().all(|&b| b == 0) {
                return Vec::new();
            }
            let Some((slot, operand)) = learn_request(hero, req.ability_slot) else {
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
                            if reply.is(Opcode::PlayPointAbility)
                                && let Ok(point_body) =
                                    halcyon_protocol::PlayPointAbilityBody::read_body(&reply.body)
                            {
                                reply = Message::ability_point_used(
                                    wire,
                                    [clamped.x, point_body.coordinates[1], clamped.y],
                                    point_body.selector,
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
                        if m.known_opcode() == Some(Opcode::RequestTargetAbility)
                            && let Ok(req) =
                                halcyon_protocol::RequestTargetAbilityBody::read_body(&m.body)
                        {
                            reply = Message::ability_target_used(wire, u32::MAX, req.selector);
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

pub(crate) fn publish_event(room: &mut Room, event: &Event) {
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
#[path = "catalog_tests.rs"]
mod tests;
