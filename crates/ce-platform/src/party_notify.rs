use diesel::{prelude::*, sql_types::Text};
use halcyon_community::{Action, Community, GroupKind, party_invited, party_snapshot};
use halcyon_platform::AppState;
use serde_json::Value;
use uuid::Uuid;

use crate::{Error, wire::PartyNotice};

// The platform owns the table and its cursor trigger. Both drivers insert the same columns.
diesel::table! {
    events (cursor) {
        cursor -> BigInt,
        player -> Text,
        body -> Text,
    }
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
        .orm_transaction(move |tx| {
            let id = actor.to_string();
            let before = party_snapshot(tx, &id)?;
            let invite_target = match &action {
                Action::Invite { target, .. } => Some(target.to_string()),
                _ => None,
            };
            let existing_invite = if let (Some(group), Some(target)) = (&before, &invite_target) {
                party_invited(tx, &group.id, target)?
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
                party_snapshot(tx, &successor.0)?.filter(|next| {
                    before.as_ref().is_some_and(|old| old.id == next.id)
                })
            } else {
                party_snapshot(tx, &id)?
            };
            if let Some(old) = &before {
                // Keep the recipient's cursor contiguous while withdrawing stale replays.
                diesel::sql_query(
                    "UPDATE events SET body=json_object('type','party_refresh')
                     WHERE json_extract(body,'$.party')=?1 AND (
                        (json_extract(body,'$.type')=?2
                         AND NOT EXISTS (SELECT 1 FROM community_invites i
                             WHERE i.group_id=?1 AND i.player=events.player)) OR
                        (json_extract(body,'$.type') IN (?3,?4)
                         AND NOT EXISTS (SELECT 1 FROM community_members m
                             WHERE m.group_id=?1 AND m.player=events.player)))",
                )
                .bind::<Text, _>(&old.id)
                .bind::<Text, _>(PartyNotice::JoinRequest.as_str())
                .bind::<Text, _>(PartyNotice::StateUpdate.as_str())
                .bind::<Text, _>(PartyNotice::PlayerLeft.as_str())
                .execute(tx)?;
            }
            if resolves_invites {
                diesel::sql_query(
                    "UPDATE events SET body=json_object('type','party_refresh')
                     WHERE player=?1 AND json_extract(body,'$.type')=?2
                       AND NOT EXISTS (SELECT 1 FROM community_invites i WHERE i.player=events.player
                           AND i.group_id=json_extract(events.body,'$.party'))",
                )
                .bind::<Text, _>(&id)
                .bind::<Text, _>(PartyNotice::JoinRequest.as_str())
                .execute(tx)?;
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
    tx: &mut SqliteConnection,
    player: &str,
    kind: PartyNotice,
    party: Option<&str>,
) -> halcyon_community::Result<()> {
    let event = serde_json::to_string(&StoredPartyNotice { kind, party })?;
    diesel::insert_into(events::table)
        .values((events::player.eq(player), events::body.eq(event)))
        .execute(tx)?;
    Ok(())
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
