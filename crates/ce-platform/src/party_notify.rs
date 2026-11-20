use halcyon_community::{Action, Community, GroupKind};
use halcyon_platform::AppState;
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{Error, wire::PartyNotice};

#[derive(PartialEq)]
struct Party {
    id: String,
    settings: (String, bool),
    members: Vec<(String, String, u8)>,
}

fn party(tx: &Transaction<'_>, actor: &str) -> rusqlite::Result<Option<Party>> {
    let group = tx
        .query_row(
            "SELECT g.id,g.mode,g.lobby_open FROM community_groups g
             JOIN community_members m ON g.id=m.group_id WHERE m.kind='party' AND m.player=?",
            [actor],
            |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?))),
        )
        .optional()?;
    let Some((id, settings)) = group else {
        return Ok(None);
    };
    let mut members = tx.prepare(
        "SELECT player,role,side FROM community_members WHERE group_id=? ORDER BY player",
    )?;
    let members = members
        .query_map([&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(Party {
        id,
        settings,
        members,
    }))
}

pub(crate) async fn execute(
    community: &Community,
    app: Option<&AppState>,
    actor: Uuid,
    action: Action,
) -> Result<Value, Error> {
    let Some(app) = app else {
        return Ok(community.execute(actor, action).await?);
    };
    let value = community
        .transaction(move |tx| {
            let id = actor.to_string();
            let before = party(tx, &id)?;
            let invite_target = match &action {
                Action::Invite { target, .. } => Some(target.to_string()),
                _ => None,
            };
            let existing_invite = if let (Some(group), Some(target)) = (&before, &invite_target) {
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM community_invites WHERE group_id=? AND player=?)",
                    params![group.id, target],
                    |row| row.get(0),
                )?
            } else {
                false
            };
            let resolves_invites = matches!(
                action,
                Action::ResolveInvite { .. } | Action::JoinPartyByCode { .. }
            );
            let kick = match &action {
                Action::Kick { target, .. } => Some(target.to_string()),
                _ => None,
            };
            let value = Community::execute_in_transaction(tx, actor, action)?;
            let after = if let Some(successor) = before.as_ref().and_then(|party| {
                party.members.iter().find(|(member, _, _)| member != &id)
            }) && value.is_null() {
                party(tx, &successor.0)?.filter(|next| {
                    before.as_ref().is_some_and(|old| old.id == next.id)
                })
            } else {
                party(tx, &id)?
            };
            if let Some(old) = &before {
                // Keep the recipient's cursor contiguous while withdrawing stale replays.
                tx.execute(
                    "UPDATE events SET body=json_object('type','party_refresh')
                     WHERE json_extract(body,'$.party')=?1 AND (
                        (json_extract(body,'$.type')=?2
                         AND NOT EXISTS (SELECT 1 FROM community_invites i
                             WHERE i.group_id=?1 AND i.player=events.player)) OR
                        (json_extract(body,'$.type') IN (?3,?4)
                         AND NOT EXISTS (SELECT 1 FROM community_members m
                             WHERE m.group_id=?1 AND m.player=events.player)))",
                    params![old.id, PartyNotice::JoinRequest.as_str(), PartyNotice::StateUpdate.as_str(), PartyNotice::PlayerLeft.as_str()],
                )?;
            }
            if resolves_invites {
                tx.execute(
                    "UPDATE events SET body=json_object('type','party_refresh')
                     WHERE player=?1 AND json_extract(body,'$.type')=?2
                       AND NOT EXISTS (SELECT 1 FROM community_invites i WHERE i.player=events.player
                           AND i.group_id=json_extract(events.body,'$.party'))",
                    params![id, PartyNotice::JoinRequest.as_str()],
                )?;
            }
            if let (Some(group), Some(target), false) = (&after, invite_target, existing_invite) {
                notice(tx, &target, PartyNotice::JoinRequest, Some(&group.id))?;
            }
            if let Some(target) = kick {
                notice(tx, &target, PartyNotice::MemberKick, None)?;
            }
            if before != after {
                let left = before.as_ref().is_some_and(|group| {
                    group.members.iter().any(|(member, _, _)| member == &id)
                }) && after.as_ref().is_none_or(|group| {
                    !group.members.iter().any(|(member, _, _)| member == &id)
                });
                if let Some(group) = &after {
                    for (member, _, _) in &group.members {
                        notice(
                            tx,
                            member,
                            if left { PartyNotice::PlayerLeft } else { PartyNotice::StateUpdate },
                            Some(&group.id),
                        )?;
                    }
                } else if let Some(group) = &before {
                    for (member, _, _) in &group.members {
                        notice(tx, member, PartyNotice::StateUpdate, None)?;
                    }
                }
            }
            Ok(value)
        })
        .await?;
    app.wake_notification_readers();
    Ok(value)
}

fn notice(
    tx: &Transaction<'_>,
    player: &str,
    kind: PartyNotice,
    party: Option<&str>,
) -> rusqlite::Result<()> {
    let event =
        serde_json::to_value(StoredPartyNotice { kind, party }).expect("party notice serializes");
    halcyon_platform::record_external_notification(tx, player, &event)
}

pub(crate) fn is_mutation(action: &Action) -> bool {
    matches!(
        action,
        Action::CreateGroup {
            kind: GroupKind::Party,
            ..
        } | Action::JoinPartyByCode { .. }
            | Action::DisbandGroup {
                kind: GroupKind::Party
            }
            | Action::LeaveGroup {
                kind: GroupKind::Party
            }
            | Action::Invite {
                kind: GroupKind::Party,
                ..
            }
            | Action::Kick {
                kind: GroupKind::Party,
                ..
            }
            | Action::MovePartyMember { .. }
            | Action::BalanceParty
            | Action::UpdatePartyState { .. }
            | Action::UpdateGroup {
                kind: GroupKind::Party,
                ..
            }
            | Action::SetPartyLobby { .. }
    )
}

#[derive(serde::Serialize)]
struct StoredPartyNotice<'a> {
    #[serde(rename = "type")]
    kind: PartyNotice,
    party: Option<&'a str>,
}
