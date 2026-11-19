use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
    sync::{Arc, Mutex},
};

use halcyon_platform::PlayerProfile;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{Error, Platform, now};

const AUTH_LIFETIME: u64 = 10 * 60;

#[derive(Default)]
pub(crate) struct Chat {
    pub(crate) authority: Option<String>,
    grants: Mutex<Index>,
}

struct Grant {
    password: String,
    session: String,
    expires: u64,
}

#[derive(Default)]
struct Index {
    grants: HashMap<Uuid, Grant>,
    expiries: BinaryHeap<Reverse<(u64, Uuid)>>,
}

impl Index {
    fn expire(&mut self, current: u64) {
        while let Some(&Reverse((deadline, player))) = self.expiries.peek() {
            if deadline > current {
                break;
            }
            self.expiries.pop();
            if let Some(grant) = self.grants.get(&player) {
                if grant.expires > current {
                    self.expiries.push(Reverse((grant.expires, player)));
                } else {
                    self.grants.remove(&player);
                }
            }
        }
    }
}

impl Chat {
    pub(crate) fn fields(
        &self,
        session: &str,
        profile: &PlayerProfile,
    ) -> Result<Option<Value>, Error> {
        let Some(authority) = self.authority.as_deref() else {
            return Ok(None);
        };
        let pass = self.issue(profile.player_id, session)?;
        Ok(Some(json!({
            "chatUrl":format!("xmpp://{authority}"),
            "chatUser":profile.player_id,
            "chatPass":pass,
        })))
    }

    fn issue(&self, player: Uuid, session: &str) -> Result<String, Error> {
        let mut index = self.grants.lock().map_err(|_| Error::Internal)?;
        let current = now();
        index.expire(current);
        if let Some(grant) = index.grants.get(&player)
            && grant.session == session
            && grant.expires > current + 60
        {
            return Ok(grant.password.clone());
        }
        let mut password = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        password.truncate(40);
        let expires = current + AUTH_LIFETIME;
        let previous = index.grants.insert(
            player,
            Grant {
                password: password.clone(),
                session: session.to_owned(),
                expires,
            },
        );
        if previous.is_none() {
            index.expiries.push(Reverse((expires, player)));
        }
        Ok(password)
    }

    fn check(&self, player: Uuid, password: &str) -> Option<String> {
        if password.len() != 40 {
            return None;
        }
        let mut index = self.grants.lock().ok()?;
        index.expire(now());
        let grant = index.grants.get(&player)?;
        bool::from(grant.password.as_bytes().ct_eq(password.as_bytes()))
            .then(|| grant.session.clone())
    }
}

impl Platform {
    pub fn with_xmpp(mut self: Arc<Self>, authority: &str) -> Result<Arc<Self>, Error> {
        let authority = authority
            .parse::<axum::http::uri::Authority>()
            .ok()
            .filter(|value| {
                value.port_u16().is_some_and(|port| port != 0) && !value.as_str().contains('@')
            })
            .ok_or(Error::Config("expected XMPP public host:port"))?;
        let platform =
            Arc::get_mut(&mut self).ok_or(Error::Config("configure XMPP before serving"))?;
        platform.xmpp.authority = Some(authority.to_string());
        Ok(self)
    }

    async fn xmpp_presence_preferences(&self, session: &str) -> (bool, bool) {
        let Ok(actor) = self.app.profile(session).await else {
            return (false, false);
        };
        self.community
            .presence_preferences(actor.player_id)
            .await
            .unwrap_or((false, false))
    }
}

#[async_trait::async_trait]
impl halcyon_chat::Directory for Platform {
    async fn authenticate(&self, user: Uuid, pass: &str) -> Option<String> {
        let session = self.xmpp.check(user, pass)?;
        let actor = self.app.profile(&session).await.ok()?.player_id;
        (actor == user).then_some(session)
    }

    async fn contacts(&self, session: &str) -> Option<Vec<halcyon_chat::Contact>> {
        let actor = self.app.profile(session).await.ok()?.player_id;
        let mut contacts = BTreeMap::new();
        for (id, name) in self.app.local_match_contacts(session).await.ok()? {
            contacts.insert(id, name);
        }
        let friends = self
            .community
            .execute(actor, halcyon_community::Action::Friends)
            .await
            .ok()?;
        for contact in friends["friends"].as_array()? {
            let id = Uuid::parse_str(contact["playerId"].as_str()?).ok()?;
            let handle = contact["handle"].as_str()?;
            contacts.insert(id, handle.to_owned());
        }
        Some(
            contacts
                .into_iter()
                .map(|(id, name)| halcyon_chat::Contact { id, name })
                .collect(),
        )
    }

    async fn presence_allowed(&self, session: &str) -> bool {
        self.xmpp_presence_preferences(session).await.0
    }

    async fn receives_presence(&self, session: &str) -> bool {
        self.xmpp_presence_preferences(session).await.1
    }
}
