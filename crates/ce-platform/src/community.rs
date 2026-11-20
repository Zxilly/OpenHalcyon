//! Community projection for cataloged RPC names. The friend-list envelope is
//! captured, along with empty native guild/team responses. Populated local
//! guild/team projections still need validation against original clients.

use crate::RpcMethod;
use std::collections::BTreeMap;

use crate::wire::Empty;
use halcyon_community::views::{
    Friends, GroupView, Inventory, InventoryItem, Items, Quest, QuestStatus,
};
use halcyon_community::{Action, Community, GroupKind, GroupSettings, Role};
use halcyon_platform::{AppState, PlayerProfile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::Error;

struct Args<'a> {
    entries: &'a [Value],
    structured: Option<Value>,
}

impl<'a> Args<'a> {
    /// Positional scalars and the equivalent named payload share the same validation.
    fn new(params: &'a [Value]) -> Result<Self, Error> {
        let entries = params.get(1..).ok_or(Error::Invalid)?;
        let structured = match entries.first() {
            Some(value) if value.is_object() => Some(value.clone()),
            Some(Value::String(raw)) if raw.trim_start().starts_with('{') => {
                Some(serde_json::from_str::<Value>(raw).map_err(|_| Error::Invalid)?)
            }
            _ => None,
        };
        Ok(Self {
            entries,
            structured,
        })
    }

    fn field(&self, position: usize, keys: &[&str]) -> Option<&Value> {
        match &self.structured {
            Some(value) => keys.iter().find_map(|key| value.get(*key)),
            None => self.entries.get(position),
        }
    }

    fn text(&self, position: usize, keys: &[&str]) -> Result<String, Error> {
        self.field(position, keys)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(Error::Invalid)
    }

    fn flag(&self, position: usize, keys: &[&str]) -> Result<bool, Error> {
        match self.field(position, keys) {
            Some(Value::Bool(value)) => Ok(*value),
            Some(Value::Number(value)) if value.as_u64() == Some(0) => Ok(false),
            Some(Value::Number(value)) if value.as_u64() == Some(1) => Ok(true),
            _ => Err(Error::Invalid),
        }
    }

    fn id(&self, position: usize, keys: &[&str]) -> Result<Uuid, Error> {
        Uuid::parse_str(&self.text(position, keys)?).map_err(|_| Error::Invalid)
    }

    fn item(&self, position: usize, keys: &[&str]) -> Result<Option<String>, Error> {
        match self.field(position, keys) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(value)) if value.is_empty() => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            _ => Err(Error::Invalid),
        }
    }
}

/// The archive only establishes this browse preset and 1-based page numbers.
fn native_guild_page(args: &Args<'_>) -> Result<Option<u32>, Error> {
    let (Some(Value::String(_)), Some(Value::Object(options))) =
        (args.entries.first(), args.structured.as_ref())
    else {
        return Ok(None);
    };
    if !options.contains_key("page") && !options.contains_key("minimumSkillTier") {
        return Ok(None);
    }
    let preset =
        GuildBrowseOptions::deserialize(args.structured.as_ref().expect("structured guild search"))
            .map_err(|_| Error::Invalid)?;
    if args.entries.len() != 1
        || preset.page == 0
        || preset.minimum_skill_tier != -2
        || preset.min_num_members != 0
        || preset.max_num_members != 200
        || preset.guild_type != GuildBrowseType::Any
        || preset.guild_language_code != "EN"
        || preset.guild_invite_only
        || !preset.name.is_empty()
        || !preset.tag.is_empty()
    {
        return Err(Error::Invalid);
    }
    Ok(Some(preset.page))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GuildBrowseOptions {
    page: u32,
    minimum_skill_tier: i32,
    min_num_members: u32,
    max_num_members: u32,
    guild_type: GuildBrowseType,
    guild_language_code: String,
    guild_invite_only: bool,
    name: String,
    tag: String,
}

#[derive(Deserialize, PartialEq, Eq)]
enum GuildBrowseType {
    #[serde(rename = "GUILD_TYPE_ANY")]
    Any,
}

const PLAYER_ID: &[&str] = &["playerUUID", "playerId", "uuid", "targetUUID"];
const GROUP_ID: &[&str] = &["partyUUID", "partyUuid", "guildUUID", "teamUUID", "groupId"];
const MODE: &[&str] = &["partyQueueMode", "queueMode", "mode", "lobby"];
const SLOT: &[&str] = &["slot", "slotId", "equipmentSlot"];
const ITEM: &[&str] = &["item", "itemId", "resourceId", "equipped"];

enum View {
    Ack,
    Friends,
    Party,
    PartyInvites,
    Team,
    TeamInvites,
    Guild,
    GuildInvites,
    GuildSearch,
    Inventory,
    Quests,
}

#[derive(Serialize)]
struct Acknowledged {
    success: bool,
    code: u8,
}

fn basic() -> Result<Value, Error> {
    Ok(serde_json::to_value(Acknowledged {
        success: true,
        code: 0,
    })?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FriendList<'a> {
    confirmed: Vec<ConfirmedFriend<'a>>,
    pending: Vec<PendingFriend<'a>>,
    num_offline: usize,
}

#[derive(Serialize)]
struct ConfirmedFriend<'a> {
    #[serde(rename = "playerUUID")]
    player_uuid: &'a str,
    uuid: &'a str,
    handle: &'a str,
    status: &'a Option<String>,
    online: bool,
    favorite: bool,
}

#[derive(Serialize)]
struct PendingFriend<'a> {
    #[serde(rename = "playerUUID")]
    player_uuid: &'a str,
    uuid: &'a str,
    handle: &'a str,
    incoming: bool,
}

fn friends(result: Value) -> Result<Value, Error> {
    let result = Friends::deserialize(result)?;
    let view = FriendList {
        num_offline: result.friends.iter().filter(|entry| !entry.online).count(),
        confirmed: result
            .friends
            .iter()
            .map(|friend| ConfirmedFriend {
                player_uuid: &friend.player_id,
                uuid: &friend.player_id,
                handle: &friend.handle,
                status: &friend.status,
                online: friend.online,
                favorite: friend.favorite,
            })
            .collect(),
        pending: result
            .requests
            .iter()
            .map(|request| PendingFriend {
                player_uuid: &request.player_id,
                uuid: &request.player_id,
                handle: &request.handle,
                incoming: request.incoming,
            })
            .collect(),
    };
    Ok(serde_json::to_value(view)?)
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Member<'a> {
    uuid: &'a str,
    #[serde(rename = "playerUUID")]
    player_uuid: &'a str,
    player_id: &'a str,
    handle: &'a str,
    player_handle: &'a str,
    member_type: Role,
    is_captain: bool,
    is_organizer: bool,
    allowed_to_invite: bool,
    is_self: bool,
    user_team: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PartyView<'a> {
    success: bool,
    #[serde(rename = "partyUUID")]
    party_uuid: Option<&'a str>,
    party_members: &'a [Member<'a>],
    #[serde(flatten)]
    details: Option<PartyDetails<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PartyDetails<'a> {
    party_type: GroupKind,
    party_queue_mode: &'a str,
    quint_code: &'a Option<String>,
    lobby_open: bool,
    members: &'a [Member<'a>],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamView<'a> {
    success: bool,
    #[serde(rename = "teamUUID")]
    team_uuid: Option<&'a str>,
    members: &'a [Member<'a>],
    #[serde(flatten)]
    details: Option<TeamDetails<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamDetails<'a> {
    team_name: &'a str,
    num_members: usize,
    max_members: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildView<'a> {
    success: bool,
    #[serde(rename = "guildUUID")]
    guild_uuid: Option<&'a str>,
    members: &'a [Member<'a>],
    #[serde(flatten)]
    details: Option<GuildDetails<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildDetails<'a> {
    guild_name: &'a str,
    guild_tag: &'a str,
    guild_message: &'a str,
    guild_invite_only: bool,
    num_members: usize,
    max_members: u8,
    created_time: i64,
}

fn group(view: View, result: Value, actor: Uuid) -> Result<Value, Error> {
    let result = Option::<GroupView>::deserialize(result)?;
    let group = result.as_ref();
    let actor = actor.to_string();
    let members: Vec<_> = group
        .and_then(|g| g.members.as_ref())
        .iter()
        .flat_map(|members| members.iter())
        .map(|m| Member {
            uuid: &m.player_id,
            player_uuid: &m.player_id,
            player_id: &m.player_id,
            handle: &m.handle,
            player_handle: &m.handle,
            member_type: m.role,
            is_captain: m.role == Role::Owner,
            is_organizer: m.role == Role::Owner,
            allowed_to_invite: matches!(m.role, Role::Owner | Role::Officer),
            is_self: m.player_id == actor,
            user_team: m.side,
        })
        .collect();
    let id = group.map(|g| g.id.as_str());
    Ok(match view {
        View::Party => serde_json::to_value(PartyView {
            success: true,
            party_uuid: id,
            party_members: &members,
            details: group.map(|g| PartyDetails {
                party_type: g.kind,
                party_queue_mode: &g.mode,
                quint_code: &g.join_code,
                lobby_open: g.lobby_open,
                members: &members,
            }),
        })?,
        View::Team => serde_json::to_value(TeamView {
            success: true,
            team_uuid: id,
            members: &members,
            details: group.map(|g| TeamDetails {
                team_name: &g.name,
                num_members: g.member_count,
                max_members: g.max_members,
            }),
        })?,
        _ => serde_json::to_value(GuildView {
            success: true,
            guild_uuid: id,
            members: &members,
            details: group.map(|g| GuildDetails {
                guild_name: &g.name,
                guild_tag: &g.tag,
                guild_message: &g.motto,
                guild_invite_only: g.invite_only,
                num_members: g.member_count,
                max_members: g.max_members,
                created_time: g.created,
            }),
        })?,
    })
}

pub(crate) async fn guild_state(community: &Community, actor: Uuid) -> Result<Value, Error> {
    let data = community
        .execute(
            actor,
            Action::Group {
                kind: GroupKind::Guild,
            },
        )
        .await?;
    group(View::Guild, data, actor)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Loadout<'a> {
    player_global_loadout: &'a BTreeMap<String, String>,
    equipped: &'a BTreeMap<String, String>,
    owned: &'a [InventoryItem],
    inventory_groups: BTreeMap<&'a str, Vec<&'a InventoryItem>>,
}

fn inventory(result: Value) -> Result<Value, Error> {
    let result = Inventory::deserialize(result)?;
    let mut groups = BTreeMap::<&str, Vec<&InventoryItem>>::new();
    for item in &result.items {
        groups.entry(&item.slot).or_default().push(item);
    }
    Ok(serde_json::to_value(Loadout {
        player_global_loadout: &result.equipped,
        equipped: &result.equipped,
        owned: &result.items,
        inventory_groups: groups,
    })?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuestDisplay<'a> {
    quest_id: &'a str,
    short_desc: &'a str,
    progress: u32,
    progress_max: u32,
    progress_state: QuestStatus,
    redeemed: bool,
    metadata: QuestMetadata<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuestMetadata<'a> {
    category: &'a str,
    reward_item: &'a Option<String>,
    reward_count: Option<u32>,
}

fn quests(profile: &PlayerProfile, result: Value) -> Result<Value, Error> {
    let quests = Items::<Quest>::deserialize(result)?;
    let data: Vec<_> = quests
        .items
        .iter()
        .map(|quest| QuestDisplay {
            quest_id: &quest.id,
            short_desc: &quest.title,
            progress: quest.progress,
            progress_max: quest.goal,
            progress_state: quest.state,
            redeemed: quest.state == QuestStatus::Claimed,
            metadata: QuestMetadata {
                category: &quest.category,
                reward_item: &quest.reward_item,
                reward_count: quest.reward_count,
            },
        })
        .collect();
    let mut output = crate::player_info(profile);
    output["questDisplayData"] = serde_json::to_value(data)?;
    Ok(output)
}

fn group_settings(args: &Args<'_>) -> Result<GroupSettings, Error> {
    let payload = args.structured.as_ref().ok_or(Error::Invalid)?;
    if payload.as_object().is_none_or(|object| {
        object.is_empty()
            || object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "name"
                        | "guildName"
                        | "tag"
                        | "guildTag"
                        | "motto"
                        | "guildMessage"
                        | "inviteOnly"
                        | "guildInviteOnly"
                )
            })
    }) {
        return Err(Error::Invalid);
    }
    fn optional_text(payload: &Value, keys: &[&str]) -> Result<Option<String>, Error> {
        keys.iter()
            .find_map(|key| payload.get(*key))
            .map(|value| value.as_str().map(str::to_owned).ok_or(Error::Invalid))
            .transpose()
    }
    let invite_only = payload
        .get("inviteOnly")
        .or_else(|| payload.get("guildInviteOnly"))
        .map(|value| value.as_bool().ok_or(Error::Invalid))
        .transpose()?;
    Ok(GroupSettings {
        name: optional_text(payload, &["guildName", "name"])?,
        tag: optional_text(payload, &["guildTag", "tag"])?,
        motto: optional_text(payload, &["guildMessage", "motto"])?,
        invite_only,
        mode: None,
    })
}

pub(crate) async fn dispatch(
    community: &Community,
    profile: &PlayerProfile,
    method: RpcMethod,
    params: &[Value],
    notify: Option<&AppState>,
) -> Result<Value, Error> {
    let args = Args::new(params)?;
    let actor = profile.player_id;
    let native_search = if method == RpcMethod::GuildSearch {
        native_guild_page(&args)?
    } else {
        None
    };
    let native_info = method == RpcMethod::QueryGuildInfo
        && matches!(args.entries, [Value::String(_), Value::Bool(true)]);
    let native_empty = native_search.is_some()
        || native_info
        || (args.entries.is_empty()
            && matches!(
                method,
                RpcMethod::GuildGetData
                    | RpcMethod::QueryGuildInvites
                    | RpcMethod::QueryTeamInvites
            ));
    match (method, args.entries) {
        (RpcMethod::JoinGuild, [Value::String(group)]) if group.is_empty() => {
            return Ok(serde_json::to_value(Empty {})?);
        }
        (RpcMethod::CreateTeam, [Value::String(prefix), Value::String(a), Value::String(b)])
            if prefix.is_empty() && !a.trim().is_empty() && !b.trim().is_empty() =>
        {
            // The two strings' roles and the original mutation outcome are unknown.
            return Ok(serde_json::to_value(Empty {})?);
        }
        _ => {}
    }

    let (action, view) = match method {
        RpcMethod::FriendListAll | RpcMethod::FriendsListUpdate => (Action::Friends, View::Friends),
        RpcMethod::FriendRequestIssueByHandle => {
            let handle = args.text(0, &["handle", "playerHandle"])?;
            let target = community.resolve_handle(handle).await?;
            (Action::RequestFriend { target }, View::Friends)
        }
        RpcMethod::FriendRequestConfirm | RpcMethod::FriendRequestReject => (
            Action::ResolveFriend {
                requester: args.id(0, PLAYER_ID)?,
                accept: method == RpcMethod::FriendRequestConfirm,
            },
            View::Friends,
        ),
        RpcMethod::FriendDelete => (
            Action::RemoveFriend {
                target: args.id(0, PLAYER_ID)?,
            },
            View::Ack,
        ),
        RpcMethod::FriendAddFavorite | RpcMethod::FriendDeleteFavorite => (
            Action::FavoriteFriend {
                target: args.id(0, PLAYER_ID)?,
                favorite: method == RpcMethod::FriendAddFavorite,
            },
            View::Ack,
        ),
        RpcMethod::PresenceBroadcast => (
            Action::SetPresence {
                status: args.text(0, &["status", "presence"])?,
            },
            View::Ack,
        ),
        RpcMethod::PresenceSetReceiveBroadcast => (
            Action::SetReceivePresence {
                receive: args.flag(0, &["receive", "enabled"])?,
            },
            View::Ack,
        ),
        RpcMethod::SetPresenceInvisibility => (
            Action::SetInvisible {
                invisible: args.flag(0, &["invisible", "enabled"])?,
            },
            View::Ack,
        ),
        RpcMethod::CreateParty | RpcMethod::CreateQuintParty => (
            Action::CreateGroup {
                kind: GroupKind::Party,
                name: format!("{} party", profile.name),
                settings: GroupSettings {
                    mode: args
                        .field(0, MODE)
                        .map(|v| v.as_str().map(str::to_owned).ok_or(Error::Invalid))
                        .transpose()?,
                    ..Default::default()
                },
            },
            View::Party,
        ),
        RpcMethod::JoinQuintParty => (
            Action::JoinPartyByCode {
                code: args.text(0, &["quintCode", "code"])?,
            },
            View::Party,
        ),
        RpcMethod::DestroyQuintParty => (
            Action::DisbandGroup {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        RpcMethod::LeaveParty | RpcMethod::LeaveQuintParty => (
            Action::LeaveGroup {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        RpcMethod::QueryPartyInfo => (
            Action::Group {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        RpcMethod::QueryPartyInvites => (
            Action::GroupInvites {
                kind: GroupKind::Party,
            },
            View::PartyInvites,
        ),
        RpcMethod::PartyInviteSend => (
            Action::Invite {
                kind: GroupKind::Party,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Party,
        ),
        RpcMethod::PartyInviteConfirm | RpcMethod::PartyInviteReject => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Party,
                method == RpcMethod::PartyInviteConfirm,
                notify,
            )
            .await;
        }
        RpcMethod::PartyMemberKick => (
            Action::Kick {
                kind: GroupKind::Party,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Party,
        ),
        RpcMethod::PartyMemberMove => (
            Action::MovePartyMember {
                target: args.id(0, PLAYER_ID)?,
                side: args
                    .field(1, &["userTeam", "side", "team"])
                    .and_then(Value::as_u64)
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or(Error::Invalid)?,
            },
            View::Party,
        ),
        RpcMethod::PartyBalanceTeams => (Action::BalanceParty, View::Party),
        RpcMethod::UpdateQuintPartyState => {
            if args
                .structured
                .as_ref()
                .and_then(Value::as_object)
                .is_some_and(|object| {
                    object
                        .keys()
                        .any(|key| !MODE.contains(&key.as_str()) && key != "lobbyOpen")
                })
            {
                return Err(Error::Invalid);
            }
            let (mode, open) = if args.structured.is_some() {
                (
                    args.field(0, MODE)
                        .map(|_| args.text(0, MODE))
                        .transpose()?,
                    args.field(0, &["lobbyOpen"])
                        .map(|_| args.flag(0, &["lobbyOpen"]))
                        .transpose()?,
                )
            } else if args.entries.first().is_some_and(Value::is_boolean) {
                (None, Some(args.flag(0, &[])?))
            } else {
                (Some(args.text(0, &[])?), None)
            };
            (Action::UpdatePartyState { mode, open }, View::Party)
        }
        RpcMethod::PartyChangeQueueMode => (
            Action::UpdateGroup {
                kind: GroupKind::Party,
                settings: GroupSettings {
                    mode: Some(args.text(0, MODE)?),
                    ..Default::default()
                },
            },
            View::Party,
        ),
        RpcMethod::PartyLobbyEnter
        | RpcMethod::QuintPartyLobbyEnter
        | RpcMethod::PartyLobbyExit => (
            Action::SetPartyLobby {
                open: method != RpcMethod::PartyLobbyExit,
            },
            View::Party,
        ),
        RpcMethod::CreateTeam => (
            Action::CreateGroup {
                kind: GroupKind::Team,
                name: args.text(0, &["name", "teamName"])?,
                settings: Default::default(),
            },
            View::Team,
        ),
        RpcMethod::LeaveTeam => (
            Action::LeaveGroup {
                kind: GroupKind::Team,
            },
            View::Team,
        ),
        RpcMethod::QueryTeamInfo => (
            Action::Group {
                kind: GroupKind::Team,
            },
            View::Team,
        ),
        RpcMethod::QueryTeamInvites => (
            Action::GroupInvites {
                kind: GroupKind::Team,
            },
            View::TeamInvites,
        ),
        RpcMethod::TeamInviteSend => (
            Action::Invite {
                kind: GroupKind::Team,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Team,
        ),
        RpcMethod::TeamInviteConfirm | RpcMethod::TeamInviteReject => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Team,
                method == RpcMethod::TeamInviteConfirm,
                None,
            )
            .await;
        }
        RpcMethod::TeamMemberKick => (
            Action::Kick {
                kind: GroupKind::Team,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Team,
        ),
        RpcMethod::CreateGuild => (
            Action::CreateGroup {
                kind: GroupKind::Guild,
                name: args.text(0, &["name", "guildName"])?,
                settings: if args.structured.is_some() {
                    group_settings(&args)?
                } else {
                    GroupSettings {
                        tag: args
                            .field(1, &[])
                            .map(|v| v.as_str().map(str::to_owned).ok_or(Error::Invalid))
                            .transpose()?,
                        ..Default::default()
                    }
                },
            },
            View::Guild,
        ),
        RpcMethod::JoinGuild => (
            Action::JoinOpenGuild {
                group: args.id(0, GROUP_ID)?,
            },
            View::Guild,
        ),
        RpcMethod::ChangeGuildMembership => {
            let join = match args.field(1, &["join", "action"]) {
                Some(Value::String(action)) if action == "join" => true,
                Some(Value::String(action)) if action == "leave" => false,
                _ => args.flag(1, &["join", "action"])?,
            };
            (
                Action::ChangeGuildMembership {
                    group: args.id(0, GROUP_ID)?,
                    join,
                },
                View::Guild,
            )
        }
        RpcMethod::LeaveGuild => (
            Action::LeaveGroup {
                kind: GroupKind::Guild,
            },
            View::Guild,
        ),
        RpcMethod::QueryGuildInfo | RpcMethod::GuildGetData => {
            let action = match args.field(0, GROUP_ID) {
                Some(_) => Action::GroupById {
                    group: args.id(0, GROUP_ID)?,
                },
                None => Action::Group {
                    kind: GroupKind::Guild,
                },
            };
            (action, View::Guild)
        }
        RpcMethod::QueryGuildInvites => (
            Action::GroupInvites {
                kind: GroupKind::Guild,
            },
            View::GuildInvites,
        ),
        RpcMethod::GuildInviteSend => (
            Action::Invite {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        RpcMethod::GuildInviteConfirm | RpcMethod::GuildInviteReject => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Guild,
                method == RpcMethod::GuildInviteConfirm,
                None,
            )
            .await;
        }
        RpcMethod::GuildMemberKick => (
            Action::Kick {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        RpcMethod::GuildTransferCaptain => (
            Action::TransferOwnership {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        RpcMethod::GuildChangeOfficerStatus => (
            Action::SetRole {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
                role: if args.flag(1, &["officer", "isOfficer"])? {
                    Role::Officer
                } else {
                    Role::Member
                },
            },
            View::Guild,
        ),
        RpcMethod::GuildChangeName
        | RpcMethod::GuildChangeTag
        | RpcMethod::ChangeGuildMotto
        | RpcMethod::GuildChangeSettings
        | RpcMethod::GuildInviteOnly => {
            let mut settings = GroupSettings::default();
            match method {
                RpcMethod::GuildChangeName => {
                    settings.name = Some(args.text(0, &["guildName", "name"])?)
                }
                RpcMethod::GuildChangeTag => {
                    settings.tag = Some(args.text(0, &["guildTag", "tag"])?)
                }
                RpcMethod::ChangeGuildMotto => {
                    settings.motto = Some(args.text(0, &["guildMessage", "motto"])?)
                }
                RpcMethod::GuildInviteOnly => {
                    settings.invite_only = Some(args.flag(0, &["guildInviteOnly", "inviteOnly"])?)
                }
                _ => settings = group_settings(&args)?,
            }
            (
                Action::UpdateGroup {
                    kind: GroupKind::Guild,
                    settings,
                },
                View::Guild,
            )
        }
        RpcMethod::GuildSearch => (
            match native_search {
                Some(page) => Action::BrowseOpenGuilds { page },
                None => Action::SearchGuilds {
                    term: args.text(0, &["query", "name", "tag"])?,
                    limit: 50,
                },
            },
            View::GuildSearch,
        ),
        RpcMethod::GetInventoryGroups | RpcMethod::GetEquipmentSlotInfo => {
            (Action::Inventory, View::Inventory)
        }
        RpcMethod::EquipToSlot => (
            Action::Equip {
                slot: args.text(0, SLOT)?,
                item: args.item(1, ITEM)?,
            },
            View::Inventory,
        ),
        RpcMethod::EquippedHat
        | RpcMethod::EquippedEmoji
        | RpcMethod::EquippedPingPack
        | RpcMethod::Charm
        | RpcMethod::SocialPingPack => {
            let slot = match method {
                RpcMethod::EquippedHat => "hat",
                RpcMethod::EquippedEmoji => "emoji",
                RpcMethod::Charm => "charm",
                _ => "social_ping_pack",
            };
            let action = if args.field(0, ITEM).is_some() {
                Action::Equip {
                    slot: slot.into(),
                    item: args.item(0, ITEM)?,
                }
            } else {
                Action::Inventory
            };
            (action, View::Inventory)
        }
        RpcMethod::PlayerGlobalLoadout => {
            let action = if let Some(payload) = &args.structured {
                let payload = payload
                    .get("playerGlobalLoadout")
                    .unwrap_or(payload)
                    .clone();
                let slots = serde_json::from_value(payload).map_err(|_| Error::Invalid)?;
                Action::SetLoadout { slots }
            } else if args.entries.is_empty() {
                Action::Inventory
            } else {
                return Err(Error::Invalid);
            };
            (action, View::Inventory)
        }
        RpcMethod::GetQuestDisplayDataForPlayer => {
            (Action::Quests { category: None }, View::Quests)
        }
        RpcMethod::GetQuestDisplayDataForPlayerAndType => (
            Action::Quests {
                category: Some(args.text(0, &["category", "questType", "type"])?),
            },
            View::Quests,
        ),
        RpcMethod::RedeemQuestForPlayer => (
            Action::ClaimQuest {
                quest: args.text(0, &["questId", "id"])?,
            },
            View::Quests,
        ),
        RpcMethod::SkipQuestForPlayer => (
            Action::SkipQuest {
                quest: args.text(0, &["questId", "id"])?,
            },
            View::Quests,
        ),
        _ => return Err(Error::Unsupported),
    };
    let mut refresh = match &action {
        Action::RequestFriend { target } | Action::RemoveFriend { target } => vec![actor, *target],
        Action::ResolveFriend { requester, .. } => vec![actor, *requester],
        Action::FavoriteFriend { .. } | Action::SetReceivePresence { .. } => vec![actor],
        _ => Vec::new(),
    };
    let publish_presence = matches!(
        action,
        Action::SetPresence { .. } | Action::SetInvisible { .. }
    );
    let data = if crate::party_notify::is_mutation(&action) {
        crate::party_notify::execute(community, notify, actor, action).await?
    } else {
        match community.execute(actor, action).await {
            Ok(data) => data,
            Err(
                halcyon_community::CommunityError::Missing(_)
                | halcyon_community::CommunityError::Forbidden,
            ) if native_info => {
                return Ok(serde_json::to_value(Empty {})?);
            }
            Err(error) => return Err(error.into()),
        }
    };
    if native_empty
        && (data.is_null()
            || (matches!(
                view,
                View::GuildSearch | View::GuildInvites | View::TeamInvites
            ) && array(&data, "items").is_empty())
            || (native_info && group_kind(&data) != Some(GroupKind::Guild)))
    {
        return Ok(serde_json::to_value(Empty {})?);
    }
    if let Some(notify) = notify {
        if publish_presence {
            refresh.extend(
                array(&data, "friends")
                    .iter()
                    .filter_map(|entry| Uuid::parse_str(entry["playerId"].as_str()?).ok()),
            );
        }
        if !refresh.is_empty()
            && let Err(error) = notify.refresh_friends(refresh).await
        {
            tracing::warn!(%error, "local friends refresh could not be published");
        }
    }
    if matches!(view, View::Guild) && !data.is_null() && group_kind(&data) != Some(GroupKind::Guild)
    {
        return Err(Error::Invalid);
    }
    match view {
        View::Ack => basic(),
        View::Friends => friends(data),
        View::Party | View::Team | View::Guild => group(view, data, actor),
        View::PartyInvites | View::TeamInvites | View::GuildInvites => {
            let invitations =
                Items::<halcyon_community::views::Invitation>::deserialize(data)?.items;
            let pending = match view {
                View::PartyInvites => PendingInvites::Party(invitations),
                View::TeamInvites => PendingInvites::Team(invitations),
                _ => PendingInvites::Guild(invitations),
            };
            Ok(serde_json::to_value(InviteList {
                success: true,
                pending,
            })?)
        }
        View::GuildSearch => Ok(serde_json::to_value(GuildSearch {
            success: true,
            nearest_guilds: Items::<halcyon_community::views::GuildListing>::deserialize(data)?
                .items,
        })?),
        View::Inventory => inventory(data),
        View::Quests => quests(profile, data),
    }
}

#[derive(Serialize)]
struct InviteList {
    success: bool,
    #[serde(flatten)]
    pending: PendingInvites,
}

#[derive(Serialize)]
enum PendingInvites {
    #[serde(rename = "pendingPartyInvites")]
    Party(Vec<halcyon_community::views::Invitation>),
    #[serde(rename = "pendingTeamInvites")]
    Team(Vec<halcyon_community::views::Invitation>),
    #[serde(rename = "pendingGuildInvites")]
    Guild(Vec<halcyon_community::views::Invitation>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuildSearch {
    success: bool,
    nearest_guilds: Vec<halcyon_community::views::GuildListing>,
}

fn group_kind(value: &Value) -> Option<GroupKind> {
    GroupKind::deserialize(value.get("kind")?).ok()
}

async fn resolve_invite(
    community: &Community,
    actor: Uuid,
    args: &Args<'_>,
    kind: GroupKind,
    accept: bool,
    notify: Option<&halcyon_platform::AppState>,
) -> Result<Value, Error> {
    let id = args.id(0, GROUP_ID)?;
    let observed = community
        .execute(actor, Action::GroupById { group: id })
        .await?;
    if group_kind(&observed) != Some(kind) {
        return Err(Error::Invalid);
    }
    let action = Action::ResolveInvite { group: id, accept };
    let result = if kind == GroupKind::Party {
        crate::party_notify::execute(community, notify, actor, action).await?
    } else {
        community.execute(actor, action).await?
    };
    let view = match kind {
        GroupKind::Party => View::Party,
        GroupKind::Team => View::Team,
        GroupKind::Guild => View::Guild,
    };
    if accept {
        group(view, result, actor)
    } else {
        basic()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct HandleArgument<'a> {
        handle: &'a str,
    }
    #[derive(Serialize)]
    struct PresenceArgument {
        invisible: bool,
    }
    #[derive(Serialize)]
    struct PlayerArgument {
        #[serde(rename = "playerUUID")]
        player_uuid: Uuid,
    }
    #[derive(Serialize)]
    struct GuildArgument<'a> {
        name: &'a str,
        tag: &'a str,
    }
    #[derive(Serialize)]
    struct EquipmentArgument<'a> {
        slot: &'a str,
        item: &'a str,
    }

    fn player(name: &str) -> PlayerProfile {
        PlayerProfile {
            player_id: Uuid::new_v4(),
            name: name.into(),
            lobby: None,
        }
    }

    async fn call(
        community: &Community,
        player: &PlayerProfile,
        name: RpcMethod,
        payload: Option<Value>,
    ) -> Result<Value, Error> {
        let mut params = vec![Value::Null];
        params.extend(payload);
        dispatch(community, player, name, &params, None).await
    }

    #[tokio::test]
    async fn friend_parser_shape_uses_real_requests_presence_and_favorites() {
        let community = Community::open(":memory:".as_ref()).unwrap();
        let harbor = player("Harbor");
        let beacon = player("Beacon");
        for profile in [&harbor, &beacon] {
            community
                .observe_player(profile.player_id, profile.name.clone(), true)
                .await
                .unwrap();
        }
        let empty = call(&community, &harbor, RpcMethod::FriendListAll, None)
            .await
            .unwrap();
        assert_eq!(
            empty,
            serde_json::to_value(FriendList {
                confirmed: Vec::new(),
                pending: Vec::new(),
                num_offline: 0
            })
            .unwrap()
        );
        call(
            &community,
            &harbor,
            RpcMethod::FriendRequestIssueByHandle,
            Some(serde_json::to_value(HandleArgument { handle: "Beacon" }).unwrap()),
        )
        .await
        .unwrap();
        let pending = call(&community, &beacon, RpcMethod::FriendListAll, None)
            .await
            .unwrap();
        assert_eq!(pending["pending"][0]["incoming"], true);
        assert_eq!(
            pending["pending"][0]["playerUUID"],
            harbor.player_id.to_string()
        );
        call(
            &community,
            &beacon,
            RpcMethod::FriendRequestConfirm,
            Some(serde_json::to_value(harbor.player_id).unwrap()),
        )
        .await
        .unwrap();
        call(
            &community,
            &harbor,
            RpcMethod::SetPresenceInvisibility,
            Some(serde_json::to_value(PresenceArgument { invisible: true }).unwrap()),
        )
        .await
        .unwrap();
        call(
            &community,
            &beacon,
            RpcMethod::FriendAddFavorite,
            Some(
                serde_json::to_value(PlayerArgument {
                    player_uuid: harbor.player_id,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        let friends = call(&community, &beacon, RpcMethod::FriendListAll, None)
            .await
            .unwrap();
        assert_eq!(friends["confirmed"][0]["handle"], "Harbor");
        assert_eq!(friends["confirmed"][0]["favorite"], true);
        assert_eq!(friends["numOffline"], 1);
    }

    #[tokio::test]
    async fn membership_invites_and_real_inventory_have_expected_wire_fields() {
        let community = Community::open(":memory:".as_ref()).unwrap();
        let owner = player("Captain");
        let guest = player("Ranger");
        for profile in [&owner, &guest] {
            community
                .observe_player(profile.player_id, profile.name.clone(), true)
                .await
                .unwrap();
        }
        let guild = call(
            &community,
            &owner,
            RpcMethod::CreateGuild,
            Some(
                serde_json::to_value(GuildArgument {
                    name: "Pathfinders",
                    tag: "PATH",
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(guild["guildName"], "Pathfinders");
        assert_eq!(guild["members"][0]["isCaptain"], true);
        call(
            &community,
            &owner,
            RpcMethod::GuildInviteSend,
            Some(serde_json::to_value(guest.player_id).unwrap()),
        )
        .await
        .unwrap();
        let invite = &call(&community, &guest, RpcMethod::QueryGuildInvites, None)
            .await
            .unwrap()["pendingGuildInvites"][0]["groupId"];
        assert_eq!(invite, &guild["guildUUID"]);
        assert!(matches!(
            call(
                &community,
                &guest,
                RpcMethod::TeamInviteConfirm,
                Some(invite.clone())
            )
            .await,
            Err(Error::Invalid)
        ));
        let accepted = call(
            &community,
            &guest,
            RpcMethod::GuildInviteConfirm,
            Some(invite.clone()),
        )
        .await
        .unwrap();
        assert_eq!(accepted["numMembers"], 2);
        community
            .define_item(halcyon_community::ItemDefinition {
                id: "wayfinder_badge".into(),
                slot: "badge".into(),
                name: "Wayfinder".into(),
            })
            .await
            .unwrap();
        community
            .award_item(Uuid::new_v4(), guest.player_id, "wayfinder_badge".into(), 1)
            .await
            .unwrap();
        let equipment = call(
            &community,
            &guest,
            RpcMethod::EquipToSlot,
            Some(
                serde_json::to_value(EquipmentArgument {
                    slot: "badge",
                    item: "wayfinder_badge",
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(equipment["playerGlobalLoadout"]["badge"], "wayfinder_badge");
        assert_eq!(equipment["inventoryGroups"]["badge"][0]["count"], 1);
        assert!(matches!(
            call(&community, &guest, RpcMethod::StoreProcessIAP, None).await,
            Err(Error::Unsupported)
        ));
    }
}
