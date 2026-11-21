use crate::auth::profile::{self, Currency, TutorialState};
use crate::rpc::wire::{Empty, PlayerHandle, Success};
use crate::store::{
    match_stats::{PlayerMatches, read_player_matches},
    meta::{Meta, Preference},
};
use crate::{Error, Platform, RpcMethod};
use diesel::{
    dsl::exists,
    prelude::*,
    sql_types::{BigInt, Bool, Text},
    upsert::excluded,
};
use halcyon_community::GroupKind;
use halcyon_platform::PlayerProfile;
use halcyon_storage::schema::{
    ce_honor, ce_match_handles, ce_messages, ce_pending_gifts, ce_preferences, ce_scores,
    ce_wait_history, community_friends, community_groups, community_inventory, community_invites,
    community_items, community_members, events,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

diesel::define_sql_function!(#[sql_name = "max"] fn max_score(a: BigInt, b: BigInt) -> BigInt);

fn field<'a>(params: &'a [Value], pos: usize, names: &[&str]) -> Result<&'a Value, Error> {
    if let Some(obj) = params.get(1).and_then(Value::as_object) {
        names.iter().find_map(|k| obj.get(*k)).ok_or(Error::Invalid)
    } else {
        params.get(pos).ok_or(Error::Invalid)
    }
}

fn text(params: &[Value], pos: usize, names: &[&str], max: usize) -> Result<String, Error> {
    field(params, pos, names)?
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(Error::Invalid)
}

fn id(params: &[Value], pos: usize, names: &[&str]) -> Result<Uuid, Error> {
    Uuid::parse_str(&text(params, pos, names, 36)?).map_err(|_| Error::Invalid)
}

pub(crate) fn handles(m: RpcMethod) -> bool {
    matches!(
        m,
        RpcMethod::GetPlayerInfos
            | RpcMethod::EstimatedWaitTime
            | RpcMethod::AchievementGetScore
            | RpcMethod::AchievementStoreScore
            | RpcMethod::AddDeviceToken
            | RpcMethod::QueryPlayerInboxMessages
            | RpcMethod::ProcessMessage
            | RpcMethod::DismissReliableMessage
            | RpcMethod::FriendNotify
            | RpcMethod::AskInGameFriendToPlay
            | RpcMethod::FriendReplyFromInGame
            | RpcMethod::StoreRecordPendingGift
            | RpcMethod::StoreCancelPendingGift
            | RpcMethod::NotifyGameResults
            | RpcMethod::UpdateMatchInfo
            | RpcMethod::ReportHonorPlayer
    )
}

impl Meta {
    pub(crate) async fn player_fields(&self, player: Uuid) -> Result<PlayerActivityFields, Error> {
        self.read(move |db| {
            let player = player.to_string();
            let matches = read_player_matches(db, &player)?;
            let balances = community_inventory::table
                .inner_join(community_items::table)
                .filter(community_inventory::player.eq(&player))
                .filter(community_items::slot.eq("currency"))
                .select((community_inventory::item, community_inventory::count))
                .load::<(String, i64)>(db)?;
            let mut currency = Currency::default();
            currency.extend(
                balances
                    .into_iter()
                    .map(|(item, count)| (item, count as u32)),
            );
            let pending_messages = ce_messages::table
                .filter(ce_messages::recipient.eq(&player))
                .filter(ce_messages::dismissed.eq(false))
                .count()
                .get_result::<i64>(db)? as u32;
            let mut info = PlayerActivityFields {
                matches,
                currency,
                pending: PendingCounts {
                    messages: pending_messages,
                    ..Default::default()
                },
                ..Default::default()
            };
            let invites = community_invites::table
                .inner_join(community_groups::table)
                .filter(community_invites::player.eq(&player))
                .select(community_groups::kind)
                .load::<String>(db)?;
            for kind in invites {
                match GroupKind::deserialize(Value::String(kind)) {
                    Ok(GroupKind::Party) => *info.pending.party.get_or_insert(0) += 1,
                    Ok(GroupKind::Team) => *info.pending.team.get_or_insert(0) += 1,
                    Ok(GroupKind::Guild) => *info.pending.guild.get_or_insert(0) += 1,
                    Err(_) => {}
                }
            }
            let groups = community_members::table
                .filter(community_members::player.eq(&player))
                .select((community_members::kind, community_members::group_id))
                .load::<(String, String)>(db)?;
            for (kind, id) in groups {
                match GroupKind::deserialize(Value::String(kind)) {
                    Ok(GroupKind::Party) => info.party_uuid = Some(Some(id)),
                    Ok(GroupKind::Team) => info.team_uuid = Some(Some(id)),
                    Ok(GroupKind::Guild) => info.guild_uuid = Some(Some(id)),
                    Err(_) => {}
                }
            }
            let prefs = ce_preferences::table
                .filter(ce_preferences::player.eq(&player))
                .filter(ce_preferences::category.eq_any(["tutorial", "last_login"]))
                .select((ce_preferences::category, ce_preferences::data))
                .load::<(String, String)>(db)?;
            for (category, raw) in prefs {
                let data = serde_json::from_str(&raw)?;
                if category == Preference::LastLogin.as_str() {
                    info.last_login = Some(data);
                } else {
                    info.tutorial_state = profile::tutorial_state(&data);
                }
            }
            Ok(info)
        })
        .await
    }
}

fn match_finished_rosters(
    db: &mut diesel::sqlite::SqliteConnection,
    actor: &str,
    room: &str,
) -> Result<Vec<Vec<String>>, Error> {
    let raw_events = events::table
        .filter(events::player.eq(actor))
        .select(events::body)
        .load::<String>(db)?;
    let mut rosters = Vec::new();
    for raw in raw_events {
        let Ok(body) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if body.get("type").and_then(Value::as_str) != Some("match_finished") {
            continue;
        }
        if body.get("matchId").and_then(Value::as_str) != Some(room) {
            continue;
        }
        let Some(players) = body
            .get("result")
            .and_then(|r| r.get("players"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        rosters.push(players.keys().cloned().collect());
    }
    Ok(rosters)
}

impl Platform {
    pub(crate) async fn activity_rpc(
        &self,
        profile: &PlayerProfile,
        m: RpcMethod,
        params: &[Value],
    ) -> Result<Value, Error> {
        let actor = profile.player_id.to_string();
        match m {
            RpcMethod::GetPlayerInfos => {
                let ids = field(params, 1, &["playerUUIDs", "players"])?
                    .as_array()
                    .filter(|ids| ids.len() <= 50)
                    .ok_or(Error::Invalid)?;
                let mut profiles = Vec::new();
                for id in ids {
                    let key = Uuid::parse_str(id.as_str().ok_or(Error::Invalid)?)
                        .map_err(|_| Error::Invalid)?;
                    if let Some(activity) = self.app.player_activity(key).await? {
                        profiles.push(PlayerHandle {
                            player_uuid: key,
                            handle: activity.profile.name,
                        });
                    }
                }
                Ok(serde_json::to_value(PlayerHandles {
                    player_infos: profiles,
                })?)
            }
            RpcMethod::EstimatedWaitTime => {
                let mode = text(params, 1, &["lobby", "mode", "queueName"], 128)?;
                self.meta
                    .read(move |db| {
                        let wait = ce_wait_history::table
                            .find(&mode)
                            .select(ce_wait_history::seconds)
                            .first::<f64>(db)
                            .optional()?;
                        Ok(serde_json::to_value(WaitEstimate {
                            estimated_wait: wait.unwrap_or(0.0).round() as u64,
                        })?)
                    })
                    .await
            }
            RpcMethod::AchievementGetScore => {
                let key = text(params, 1, &["achievementId", "achievement"], 128)?;
                self.meta
                    .read(move |db| {
                        let score = ce_scores::table
                            .find((&actor, &key))
                            .select(ce_scores::score)
                            .first::<i64>(db)
                            .optional()?;
                        Ok(serde_json::to_value(AchievementScore {
                            achievement_id: key,
                            score: score.unwrap_or(0),
                        })?)
                    })
                    .await
            }
            RpcMethod::AchievementStoreScore => {
                let key = text(params, 1, &["achievementId", "achievement"], 128)?;
                let score = field(params, 2, &["score"])?
                    .as_i64()
                    .filter(|n| *n >= 0)
                    .ok_or(Error::Invalid)?;
                self.meta
                    .write(move |db| {
                        diesel::insert_into(ce_scores::table)
                            .values((
                                ce_scores::player.eq(&actor),
                                ce_scores::achievement.eq(&key),
                                ce_scores::score.eq(score),
                            ))
                            .on_conflict((ce_scores::player, ce_scores::achievement))
                            .do_update()
                            .set(
                                ce_scores::score
                                    .eq(max_score(ce_scores::score, excluded(ce_scores::score))),
                            )
                            .execute(db)?;
                        let current = ce_scores::table
                            .find((&actor, &key))
                            .select(ce_scores::score)
                            .first::<i64>(db)
                            .optional()?;
                        Ok(serde_json::to_value(AchievementScore {
                            achievement_id: key,
                            score: current.unwrap_or(0),
                        })?)
                    })
                    .await
            }
            RpcMethod::AddDeviceToken => {
                let token = text(params, 1, &["deviceToken", "token"], 4096)?;
                self.meta
                    .set(
                        profile.player_id,
                        Preference::DeviceToken,
                        Value::from(token),
                    )
                    .await?;
                Ok(serde_json::to_value(Success { success: true })?)
            }
            RpcMethod::QueryPlayerInboxMessages => {
                self.meta
                    .read(move |db| {
                        let entries = ce_messages::table
                            .filter(ce_messages::recipient.eq(&actor))
                            .filter(ce_messages::dismissed.eq(false))
                            .order((
                                ce_messages::created,
                                diesel::dsl::sql::<BigInt>("ce_messages.rowid"),
                                ce_messages::id,
                            ))
                            .limit(100)
                            .select((
                                ce_messages::id,
                                ce_messages::body,
                                ce_messages::sender,
                                ce_messages::created,
                            ))
                            .load::<(String, String, String, i64)>(db)?
                            .into_iter()
                            .map(|(message_id, message, player_uuid, time)| InboxMessage {
                                message_id,
                                message,
                                player_uuid,
                                time,
                            })
                            .collect::<Vec<_>>();
                        Ok(serde_json::to_value(Inbox {
                            pending_messages: !entries.is_empty(),
                            messages: entries,
                        })?)
                    })
                    .await
            }
            RpcMethod::ProcessMessage | RpcMethod::DismissReliableMessage => {
                let message = id(params, 1, &["messageId", "id"])?.to_string();
                self.meta
                    .write(move |db| {
                        let changed = diesel::update(
                            ce_messages::table
                                .filter(ce_messages::recipient.eq(&actor))
                                .filter(ce_messages::id.eq(&message))
                                .filter(ce_messages::dismissed.eq(false)),
                        )
                        .set(ce_messages::dismissed.eq(true))
                        .execute(db)?;
                        if changed == 0 {
                            let already_dismissed = diesel::select(exists(
                                ce_messages::table
                                    .filter(ce_messages::recipient.eq(&actor))
                                    .filter(ce_messages::id.eq(&message))
                                    .filter(ce_messages::dismissed.eq(true)),
                            ))
                            .get_result::<bool>(db)?;
                            if !already_dismissed {
                                return Err(Error::Unavailable("no such message"));
                            }
                        }
                        Ok(serde_json::to_value(Success { success: true })?)
                    })
                    .await
            }
            RpcMethod::FriendNotify
            | RpcMethod::AskInGameFriendToPlay
            | RpcMethod::FriendReplyFromInGame => {
                let target = id(params, 1, &["playerUUID", "targetUUID"])?.to_string();
                let message = if m == RpcMethod::AskInGameFriendToPlay {
                    m.as_str().to_owned()
                } else {
                    text(params, 2, &["message", "text"], 512)?
                };
                let notice = serde_json::to_string(&InboxNotice {
                    kind: "inbox_message",
                })?;
                let sent = self
                    .meta
                    .write(move |db| {
                        let friends = diesel::select(exists(
                            community_friends::table.find((&actor, &target)),
                        ))
                        .get_result::<bool>(db)?;
                        if !friends {
                            return Err(Error::Community(
                                halcyon_community::CommunityError::Forbidden,
                            ));
                        }
                        let pending = ce_messages::table
                            .filter(ce_messages::recipient.eq(&target))
                            .filter(ce_messages::dismissed.eq(false))
                            .count()
                            .get_result::<i64>(db)?;
                        if pending >= 100 {
                            return Err(Error::Unavailable("inbox full"));
                        }
                        let id = Uuid::new_v4();
                        diesel::insert_into(ce_messages::table)
                            .values((
                                ce_messages::id.eq(id.to_string()),
                                ce_messages::sender.eq(&actor),
                                ce_messages::recipient.eq(&target),
                                ce_messages::body.eq(&message),
                            ))
                            .execute(db)?;
                        diesel::insert_into(events::table)
                            .values((events::player.eq(&target), events::body.eq(&notice)))
                            .execute(db)?;
                        Ok(serde_json::to_value(MessageSent {
                            message_id: id,
                            success: true,
                        })?)
                    })
                    .await?;
                self.app.wake_notification_readers();
                Ok(sent)
            }
            RpcMethod::StoreRecordPendingGift | RpcMethod::StoreCancelPendingGift => {
                let target = id(params, 1, &["recipient", "playerUUID"])?;
                let sku = text(params, 2, &["product_id", "sku"], 128)?;
                if target == profile.player_id || self.app.player_activity(target).await?.is_none()
                {
                    return Err(Error::Invalid);
                }
                let record = m == RpcMethod::StoreRecordPendingGift;
                let target_str = target.to_string();
                self.meta
                    .write(move |db| {
                        if record {
                            diesel::insert_into(ce_pending_gifts::table)
                                .values((
                                    ce_pending_gifts::player.eq(&actor),
                                    ce_pending_gifts::target.eq(&target_str),
                                    ce_pending_gifts::sku.eq(&sku),
                                ))
                                .on_conflict_do_nothing()
                                .execute(db)?;
                        } else {
                            diesel::delete(ce_pending_gifts::table.find((
                                &actor,
                                &target_str,
                                &sku,
                            )))
                            .execute(db)?;
                        }
                        Ok(serde_json::to_value(Success { success: true })?)
                    })
                    .await
            }
            RpcMethod::ReportHonorPlayer => {
                let room = id(params, 1, &["matchId"])?.to_string();
                let identifier = text(params, 2, &["playerUUID", "targetUUID", "handle"], 128)?;
                let kind = text(params, 3, &["honor", "reason"], 64)?;
                self.meta
                    .write(move |db| {
                        let rosters = match_finished_rosters(db, &actor, &room)?;
                        let uuid = Uuid::parse_str(&identifier).ok();
                        let target = if let Some(id) = uuid {
                            id.to_string()
                        } else {
                            let mut complete = false;
                            for seats in &rosters {
                                if seats.is_empty() {
                                    complete = true;
                                    break;
                                }
                                let found = ce_match_handles::table
                                    .filter(ce_match_handles::room.eq(&room))
                                    .filter(ce_match_handles::player.eq_any(seats))
                                    .count()
                                    .get_result::<i64>(db)?;
                                if found as usize == seats.len() {
                                    complete = true;
                                    break;
                                }
                            }
                            if !complete {
                                return Err(Error::Unavailable("match handle roster incomplete"));
                            }
                            let hits = ce_match_handles::table
                                .filter(ce_match_handles::room.eq(&room))
                                .filter(
                                    diesel::dsl::sql::<Bool>("ce_match_handles.handle = ")
                                        .bind::<Text, _>(&identifier)
                                        .sql(" COLLATE NOCASE"),
                                )
                                .limit(2)
                                .select(ce_match_handles::player)
                                .load::<String>(db)?;
                            let [one] = hits.as_slice() else {
                                return Err(Error::Invalid);
                            };
                            one.clone()
                        };
                        if actor == target {
                            return Err(Error::Invalid);
                        }
                        let evidence = rosters
                            .iter()
                            .any(|seats| seats.iter().any(|seat| seat == &target));
                        if !evidence {
                            return Err(Error::Unavailable("no shared completed match"));
                        }
                        diesel::insert_into(ce_honor::table)
                            .values((
                                ce_honor::match_id.eq(&room),
                                ce_honor::source.eq(&actor),
                                ce_honor::target.eq(&target),
                                ce_honor::kind.eq(&kind),
                            ))
                            .on_conflict_do_nothing()
                            .execute(db)?;
                        Ok(if uuid.is_some() {
                            serde_json::to_value(Success { success: true })?
                        } else {
                            serde_json::to_value(Empty {})?
                        })
                    })
                    .await
            }
            RpcMethod::NotifyGameResults | RpcMethod::UpdateMatchInfo => {
                let room = id(params, 1, &["matchId"])?;
                if profile.lobby.as_ref().and_then(|l| l.match_id) != Some(room) {
                    return Err(Error::Invalid);
                }
                // Client telemetry is separate from the authoritative host outcome.
                self.meta
                    .set(
                        profile.player_id,
                        if m == RpcMethod::NotifyGameResults {
                            Preference::ReportedResult
                        } else {
                            Preference::ReportedMatch
                        },
                        Value::Array(params[1..].to_vec()),
                    )
                    .await?;
                self.project_player_info(profile).await
            }
            _ => Err(Error::Unsupported),
        }
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerActivityFields {
    #[serde(flatten)]
    pub matches: PlayerMatches,
    pub currency: Currency,
    #[serde(flatten)]
    pub pending: PendingCounts,
    #[serde(rename = "partyUUID", skip_serializing_if = "Option::is_none")]
    party_uuid: Option<Option<String>>,
    #[serde(rename = "teamUUID", skip_serializing_if = "Option::is_none")]
    team_uuid: Option<Option<String>>,
    #[serde(rename = "guildUUID", skip_serializing_if = "Option::is_none")]
    guild_uuid: Option<Option<String>>,
    #[serde(rename = "last_login", skip_serializing_if = "Option::is_none")]
    last_login: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tutorial_state: Option<TutorialState>,
}

impl PlayerActivityFields {
    // A complete profile materializes defaults. A configured manifest only
    // receives the fields for which the home has an authoritative value.
    pub(crate) fn with_defaults(mut self) -> Self {
        self.matches = self.matches.with_streak_defaults();
        self.party_uuid.get_or_insert(None);
        self.team_uuid.get_or_insert(None);
        self.guild_uuid.get_or_insert(None);
        self.last_login.get_or_insert_with(|| Value::from(0.0));
        self.tutorial_state.get_or_insert_with(Default::default);
        self.pending = self.pending.with_defaults();
        self
    }
}

#[derive(Clone, Copy, Default, Serialize)]
pub(crate) struct PendingCounts {
    #[serde(rename = "pendingMessages")]
    messages: u32,
    #[serde(
        rename = "pendingPartyInvites",
        skip_serializing_if = "Option::is_none"
    )]
    party: Option<u32>,
    #[serde(rename = "pendingTeamInvites", skip_serializing_if = "Option::is_none")]
    team: Option<u32>,
    #[serde(
        rename = "pendingGuildInvites",
        skip_serializing_if = "Option::is_none"
    )]
    guild: Option<u32>,
}

impl PendingCounts {
    pub(crate) fn with_defaults(mut self) -> Self {
        self.party.get_or_insert(0);
        self.team.get_or_insert(0);
        self.guild.get_or_insert(0);
        self
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerHandles {
    player_infos: Vec<PlayerHandle>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WaitEstimate {
    estimated_wait: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AchievementScore {
    achievement_id: String,
    score: i64,
}

#[derive(Serialize)]
struct InboxNotice {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageSent {
    message_id: Uuid,
    success: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Inbox {
    messages: Vec<InboxMessage>,
    pending_messages: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InboxMessage {
    message_id: String,
    message: String,
    #[serde(rename = "playerUUID")]
    player_uuid: String,
    time: i64,
}
