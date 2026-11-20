use crate::{
    Action, CommunityError, GroupKind, GroupSettings, Result, Role,
    groups::{normalize_settings, require_owner, validate_settings_scope},
    orm::ensure_player,
    schema::{
        community_groups as groups, community_invites as invites, community_members as members,
        community_party_queues as queued, community_players as players,
    },
    validated,
    views::{GroupMember, GroupView, GuildListing, Invitation, Items},
};
use diesel::{
    dsl::{exists, not, sql},
    prelude::*,
    sql_types::{BigInt, Text},
};
use serde_json::Value;
use uuid::Uuid;

diesel::define_sql_function!(fn lower(value:Text)->Text);
diesel::define_sql_function!(fn instr(value:Text,term:Text)->BigInt);

#[derive(Queryable, Selectable)]
#[diesel(table_name=groups)]
struct GroupRow {
    id: String,
    kind: String,
    name: String,
    tag: String,
    motto: String,
    mode: String,
    invite_only: bool,
    lobby_open: bool,
    code: Option<String>,
    created: i64,
}

#[derive(AsChangeset)]
#[diesel(table_name=groups)]
struct GroupEdit {
    name: Option<String>,
    tag: Option<String>,
    motto: Option<String>,
    mode: Option<String>,
    invite_only: Option<bool>,
}
impl From<GroupSettings> for GroupEdit {
    fn from(s: GroupSettings) -> Self {
        Self {
            name: s.name,
            tag: s.tag,
            motto: s.motto,
            mode: s.mode,
            invite_only: s.invite_only,
        }
    }
}

fn group(db: &mut SqliteConnection, id: &str) -> Result<GroupRow> {
    groups::table
        .find(id)
        .select(GroupRow::as_select())
        .first(db)
        .optional()?
        .ok_or(CommunityError::Missing("group does not exist"))
}

fn membership(
    db: &mut SqliteConnection,
    actor: &str,
    kind: GroupKind,
) -> Result<Option<(String, String)>> {
    Ok(members::table
        .filter(members::player.eq(actor))
        .filter(members::kind.eq(kind.as_str()))
        .select((members::group_id, members::role))
        .first(db)
        .optional()?)
}
fn member(db: &mut SqliteConnection, actor: &str, kind: GroupKind) -> Result<(String, String)> {
    membership(db, actor, kind)?.ok_or(CommunityError::Missing("membership does not exist"))
}
fn member_role(db: &mut SqliteConnection, id: &str, actor: &str) -> Result<String> {
    members::table
        .find((id, actor))
        .select(members::role)
        .first(db)
        .optional()?
        .ok_or(CommunityError::Missing("group member does not exist"))
}
fn invited(db: &mut SqliteConnection, id: &str, actor: &str) -> Result<bool> {
    Ok(diesel::select(exists(invites::table.find((id, actor)))).get_result(db)?)
}
fn unlocked(db: &mut SqliteConnection, id: &str, kind: GroupKind) -> Result<()> {
    if kind == GroupKind::Party && diesel::select(exists(queued::table.find(id))).get_result(db)? {
        return Err(CommunityError::Conflict(
            "party is already queued or playing",
        ));
    }
    Ok(())
}
fn row_order() -> diesel::expression::SqlLiteral<BigInt> {
    sql("community_members.rowid")
}
fn narrow(n: i64) -> Result<u8> {
    u8::try_from(n).map_err(|_| CommunityError::WorkerUnavailable)
}

fn group_view(db: &mut SqliteConnection, actor: &str, id: &str) -> Result<Value> {
    let row = group(db, id)?;
    let kind = GroupKind::parse(&row.kind)?;
    let member = diesel::select(exists(members::table.find((id, actor)))).get_result::<bool>(db)?;
    if !(member || invited(db, id, actor)? || kind == GroupKind::Guild && !row.invite_only) {
        return Err(CommunityError::Forbidden);
    }
    let members = members::table
        .filter(members::group_id.eq(id))
        .inner_join(players::table.on(players::id.eq(members::player)))
        .order(row_order())
        .select((
            members::player,
            players::handle,
            members::role,
            members::side,
        ))
        .load::<(String, String, String, i64)>(db)?
        .into_iter()
        .map(|(player_id, handle, role, side)| {
            Ok(GroupMember {
                player_id,
                handle,
                role: serde::Deserialize::deserialize(serde::de::value::StrDeserializer::<
                    serde::de::value::Error,
                >::new(&role))
                .map_err(|_| CommunityError::WorkerUnavailable)?,
                side: narrow(side)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_value(GroupView {
        id: row.id,
        kind,
        name: row.name,
        tag: row.tag,
        motto: row.motto,
        mode: row.mode,
        invite_only: row.invite_only,
        lobby_open: row.lobby_open,
        join_code: if member { row.code } else { None },
        created: row.created,
        max_members: kind.limit(),
        member_count: members.len(),
        members: member.then_some(members),
    })?)
}
fn search(db: &mut SqliteConnection, term: String, limit: usize, offset: i64) -> Result<Value> {
    if !(1..=100).contains(&limit) {
        return Err(CommunityError::Invalid("invalid guild page size"));
    }
    let term = validated(term, 48, false)?.to_ascii_lowercase();
    let rows = groups::table
        .filter(groups::kind.eq(GroupKind::Guild.as_str()))
        .filter(not(groups::invite_only))
        .filter(
            instr(lower(groups::name), &term)
                .gt(0)
                .or(instr(lower(groups::tag), &term).gt(0)),
        )
        .order((lower(groups::name), groups::id))
        .limit(limit as i64)
        .offset(offset)
        .select((groups::id, groups::name, groups::tag, groups::motto))
        .load::<(String, String, String, String)>(db)?;
    let ids: Vec<_> = rows.iter().map(|(id, _, _, _)| id).collect();
    let counts = members::table
        .filter(members::group_id.eq_any(ids))
        .group_by(members::group_id)
        .select((members::group_id, diesel::dsl::count_star()))
        .load::<(String, i64)>(db)?
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let items = rows
        .into_iter()
        .map(|(id, name, tag, motto)| {
            Ok(GuildListing {
                member_count: narrow(counts.get(&id).copied().unwrap_or_default())?,
                id,
                name,
                tag,
                motto,
                max_members: GroupKind::Guild.limit(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_value(Items { items })?)
}
fn invite_view(db: &mut SqliteConnection, actor: &str, kind: GroupKind) -> Result<Value> {
    let items = invites::table
        .inner_join(groups::table.on(groups::id.eq(invites::group_id)))
        .inner_join(players::table.on(players::id.eq(invites::inviter)))
        .filter(invites::player.eq(actor))
        .filter(groups::kind.eq(kind.as_str()))
        .select((
            groups::id,
            groups::name,
            invites::inviter,
            players::handle,
            invites::created,
        ))
        .order((invites::created, groups::id))
        .load::<(String, String, String, String, i64)>(db)?
        .into_iter()
        .map(
            |(group_id, name, inviter_id, inviter, created)| Invitation {
                group_id,
                name,
                inviter_id,
                inviter,
                created,
            },
        )
        .collect::<Vec<_>>();
    Ok(serde_json::to_value(Items { items })?)
}
fn apply_edit(
    db: &mut SqliteConnection,
    id: &str,
    kind: GroupKind,
    settings: GroupSettings,
) -> Result<()> {
    validate_settings_scope(kind, &settings)?;
    unlocked(db, id, kind)?;
    let settings = normalize_settings(settings)?;
    if settings.name.is_some()
        || settings.tag.is_some()
        || settings.motto.is_some()
        || settings.mode.is_some()
        || settings.invite_only.is_some()
    {
        diesel::update(groups::table.find(id))
            .set(GroupEdit::from(settings))
            .execute(db)?;
    }
    Ok(())
}
fn role(db: &mut SqliteConnection, id: &str, actor: &str, value: Role) -> Result<()> {
    diesel::update(members::table.find((id, actor)))
        .set(members::role.eq(value.as_str()))
        .execute(db)?;
    Ok(())
}
fn remove_invites(db: &mut SqliteConnection, id: &str, actor: &str) -> Result<()> {
    diesel::delete(
        invites::table
            .filter(invites::group_id.eq(id))
            .filter(invites::inviter.eq(actor)),
    )
    .execute(db)?;
    Ok(())
}
fn remove(db: &mut SqliteConnection, id: &str, actor: &str) -> Result<()> {
    remove_invites(db, id, actor)?;
    diesel::delete(members::table.find((id, actor))).execute(db)?;
    Ok(())
}
fn join(db: &mut SqliteConnection, actor: &str, id: &str, kind: GroupKind) -> Result<()> {
    if let Some((present, _)) = membership(db, actor, kind)? {
        return if present == id {
            Ok(())
        } else {
            Err(CommunityError::Conflict("already in a group of this kind"))
        };
    }
    unlocked(db, id, kind)?;
    let members_in_group = members::table.filter(members::group_id.eq(id));
    if members_in_group.count().get_result::<i64>(db)? >= i64::from(kind.limit()) {
        return Err(CommunityError::Conflict("group is full"));
    }
    let side = if kind == GroupKind::Party {
        let side0 = members_in_group
            .filter(members::side.eq(0))
            .count()
            .get_result::<i64>(db)?;
        let side1 = members_in_group
            .filter(members::side.eq(1))
            .count()
            .get_result::<i64>(db)?;
        i64::from(side0 > side1)
    } else {
        0
    };
    diesel::insert_into(members::table)
        .values((
            members::group_id.eq(id),
            members::kind.eq(kind.as_str()),
            members::player.eq(actor),
            members::role.eq(Role::Member.as_str()),
            members::side.eq(side),
        ))
        .execute(db)?;
    diesel::delete(
        invites::table.filter(invites::player.eq(actor)).filter(
            invites::group_id.eq_any(
                groups::table
                    .filter(groups::kind.eq(kind.as_str()))
                    .select(groups::id),
            ),
        ),
    )
    .execute(db)?;
    Ok(())
}
fn leave(
    db: &mut SqliteConnection,
    actor: &str,
    kind: GroupKind,
    expected: Option<Uuid>,
) -> Result<Value> {
    let Some((id, current)) = membership(db, actor, kind)? else {
        return Ok(Value::Null);
    };
    if expected.is_some_and(|id_expected| id_expected.to_string() != id) {
        return Err(CommunityError::Conflict(
            "player is no longer in this guild",
        ));
    }
    unlocked(db, &id, kind)?;
    if current == Role::Owner.as_str() {
        let successor = members::table
            .filter(members::group_id.eq(&id))
            .filter(members::player.ne(actor))
            .select(members::player)
            .order((members::role.eq(Role::Officer.as_str()).desc(), row_order()))
            .first::<String>(db)
            .optional()?;
        if let Some(successor) = successor {
            role(db, &id, &successor, Role::Owner)?;
        } else {
            diesel::delete(groups::table.find(&id)).execute(db)?;
            return Ok(Value::Null);
        }
    }
    remove(db, &id, actor)?;
    Ok(Value::Null)
}
fn update_party(
    db: &mut SqliteConnection,
    actor: &str,
    mode: Option<String>,
    open: Option<bool>,
) -> Result<Value> {
    if mode.is_none() && open.is_none() {
        return Err(CommunityError::Invalid("party state update is empty"));
    }
    let (id, current) = member(db, actor, GroupKind::Party)?;
    require_owner(&current)?;
    if mode.is_some() {
        apply_edit(
            db,
            &id,
            GroupKind::Party,
            GroupSettings {
                mode,
                ..Default::default()
            },
        )?;
    }
    if let Some(open) = open {
        diesel::update(groups::table.find(&id))
            .set(groups::lobby_open.eq(open))
            .execute(db)?;
    }
    group_view(db, actor, &id)
}
pub(crate) fn execute(db: &mut SqliteConnection, actor: &str, action: Action) -> Result<Value> {
    use Action::*;
    match action {
        CreateGroup {
            kind,
            name,
            settings,
        } => {
            if membership(db, actor, kind)?.is_some() {
                return Err(CommunityError::Conflict("already in a group of this kind"));
            }
            let id = Uuid::new_v4().to_string();
            let code = (kind == GroupKind::Party)
                .then(|| Uuid::new_v4().simple().to_string().to_ascii_uppercase());
            diesel::insert_into(groups::table)
                .values((
                    groups::id.eq(&id),
                    groups::kind.eq(kind.as_str()),
                    groups::name.eq(validated(name, 48, true)?),
                    groups::code.eq(code),
                ))
                .execute(db)?;
            diesel::insert_into(members::table)
                .values((
                    members::group_id.eq(&id),
                    members::kind.eq(kind.as_str()),
                    members::player.eq(actor),
                    members::role.eq(Role::Owner.as_str()),
                ))
                .execute(db)?;
            apply_edit(db, &id, kind, settings)?;
            group_view(db, actor, &id)
        }
        Group { kind } => match membership(db, actor, kind)? {
            Some((id, _)) => group_view(db, actor, &id),
            None => Ok(Value::Null),
        },
        GroupById { group } => group_view(db, actor, &group.to_string()),
        GroupInvites { kind } => invite_view(db, actor, kind),
        SearchGuilds { term, limit } => search(db, term, limit, 0),
        BrowseOpenGuilds { page } => {
            let index = page
                .checked_sub(1)
                .ok_or(CommunityError::Invalid("invalid guild page"))?;
            search(db, String::new(), 50, i64::from(index) * 50)
        }
        Invite { kind, target } => {
            let (id, current) = member(db, actor, kind)?;
            if current != Role::Owner.as_str() && current != Role::Officer.as_str() {
                return Err(CommunityError::Forbidden);
            }
            let target = target.to_string();
            ensure_player(db, &target)?;
            if membership(db, &target, kind)?.is_some() {
                return Err(CommunityError::Conflict(
                    "target is already in a group of this kind",
                ));
            }
            diesel::insert_into(invites::table)
                .values((
                    invites::group_id.eq(&id),
                    invites::player.eq(&target),
                    invites::inviter.eq(actor),
                ))
                .on_conflict_do_nothing()
                .execute(db)?;
            group_view(db, actor, &id)
        }
        ResolveInvite {
            group: group_id,
            accept,
        } => {
            let id = group_id.to_string();
            let kind = GroupKind::parse(&group(db, &id)?.kind)?;
            if !invited(db, &id, actor)? {
                if accept && membership(db, actor, kind)?.is_some_and(|(present, _)| present == id)
                {
                    return group_view(db, actor, &id);
                }
                return Err(CommunityError::Missing("invite does not exist"));
            }
            if accept {
                join(db, actor, &id, kind)?;
                group_view(db, actor, &id)
            } else {
                diesel::delete(invites::table.find((&id, actor))).execute(db)?;
                invite_view(db, actor, kind)
            }
        }
        JoinOpenGuild { group } | ChangeGuildMembership { group, join: true } => {
            let id = group.to_string();
            let private = groups::table
                .find(&id)
                .filter(groups::kind.eq(GroupKind::Guild.as_str()))
                .select(groups::invite_only)
                .first::<bool>(db)
                .optional()?
                .ok_or(CommunityError::Missing("guild does not exist"))?;
            if private {
                return Err(CommunityError::Forbidden);
            }
            join(db, actor, &id, GroupKind::Guild)?;
            group_view(db, actor, &id)
        }
        ChangeGuildMembership { group, join: false } => {
            leave(db, actor, GroupKind::Guild, Some(group))
        }
        JoinPartyByCode { code } => {
            let code = validated(code, 32, true)?.to_ascii_uppercase();
            let id = groups::table
                .filter(groups::code.eq(code))
                .filter(groups::kind.eq(GroupKind::Party.as_str()))
                .select(groups::id)
                .first::<String>(db)
                .optional()?
                .ok_or(CommunityError::Missing("party does not exist"))?;
            join(db, actor, &id, GroupKind::Party)?;
            group_view(db, actor, &id)
        }
        LeaveGroup { kind } => leave(db, actor, kind, None),
        DisbandGroup { kind } => {
            let (id, current) = member(db, actor, kind)?;
            require_owner(&current)?;
            unlocked(db, &id, kind)?;
            diesel::delete(groups::table.find(&id)).execute(db)?;
            Ok(Value::Null)
        }
        Kick { kind, target } => {
            let (id, current) = member(db, actor, kind)?;
            unlocked(db, &id, kind)?;
            let target = target.to_string();
            let target_role = member_role(db, &id, &target)?;
            if actor == target
                || target_role == Role::Owner.as_str()
                || !(current == Role::Owner.as_str()
                    || current == Role::Officer.as_str() && target_role == Role::Member.as_str())
            {
                return Err(CommunityError::Forbidden);
            }
            remove(db, &id, &target)?;
            group_view(db, actor, &id)
        }
        SetRole {
            kind,
            target,
            role: new_role,
        } => {
            let (id, current) = member(db, actor, kind)?;
            require_owner(&current)?;
            if kind != GroupKind::Guild || new_role == Role::Owner {
                return Err(CommunityError::Forbidden);
            }
            let target = target.to_string();
            if member_role(db, &id, &target)? == Role::Owner.as_str() {
                return Err(CommunityError::Forbidden);
            }
            role(db, &id, &target, new_role)?;
            if new_role == Role::Member {
                remove_invites(db, &id, &target)?;
            }
            group_view(db, actor, &id)
        }
        TransferOwnership { kind, target } => {
            let (id, current) = member(db, actor, kind)?;
            require_owner(&current)?;
            unlocked(db, &id, kind)?;
            let target = target.to_string();
            member_role(db, &id, &target)?;
            if actor != target {
                let prior = if kind == GroupKind::Guild {
                    Role::Officer
                } else {
                    Role::Member
                };
                role(db, &id, actor, prior)?;
                role(db, &id, &target, Role::Owner)?;
                if prior == Role::Member {
                    remove_invites(db, &id, actor)?;
                }
            }
            group_view(db, actor, &id)
        }
        UpdateGroup { kind, settings } => {
            let (id, current) = member(db, actor, kind)?;
            require_owner(&current)?;
            apply_edit(db, &id, kind, settings)?;
            group_view(db, actor, &id)
        }
        MovePartyMember { target, side } => {
            let (id, current) = member(db, actor, GroupKind::Party)?;
            require_owner(&current)?;
            unlocked(db, &id, GroupKind::Party)?;
            if side > 1 {
                return Err(CommunityError::Invalid("invalid team side"));
            }
            let target = target.to_string();
            member_role(db, &id, &target)?;
            let side = i64::from(side);
            let count = members::table
                .filter(members::group_id.eq(&id))
                .filter(members::side.eq(side))
                .filter(members::player.ne(&target))
                .count()
                .get_result::<i64>(db)?;
            if count >= 5 {
                return Err(CommunityError::Conflict("side is full"));
            }
            diesel::update(members::table.find((&id, &target)))
                .set(members::side.eq(side))
                .execute(db)?;
            group_view(db, actor, &id)
        }
        BalanceParty => {
            let (id, current) = member(db, actor, GroupKind::Party)?;
            require_owner(&current)?;
            unlocked(db, &id, GroupKind::Party)?;
            let ids = members::table
                .filter(members::group_id.eq(&id))
                .order(row_order())
                .select(members::player)
                .load::<String>(db)?;
            for (slot, player) in ids.into_iter().enumerate() {
                diesel::update(members::table.find((&id, &player)))
                    .set(members::side.eq((slot % 2) as i64))
                    .execute(db)?;
            }
            group_view(db, actor, &id)
        }
        SetPartyLobby { open } => update_party(db, actor, None, Some(open)),
        UpdatePartyState { mode, open } => update_party(db, actor, mode, open),
        _ => Err(CommunityError::Invalid("invalid group action")),
    }
}

/// Trusted projection for transaction-local party notification fanout.
#[derive(PartialEq)]
pub struct PartySnapshot {
    pub id: String,
    pub settings: (String, bool),
    pub members: Vec<(String, String, u8)>,
}

pub fn party_snapshot(db: &mut SqliteConnection, actor: &str) -> Result<Option<PartySnapshot>> {
    let Some((id, _)) = membership(db, actor, GroupKind::Party)? else {
        return Ok(None);
    };
    let settings = groups::table
        .find(&id)
        .select((groups::mode, groups::lobby_open))
        .first(db)?;
    let members = members::table
        .filter(members::group_id.eq(&id))
        .order(members::player)
        .select((members::player, members::role, members::side))
        .load::<(String, String, i64)>(db)?
        .into_iter()
        .map(|(player, role, side)| Ok((player, role, narrow(side)?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(PartySnapshot {
        id,
        settings,
        members,
    }))
}

pub fn party_invited(db: &mut SqliteConnection, party_id: &str, actor: &str) -> Result<bool> {
    invited(db, party_id, actor)
}
