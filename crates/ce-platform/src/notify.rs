use super::{Error, Platform, now};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Grants(Mutex<HashMap<Uuid, (String, u64)>>);
impl Grants {
    pub fn issue(&self, session: &str) -> Result<Uuid, Error> {
        let mut grants = self.0.lock().map_err(|_| Error::Internal)?;
        let current = now();
        grants.retain(|_, (_, expires)| *expires > current);
        if let Some((id, (_, expiry))) = grants.iter_mut().find(|(_, (s, _))| s == session) {
            *expiry = current + 86400;
            return Ok(*id);
        }
        let id = Uuid::new_v4();
        grants.insert(id, (session.to_owned(), current + 86400));
        Ok(id)
    }
    fn resolve(&self, id: Uuid) -> Option<String> {
        self.0
            .lock()
            .ok()?
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
impl Cursor {
    fn after(&self) -> Result<i64, StatusCode> {
        if self
            .time
            .as_ref()
            .is_some_and(|v| !v.is_empty() && v.parse::<u64>().is_err())
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        match self.tag.as_deref() {
            Some(tag) if !tag.is_empty() => Uuid::parse_str(tag)
                .ok()
                .and_then(|id| i64::try_from(id.as_u128()).ok())
                .ok_or(StatusCode::BAD_REQUEST),
            _ => Ok(0),
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

async fn update(s: &Platform, key: &str, after: i64) -> Result<(i64, Value), StatusCode> {
    let batch = s
        .app
        .events(key, after, true)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let changed = !batch.events.is_empty();
    let mut rv = json!({"httpStatus":if changed{200}else{304},"time":now().to_string(),"tag":Uuid::from_u128(batch.cursor as u128)});
    if changed {
        let profile = s
            .app
            .profile(key)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
        let state = super::state(&profile);
        let states: serde_json::Map<_, _> = state
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    json!(
                        v.as_str()
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| v.to_string())
                    ),
                )
            })
            .collect();
        rv["text"] = json!({"type":"stateUpdate","code":0,"states":states,"counts":[1]});
    }
    Ok((batch.cursor, rv))
}

async fn poll(
    State(s): State<Arc<Platform>>,
    Path(id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>, StatusCode> {
    let key = s.grants.resolve(id).ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(Json(update(&s, &key, cursor.after()?).await?.1))
}

async fn socket(
    State(s): State<Arc<Platform>>,
    Path(id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let key = s.grants.resolve(id).ok_or(StatusCode::UNAUTHORIZED)?;
    let after = cursor.after()?;
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
