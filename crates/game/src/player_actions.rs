use super::*;
use halcyon_protocol::player_actions::*;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Receipts(VecDeque<(ActionRequest, ActionDecision)>);
impl Receipts {
    fn next(&self) -> u64 {
        self.0.back().map_or(1, |(r, _)| r.sequence + 1)
    }
    fn previous(&self, r: ActionRequest) -> Option<ActionDecision> {
        if r.sequence >= self.next() {
            return None;
        }
        Some(
            self.0
                .iter()
                .find(|(old, _)| *old == r)
                .map_or(ActionDecision::Stale, |(_, d)| *d),
        )
    }
    fn record(&mut self, r: ActionRequest, d: ActionDecision) {
        if self.0.len() >= 16 {
            self.0.pop_front();
        }
        self.0.push_back((r, d));
    }
}
impl Rooms {
    pub fn room_action(
        &self,
        room: Uuid,
        player: Uuid,
        origin: Option<(Uuid, Uuid)>,
        call: ActionCall,
    ) -> ActionReply {
        let no = ActionReply::Decision(ActionDecision::Unavailable);
        let Some(shared) = self.room(room) else {
            return no;
        };
        let Ok(mut room) = shared.lock() else {
            return no;
        };
        let Some(seat) = room.players.iter().position(|p| *p == player) else {
            return no;
        };
        let authorized = match (&room.federation, origin) {
            (None, None) => true,
            (Some(f), Some((pool, home))) => f.pool == pool && f.homes.get(seat) == Some(&home),
            _ => false,
        };
        if !authorized
            || !room.admitted
            || room.phase != RoomPhase::Running
            || room.world.winner.is_some()
        {
            return no;
        }
        let hero = &room.world.entities[seat];
        let actor = hero.id;
        let ActionCall::Execute(r) = call else {
            let items = if let Some(slots) = &hero.aral_inventory {
                slots
                    .slots()
                    .iter()
                    .flatten()
                    .map(|h| HeldInstance {
                        template: h.template,
                        instance: h.instance,
                    })
                    .collect()
            } else {
                hero.inventory
                    .iter()
                    .map(|h| HeldInstance {
                        template: h.kind.template(),
                        instance: h.instance,
                    })
                    .collect()
            };
            return ActionReply::Context(ActionContext {
                x: hero.position.x,
                z: hero.position.y,
                next_sequence: room.seats[seat].action_receipts.next(),
                items,
                visible_enemy_cams: room.world.visible_enemy_cams(actor),
            });
        };
        if !r.valid_at(Utc::now().timestamp_millis()) {
            return ActionReply::Decision(ActionDecision::Invalid);
        }
        if let Some(old) = room.seats[seat].action_receipts.previous(r) {
            return ActionReply::Decision(old);
        }
        if r.sequence != room.seats[seat].action_receipts.next() {
            return ActionReply::Decision(ActionDecision::Stale);
        }
        use halcyon_sim::{Command, Position};
        let command = match r.action {
            PlayerAction::ConsumeInfusion { instance } => Command::ConsumeInfusion { instance },
            PlayerAction::DeployScoutTrap { instance } => Command::DeployScoutTrap { instance },
            PlayerAction::DeployVisionTotem { instance, x, z } => Command::DeployVisionTotem {
                instance,
                point: Position { x, y: z },
            },
            PlayerAction::Sell { instance } => Command::Sell { instance },
            PlayerAction::HitScoutCam { device } => Command::HitScoutCam { device },
        };
        let d = if room.world.command(actor, command).is_ok() {
            ActionDecision::Applied
        } else {
            ActionDecision::Rejected
        };
        room.seats[seat].action_receipts.record(r, d);
        ActionReply::Decision(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn remote_room_checks_exact_home_pool_and_seat_after_routing() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let room = Uuid::new_v4();
        let pool = Uuid::new_v4();
        let players = [Uuid::new_v4(), Uuid::new_v4()];
        let homes = [Uuid::new_v4(), Uuid::new_v4()];
        let req = Reservation {
            match_id: room,
            players: players.to_vec(),
            teams: vec![],
            mode: "5v5_pvp_casual".into(),
            difficulty: None,
            talent_levels: Vec::new(),
        };
        rooms.prepare_federated(req, pool, homes.to_vec()).unwrap();
        let read = |p, origin| rooms.room_action(room, p, origin, ActionCall::Inspect);
        assert!(matches!(
            read(players[0], Some((pool, homes[0]))),
            ActionReply::Decision(ActionDecision::Unavailable)
        ));
        rooms.activate(room).unwrap();
        rooms
            .room(room)
            .unwrap()
            .lock()
            .unwrap()
            .phase(RoomPhase::Running);
        assert!(matches!(
            read(players[0], Some((pool, homes[0]))),
            ActionReply::Context(_)
        ));
        for (player, origin) in [
            (players[0], None),
            (players[0], Some((pool, homes[1]))),
            (players[1], Some((pool, homes[0]))),
            (players[0], Some((Uuid::new_v4(), homes[0]))),
            (Uuid::new_v4(), Some((pool, homes[0]))),
        ] {
            assert!(matches!(
                read(player, origin),
                ActionReply::Decision(ActionDecision::Unavailable)
            ));
        }
    }

    #[tokio::test]
    async fn player_actions_bind_identity_inventory_visibility_and_retries() {
        let rooms = Rooms::new("127.0.0.1".parse().unwrap(), "127.0.0.1".into(), 1, None);
        let room = Uuid::new_v4();
        let players: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        rooms
            .reserve(Reservation {
                match_id: room,
                mode: "casual_aral".into(),
                players: players.clone(),
                teams: vec![0, 1, 0, 1, 0, 1],
                difficulty: None,
                talent_levels: Vec::new(),
            })
            .unwrap();
        let shared = rooms.room(room).unwrap();
        let caller = players[0];
        let inspect = |p| match rooms.room_action(room, p, None, ActionCall::Inspect) {
            ActionReply::Context(c) => c,
            _ => panic!("missing context"),
        };
        let execute = |req| match rooms.room_action(room, caller, None, ActionCall::Execute(req)) {
            ActionReply::Decision(d) => d,
            _ => panic!("missing decision"),
        };
        let (pos, instance) = {
            let mut state = shared.lock().unwrap();
            state.phase(RoomPhase::Running);
            let actor = state.world.entities[0].id;
            let purchase = state
                .world
                .purchase(actor, halcyon_sim::BasicItem::ScoutTrap)
                .unwrap();
            assert_eq!(state.world.entities[0].gold, 1950);
            (state.world.entities[0].position, purchase.granted.instance)
        };
        assert!(matches!(
            rooms.room_action(room, Uuid::new_v4(), None, ActionCall::Inspect),
            ActionReply::Decision(ActionDecision::Unavailable)
        ));
        assert!(matches!(
            rooms.room_action(
                room,
                caller,
                Some((Uuid::new_v4(), Uuid::new_v4())),
                ActionCall::Inspect
            ),
            ActionReply::Decision(ActionDecision::Unavailable)
        ));
        let now = Utc::now().timestamp_millis();
        let sale = ActionRequest {
            sequence: 1,
            expires_at_ms: now + 25_000,
            action: PlayerAction::Sell { instance },
        };
        assert_eq!(
            execute(ActionRequest {
                expires_at_ms: now - 1,
                ..sale
            }),
            ActionDecision::Invalid
        );
        assert_eq!(inspect(caller).next_sequence, 1);
        assert_eq!(execute(sale), ActionDecision::Applied);
        assert_eq!(execute(sale), ActionDecision::Applied);
        assert_eq!(shared.lock().unwrap().world.entities[0].gold, 1975);
        assert_eq!(
            execute(ActionRequest {
                action: PlayerAction::DeployScoutTrap { instance },
                ..sale
            }),
            ActionDecision::Stale
        );
        let cam = ActionRequest {
            sequence: 2,
            action: PlayerAction::DeployVisionTotem {
                instance: 2001,
                x: pos.x,
                z: pos.y,
            },
            ..sale
        };
        assert_eq!(execute(cam), ActionDecision::Applied);
        let device = shared.lock().unwrap().world.vision_devices()[0].id;
        assert!(inspect(players[1]).visible_enemy_cams.is_empty());
        let guess = ActionRequest {
            sequence: 1,
            action: PlayerAction::HitScoutCam { device },
            ..sale
        };
        let hit = |req| match rooms.room_action(room, players[1], None, ActionCall::Execute(req)) {
            ActionReply::Decision(d) => d,
            _ => panic!(),
        };
        assert_eq!(hit(guess), ActionDecision::Rejected);
        assert_eq!(
            hit(ActionRequest {
                sequence: 2,
                action: PlayerAction::HitScoutCam { device: device + 1 },
                ..guess
            }),
            ActionDecision::Rejected
        );
        {
            let mut state = shared.lock().unwrap();
            state.world.entities[1].position = pos;
        }
        assert_eq!(inspect(players[1]).visible_enemy_cams, [device]);
        assert_eq!(
            hit(ActionRequest {
                sequence: 3,
                ..guess
            }),
            ActionDecision::Applied
        );
        for seq in 3..21 {
            assert_eq!(
                execute(ActionRequest {
                    sequence: seq,
                    ..sale
                }),
                ActionDecision::Rejected
            );
        }
        assert_eq!(execute(sale), ActionDecision::Stale);
        shared.lock().unwrap().phase(RoomPhase::Ended);
        assert_eq!(execute(cam), ActionDecision::Unavailable);
    }
}
