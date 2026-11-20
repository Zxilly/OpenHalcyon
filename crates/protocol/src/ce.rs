//! Shared CE menu and JSON-RPC wire identities.
mod queue;
pub use queue::{KnownQueue, NativeMode};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyState {
    Menus,
    PendingAuto,
    MatchPending,
    MatchedPartners,
    Playing,
    Spectating,
    PostMatch,
}

impl LobbyState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Menus => "menus",
            Self::PendingAuto => "pending_auto",
            Self::MatchPending => "match_pending",
            Self::MatchedPartners => "matched_partners",
            Self::Playing => "playing",
            Self::Spectating => "spectating",
            Self::PostMatch => "post_match",
        }
    }
}

// Keep lookup, serialization and dispatch on the same sorted registry.
macro_rules! rpc_methods {
    ($( $method:ident => $name:literal, )+) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum RpcMethod { $( $method, )+ Unknown }

        const METHODS: &[(&str, RpcMethod)] = &[$( ($name, RpcMethod::$method), )+];

        impl RpcMethod {
            pub const fn as_str(self) -> &'static str {
                match self { $( Self::$method => $name, )+ Self::Unknown => "" }
            }
        }
    };
}

rpc_methods! {
    AcceptMatch => "acceptMatch",
    AchievementGetScore => "achievementGetScore",
    AchievementStoreScore => "achievementStoreScore",
    AddDeviceToken => "addDeviceToken",
    AskInGameFriendToPlay => "askInGameFriendToPlay",
    AttemptRedeemAscensionChest => "attemptRedeemAscensionChest",
    AttemptRedeemAscensionRankUpChest => "attemptRedeemAscensionRankUpChest",
    AttemptRedeemAscensionSeasonEndChest => "attemptRedeemAscensionSeasonEndChest",
    AttemptRedeemSeasonalAscensionChest => "attemptRedeemSeasonalAscensionChest",
    Authenticate => "authenticate",
    BuyAscensionSeasonalBundle => "buyAscensionSeasonalBundle",
    ChangeGuildMembership => "changeGuildMembership",
    ChangeGuildMotto => "changeGuildMotto",
    Charm => "charm",
    CollectHeroChest => "collectHeroChest",
    CollectProgressiveChest => "collectProgressiveChest",
    CreateAccountForPlayer => "createAccountForPlayer",
    CreateGuestPlayer => "createGuestPlayer",
    CreateGuild => "createGuild",
    CreateParty => "createParty",
    CreateQuintParty => "createQuintParty",
    CreateTeam => "createTeam",
    DecraftSkinBlueprint => "decraftSkinBlueprint",
    DestroyQuintParty => "destroyQuintParty",
    DismissReliableMessage => "dismissReliableMessage",
    EndSession => "endSession",
    EquipToSlot => "equipToSlot",
    EquippedEmoji => "equippedEmoji",
    EquippedHat => "equippedHat",
    EquippedPingPack => "equippedPingPack",
    EstimatedWaitTime => "estimatedWaitTime",
    ExitLobby => "exitLobby",
    ForgeCard => "forgeCard",
    ForgeEssence => "forgeEssence",
    FriendAddFavorite => "friendAddFavorite",
    FriendDelete => "friendDelete",
    FriendDeleteFavorite => "friendDeleteFavorite",
    FriendListAll => "friendListAll",
    FriendNotify => "friendNotify",
    FriendReplyFromInGame => "friendReplyFromInGame",
    FriendRequestConfirm => "friendRequestConfirm",
    FriendRequestIssueByHandle => "friendRequestIssueByHandle",
    FriendRequestReject => "friendRequestReject",
    FriendsListUpdate => "friendsListUpdate",
    GetAscensionDisplayData => "getAscensionDisplayData",
    GetBuffManifest => "getBuffManifest",
    GetCardBoxManifest => "getCardBoxManifest",
    GetDailyPicker => "getDailyPicker",
    GetEquipmentSlotInfo => "getEquipmentSlotInfo",
    GetForgeManifest => "getForgeManifest",
    GetHeroChestData => "getHeroChestData",
    GetHeroMastery => "getHeroMastery",
    GetInventoryGroups => "getInventoryGroups",
    GetLeaderboardData => "getLeaderboardData",
    GetLiveEventData => "getLiveEventData",
    GetPlayerCardInventory => "getPlayerCardInventory",
    GetPlayerEsportsBadgeData => "getPlayerEsportsBadgeData",
    GetPlayerForGuestAccount => "getPlayerForGuestAccount",
    GetPlayerFromPlatform => "getPlayerFromPlatform",
    GetPlayerInfo => "getPlayerInfo",
    GetPlayerInfos => "getPlayerInfos",
    GetPlayerSkinProgress => "getPlayerSkinProgress",
    GetProgressiveChestDescriptions => "getProgressiveChestDescriptions",
    GetQuestDisplayDataForPlayer => "getQuestDisplayDataForPlayer",
    GetQuestDisplayDataForPlayerAndType => "getQuestDisplayDataForPlayerAndType",
    GetRewardChestDefinitions => "getRewardChestDefinitions",
    GetRewardsManifest => "getRewardsManifest",
    GetSeasonRewardsManifest => "getSeasonRewardsManifest",
    GetSkinManifest => "getSkinManifest",
    GetSkinProgress => "getSkinProgress",
    GetTalentsData => "getTalentsData",
    GetTrophyCase => "getTrophyCase",
    GuildChangeName => "guildChangeName",
    GuildChangeOfficerStatus => "guildChangeOfficerStatus",
    GuildChangeSettings => "guildChangeSettings",
    GuildChangeTag => "guildChangeTag",
    GuildCollectJoinBonus => "guildCollectJoinBonus",
    GuildGetData => "guildGetData",
    GuildInviteConfirm => "guildInviteConfirm",
    GuildInviteOnly => "guildInviteOnly",
    GuildInviteReject => "guildInviteReject",
    GuildInviteSend => "guildInviteSend",
    GuildMemberKick => "guildMemberKick",
    GuildRedeemChest => "guildRedeemChest",
    GuildSearch => "guildSearch",
    GuildTransferCaptain => "guildTransferCaptain",
    IsGovernmentIdVerified => "isGovernmentIdVerified",
    JoinGuild => "joinGuild",
    JoinLobby => "joinLobby",
    JoinQuintParty => "joinQuintParty",
    LeaveGuild => "leaveGuild",
    LeaveParty => "leaveParty",
    LeaveQuintParty => "leaveQuintParty",
    LeaveTeam => "leaveTeam",
    LinkAccount => "link_account",
    NotifyExitPostMatch => "notifyExitPostMatch",
    NotifyGameResults => "notifyGameResults",
    NotifyPlayerAction => "notifyPlayerAction",
    OffersGetData => "offersGetData",
    OffersRedeemSku => "offersRedeemSku",
    OpenInventoryChest => "openInventoryChest",
    OpenRewardChest => "openRewardChest",
    PartyBalanceTeams => "partyBalanceTeams",
    PartyChangeQueueMode => "partyChangeQueueMode",
    PartyInviteConfirm => "partyInviteConfirm",
    PartyInviteReject => "partyInviteReject",
    PartyInviteSend => "partyInviteSend",
    PartyLobbyEnter => "partyLobbyEnter",
    PartyLobbyExit => "partyLobbyExit",
    PartyMemberKick => "partyMemberKick",
    PartyMemberMove => "partyMemberMove",
    PlayerGlobalLoadout => "player_global_loadout",
    PresenceBroadcast => "presenceBroadcast",
    PresenceSetReceiveBroadcast => "presenceSetReceiveBroadcast",
    ProcessMessage => "processMessage",
    PurchaseCardPack => "purchaseCardPack",
    PurchaseDailyPick => "purchaseDailyPick",
    QueryGuildInfo => "queryGuildInfo",
    QueryGuildInvites => "queryGuildInvites",
    QueryPartyInfo => "queryPartyInfo",
    QueryPartyInvites => "queryPartyInvites",
    QueryPendingMatch => "queryPendingMatch",
    QueryPlayerInboxMessages => "queryPlayerInboxMessages",
    QueryTeamInfo => "queryTeamInfo",
    QueryTeamInvites => "queryTeamInvites",
    QuintPartyLobbyEnter => "quintPartyLobbyEnter",
    RecordMatchExperienceMetrics => "recordMatchExperienceMetrics",
    RedeemQuestForPlayer => "redeemQuestForPlayer",
    RefreshCardBoxManifest => "refreshCardBoxManifest",
    RejectMatch => "rejectMatch",
    RenamePlayerHandle => "renamePlayerHandle",
    ReportHonorPlayer => "reportHonorPlayer",
    SetPlayerHandle => "setPlayerHandle",
    SetPresenceInvisibility => "setPresenceInvisibility",
    SetTutorialState => "setTutorialState",
    SkipQuestForPlayer => "skipQuestForPlayer",
    SocialPingPack => "social_ping_pack",
    SpectateFriend => "spectateFriend",
    SpectatorExitMatch => "spectatorExitMatch",
    StartSessionForPlayer => "startSessionForPlayer",
    StoreCancelPendingGift => "storeCancelPendingGift",
    StorePrepareIAP => "storePrepareIAP",
    StoreProcessIAP => "storeProcessIAP",
    StoreRecordPendingGift => "storeRecordPendingGift",
    StoreRequestPurchaseSKU => "storeRequestPurchaseSKU",
    TeamInviteConfirm => "teamInviteConfirm",
    TeamInviteReject => "teamInviteReject",
    TeamInviteSend => "teamInviteSend",
    TeamMemberKick => "teamMemberKick",
    Update => "update",
    UpdateMatchInfo => "updateMatchInfo",
    UpdatePlatformPlayerConfig => "updatePlatformPlayerConfig",
    UpdateQuintPartyState => "updateQuintPartyState",
    UpgradeTalent => "upgradeTalent",
    VerifyGovernmentId => "verifyGovernmentId",
    WeaveHeroSkin => "weaveHeroSkin",
}

impl serde::Serialize for RpcMethod {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl RpcMethod {
    pub fn from_wire(name: &str) -> Self {
        METHODS
            .binary_search_by_key(&name, |(key, _)| key)
            .map_or(Self::Unknown, |index| METHODS[index].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_and_native_method_names_agree() {
        let catalog = include_str!("../../../docs/ce-rpc.tsv");
        for row in catalog.lines().skip(1) {
            let (name, _) = row.split_once('\t').unwrap();
            let method = RpcMethod::from_wire(name);
            assert_ne!(method, RpcMethod::Unknown, "{name}");
            assert_eq!(name, method.as_str());
        }
        assert_eq!(
            RpcMethod::from_wire("unsupportedMethod"),
            RpcMethod::Unknown
        );
    }
}
