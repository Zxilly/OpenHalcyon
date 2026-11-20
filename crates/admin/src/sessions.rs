use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{Admin, AdminConfig, AdminConfigError, Problem, Result};

const COOKIE: &str = "__Host-oh_admin";
const SESSION_SECONDS: u64 = 8 * 60 * 60;
const ATTEMPTS: usize = 5;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    pub(crate) username: String,
    expires_at: DateTime<Utc>,
}

struct Stored {
    key: [u8; 32],
    session: Session,
    expires: Instant,
}

#[derive(Default)]
pub(crate) struct Sessions {
    current: Mutex<Option<Stored>>,
    attempts: Mutex<VecDeque<Instant>>,
}

pub(crate) fn validate_credentials(
    config: &AdminConfig,
) -> std::result::Result<(), AdminConfigError> {
    if config.username.trim().is_empty()
        || config.username.chars().count() > 128
        || config.username.chars().any(char::is_control)
    {
        return Err(AdminConfigError::Username);
    }
    let hash =
        PasswordHash::new(&config.password_hash).map_err(|_| AdminConfigError::PasswordHash)?;
    if hash.algorithm.as_str() != "argon2id" || hash.hash.is_none() || hash.salt.is_none() {
        return Err(AdminConfigError::PasswordHash);
    }
    argon2::Params::try_from(&hash).map_err(|_| AdminConfigError::PasswordHash)?;
    Ok(())
}

impl Sessions {
    pub(crate) fn lookup(&self, headers: &HeaderMap) -> Option<Session> {
        let key = cookie_key(headers)?;
        let current = self.current.lock().ok()?;
        current
            .as_ref()
            .filter(|stored| stored.expires > Instant::now() && bool::from(stored.key.ct_eq(&key)))
            .map(|stored| stored.session.clone())
    }

    fn begin_attempt(&self) -> Result<()> {
        let now = Instant::now();
        let mut attempts = self.attempts.lock().map_err(|_| Problem::internal())?;
        while attempts
            .front()
            .is_some_and(|time| now.duration_since(*time) >= WINDOW)
        {
            attempts.pop_front();
        }
        if let Some(first) = attempts.front().filter(|_| attempts.len() >= ATTEMPTS) {
            let remaining = WINDOW.saturating_sub(now.duration_since(*first));
            let mut response = Problem::new(
                StatusCode::TOO_MANY_REQUESTS,
                super::ProblemCode::RateLimited,
                "Too many login attempts",
            );
            response.retry = Some(remaining.as_secs().saturating_add(1));
            return Err(response);
        }
        attempts.push_back(now);
        Ok(())
    }

    fn rotate(&self, username: String) -> Result<(String, Session)> {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let session = Session {
            username,
            expires_at: Utc::now() + chrono::Duration::seconds(SESSION_SECONDS as i64),
        };
        *self.current.lock().map_err(|_| Problem::internal())? = Some(Stored {
            key: digest(&token),
            session: session.clone(),
            expires: Instant::now() + Duration::from_secs(SESSION_SECONDS),
        });
        Ok((token, session))
    }

    fn revoke(&self, headers: &HeaderMap) -> Result<()> {
        if let Some(key) = cookie_key(headers) {
            let mut stored = self.current.lock().map_err(|_| Problem::internal())?;
            if stored
                .as_ref()
                .is_some_and(|session| bool::from(session.key.ct_eq(&key)))
            {
                *stored = None;
            }
        }
        Ok(())
    }
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn cookie_key(headers: &HeaderMap) -> Option<[u8; 32]> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| {
            let (name, value) = cookie.trim().split_once('=')?;
            (name == COOKIE).then_some(value)
        });
    let value = values.next()?;
    if values.next().is_some() || value.len() != 64 {
        return None;
    }
    Some(digest(value))
}

fn cookie(token: &str, age: u64) -> Result<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{COOKIE}={token}; Path=/; Max-Age={age}; Secure; HttpOnly; SameSite=Strict"
    ))
    .map_err(|_| Problem::internal())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credentials {
    username: String,
    password: String,
}

pub(crate) async fn login(
    State(state): State<Admin>,
    input: std::result::Result<Json<Credentials>, JsonRejection>,
) -> Result<Response> {
    state.sessions.begin_attempt()?;
    let Json(input) = input.map_err(|_| Problem::invalid())?;
    if input.username.is_empty()
        || input.username.chars().count() > 128
        || input.password.is_empty()
        || input.password.chars().count() > 1024
    {
        return Err(Problem::invalid());
    }
    let config = state.config.clone();
    let valid = tokio::task::spawn_blocking(move || {
        let hash = PasswordHash::new(&config.password_hash).ok()?;
        let expected = digest(&config.username);
        let provided = digest(&input.username);
        let verified = Argon2::default()
            .verify_password(input.password.as_bytes(), &hash)
            .is_ok();
        Some(verified & bool::from(expected.ct_eq(&provided)))
    })
    .await
    .map_err(|_| Problem::internal())?
    .unwrap_or(false);
    if !valid {
        return Err(Problem::unauthorized());
    }
    let (token, session) = state.sessions.rotate(state.config.username.clone())?;
    Ok((
        [(header::SET_COOKIE, cookie(&token, SESSION_SECONDS)?)],
        Json(session),
    )
        .into_response())
}

pub(crate) async fn current(Extension(session): Extension<Session>) -> Json<Session> {
    Json(session)
}

pub(crate) async fn logout(State(state): State<Admin>, headers: HeaderMap) -> Result<Response> {
    state.sessions.revoke(&headers)?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie("", 0)?)],
    )
        .into_response())
}
