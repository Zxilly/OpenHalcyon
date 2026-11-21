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
        use halcyon_sim::{Command, ItemTarget, Position};
        let command = match r.action {
            PlayerAction::ActivateReflexBlock { instance } => {
                Command::ActivateReflexBlock { instance }
            }
            PlayerAction::ActivateItem {
                instance,
                target_actor,
                x,
                z,
            } => Command::ActivateItem {
                instance,
                target: match (target_actor, x, z) {
                    (Some(target_id), _, _) => ItemTarget::Entity(target_id),
                    (None, Some(px), Some(pz)) => ItemTarget::Point(Position { x: px, y: pz }),
                    _ => ItemTarget::SelfCast,
                },
            },
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
#[path = "player_actions_tests.rs"]
mod tests;
