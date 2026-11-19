//! Community projection for cataloged RPC names. The friend-list envelope is
//! captured; other argument layouts and field mappings are inferred and need
//! client validation. Unmapped methods return the RPC method-not-found error.

use std::collections::BTreeMap;

use halcyon_community::{Action, Community, GroupKind, GroupSettings, Role};
use halcyon_platform::PlayerProfile;
use serde_json::{Value, json};
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

fn basic() -> Value {
    json!({"success":true, "code":0})
}

fn friends(result: Value) -> Value {
    let confirmed: Vec<_> = array(&result, "friends").iter().map(|friend| json!({
        "playerUUID":friend["playerId"], "uuid":friend["playerId"], "handle":friend["handle"],
        "status":friend["status"], "online":friend["online"], "favorite":friend["favorite"],
    })).collect();
    let offline = confirmed
        .iter()
        .filter(|entry| entry["online"] == false)
        .count();
    let pending: Vec<_> = array(&result, "requests").iter().map(|request| json!({
        "playerUUID":request["playerId"], "uuid":request["playerId"], "handle":request["handle"],
        "incoming":request["incoming"],
    })).collect();
    json!({"confirmed":confirmed, "pending":pending, "numOffline":offline})
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}

fn members(result: &Value, actor: Uuid) -> Vec<Value> {
    let actor = actor.to_string();
    array(result, "members").iter().map(|member| {
        let role = &member["role"];
        json!({
            "uuid":member["playerId"], "playerUUID":member["playerId"], "playerId":member["playerId"],
            "handle":member["handle"], "playerHandle":member["handle"],
            "memberType":role, "isCaptain":role=="owner", "isOrganizer":role=="owner",
            "allowedToInvite":role=="owner" || role=="officer",
            "isSelf":member["playerId"]==actor, "userTeam":member["side"],
        })
    }).collect()
}

fn group(view: View, result: Value, actor: Uuid) -> Value {
    if result.is_null() {
        return match view {
            View::Party => json!({"success":true,"partyUUID":null,"partyMembers":[]}),
            View::Team => json!({"success":true,"teamUUID":null,"members":[]}),
            _ => json!({"success":true,"guildUUID":null,"members":[]}),
        };
    }
    let members = members(&result, actor);
    match view {
        View::Party => json!({
            "success":true,"partyUUID":result["id"],"partyMembers":members,
            "partyType":result["kind"],"partyQueueMode":result["mode"],"quintCode":result["joinCode"],
            "lobbyOpen":result["lobbyOpen"],"members":members,
        }),
        View::Team => json!({
            "success":true,"teamUUID":result["id"],"teamName":result["name"],
            "members":members,"numMembers":result["memberCount"],"maxMembers":result["maxMembers"],
        }),
        _ => json!({
            "success":true,"guildUUID":result["id"],"guildName":result["name"],"guildTag":result["tag"],
            "guildMessage":result["motto"],"guildInviteOnly":result["inviteOnly"],"members":members,
            "numMembers":result["memberCount"],"maxMembers":result["maxMembers"],"createdTime":result["created"],
        }),
    }
}

fn inventory(result: Value) -> Value {
    let mut groups = BTreeMap::<String, Vec<Value>>::new();
    for item in array(&result, "items") {
        if let Some(slot) = item["slot"].as_str() {
            groups
                .entry(slot.to_owned())
                .or_default()
                .push(item.clone());
        }
    }
    json!({
        "player_global_loadout":result["equipped"], "equipped":result["equipped"],
        "owned":result["items"], "inventoryGroups":groups,
    })
}

fn quests(profile: &PlayerProfile, result: Value) -> Value {
    let data: Vec<_> = array(&result, "items").iter().map(|quest| json!({
        "questId":quest["id"],"shortDesc":quest["title"],"progress":quest["progress"],
        "progressMax":quest["goal"],"progressState":quest["state"],
        "redeemed":quest["state"]=="claimed","metadata":{
            "category":quest["category"],"rewardItem":quest["rewardItem"],"rewardCount":quest["rewardCount"],
        },
    })).collect();
    let mut output = crate::player_info(profile);
    output["questDisplayData"] = json!(data);
    output
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
    method: &str,
    params: &[Value],
) -> Result<Value, Error> {
    let args = Args::new(params)?;
    let actor = profile.player_id;
    let (action, view) = match method {
        "friendListAll" | "friendsListUpdate" => (Action::Friends, View::Friends),
        "friendRequestIssueByHandle" => {
            let handle = args.text(0, &["handle", "playerHandle"])?;
            let target = community.resolve_handle(handle).await?;
            (Action::RequestFriend { target }, View::Friends)
        }
        "friendRequestConfirm" | "friendRequestReject" => (
            Action::ResolveFriend {
                requester: args.id(0, PLAYER_ID)?,
                accept: method == "friendRequestConfirm",
            },
            View::Friends,
        ),
        "friendDelete" => (
            Action::RemoveFriend {
                target: args.id(0, PLAYER_ID)?,
            },
            View::Ack,
        ),
        "friendAddFavorite" | "friendDeleteFavorite" => (
            Action::FavoriteFriend {
                target: args.id(0, PLAYER_ID)?,
                favorite: method == "friendAddFavorite",
            },
            View::Ack,
        ),
        "presenceBroadcast" => (
            Action::SetPresence {
                status: args.text(0, &["status", "presence"])?,
            },
            View::Ack,
        ),
        "presenceSetReceiveBroadcast" => (
            Action::SetReceivePresence {
                receive: args.flag(0, &["receive", "enabled"])?,
            },
            View::Ack,
        ),
        "setPresenceInvisibility" => (
            Action::SetInvisible {
                invisible: args.flag(0, &["invisible", "enabled"])?,
            },
            View::Ack,
        ),
        "createParty" | "createQuintParty" => (
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
        "joinQuintParty" => (
            Action::JoinPartyByCode {
                code: args.text(0, &["quintCode", "code"])?,
            },
            View::Party,
        ),
        "destroyQuintParty" => (
            Action::DisbandGroup {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        "leaveParty" | "leaveQuintParty" => (
            Action::LeaveGroup {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        "queryPartyInfo" => (
            Action::Group {
                kind: GroupKind::Party,
            },
            View::Party,
        ),
        "queryPartyInvites" => (
            Action::GroupInvites {
                kind: GroupKind::Party,
            },
            View::PartyInvites,
        ),
        "partyInviteSend" => (
            Action::Invite {
                kind: GroupKind::Party,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Party,
        ),
        "partyInviteConfirm" | "partyInviteReject" => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Party,
                method == "partyInviteConfirm",
            )
            .await;
        }
        "partyMemberKick" => (
            Action::Kick {
                kind: GroupKind::Party,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Party,
        ),
        "partyMemberMove" => (
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
        "partyBalanceTeams" => (Action::BalanceParty, View::Party),
        "partyChangeQueueMode" => (
            Action::UpdateGroup {
                kind: GroupKind::Party,
                settings: GroupSettings {
                    mode: Some(args.text(0, MODE)?),
                    ..Default::default()
                },
            },
            View::Party,
        ),
        "partyLobbyEnter" | "quintPartyLobbyEnter" | "partyLobbyExit" => (
            Action::SetPartyLobby {
                open: method != "partyLobbyExit",
            },
            View::Party,
        ),
        "createTeam" => (
            Action::CreateGroup {
                kind: GroupKind::Team,
                name: args.text(0, &["name", "teamName"])?,
                settings: Default::default(),
            },
            View::Team,
        ),
        "leaveTeam" => (
            Action::LeaveGroup {
                kind: GroupKind::Team,
            },
            View::Team,
        ),
        "queryTeamInfo" => (
            Action::Group {
                kind: GroupKind::Team,
            },
            View::Team,
        ),
        "queryTeamInvites" => (
            Action::GroupInvites {
                kind: GroupKind::Team,
            },
            View::TeamInvites,
        ),
        "teamInviteSend" => (
            Action::Invite {
                kind: GroupKind::Team,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Team,
        ),
        "teamInviteConfirm" | "teamInviteReject" => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Team,
                method == "teamInviteConfirm",
            )
            .await;
        }
        "teamMemberKick" => (
            Action::Kick {
                kind: GroupKind::Team,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Team,
        ),
        "createGuild" => (
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
        "joinGuild" => (
            Action::JoinOpenGuild {
                group: args.id(0, GROUP_ID)?,
            },
            View::Guild,
        ),
        "leaveGuild" => (
            Action::LeaveGroup {
                kind: GroupKind::Guild,
            },
            View::Guild,
        ),
        "queryGuildInfo" | "guildGetData" => {
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
        "queryGuildInvites" => (
            Action::GroupInvites {
                kind: GroupKind::Guild,
            },
            View::GuildInvites,
        ),
        "guildInviteSend" => (
            Action::Invite {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        "guildInviteConfirm" | "guildInviteReject" => {
            return resolve_invite(
                community,
                actor,
                &args,
                GroupKind::Guild,
                method == "guildInviteConfirm",
            )
            .await;
        }
        "guildMemberKick" => (
            Action::Kick {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        "guildTransferCaptain" => (
            Action::TransferOwnership {
                kind: GroupKind::Guild,
                target: args.id(0, PLAYER_ID)?,
            },
            View::Guild,
        ),
        "guildChangeOfficerStatus" => (
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
        "guildChangeName"
        | "guildChangeTag"
        | "changeGuildMotto"
        | "guildChangeSettings"
        | "guildInviteOnly" => {
            let mut settings = GroupSettings::default();
            match method {
                "guildChangeName" => settings.name = Some(args.text(0, &["guildName", "name"])?),
                "guildChangeTag" => settings.tag = Some(args.text(0, &["guildTag", "tag"])?),
                "changeGuildMotto" => {
                    settings.motto = Some(args.text(0, &["guildMessage", "motto"])?)
                }
                "guildInviteOnly" => {
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
        "guildSearch" => (
            Action::SearchGuilds {
                term: args.text(0, &["query", "name", "tag"])?,
                limit: 50,
            },
            View::GuildSearch,
        ),
        "getInventoryGroups" | "getEquipmentSlotInfo" => (Action::Inventory, View::Inventory),
        "equipToSlot" => (
            Action::Equip {
                slot: args.text(0, SLOT)?,
                item: args.item(1, ITEM)?,
            },
            View::Inventory,
        ),
        "equippedHat" | "equippedEmoji" | "equippedPingPack" => {
            let slot = match method {
                "equippedHat" => "hat",
                "equippedEmoji" => "emoji",
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
        "player_global_loadout" => {
            let action = if let Some(payload) = &args.structured {
                let payload = payload
                    .get("player_global_loadout")
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
        "getQuestDisplayDataForPlayer" => (Action::Quests { category: None }, View::Quests),
        "getQuestDisplayDataForPlayerAndType" => (
            Action::Quests {
                category: Some(args.text(0, &["category", "questType", "type"])?),
            },
            View::Quests,
        ),
        "redeemQuestForPlayer" => (
            Action::ClaimQuest {
                quest: args.text(0, &["questId", "id"])?,
            },
            View::Quests,
        ),
        "skipQuestForPlayer" => (
            Action::SkipQuest {
                quest: args.text(0, &["questId", "id"])?,
            },
            View::Quests,
        ),
        _ => return Err(Error::Unsupported),
    };
    let data = community.execute(actor, action).await?;
    if matches!(view, View::Guild) && !data.is_null() && data["kind"] != "guild" {
        return Err(Error::Invalid);
    }
    Ok(match view {
        View::Ack => basic(),
        View::Friends => friends(data),
        View::Party | View::Team | View::Guild => group(view, data, actor),
        View::PartyInvites => json!({"success":true,"pendingPartyInvites":data["items"]}),
        View::TeamInvites => json!({"success":true,"pendingTeamInvites":data["items"]}),
        View::GuildInvites => json!({"success":true,"pendingGuildInvites":data["items"]}),
        View::GuildSearch => json!({"success":true,"nearestGuilds":data["items"]}),
        View::Inventory => inventory(data),
        View::Quests => quests(profile, data),
    })
}

async fn resolve_invite(
    community: &Community,
    actor: Uuid,
    args: &Args<'_>,
    kind: GroupKind,
    accept: bool,
) -> Result<Value, Error> {
    let id = args.id(0, GROUP_ID)?;
    let observed = community
        .execute(actor, Action::GroupById { group: id })
        .await?;
    if observed["kind"] != json!(kind) {
        return Err(Error::Invalid);
    }
    let result = community
        .execute(actor, Action::ResolveInvite { group: id, accept })
        .await?;
    let view = match kind {
        GroupKind::Party => View::Party,
        GroupKind::Team => View::Team,
        GroupKind::Guild => View::Guild,
    };
    Ok(if accept {
        group(view, result, actor)
    } else {
        basic()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        name: &str,
        payload: Option<Value>,
    ) -> Result<Value, Error> {
        let mut params = vec![Value::Null];
        params.extend(payload);
        dispatch(community, player, name, &params).await
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
        let empty = call(&community, &harbor, "friendListAll", None)
            .await
            .unwrap();
        assert_eq!(empty, json!({"confirmed":[],"pending":[],"numOffline":0}));
        call(
            &community,
            &harbor,
            "friendRequestIssueByHandle",
            Some(json!({"handle":"Beacon"})),
        )
        .await
        .unwrap();
        let pending = call(&community, &beacon, "friendListAll", None)
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
            "friendRequestConfirm",
            Some(json!(harbor.player_id)),
        )
        .await
        .unwrap();
        call(
            &community,
            &harbor,
            "setPresenceInvisibility",
            Some(json!({"invisible":true})),
        )
        .await
        .unwrap();
        call(
            &community,
            &beacon,
            "friendAddFavorite",
            Some(json!({"playerUUID":harbor.player_id})),
        )
        .await
        .unwrap();
        let friends = call(&community, &beacon, "friendListAll", None)
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
            "createGuild",
            Some(json!({"name":"Pathfinders","tag":"PATH"})),
        )
        .await
        .unwrap();
        assert_eq!(guild["guildName"], "Pathfinders");
        assert_eq!(guild["members"][0]["isCaptain"], true);
        call(
            &community,
            &owner,
            "guildInviteSend",
            Some(json!(guest.player_id)),
        )
        .await
        .unwrap();
        let invite = &call(&community, &guest, "queryGuildInvites", None)
            .await
            .unwrap()["pendingGuildInvites"][0]["groupId"];
        assert_eq!(invite, &guild["guildUUID"]);
        assert!(matches!(
            call(
                &community,
                &guest,
                "teamInviteConfirm",
                Some(invite.clone())
            )
            .await,
            Err(Error::Invalid)
        ));
        let accepted = call(
            &community,
            &guest,
            "guildInviteConfirm",
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
            "equipToSlot",
            Some(json!({"slot":"badge","item":"wayfinder_badge"})),
        )
        .await
        .unwrap();
        assert_eq!(
            equipment["player_global_loadout"]["badge"],
            "wayfinder_badge"
        );
        assert_eq!(equipment["inventoryGroups"]["badge"][0]["count"], 1);
        assert!(matches!(
            call(&community, &guest, "storeProcessIAP", None).await,
            Err(Error::Unsupported)
        ));
    }
}
