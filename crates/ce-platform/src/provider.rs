//! Optional operator attestations. The verifier holds only a public key.
use crate::RpcMethod;
use crate::{Error, Platform, meta::Meta, now};
use halcyon_community::Community;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub public_key_pem: PathBuf,
    pub issuer: String,
    pub audience: String,
    #[serde(default)]
    pub products: BTreeMap<String, ProviderProduct>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProduct {
    pub item: String,
    pub count: u32,
}

pub(crate) struct Verifier {
    key: DecodingKey,
    validation: Validation,
    issuer: String,
    products: BTreeMap<String, ProviderProduct>,
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'_' | b'-'))
}
impl Platform {
    pub fn configure_provider(this: &mut Arc<Self>, config: ProviderConfig) -> Result<(), Error> {
        let verifier = Verifier::new(config)?;
        Arc::get_mut(this)
            .ok_or(Error::Config(
                "configure the provider before sharing the platform",
            ))?
            .provider = Some(verifier);
        Ok(())
    }
}
impl Verifier {
    fn new(c: ProviderConfig) -> Result<Self, Error> {
        if c.issuer.trim().is_empty()
            || c.audience.trim().is_empty()
            || c.products
                .iter()
                .any(|(p, g)| !identifier(p) || !identifier(&g.item) || g.count == 0)
        {
            return Err(Error::Config("invalid provider configuration"));
        }
        let key = DecodingKey::from_ed_pem(&std::fs::read(c.public_key_pem)?)
            .map_err(|_| Error::Config("invalid provider Ed25519 public key"))?;
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[&c.issuer]);
        validation.set_audience(&[&c.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 0;
        Ok(Self {
            key,
            validation,
            issuer: c.issuer,
            products: c.products,
        })
    }
    fn verify(&self, raw: &str, account: Uuid) -> Result<Claims, Error> {
        if raw.len() > 8192 {
            return Err(Error::Invalid);
        }
        let claims = decode::<Claims>(raw, &self.key, &self.validation)
            .map_err(|_| Error::Unauthorized)?
            .claims;
        let t = now();
        if claims.sub != account
            || claims.iat > t
            || claims.exp <= t
            || claims.exp <= claims.iat
            || claims.exp - claims.iat > 300
        {
            return Err(Error::Unauthorized);
        }
        Ok(claims)
    }
}
#[derive(Deserialize, Serialize)]
struct Claims {
    sub: Uuid,
    iat: u64,
    exp: u64,
    #[serde(flatten)]
    proof: Proof,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "purpose")]
enum Proof {
    #[serde(rename = "iap.purchase")]
    Purchase {
        order: Uuid,
        product: String,
        receipt: String,
    },
    #[serde(rename = "identity.verified")]
    Verified { valid_until: i64 },
}

pub(crate) fn initialize(db: &Connection) -> Result<(), Error> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS ce_provider_orders(
        id TEXT PRIMARY KEY, player TEXT NOT NULL, request TEXT NOT NULL, product TEXT NOT NULL, item TEXT NOT NULL,
        quantity INTEGER NOT NULL, expires INTEGER NOT NULL, receipt BLOB UNIQUE, UNIQUE(player,request));
        CREATE TABLE IF NOT EXISTS ce_provider_identity(player TEXT PRIMARY KEY, valid_until INTEGER NOT NULL);")?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    product: String,
    request_id: Option<Uuid>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Attestation {
    attestation: String,
}
fn input<T: serde::de::DeserializeOwned>(args: &[Value]) -> Result<T, Error> {
    let [v] = args else {
        return Err(Error::Invalid);
    };
    serde_json::from_value(v.clone()).map_err(|_| Error::Invalid)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedOrder {
    order_id: String,
    product: String,
    expires_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FulfilledOrder {
    success: bool,
    order_id: Uuid,
    product: String,
    item: String,
    count: u32,
}

#[derive(Serialize)]
struct Verification {
    verified: bool,
}

fn prepare_order(
    tx: &rusqlite::Transaction<'_>,
    player: &str,
    request: Prepare,
    grant: ProviderProduct,
) -> Result<Value, Error> {
    let request_id = request.request_id.unwrap_or_else(Uuid::new_v4).to_string();
    let expires = now() + 3600;
    tx.execute(
        "INSERT INTO ce_provider_orders(id,player,request,product,item,quantity,expires)
         VALUES(?,?,?,?,?,?,?) ON CONFLICT(player,request) DO NOTHING",
        params![
            Uuid::new_v4().to_string(),
            player,
            request_id,
            request.product,
            grant.item,
            grant.count,
            expires
        ],
    )?;
    let (order, product, expiry): (String, String, u64) = tx.query_row(
        "SELECT id,product,expires FROM ce_provider_orders WHERE player=? AND request=?",
        params![player, request_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if product != request.product {
        return Err(Error::Invalid);
    }
    Ok(serde_json::to_value(PreparedOrder {
        order_id: order,
        product,
        expires_at: expiry,
    })?)
}

fn fulfill(
    tx: &rusqlite::Transaction<'_>,
    account: Uuid,
    order: Uuid,
    product: String,
    reference: Vec<u8>,
) -> Result<Value, Error> {
    let order_id = order.to_string();
    let (item, count, expires, previous) = tx.query_row(
        "SELECT item,quantity,expires,receipt FROM ce_provider_orders WHERE id=? AND player=? AND product=?",
        params![order_id,account.to_string(),product],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,u32>(1)?, r.get::<_,u64>(2)?, r.get::<_,Option<Vec<u8>>>(3)?)),
    ).optional()?.ok_or(Error::Invalid)?;
    if let Some(previous) = previous {
        if previous != reference {
            return Err(Error::Invalid);
        }
    } else {
        if expires <= now() {
            return Err(Error::Invalid);
        }
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ce_provider_orders WHERE receipt=?)",
            [&reference],
            |r| r.get(0),
        )?;
        if used {
            return Err(Error::Invalid);
        }
        Community::award_item_in_transaction(tx, order, account, &item, count)?;
        tx.execute(
            "UPDATE ce_provider_orders SET receipt=? WHERE id=?",
            params![reference, order_id],
        )?;
    }
    Ok(serde_json::to_value(FulfilledOrder {
        success: true,
        order_id: order,
        product,
        item,
        count,
    })?)
}

pub(crate) async fn dispatch(
    provider: Option<&Verifier>,
    meta: &Meta,
    account: Uuid,
    method: RpcMethod,
    args: &[Value],
) -> Result<Value, Error> {
    let player = account.to_string();
    if method == RpcMethod::IsGovernmentIdVerified {
        return meta.transaction(move |tx| {
            let verified: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM ce_provider_identity WHERE player=? AND valid_until>?)",
                params![player,now()], |r| r.get(0),
            )?;
            Ok(serde_json::to_value(Verification { verified })?)
        }).await;
    }
    let unavailable = if method == RpcMethod::VerifyGovernmentId {
        "identity verification provider unavailable"
    } else {
        "payment provider unavailable"
    };
    let verifier = provider.ok_or(Error::Unavailable(unavailable))?;
    if method == RpcMethod::StorePrepareIAP {
        let request: Prepare = input(args)?;
        let grant = verifier
            .products
            .get(&request.product)
            .cloned()
            .ok_or(Error::Invalid)?;
        return meta
            .with_behavior(TransactionBehavior::Immediate, move |tx| {
                prepare_order(tx, &player, request, grant)
            })
            .await;
    }
    let signed: Attestation = input(args)?;
    let claims = verifier.verify(&signed.attestation, account)?;
    match (method, claims.proof) {
        (RpcMethod::VerifyGovernmentId, Proof::Verified { valid_until })
            if valid_until > now() as i64 =>
        {
            meta.with_behavior(TransactionBehavior::Immediate, move |tx| {
                tx.execute(
                    "INSERT INTO ce_provider_identity VALUES(?,?) ON CONFLICT(player) DO UPDATE
                     SET valid_until=MAX(valid_until,excluded.valid_until)",
                    params![player, valid_until],
                )?;
                Ok(serde_json::to_value(Verification { verified: true })?)
            })
            .await
        }
        (
            RpcMethod::StoreProcessIAP,
            Proof::Purchase {
                order,
                product,
                receipt,
            },
        ) => {
            if receipt.is_empty()
                || receipt.len() > 512
                || !verifier.products.contains_key(&product)
            {
                return Err(Error::Invalid);
            }
            let mut digest = Sha256::new();
            digest.update(verifier.issuer.as_bytes());
            digest.update([0]);
            digest.update(receipt.as_bytes());
            let reference = digest.finalize().to_vec();
            meta.with_behavior(TransactionBehavior::Immediate, move |tx| {
                fulfill(tx, account, order, product, reference)
            })
            .await
        }
        _ => Err(Error::Unauthorized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{
        SigningKey,
        pkcs8::{EncodePrivateKey, EncodePublicKey},
    };
    use halcyon_community::ItemDefinition;
    use jsonwebtoken::{EncodingKey, Header, encode};

    fn key(seed: u8) -> (String, EncodingKey) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let private = signing.to_pkcs8_der().unwrap();
        (
            signing
                .verifying_key()
                .to_public_key_pem(Default::default())
                .unwrap(),
            EncodingKey::from_ed_der(private.as_bytes()),
        )
    }
    #[derive(Serialize)]
    struct TestAttestation {
        iss: &'static str,
        aud: &'static str,
        #[serde(flatten)]
        claims: Claims,
    }

    fn claims(sub: Uuid, proof: Proof) -> Value {
        let iat = now();
        serde_json::to_value(TestAttestation {
            iss: "operator",
            aud: "test-instance",
            claims: Claims {
                sub,
                iat,
                exp: iat + 180,
                proof,
            },
        })
        .unwrap()
    }

    fn sign(claims: &Value, key: &EncodingKey) -> Value {
        serde_json::to_value(Attestation {
            attestation: encode(&Header::new(Algorithm::EdDSA), claims, key).unwrap(),
        })
        .unwrap()
    }
    async fn prepare(v: &Verifier, m: &Meta, id: Uuid, request: Uuid) -> Value {
        dispatch(
            Some(v),
            m,
            id,
            RpcMethod::StorePrepareIAP,
            &[serde_json::to_value(Prepare {
                product: "credits".into(),
                request_id: Some(request),
            })
            .unwrap()],
        )
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn signed_grants_are_scoped_replay_safe_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let path = dir.path().join("provider.pub");
        let (public, signer) = key(17);
        std::fs::write(&path, public).unwrap();
        let verifier = Verifier::new(ProviderConfig {
            public_key_pem: path,
            issuer: "operator".into(),
            audience: "test-instance".into(),
            products: BTreeMap::from([(
                "credits".into(),
                ProviderProduct {
                    item: "coin".into(),
                    count: 5,
                },
            )]),
        })
        .unwrap();
        struct NoHost;
        impl halcyon_platform::MatchHost for NoHost {
            fn reserve(
                &self,
                _: halcyon_platform::Reservation,
            ) -> halcyon_platform::Result<halcyon_platform::HostedMatch> {
                Err(halcyon_platform::PlatformError::HostUnavailable)
            }
        }
        let _platform = halcyon_platform::AppState::open(
            halcyon_platform::PlatformConfig {
                database_path: db.clone(),
                public_address: "localhost:1".into(),
                seats_per_match: 1,
            },
            Arc::new(NoHost),
        )
        .unwrap();
        let community = Community::open(&db).unwrap();
        let meta = Meta::open(&db).unwrap();
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        community
            .observe_player(id, "one".into(), true)
            .await
            .unwrap();
        community
            .observe_player(other, "two".into(), true)
            .await
            .unwrap();
        let request = Uuid::new_v4();
        let order = prepare(&verifier, &meta, id, request).await;
        assert_eq!(order, prepare(&verifier, &meta, id, request).await);
        let valid = claims(
            id,
            Proof::Purchase {
                order: serde_json::from_value(order["orderId"].clone()).unwrap(),
                product: "credits".into(),
                receipt: "test-receipt-001".into(),
            },
        );
        let signed = sign(&valid, &signer);
        // Unknown operator item rolls back the receipt, allowing the same order to be fulfilled later.
        assert!(
            dispatch(
                Some(&verifier),
                &meta,
                id,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed)
            )
            .await
            .is_err()
        );
        community
            .define_item(ItemDefinition {
                id: "coin".into(),
                name: "Coin".into(),
                slot: "currency".into(),
            })
            .await
            .unwrap();
        for (field, bad) in [
            ("sub", serde_json::to_value(other).unwrap()),
            ("aud", Value::from("wrong")),
            ("iss", Value::from("wrong")),
            ("exp", Value::from(now() - 1)),
            ("iat", Value::from(now() + 30)),
            ("product", Value::from("other")),
            ("purpose", Value::from("identity.verified")),
        ] {
            let mut forged = valid.clone();
            forged[field] = bad;
            assert!(
                dispatch(
                    Some(&verifier),
                    &meta,
                    id,
                    RpcMethod::StoreProcessIAP,
                    &[sign(&forged, &signer)]
                )
                .await
                .is_err(),
                "{field}"
            );
        }
        assert!(
            dispatch(
                Some(&verifier),
                &meta,
                id,
                RpcMethod::StoreProcessIAP,
                &[sign(&valid, &key(3).1)]
            )
            .await
            .is_err()
        );
        let args = std::slice::from_ref(&signed);
        let (a, b) = tokio::join!(
            dispatch(Some(&verifier), &meta, id, RpcMethod::StoreProcessIAP, args),
            dispatch(Some(&verifier), &meta, id, RpcMethod::StoreProcessIAP, args)
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let second = prepare(&verifier, &meta, other, Uuid::new_v4()).await;
        let mut reused = valid.clone();
        reused["sub"] = serde_json::to_value(other).unwrap();
        reused["order"] = second["orderId"].clone();
        assert!(
            dispatch(
                Some(&verifier),
                &meta,
                other,
                RpcMethod::StoreProcessIAP,
                &[sign(&reused, &signer)]
            )
            .await
            .is_err()
        );
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            conn.query_row("SELECT SUM(count) FROM community_inventory", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert_eq!(
            conn.query_row(
                "SELECT length(receipt) FROM ce_provider_orders WHERE receipt IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            32
        );
        let get = RpcMethod::IsGovernmentIdVerified;
        assert_eq!(
            dispatch(None, &meta, id, get, &[]).await.unwrap()["verified"],
            false
        );
        assert!(matches!(
            dispatch(None, &meta, id, RpcMethod::VerifyGovernmentId, &[]).await,
            Err(Error::Unavailable(_))
        ));
        let proof = claims(
            id,
            Proof::Verified {
                valid_until: now() as i64 + 3600,
            },
        );
        let signed_id = sign(&proof, &signer);
        assert!(
            dispatch(
                Some(&verifier),
                &meta,
                id,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed_id)
            )
            .await
            .is_err()
        );
        dispatch(
            Some(&verifier),
            &meta,
            id,
            RpcMethod::VerifyGovernmentId,
            &[signed_id],
        )
        .await
        .unwrap();
        let reopened = Meta::open(&db).unwrap();
        assert_eq!(
            dispatch(None, &reopened, id, get, &[]).await.unwrap()["verified"],
            true
        );
        assert_eq!(
            dispatch(None, &reopened, other, get, &[]).await.unwrap()["verified"],
            false
        );
    }
}
