//! Draft preferences occupy three bytes (`12..15`) of each 161-byte roster seat.
use halcyon_protocol::{
    DraftPreferenceFlags, Opcode,
    draft::{DraftBuildPath, DraftPosition},
};

#[derive(Clone, Copy, Default)]
pub(super) struct Preferences {
    position: Option<DraftPosition>,
    build: Option<DraftBuildPath>,
}

impl Preferences {
    pub(super) fn to_wire(self) -> DraftPreferenceFlags {
        DraftPreferenceFlags::new(
            self.position.map(DraftPosition::to_wire),
            self.build.map(DraftBuildPath::to_wire),
        )
    }

    pub(super) fn set_position(&mut self, value: Option<DraftPosition>) {
        self.position = value;
    }

    pub(super) fn set_build(&mut self, value: Option<DraftBuildPath>) {
        self.build = value;
    }

    pub(super) fn position_raw(self) -> u8 {
        self.position.map_or(u8::MAX, DraftPosition::to_wire)
    }

    pub(super) fn build_raw(self) -> u8 {
        self.build.map_or(u8::MAX, DraftBuildPath::to_wire)
    }
}

pub(super) enum Choice {
    Position(Option<DraftPosition>),
    Build(Option<DraftBuildPath>),
}

impl Choice {
    pub(super) fn parse(opcode: Option<Opcode>, value: u8) -> Option<Self> {
        match opcode? {
            Opcode::DraftSelectPosition => {
                if value == u8::MAX {
                    Some(Self::Position(None))
                } else {
                    let pos = DraftPosition::from_wire(value)?;
                    matches!(
                        pos,
                        DraftPosition::Top | DraftPosition::Jungle | DraftPosition::Captain
                    )
                    .then_some(Self::Position(Some(pos)))
                }
            }
            Opcode::DraftSelectBuildPath => {
                if value == u8::MAX {
                    Some(Self::Build(None))
                } else {
                    Some(Self::Build(Some(DraftBuildPath::from_wire(value)?)))
                }
            }
            _ => None,
        }
    }

    pub(super) fn apply(self, preferences: &mut Preferences) {
        match self {
            Self::Position(v) => preferences.position = v,
            Self::Build(v) => preferences.build = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Message, Opcode, RoomPhase, Rooms, transport::Delivery};
    use halcyon_platform::{MatchHost, Reservation};
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    #[tokio::test]
    async fn draft_preferences_are_seat_scoped_and_reappear_on_reconnect() {
        for (mode, roster) in [
            ("ranked", Opcode::DraftRosterFull),
            ("casual_aral", Opcode::DraftRosterAral),
        ] {
            let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
            let id = Uuid::new_v4();
            let players = (0..6).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
            rooms
                .reserve(Reservation {
                    match_id: id,
                    players: players.clone(),
                    mode: mode.into(),
                    teams: vec![],
                    difficulty: None,
                    talent_levels: Vec::new(),
                })
                .unwrap();
            let shared = rooms.room(id).unwrap();
            let mut room = shared.lock().unwrap();
            let (tx, _r) = mpsc::channel(32);
            assert!(
                room.attach(Uuid::new_v4(), tx.clone(), watch::channel(false).0)
                    .is_none()
            );
            let (_, _, generation, _) = room
                .attach(players[1], tx, watch::channel(false).0)
                .unwrap();
            let (observer_tx, mut other) = mpsc::channel(32);
            room.attach(players[0], observer_tx, watch::channel(false).0)
                .unwrap();
            while other.try_recv().is_ok() {}
            let position = Message::known(Opcode::DraftSelectPosition, [5, 0, 0, 0, 0, 0]);
            let build = Message::known(Opcode::DraftSelectBuildPath, [2]);
            let before = room.roster_update(Some(0));
            for (req, expected) in [
                (position.clone(), [5, 255]),
                (build.clone(), [5, 2]),
                (build.clone(), [5, 2]),
                (Message::known(Opcode::DraftSelectPosition, [255]), [255, 2]),
            ] {
                let result = room.receive(1, 0, req.clone()).unwrap();
                assert_eq!(result.len(), 2);
                assert!(result[0].is(roster));
                assert_eq!(result[1], req);
                assert_eq!(&result[0].body[161 + 13..161 + 15], &expected);
                assert_eq!(&result[0].body[8..161 + 13], &before.body[8..161 + 13]);
                assert_eq!(&result[0].body[161 + 15..], &before.body[161 + 15..]);
                let Delivery::Private(batch) = other.try_recv().unwrap() else {
                    panic!("expected control delivery")
                };
                assert_eq!(batch, result);
            }
            for (seat, ch, msg) in [
                (1, 1, position.clone()),
                (2, 0, position.clone()),
                (1, 0, Message::known(Opcode::DraftSelectBuildPath, [3])),
                (
                    1,
                    0,
                    Message::known(Opcode::DraftSelectPosition, [5, 0, 7, 0, 0, 0]),
                ),
            ] {
                assert!(room.receive(seat, ch, msg).unwrap().is_empty());
            }
            room.detach(1, 0, generation);
            let (tx, _rx) = mpsc::channel(32);
            let (_, _, _, reopened) = room
                .attach(players[1], tx, watch::channel(false).0)
                .unwrap();
            assert_eq!(
                &reopened.iter().find(|m| m.is(roster)).unwrap().body[174..176],
                &[255, 2]
            );
            let clear = Message::known(Opcode::DraftSelectBuildPath, [255]);
            assert_eq!(
                &room.receive(1, 0, clear).unwrap()[0].body[174..176],
                &[255, 255]
            );
            room.phase(RoomPhase::Loading);
            assert!(room.receive(1, 0, position).unwrap().is_empty());
        }
    }
}
