use super::{Error, Platform, now};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Grants(Mutex<GrantIndex>);

#[derive(Default)]
struct GrantIndex {
    sessions: HashMap<String, Uuid>,
    grants: HashMap<Uuid, (String, u64)>,
    deadlines: BinaryHeap<Reverse<(u64, Uuid)>>,
}

impl GrantIndex {
    fn expire(&mut self, current: u64) {
        while let Some(&Reverse((deadline, id))) = self.deadlines.peek() {
            if deadline > current {
                break;
            }
            self.deadlines.pop();
            if let Some((_, expires)) = self.grants.get(&id)
                && *expires > current
            {
                // Each capability occupies exactly one timer entry even when renewed.
                self.deadlines.push(Reverse((*expires, id)));
            } else if let Some((key, _)) = self.grants.remove(&id) {
                self.sessions.remove(&key);
            }
        }
    }
}
impl Grants {
    pub fn issue(&self, session: &str) -> Result<Uuid, Error> {
        let mut grants = self.0.lock().map_err(|_| Error::Internal)?;
        let current = now();
        grants.expire(current);
        if let Some(id) = grants.sessions.get(session).copied()
            && let Some((_, expiry)) = grants.grants.get_mut(&id)
        {
            *expiry = current + 86400;
            return Ok(id);
        }
        let id = Uuid::new_v4();
        grants.sessions.insert(session.to_owned(), id);
        grants
            .grants
            .insert(id, (session.to_owned(), current + 86400));
        grants.deadlines.push(Reverse((current + 86400, id)));
        Ok(id)
    }
    fn resolve(&self, id: Uuid) -> Option<String> {
        self.0
            .lock()
            .ok()?
            .grants
            .get(&id)
            .filter(|(_, expires)| *expires > now())
            .map(|(session, _)| session.clone())
    }
}

#[derive(Default, Deserialize)]
struct Cursor {
    tag: Option<String>,
    time: Option<String>,
}
#[derive(Clone, Copy)]
struct Position {
    after: i64,
    time: u64,
    initial: bool,
}

impl Cursor {
    fn position(&self) -> Result<Position, StatusCode> {
        let time = self.time.as_deref().filter(|time| !time.is_empty());
        let tag = self.tag.as_deref().filter(|tag| !tag.is_empty());
        let after = tag
            .map(|tag| {
                Uuid::parse_str(tag)
                    .ok()
                    .and_then(|id| i64::try_from(id.as_u128()).ok())
                    .ok_or(StatusCode::BAD_REQUEST)
            })
            .transpose()?
            .unwrap_or(0);
        let initial = time.is_none() && tag.is_none();
        let time = time
            .map(|time| time.parse::<u64>().map_err(|_| StatusCode::BAD_REQUEST))
            .transpose()?
            .unwrap_or(0);
        Ok(Position {
            after,
            time,
            initial,
        })
    }
}

fn platform_status(error: halcyon_platform::PlatformError) -> StatusCode {
    match error {
        halcyon_platform::PlatformError::Unauthorized => StatusCode::UNAUTHORIZED,
        halcyon_platform::PlatformError::Invalid(_) => StatusCode::BAD_REQUEST,
        _ => {
            tracing::warn!(%error, "notification storage unavailable");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

pub fn router(s: Arc<Platform>) -> Router {
    Router::new()
        .route("/lp/{grant}", get(poll))
        .route("/ws/{grant}", get(socket))
        .route("/chat/{grant}", get(chat))
        .with_state(s)
}

async fn update(s: &Platform, key: &str, old: Position) -> Result<(Position, Value), StatusCode> {
    let event = s
        .app
        .next_notification(key, old.after, old.initial)
        .await
        .map_err(platform_status)?;
    if event.is_none() && !old.initial {
        return Ok((
            old,
            json!({
                "httpStatus":304,"time":old.time.to_string(),"tag":Uuid::from_u128(old.after as u128),
            }),
        ));
    }
    let next = Position {
        after: event.as_ref().map_or(0, |event| event.cursor),
        time: now(),
        initial: false,
    };
    let tag = Uuid::from_u128(next.after as u128);
    let kind = (!old.initial)
        .then(|| event.as_ref()?.body["type"].as_str())
        .flatten();
    let text = match kind {
        Some("friends_changed") => {
            json!({"code":0,"type":"friendsListUpdate","friendsListUpdate":true})
        }
        Some(
            kind @ ("quintPartyStateUpdate"
            | "quintPartyJoinRequest"
            | "quintPartyPlayerLeft"
            | "quintPartyMemberKick"),
        ) => {
            json!({"code":0,(kind):{}})
        }
        _ => {
            let profile = s.app.profile(key).await.map_err(platform_status)?;
            let mut state = super::state(&profile);
            state["version"] = json!(tag);
            state["lastVersion"] = json!(Uuid::from_u128(old.after as u128));
            let states: serde_json::Map<_, _> = state
                .as_object()
                .expect("state is an object")
                .iter()
                .filter(|(key, _)| key.as_str() != "spectatorId")
                .map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map_or_else(|| value.to_string(), ToOwned::to_owned);
                    (key.clone(), json!(value))
                })
                .collect();
            json!({"type":"stateUpdate","code":0,"states":states,"counts":[1]})
        }
    };
    Ok((
        next,
        json!({"httpStatus":200,"time":next.time.to_string(),"tag":tag,"text":text}),
    ))
}

async fn poll(
    State(s): State<Arc<Platform>>,
    Path(id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
) -> Result<impl IntoResponse, StatusCode> {
    let key = s.grants.resolve(id).ok_or(StatusCode::UNAUTHORIZED)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(update(&s, &key, cursor.position()?).await?.1),
    ))
}

async fn socket(
    State(s): State<Arc<Platform>>,
    Path(id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let key = s.grants.resolve(id).ok_or(StatusCode::UNAUTHORIZED)?;
    let after = cursor.position()?;
    s.app.profile(&key).await.map_err(platform_status)?;
    Ok(ws.max_message_size(4096).on_upgrade(move |mut socket| async move {
        let mut cursor=after;
        loop {
            tokio::select!{
                res=update(&s,&key,cursor)=>{
                    let Ok((seq,v))=res else {break};
                    if !send(&mut socket,Message::Text(v.to_string().into())).await {break}
                    cursor=seq;
                }
                input=socket.recv()=>match input {Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{}, _=>break}
            }
        }
    }).into_response())
}

async fn send(s: &mut WebSocket, msg: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(5), s.send(msg)).await,
        Ok(Ok(()))
    )
}

#[derive(Deserialize)]
struct Chat {
    message: String,
}
async fn chat(
    State(s): State<Arc<Platform>>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let key = s.grants.resolve(id).ok_or(StatusCode::UNAUTHORIZED)?;
    s.app.profile(&key).await.map_err(platform_status)?;
    Ok(ws.max_message_size(4096).on_upgrade(move |mut sock|async move{
        let mut cursor=0;
        loop {
            tokio::select!{
                batch=s.app.events(&key,cursor,true)=>{
                    let Ok(batch)=batch else{break};cursor=batch.cursor;
                    for event in batch.events {
                        if event.body["type"]=="chat" && !send(&mut sock,Message::Text(event.body.to_string().into())).await{return}
                    }
                }
                msg=sock.recv()=>match msg {
                    Some(Ok(Message::Text(raw)))=>{
                        let Ok(item)=serde_json::from_str::<Chat>(&raw) else{break};
                        let Ok(profile)=s.app.profile(&key).await else{break};
                        let Some(room)=profile.lobby.and_then(|l|l.match_id) else{break};
                        if s.app.send_chat(&key,room,item.message).await.is_err(){break}
                    }
                    Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},
                    _=>break
                }
            }
        }
    }).into_response())
}
