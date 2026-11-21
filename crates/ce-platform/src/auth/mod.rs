pub(crate) mod profile;
pub(crate) mod provider;

pub use provider::{ProviderConfig, ProviderProduct};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, now};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Scope {
    Guest,
    Session,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Claims {
    pub player_id: Uuid,
    #[serde(rename = "playerUuid")]
    pub player_uuid: Uuid,
    pub scope: Scope,
    pub credential: String,
    session_id: Uuid,
    game_id: String,
    region: String,
    time: u64,
    exp: u64,
    #[serde(rename = "_entropy")]
    nonce: Uuid,
}

pub(crate) struct Tokens {
    signer: EncodingKey,
    verifier: DecodingKey,
}

impl Tokens {
    pub fn new(secret: &[u8]) -> Result<Self, Error> {
        if secret.len() < 32 {
            return Err(Error::Config("JWT key requires at least 32 bytes"));
        }
        Ok(Self {
            signer: EncodingKey::from_secret(secret),
            verifier: DecodingKey::from_secret(secret),
        })
    }

    pub fn sign(&self, id: Uuid, credential: &str, scope: Scope) -> Result<String, Error> {
        let time = now();
        let ttl = match scope {
            Scope::Guest => 10 * 365 * 86400,
            Scope::Session => 86400,
        };
        let claims = Claims {
            player_id: id,
            player_uuid: id,
            scope,
            credential: credential.to_owned(),
            session_id: Uuid::new_v4(),
            game_id: "kindred".into(),
            region: "na".into(),
            time,
            exp: time + ttl,
            nonce: Uuid::new_v4(),
        };
        encode(&Header::new(Algorithm::HS256), &claims, &self.signer).map_err(|_| Error::Internal)
    }

    pub fn verify(&self, raw: &str, scope: Scope) -> Result<Claims, Error> {
        let claims = decode::<Claims>(raw, &self.verifier, &Validation::new(Algorithm::HS256))
            .map_err(|_| Error::Unauthorized)?
            .claims;
        if claims.scope != scope
            || claims.game_id != "kindred"
            || claims.player_uuid != claims.player_id
        {
            return Err(Error::Unauthorized);
        }
        Ok(claims)
    }
}
