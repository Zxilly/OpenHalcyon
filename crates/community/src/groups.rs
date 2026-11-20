pub(crate) mod store;

use serde::{Deserialize, Serialize};

use crate::{CommunityError, Result, validated};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Party,
    Team,
    Guild,
}

impl GroupKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Party => "party",
            Self::Team => "team",
            Self::Guild => "guild",
        }
    }

    pub(crate) fn limit(self) -> u8 {
        match self {
            Self::Party => 10,
            Self::Team => 10,
            Self::Guild => 50,
        }
    }

    pub(crate) fn parse(kind: &str) -> Result<Self> {
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
    pub(crate) fn as_str(self) -> &'static str {
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

pub(crate) fn require_owner(role: &str) -> Result<()> {
    if role == Role::Owner.as_str() {
        Ok(())
    } else {
        Err(CommunityError::Forbidden)
    }
}

pub(crate) fn validate_settings_scope(kind: GroupKind, settings: &GroupSettings) -> Result<()> {
    if kind != GroupKind::Guild
        && (settings.tag.is_some() || settings.motto.is_some() || settings.invite_only.is_some())
    {
        return Err(CommunityError::Invalid("guild settings require a guild"));
    }
    if kind != GroupKind::Party && settings.mode.is_some() {
        return Err(CommunityError::Invalid("queue mode requires a party"));
    }
    Ok(())
}

pub(crate) fn normalize_settings(settings: GroupSettings) -> Result<GroupSettings> {
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
    Ok(GroupSettings {
        name,
        tag,
        motto,
        mode,
        invite_only: settings.invite_only,
    })
}
