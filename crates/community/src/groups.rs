use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    Action, CommunityError, Result, ensure_player, optional_row, queue::ensure_unlocked, validated,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Party,
    Team,
    Guild,
}

impl GroupKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Party => "party",
            Self::Team => "team",
            Self::Guild => "guild",
        }
    }

    fn limit(self) -> u8 {
        match self {
            Self::Party => 10,
            Self::Team => 10,
            Self::Guild => 50,
        }
    }

    fn parse(kind: &str) -> Result<Self> {
        match kind {
            "party" => Ok(Self::Party),
            "team" => Ok(Self::Team),
            "guild" => Ok(Self::Guild),
            _ => Err(CommunityError::Invalid("unknown group type")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Officer,
    Member,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Officer => "officer",
            Self::Member => "member",
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSettings {
    pub name: Option<String>,
    pub tag: Option<String>,
    pub motto: Option<String>,
    pub mode: Option<String>,
    pub invite_only: Option<bool>,
}

fn membership(
    tx: &Transaction<'_>,
    actor: &str,
    kind: GroupKind,
) -> Result<Option<(String, String)>> {
    tx.query_row(
        "SELECT group_id, role FROM community_members WHERE player=? AND kind=?",
        params![actor, kind.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn member(tx: &Transaction<'_>, actor: &str, kind: GroupKind) -> Result<(String, String)> {
    membership(tx, actor, kind)?.ok_or(CommunityError::Missing("membership does not exist"))
}

fn member_role(tx: &Transaction<'_>, group: &str, player: &str) -> Result<String> {
    optional_row(
        tx.query_row(
            "SELECT role FROM community_members WHERE group_id=? AND player=?",
            params![group, player],
            |row| row.get(0),
        ),
        "group member does not exist",
    )
}

fn require_owner(role: &str) -> Result<()> {
    if role == "owner" {
        Ok(())
    } else {
        Err(CommunityError::Forbidden)
    }
}

fn join(tx: &Transaction<'_>, actor: &str, group: &str, kind: GroupKind) -> Result<()> {
    if let Some((existing, _)) = membership(tx, actor, kind)? {
        if existing == group {
            return Ok(());
        }
        return Err(CommunityError::Conflict("already in a group of this kind"));
    }
    if kind == GroupKind::Party {
        ensure_unlocked(tx, group)?;
    }
    let size: u8 = tx.query_row(
        "SELECT count(*) FROM community_members WHERE group_id=?",
        [group],
        |row| row.get(0),
    )?;
    if size >= kind.limit() {
        return Err(CommunityError::Conflict("group is full"));
    }
    let side = if kind == GroupKind::Party {
        let (first, second): (u8, u8) = tx.query_row(
            "SELECT count(*) FILTER(WHERE side=0), count(*) FILTER(WHERE side=1)
             FROM community_members WHERE group_id=?",
            [group],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        u8::from(first > second)
    } else {
        0
    };
    tx.execute(
        "INSERT INTO community_members(group_id, kind, player, role, side) VALUES(?, ?, ?, 'member', ?)",
        params![group, kind.as_str(), actor, side],
    )?;
    tx.execute(
        "DELETE FROM community_invites WHERE player=? AND group_id IN (SELECT id FROM community_groups WHERE kind=?)",
        params![actor, kind.as_str()],
    )?;
    Ok(())
}

fn update(tx: &Transaction<'_>, kind: GroupKind, id: &str, settings: GroupSettings) -> Result<()> {
    if kind != GroupKind::Guild
        && (settings.tag.is_some() || settings.motto.is_some() || settings.invite_only.is_some())
    {
        return Err(CommunityError::Invalid("guild settings require a guild"));
    }
    if kind != GroupKind::Party && settings.mode.is_some() {
        return Err(CommunityError::Invalid("queue mode requires a party"));
    }
    if kind == GroupKind::Party {
        ensure_unlocked(tx, id)?;
    }
    let name = settings
        .name
        .map(|value| validated(value, 48, true))
        .transpose()?;
    let tag = settings
        .tag
        .map(|value| validated(value, 8, false))
        .transpose()?;
    let motto = settings
        .motto
        .map(|value| validated(value, 180, false))
        .transpose()?;
    let mode = settings
        .mode
        .map(|value| {
            let value = validated(value, 32, false)?;
            if !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            {
                return Err(CommunityError::Invalid("invalid party queue mode"));
            }
            Ok(value)
        })
        .transpose()?;
    tx.execute(
        "UPDATE community_groups SET name=coalesce(?, name),tag=coalesce(?, tag),motto=coalesce(?, motto),
         mode=coalesce(?, mode),invite_only=coalesce(?, invite_only) WHERE id=?",
        params![name, tag, motto, mode, settings.invite_only, id],
    )?;
    Ok(())
}

fn remove(tx: &Transaction<'_>, group: &str, player: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM community_invites WHERE group_id=? AND inviter=?",
        params![group, player],
    )?;
    tx.execute(
        "DELETE FROM community_members WHERE group_id=? AND player=?",
        params![group, player],
    )?;
    Ok(())
}

fn group_info(tx: &Transaction<'_>, actor: &str, id: &str) -> Result<Value> {
    let mut output = optional_row(
        tx.query_row(
            "SELECT id, kind, name, tag, motto, mode, invite_only, lobby_open, code, created,
                EXISTS(SELECT 1 FROM community_members WHERE group_id=?1 AND player=?2) AS member,
                EXISTS(SELECT 1 FROM community_invites WHERE group_id=?1 AND player=?2) AS invited
         FROM community_groups WHERE id=?1",
            params![id, actor],
            |row| {
                let member: bool = row.get(10)?;
                let invited: bool = row.get(11)?;
                let kind: String = row.get(1)?;
                let invite_only: bool = row.get(6)?;
                Ok((
                    member || invited || (kind == "guild" && !invite_only),
                    member,
                    json!({
                        "id":row.get::<_, String>(0)?, "kind":kind, "name":row.get::<_, String>(2)?,
                        "tag":row.get::<_, String>(3)?, "motto":row.get::<_, String>(4)?,
                        "mode":row.get::<_, String>(5)?, "inviteOnly":invite_only,
                        "lobbyOpen":row.get::<_, bool>(7)?,
                        "joinCode":if member { row.get::<_, Option<String>>(8)? } else { None },
                        "created":row.get::<_, i64>(9)?,
                    }),
                ))
            },
        ),
        "group does not exist",
    )?;
    if !output.0 {
        return Err(CommunityError::Forbidden);
    }
    let kind = GroupKind::parse(
        output.2["kind"]
            .as_str()
            .ok_or(CommunityError::WorkerUnavailable)?,
    )?;
    output.2["maxMembers"] = json!(kind.limit());
    let mut members = tx.prepare(
        "SELECT m.player,p.handle,m.role,m.side
         FROM community_members m JOIN community_players p ON p.id=m.player
         WHERE group_id=? ORDER BY m.rowid",
    )?;
    let rows = members
        .query_map([id], |row| {
            Ok(json!({
                "playerId":row.get::<_, String>(0)?, "handle":row.get::<_, String>(1)?,
                "role":row.get::<_, String>(2)?, "side":row.get::<_, u8>(3)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    output.2["memberCount"] = json!(rows.len());
    if output.1 {
        output.2["members"] = json!(rows);
    }
    Ok(output.2)
}

fn search_guilds(tx: &Transaction<'_>, term: String, limit: usize) -> Result<Value> {
    if !(1..=100).contains(&limit) {
        return Err(CommunityError::Invalid("invalid guild page size"));
    }
    let term = validated(term, 48, false)?.to_ascii_lowercase();
    let mut query = tx.prepare(
        "SELECT g.id,g.name,g.tag,g.motto,(SELECT count(*) FROM community_members m WHERE m.group_id=g.id)
         FROM community_groups g WHERE g.kind='guild' AND NOT g.invite_only
           AND (instr(lower(g.name), ?1)>0 OR instr(lower(g.tag), ?1)>0)
         ORDER BY lower(g.name),g.id LIMIT ?2",
    )?;
    let items = query.query_map(params![term, limit], |row| Ok(json!({
        "id":row.get::<_, String>(0)?, "name":row.get::<_, String>(1)?, "tag":row.get::<_, String>(2)?,
        "motto":row.get::<_, String>(3)?, "memberCount":row.get::<_, u8>(4)?, "maxMembers":GroupKind::Guild.limit(),
    })))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({ "items": items }))
}

fn invites(tx: &Transaction<'_>, actor: &str, kind: GroupKind) -> Result<Value> {
    let mut query = tx.prepare(
        "SELECT g.id,g.name,i.inviter,p.handle,i.created FROM community_invites i
         JOIN community_groups g ON g.id=i.group_id JOIN community_players p ON p.id=i.inviter
         WHERE i.player=? AND g.kind=? ORDER BY i.created,g.id",
    )?;
    let items = query
        .query_map(params![actor, kind.as_str()], |row| {
            Ok(json!({
                "groupId":row.get::<_, String>(0)?, "name":row.get::<_, String>(1)?,
                "inviterId":row.get::<_, String>(2)?, "inviter":row.get::<_, String>(3)?,
                "created":row.get::<_, i64>(4)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({ "items": items }))
}

fn apply_membership(
    tx: &Transaction<'_>,
    actor: &str,
    group: String,
    accept: bool,
) -> Result<Value> {
    let (kind, invited): (String, bool) = optional_row(tx.query_row(
        "SELECT g.kind, EXISTS(SELECT 1 FROM community_invites i WHERE i.group_id=g.id AND i.player=?2)
         FROM community_groups g WHERE g.id=?1", params![group, actor],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ), "group does not exist")?;
    let kind = GroupKind::parse(&kind)?;
    if !invited {
        if accept && membership(tx, actor, kind)?.is_some_and(|(id, _)| id == group) {
            return group_info(tx, actor, &group);
        }
        return Err(CommunityError::Missing("invite does not exist"));
    }
    if accept {
        join(tx, actor, &group, kind)?;
        group_info(tx, actor, &group)
    } else {
        tx.execute(
            "DELETE FROM community_invites WHERE group_id=? AND player=?",
            params![group, actor],
        )?;
        invites(tx, actor, kind)
    }
}

fn join_guild(tx: &Transaction<'_>, actor: &str, group: Uuid) -> Result<Value> {
    let id = group.to_string();
    let private: bool = optional_row(
        tx.query_row(
            "SELECT invite_only FROM community_groups WHERE id=? AND kind='guild'",
            [&id],
            |row| row.get(0),
        ),
        "guild does not exist",
    )?;
    if private {
        return Err(CommunityError::Forbidden);
    }
    join(tx, actor, &id, GroupKind::Guild)?;
    group_info(tx, actor, &id)
}

fn leave_group(
    tx: &Transaction<'_>,
    actor: &str,
    kind: GroupKind,
    expected: Option<Uuid>,
) -> Result<Value> {
    let Some((id, role)) = membership(tx, actor, kind)? else {
        return Ok(Value::Null);
    };
    if expected.is_some_and(|expected| expected.to_string() != id) {
        return Err(CommunityError::Conflict(
            "player is no longer in this guild",
        ));
    }
    if kind == GroupKind::Party {
        ensure_unlocked(tx, &id)?;
    }
    if role == "owner" {
        let successor: Option<String> = tx
            .query_row(
                "SELECT player FROM community_members WHERE group_id=? AND player<>?
             ORDER BY role='officer' DESC,rowid LIMIT 1",
                params![id, actor],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(successor) = successor {
            tx.execute(
                "UPDATE community_members SET role='owner' WHERE group_id=? AND player=?",
                params![id, successor],
            )?;
        } else {
            tx.execute("DELETE FROM community_groups WHERE id=?", [&id])?;
            return Ok(Value::Null);
        }
    }
    remove(tx, &id, actor)?;
    Ok(Value::Null)
}

fn update_party(
    tx: &Transaction<'_>,
    actor: &str,
    mode: Option<String>,
    open: Option<bool>,
) -> Result<Value> {
    if mode.is_none() && open.is_none() {
        return Err(CommunityError::Invalid("party state update is empty"));
    }
    let (id, role) = member(tx, actor, GroupKind::Party)?;
    require_owner(&role)?;
    if mode.is_some() {
        update(
            tx,
            GroupKind::Party,
            &id,
            GroupSettings {
                mode,
                ..Default::default()
            },
        )?;
    }
    if let Some(open) = open {
        tx.execute(
            "UPDATE community_groups SET lobby_open=? WHERE id=?",
            params![open, id],
        )?;
    }
    group_info(tx, actor, &id)
}

pub(crate) fn execute(tx: &Transaction<'_>, actor: &str, action: Action) -> Result<Value> {
    match action {
        Action::CreateGroup {
            kind,
            name,
            settings,
        } => {
            if membership(tx, actor, kind)?.is_some() {
                return Err(CommunityError::Conflict("already in a group of this kind"));
            }
            let id = Uuid::new_v4();
            let code = (kind == GroupKind::Party)
                .then(|| Uuid::new_v4().simple().to_string().to_ascii_uppercase());
            let id = id.to_string();
            let name = validated(name, 48, true)?;
            tx.execute(
                "INSERT INTO community_groups(id, kind, name, code) VALUES(?, ?, ?, ?)",
                params![id, kind.as_str(), name, code],
            )?;
            tx.execute(
                "INSERT INTO community_members(group_id,kind,player,role) VALUES(?,?,?,'owner')",
                params![id, kind.as_str(), actor],
            )?;
            update(tx, kind, &id, settings)?;
            group_info(tx, actor, &id)
        }
        Action::Group { kind } => membership(tx, actor, kind)?
            .map(|(id, _)| group_info(tx, actor, &id))
            .unwrap_or(Ok(Value::Null)),
        Action::GroupById { group } => group_info(tx, actor, &group.to_string()),
        Action::GroupInvites { kind } => invites(tx, actor, kind),
        Action::SearchGuilds { term, limit } => search_guilds(tx, term, limit),
        Action::Invite { kind, target } => {
            let (id, role) = member(tx, actor, kind)?;
            if !matches!(role.as_str(), "owner" | "officer") {
                return Err(CommunityError::Forbidden);
            }
            let target = target.to_string();
            ensure_player(tx, &target)?;
            if membership(tx, &target, kind)?.is_some() {
                return Err(CommunityError::Conflict(
                    "target is already in a group of this kind",
                ));
            }
            tx.execute(
                "INSERT INTO community_invites(group_id,player,inviter) VALUES(?,?,?)
                 ON CONFLICT DO NOTHING",
                params![id, target, actor],
            )?;
            group_info(tx, actor, &id)
        }
        Action::ResolveInvite { group, accept } => {
            apply_membership(tx, actor, group.to_string(), accept)
        }
        Action::JoinOpenGuild { group } | Action::ChangeGuildMembership { group, join: true } => {
            join_guild(tx, actor, group)
        }
        Action::ChangeGuildMembership { group, join: false } => {
            leave_group(tx, actor, GroupKind::Guild, Some(group))
        }
        Action::JoinPartyByCode { code } => {
            let code = validated(code, 32, true)?.to_ascii_uppercase();
            let id: String = optional_row(
                tx.query_row(
                    "SELECT id FROM community_groups WHERE code=? AND kind='party'",
                    [code],
                    |row| row.get(0),
                ),
                "party does not exist",
            )?;
            join(tx, actor, &id, GroupKind::Party)?;
            group_info(tx, actor, &id)
        }
        Action::LeaveGroup { kind } => leave_group(tx, actor, kind, None),
        Action::DisbandGroup { kind } => {
            let (id, role) = member(tx, actor, kind)?;
            require_owner(&role)?;
            if kind == GroupKind::Party {
                ensure_unlocked(tx, &id)?;
            }
            tx.execute("DELETE FROM community_groups WHERE id=?", [&id])?;
            Ok(Value::Null)
        }
        Action::Kick { kind, target } => {
            let (id, role) = member(tx, actor, kind)?;
            if kind == GroupKind::Party {
                ensure_unlocked(tx, &id)?;
            }
            let target = target.to_string();
            let target_role = member_role(tx, &id, &target)?;
            if actor == target
                || target_role == "owner"
                || !(role == "owner" || (role == "officer" && target_role == "member"))
            {
                return Err(CommunityError::Forbidden);
            }
            remove(tx, &id, &target)?;
            group_info(tx, actor, &id)
        }
        Action::SetRole { kind, target, role } => {
            let (id, current) = member(tx, actor, kind)?;
            require_owner(&current)?;
            if kind != GroupKind::Guild || role == Role::Owner {
                return Err(CommunityError::Forbidden);
            }
            let target = target.to_string();
            if member_role(tx, &id, &target)? == "owner" {
                return Err(CommunityError::Forbidden);
            }
            tx.execute(
                "UPDATE community_members SET role=? WHERE group_id=? AND player=?",
                params![role.as_str(), id, target],
            )?;
            if role == Role::Member {
                tx.execute(
                    "DELETE FROM community_invites WHERE group_id=? AND inviter=?",
                    params![id, target],
                )?;
            }
            group_info(tx, actor, &id)
        }
        Action::TransferOwnership { kind, target } => {
            let (id, role) = member(tx, actor, kind)?;
            require_owner(&role)?;
            if kind == GroupKind::Party {
                ensure_unlocked(tx, &id)?;
            }
            let target = target.to_string();
            member_role(tx, &id, &target)?;
            if actor != target {
                let next = if kind == GroupKind::Guild {
                    "officer"
                } else {
                    "member"
                };
                tx.execute(
                    "UPDATE community_members SET role=? WHERE group_id=? AND player=?",
                    params![next, id, actor],
                )?;
                tx.execute(
                    "UPDATE community_members SET role='owner' WHERE group_id=? AND player=?",
                    params![id, target],
                )?;
                if next == "member" {
                    tx.execute(
                        "DELETE FROM community_invites WHERE group_id=? AND inviter=?",
                        params![id, actor],
                    )?;
                }
            }
            group_info(tx, actor, &id)
        }
        Action::UpdateGroup { kind, settings } => {
            let (id, role) = member(tx, actor, kind)?;
            require_owner(&role)?;
            update(tx, kind, &id, settings)?;
            group_info(tx, actor, &id)
        }
        Action::MovePartyMember { target, side } => {
            let (id, role) = member(tx, actor, GroupKind::Party)?;
            require_owner(&role)?;
            ensure_unlocked(tx, &id)?;
            if side > 1 {
                return Err(CommunityError::Invalid("invalid team side"));
            }
            let target = target.to_string();
            member_role(tx, &id, &target)?;
            let count: u8 = tx.query_row(
                "SELECT count(*) FROM community_members WHERE group_id=? AND side=? AND player<>?",
                params![id, side, target],
                |row| row.get(0),
            )?;
            if count >= 5 {
                return Err(CommunityError::Conflict("side is full"));
            }
            tx.execute(
                "UPDATE community_members SET side=? WHERE group_id=? AND player=?",
                params![side, id, target],
            )?;
            group_info(tx, actor, &id)
        }
        Action::BalanceParty => {
            let (id, role) = member(tx, actor, GroupKind::Party)?;
            require_owner(&role)?;
            ensure_unlocked(tx, &id)?;
            let mut query =
                tx.prepare("SELECT player FROM community_members WHERE group_id=? ORDER BY rowid")?;
            let ids = query
                .query_map([&id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (slot, player) in ids.into_iter().enumerate() {
                tx.execute(
                    "UPDATE community_members SET side=? WHERE group_id=? AND player=?",
                    params![slot % 2, id, player],
                )?;
            }
            group_info(tx, actor, &id)
        }
        Action::SetPartyLobby { open } => update_party(tx, actor, None, Some(open)),
        Action::UpdatePartyState { mode, open } => update_party(tx, actor, mode, open),
        _ => Err(CommunityError::Invalid("invalid group action")),
    }
}
