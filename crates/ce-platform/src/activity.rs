use super::{Error, Platform, meta::Meta};
use crate::RpcMethod;
use crate::match_stats::PlayerMatches;
use crate::meta::Preference;
use crate::profile::Currency;
use crate::wire::{Empty, PlayerHandle, Success};
use halcyon_community::GroupKind;
use halcyon_platform::PlayerProfile;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) fn initialize(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE INDEX IF NOT EXISTS ce_event_kind ON events(player,json_extract(body,'$.type'));
        CREATE TABLE IF NOT EXISTS ce_scores(player TEXT NOT NULL,achievement TEXT NOT NULL,score INTEGER NOT NULL,
            PRIMARY KEY(player,achievement));
        CREATE TABLE IF NOT EXISTS ce_messages(id TEXT PRIMARY KEY, sender TEXT NOT NULL,
            recipient TEXT NOT NULL, body TEXT NOT NULL, created INTEGER DEFAULT(unixepoch()), dismissed INTEGER DEFAULT 0);
        CREATE INDEX IF NOT EXISTS ce_recipient ON ce_messages(recipient,dismissed,created);
        CREATE TABLE IF NOT EXISTS ce_match_handles(room TEXT NOT NULL,player TEXT NOT NULL,handle TEXT NOT NULL,
            PRIMARY KEY(room,player));
        CREATE TRIGGER IF NOT EXISTS ce_snapshot_match_handle AFTER INSERT ON events
            WHEN json_extract(NEW.body,'$.type')='match_finished' BEGIN
            INSERT OR IGNORE INTO ce_match_handles
            SELECT json_extract(NEW.body,'$.matchId'),p.id,p.name FROM players p
            WHERE p.id=NEW.player AND EXISTS(SELECT 1 FROM json_each(NEW.body,'$.result.players') seat WHERE seat.key=p.id);
            END;
        CREATE TABLE IF NOT EXISTS ce_honor(match_id TEXT NOT NULL,source TEXT NOT NULL,target TEXT NOT NULL,
            kind TEXT NOT NULL, PRIMARY KEY(match_id,source,target));
        CREATE TABLE IF NOT EXISTS ce_pending_gifts(player TEXT NOT NULL,target TEXT NOT NULL,sku TEXT NOT NULL,
            PRIMARY KEY(player,target,sku));
        CREATE TABLE IF NOT EXISTS ce_wait(player TEXT PRIMARY KEY,mode TEXT NOT NULL,joined INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS ce_wait_history(mode TEXT PRIMARY KEY,seconds REAL NOT NULL);
        CREATE TRIGGER IF NOT EXISTS ce_join_time AFTER INSERT ON events WHEN json_extract(NEW.body,'$.type')='queued'
        BEGIN INSERT OR REPLACE INTO ce_wait VALUES(NEW.player,json_extract(NEW.body,'$.mode'),unixepoch()); END;
        CREATE TRIGGER IF NOT EXISTS ce_offer_time AFTER INSERT ON events WHEN json_extract(NEW.body,'$.type')='match_offered'
        BEGIN INSERT INTO ce_wait_history SELECT mode,unixepoch()-joined FROM ce_wait WHERE player=NEW.player
        ON CONFLICT(mode) DO UPDATE SET seconds=0.875*seconds+0.125*excluded.seconds;
        DELETE FROM ce_wait WHERE player=NEW.player; END;")?;
    super::match_stats::initialize(db)
}

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
        self.transaction(move|tx| {
            let player=player.to_string();
            let matches = super::match_stats::read_player_matches(tx, &player)?;
            let mut inv=tx.prepare("SELECT item,count FROM community_inventory i JOIN community_items d ON d.id=i.item WHERE player=? AND d.slot='currency'")?;
            let balances = inv.query_map([&player], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
            })?.collect::<rusqlite::Result<BTreeMap<String, u32>>>()?;
            let mut currency = Currency::default();
            currency.extend(balances);
            let pending_messages = tx.query_row("SELECT count(*) FROM ce_messages WHERE recipient=? AND dismissed=0",
                [&player], |r| r.get::<_, u32>(0))?;
            let mut info = PlayerActivityFields { matches, currency, pending: PendingCounts { messages: pending_messages, ..Default::default() }, ..Default::default() };
            let mut invites=tx.prepare("SELECT g.kind,count(*) FROM community_invites i JOIN community_groups g ON g.id=i.group_id WHERE i.player=? GROUP BY g.kind")?;
            for row in invites.query_map([&player], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))? {
                let (kind, count) = row?;
                match GroupKind::deserialize(Value::String(kind)) {
                    Ok(GroupKind::Party) => info.pending.party = Some(count),
                    Ok(GroupKind::Team) => info.pending.team = Some(count),
                    Ok(GroupKind::Guild) => info.pending.guild = Some(count),
                    Err(_) => {}
                }
            }
            let mut groups=tx.prepare("SELECT kind,group_id FROM community_members WHERE player=?")?;
            for row in groups.query_map([&player], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (kind, id) = row?;
                match GroupKind::deserialize(Value::String(kind)) {
                    Ok(GroupKind::Party) => info.party_uuid = Some(Some(id)),
                    Ok(GroupKind::Team) => info.team_uuid = Some(Some(id)),
                    Ok(GroupKind::Guild) => info.guild_uuid = Some(Some(id)),
                    Err(_) => {}
                }
            }
            let mut pref=tx.prepare("SELECT category,data FROM ce_preferences WHERE player=? AND category IN ('tutorial','last_login')")?;
            for row in pref.query_map([player], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (category, raw) = row?;
                let data = serde_json::from_str(&raw)?;
                if category == Preference::LastLogin.as_str() {
                    info.last_login = Some(data);
                } else {
                    info.tutorial_state = super::profile::tutorial_state(&data);
                }
            }
            Ok(info)
        }).await
    }
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
                let ids=field(params,1,&["playerUUIDs","players"])?.as_array().filter(|ids| ids.len()<=50).ok_or(Error::Invalid)?;
                let mut profiles=Vec::new();
                for id in ids {
                    let key=Uuid::parse_str(id.as_str().ok_or(Error::Invalid)?).map_err(|_|Error::Invalid)?;
                    if let Some(activity)=self.app.player_activity(key).await? {
                        profiles.push(PlayerHandle { player_uuid: key, handle: activity.profile.name });
                    }
                }
                Ok(serde_json::to_value(PlayerHandles { player_infos: profiles })?)
            }
            RpcMethod::EstimatedWaitTime => {
                let mode=text(params,1,&["lobby","mode","queueName"],128)?;
                self.meta.transaction(move|tx|{
                    let wait:Option<f64>=tx.query_row("SELECT seconds FROM ce_wait_history WHERE mode=?",[mode],|r|r.get(0)).optional()?;
                    Ok(serde_json::to_value(WaitEstimate { estimated_wait: wait.unwrap_or(0.0).round() as u64 })?)
                }).await
            }
            RpcMethod::AchievementGetScore | RpcMethod::AchievementStoreScore => {
                let key=text(params,1,&["achievementId","achievement"],128)?;
                let score=if m==RpcMethod::AchievementStoreScore {Some(field(params,2,&["score"])?.as_i64().filter(|n|*n>=0).ok_or(Error::Invalid)?)}else{None};
                self.meta.transaction(move|tx|{
                    if let Some(n)=score {tx.execute("INSERT INTO ce_scores VALUES(?,?,?) ON CONFLICT(player,achievement)
                        DO UPDATE SET score=max(score,excluded.score)",params![actor,key,n])?;}
                    let n:Option<i64>=tx.query_row("SELECT score FROM ce_scores WHERE player=? AND achievement=?",params![actor,key],|r|r.get(0)).optional()?;
                    Ok(serde_json::to_value(AchievementScore { achievement_id: key, score: n.unwrap_or(0) })?)
                }).await
            }
            RpcMethod::AddDeviceToken => {
                let token=text(params,1,&["deviceToken","token"],4096)?;
                self.meta.set(profile.player_id,Preference::DeviceToken,Value::from(token)).await?;
                Ok(serde_json::to_value(Success { success: true })?)
            }
            RpcMethod::QueryPlayerInboxMessages => self.meta.transaction(move|tx|{
                let mut q=tx.prepare("SELECT id,body,sender,created FROM ce_messages WHERE recipient=? AND dismissed=0 ORDER BY created,id LIMIT 100")?;
                let entries=q.query_map([actor],|r|Ok(InboxMessage { message_id: r.get(0)?, message: r.get(1)?, player_uuid: r.get(2)?, time: r.get(3)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(serde_json::to_value(Inbox { pending_messages: !entries.is_empty(), messages: entries })?)
            }).await,
            RpcMethod::ProcessMessage | RpcMethod::DismissReliableMessage => {
                let message=id(params,1,&["messageId","id"])?.to_string();
                self.meta.transaction(move|tx|{
                    if tx.execute("UPDATE ce_messages SET dismissed=1 WHERE recipient=? AND id=?",params![actor,message])? == 0 {
                        return Err(Error::Unavailable("no such message"));
                    }
                    Ok(serde_json::to_value(Success { success: true })?)
                }).await
            }
            RpcMethod::FriendNotify | RpcMethod::AskInGameFriendToPlay | RpcMethod::FriendReplyFromInGame => {
                let target=id(params,1,&["playerUUID","targetUUID"])?.to_string();
                let message=if m==RpcMethod::AskInGameFriendToPlay{m.as_str().to_owned()}else{text(params,2,&["message","text"],512)?};
                self.meta.transaction(move|tx|{
                    let friends:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM community_friends WHERE player=? AND friend=?)",params![actor,target],|r|r.get(0))?;
                    if !friends { return Err(Error::Community(halcyon_community::CommunityError::Forbidden)); }
                    let pending:u32=tx.query_row("SELECT COUNT(*) FROM ce_messages WHERE recipient=? AND dismissed=0",[&target],|r|r.get(0))?;
                    if pending>=100 { return Err(Error::Unavailable("inbox full")); }
                    let id=Uuid::new_v4();
                    tx.execute("INSERT INTO ce_messages(id,sender,recipient,body) VALUES(?,?,?,?)",params![id.to_string(),actor,target,message])?;
                    Ok(serde_json::to_value(MessageSent { message_id: id, success: true })?)
                }).await
            }
            RpcMethod::StoreRecordPendingGift | RpcMethod::StoreCancelPendingGift => {
                let target=id(params,1,&["recipient","playerUUID"])?;
                let sku=text(params,2,&["product_id","sku"],128)?;
                if target==profile.player_id || self.app.player_activity(target).await?.is_none(){return Err(Error::Invalid);}
                let record=m==RpcMethod::StoreRecordPendingGift;
                self.meta.transaction(move|tx|{
                    tx.execute(if record {"INSERT OR IGNORE INTO ce_pending_gifts VALUES(?,?,?)"}
                        else {"DELETE FROM ce_pending_gifts WHERE player=? AND target=? AND sku=?"},params![actor,target.to_string(),sku])?;
                    Ok(serde_json::to_value(Success { success: true })?)
                }).await
            }
            RpcMethod::ReportHonorPlayer => {
                let room=id(params,1,&["matchId"])?.to_string();
                let identifier=text(params,2,&["playerUUID","targetUUID","handle"],128)?;
                let kind=text(params,3,&["honor","reason"],64)?;
                self.meta.transaction(move|tx|{
                    let uuid=Uuid::parse_str(&identifier).ok();
                    let target=if let Some(id)=uuid {id.to_string()} else {
                        // A missing remote name could collide with a locally observed one.
                        let complete:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM events e
                          WHERE e.player=? AND json_extract(e.body,'$.type')='match_finished'
                          AND json_extract(e.body,'$.matchId')=? AND json_type(e.body,'$.result.players')='object'
                          AND NOT EXISTS(SELECT 1 FROM json_each(e.body,'$.result.players') seat
                            LEFT JOIN ce_match_handles h ON h.room=? AND h.player=seat.key WHERE h.player IS NULL))",
                            params![actor,room,room],|row|row.get(0))?;
                        if !complete {return Err(Error::Unavailable("match handle roster incomplete"));}
                        let mut rows=tx.prepare("SELECT player FROM ce_match_handles WHERE room=? AND handle=? COLLATE NOCASE LIMIT 2")?;
                        let hits=rows.query_map(params![room,identifier],|r|r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                        let [one]=hits.as_slice() else {return Err(Error::Invalid)};
                        one.clone()
                    };
                    if actor==target {return Err(Error::Invalid);}
                    let evidence:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM events e,json_each(e.body,'$.result.players') p
                        WHERE e.player=? AND json_extract(e.body,'$.type')='match_finished' AND json_extract(e.body,'$.matchId')=? AND p.key=?)",
                        params![actor,room,target],|r|r.get(0))?;
                    if !evidence {return Err(Error::Unavailable("no shared completed match"));}
                    tx.execute("INSERT INTO ce_honor VALUES(?,?,?,?) ON CONFLICT DO NOTHING",params![room,actor,target,kind])?;
                    Ok(if uuid.is_some(){serde_json::to_value(Success { success: true })?}else{serde_json::to_value(Empty {})?})
                }).await
            }
            RpcMethod::NotifyGameResults | RpcMethod::UpdateMatchInfo => {
                let room=id(params,1,&["matchId"])?;
                if profile.lobby.as_ref().and_then(|l|l.match_id)!=Some(room){return Err(Error::Invalid);}
                // Client telemetry is separate from the authoritative host outcome.
                self.meta.set(profile.player_id,if m==RpcMethod::NotifyGameResults {Preference::ReportedResult} else {Preference::ReportedMatch},Value::Array(params[1..].to_vec())).await?;
                self.project_player_info(profile).await
            }
            _ => Err(Error::Unsupported)
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
    tutorial_state: Option<super::profile::TutorialState>,
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
