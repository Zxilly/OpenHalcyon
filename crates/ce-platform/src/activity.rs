use super::{Error, Platform, meta::Meta, player_info};
use halcyon_platform::PlayerProfile;
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn initialize(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE INDEX IF NOT EXISTS ce_event_kind ON events(player,json_extract(body,'$.type'));
        CREATE TABLE IF NOT EXISTS ce_scores(player TEXT NOT NULL,achievement TEXT NOT NULL,score INTEGER NOT NULL,
            PRIMARY KEY(player,achievement));
        CREATE TABLE IF NOT EXISTS ce_messages(id TEXT PRIMARY KEY, sender TEXT NOT NULL,
            recipient TEXT NOT NULL, body TEXT NOT NULL, created INTEGER DEFAULT(unixepoch()), dismissed INTEGER DEFAULT 0);
        CREATE INDEX IF NOT EXISTS ce_recipient ON ce_messages(recipient,dismissed,created);
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

pub(crate) fn handles(m: &str) -> bool {
    matches!(
        m,
        "getPlayerInfos"
            | "estimatedWaitTime"
            | "achievementGetScore"
            | "achievementStoreScore"
            | "addDeviceToken"
            | "queryPlayerInboxMessages"
            | "processMessage"
            | "dismissReliableMessage"
            | "friendNotify"
            | "askInGameFriendToPlay"
            | "friendReplyFromInGame"
            | "storeRecordPendingGift"
            | "storeCancelPendingGift"
            | "notifyGameResults"
            | "updateMatchInfo"
            | "reportHonorPlayer"
    )
}

impl Meta {
    pub(crate) async fn player_fields(&self, player: Uuid) -> Result<Value, Error> {
        self.transaction(move|tx| {
            let player=player.to_string();
            let mut info=super::match_stats::player_fields(tx, &player)?;
            let mut inv=tx.prepare("SELECT item,count FROM community_inventory i JOIN community_items d ON d.id=i.item WHERE player=? AND d.slot='currency'")?;
            let balances=inv.query_map([&player],|r|Ok((r.get::<_,String>(0)?,json!(r.get::<_,u32>(1)?))))?
                .collect::<rusqlite::Result<serde_json::Map<String,Value>>>()?;
            let mut currency=super::profile::empty_currency();
            currency.as_object_mut().unwrap().extend(balances);
            info["currency"]=currency;
            info["pendingMessages"]=json!(tx.query_row("SELECT count(*) FROM ce_messages WHERE recipient=? AND dismissed=0",[&player],|r|r.get::<_,u32>(0))?);
            let mut invites=tx.prepare("SELECT g.kind,count(*) FROM community_invites i JOIN community_groups g ON g.id=i.group_id WHERE i.player=? GROUP BY g.kind")?;
            for row in invites.query_map([&player],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?)))?{
                let (kind,count)=row?;
                let field = match kind.as_str() {"party"=>"pendingPartyInvites","team"=>"pendingTeamInvites","guild"=>"pendingGuildInvites",_=>continue};
                info[field]=json!(count);
            }
            let mut groups=tx.prepare("SELECT kind,group_id FROM community_members WHERE player=?")?;
            for row in groups.query_map([&player],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
                let (kind,id)=row?;
                let field=match kind.as_str() {"party"=>"partyUUID","team"=>"teamUUID","guild"=>"guildUUID",_=>continue};
                info[field]=json!(id);
            }
            let mut pref=tx.prepare("SELECT category,data FROM ce_preferences WHERE player=? AND category IN ('tutorial','last_login')")?;
            for row in pref.query_map([player],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
                let (category,raw)=row?;
                let data=serde_json::from_str(&raw)?;
                if category=="last_login" {
                    info["last_login"]=data;
                } else if let Some(tutorial)=super::profile::tutorial_state(&data) {
                    info["tutorialState"]=tutorial;
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
        m: &str,
        params: &[Value],
    ) -> Result<Value, Error> {
        let actor = profile.player_id.to_string();
        match m {
            "getPlayerInfos" => {
                let ids=field(params,1,&["playerUUIDs","players"])?.as_array().filter(|ids| ids.len()<=50).ok_or(Error::Invalid)?;
                let mut profiles=Vec::new();
                for id in ids {
                    let key=Uuid::parse_str(id.as_str().ok_or(Error::Invalid)?).map_err(|_|Error::Invalid)?;
                    if let Some(activity)=self.app.player_activity(key).await? {
                        profiles.push(json!({"playerUUID":key,"handle":activity.profile.name}));
                    }
                }
                Ok(json!({"playerInfos":profiles}))
            }
            "estimatedWaitTime" => {
                let mode=text(params,1,&["lobby","mode","queueName"],128)?;
                self.meta.transaction(move|tx|{
                    let wait:Option<f64>=tx.query_row("SELECT seconds FROM ce_wait_history WHERE mode=?",[mode],|r|r.get(0)).optional()?;
                    Ok(json!({"estimatedWait":wait.unwrap_or(0.0).round() as u64}))
                }).await
            }
            "achievementGetScore" | "achievementStoreScore" => {
                let key=text(params,1,&["achievementId","achievement"],128)?;
                let score=if m=="achievementStoreScore" {Some(field(params,2,&["score"])?.as_i64().filter(|n|*n>=0).ok_or(Error::Invalid)?)}else{None};
                self.meta.transaction(move|tx|{
                    if let Some(n)=score {tx.execute("INSERT INTO ce_scores VALUES(?,?,?) ON CONFLICT(player,achievement)
                        DO UPDATE SET score=max(score,excluded.score)",params![actor,key,n])?;}
                    let n:Option<i64>=tx.query_row("SELECT score FROM ce_scores WHERE player=? AND achievement=?",params![actor,key],|r|r.get(0)).optional()?;
                    Ok(json!({"achievementId":key,"score":n.unwrap_or(0)}))
                }).await
            }
            "addDeviceToken" => {
                let token=text(params,1,&["deviceToken","token"],4096)?;
                self.meta.set(profile.player_id,"device_token",json!(token)).await?;
                Ok(json!({"success":true}))
            }
            "queryPlayerInboxMessages" => self.meta.transaction(move|tx|{
                let mut q=tx.prepare("SELECT id,body,sender,created FROM ce_messages WHERE recipient=? AND dismissed=0 ORDER BY created,id LIMIT 100")?;
                let entries=q.query_map([actor],|r|Ok(json!({"messageId":r.get::<_,String>(0)?,"message":r.get::<_,String>(1)?,
                    "playerUUID":r.get::<_,String>(2)?,"time":r.get::<_,i64>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(json!({"messages":entries,"pendingMessages":!entries.is_empty()}))
            }).await,
            "processMessage" | "dismissReliableMessage" => {
                let message=id(params,1,&["messageId","id"])?.to_string();
                self.meta.transaction(move|tx|{
                    if tx.execute("UPDATE ce_messages SET dismissed=1 WHERE recipient=? AND id=?",params![actor,message])? == 0 {
                        return Err(Error::Unavailable("no such message"));
                    }
                    Ok(json!({"success":true}))
                }).await
            }
            "friendNotify" | "askInGameFriendToPlay" | "friendReplyFromInGame" => {
                let target=id(params,1,&["playerUUID","targetUUID"])?.to_string();
                let message=if m=="askInGameFriendToPlay"{m.to_owned()}else{text(params,2,&["message","text"],512)?};
                self.meta.transaction(move|tx|{
                    let friends:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM community_friends WHERE player=? AND friend=?)",params![actor,target],|r|r.get(0))?;
                    if !friends { return Err(Error::Community(halcyon_community::CommunityError::Forbidden)); }
                    let pending:u32=tx.query_row("SELECT COUNT(*) FROM ce_messages WHERE recipient=? AND dismissed=0",[&target],|r|r.get(0))?;
                    if pending>=100 { return Err(Error::Unavailable("inbox full")); }
                    let id=Uuid::new_v4();
                    tx.execute("INSERT INTO ce_messages(id,sender,recipient,body) VALUES(?,?,?,?)",params![id.to_string(),actor,target,message])?;
                    Ok(json!({"messageId":id,"success":true}))
                }).await
            }
            "storeRecordPendingGift" | "storeCancelPendingGift" => {
                let target=id(params,1,&["recipient","playerUUID"])?;
                let sku=text(params,2,&["product_id","sku"],128)?;
                if target==profile.player_id || self.app.player_activity(target).await?.is_none(){return Err(Error::Invalid);}
                let record=m=="storeRecordPendingGift";
                self.meta.transaction(move|tx|{
                    tx.execute(if record {"INSERT OR IGNORE INTO ce_pending_gifts VALUES(?,?,?)"}
                        else {"DELETE FROM ce_pending_gifts WHERE player=? AND target=? AND sku=?"},params![actor,target.to_string(),sku])?;
                    Ok(json!({"success":true}))
                }).await
            }
            "reportHonorPlayer" => {
                let room=id(params,1,&["matchId"])?.to_string();
                let target=id(params,2,&["playerUUID","targetUUID"])?.to_string();
                if actor==target {return Err(Error::Invalid);}
                let kind=text(params,3,&["honor","reason"],64)?;
                self.meta.transaction(move|tx|{
                    let evidence:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM events e,json_each(e.body,'$.result.players') p
                        WHERE e.player=? AND json_extract(e.body,'$.type')='match_finished' AND json_extract(e.body,'$.matchId')=? AND p.key=?)",
                        params![actor,room,target],|r|r.get(0))?;
                    if !evidence {return Err(Error::Unavailable("no shared completed match"));}
                    tx.execute("INSERT INTO ce_honor VALUES(?,?,?,?) ON CONFLICT DO NOTHING",params![room,actor,target,kind])?;
                    Ok(json!({"success":true}))
                }).await
            }
            "notifyGameResults" | "updateMatchInfo" => {
                let room=id(params,1,&["matchId"])?;
                if profile.lobby.as_ref().and_then(|l|l.match_id)!=Some(room){return Err(Error::Invalid);}
                // Client telemetry is separate from the authoritative host outcome.
                self.meta.set(profile.player_id,if m=="notifyGameResults" {"reported_result"} else {"reported_match"},json!(&params[1..])).await?;
                Ok(player_info(profile))
            }
            _ => Err(Error::Unsupported)
        }
    }
}
