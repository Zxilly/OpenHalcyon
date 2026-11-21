//! Optional operator attestations. The verifier holds only a public key.
use crate::{Error, Platform, RpcMethod, now};
use diesel::{dsl::exists, prelude::*, sql_types::BigInt, upsert::excluded};
use halcyon_community::Community;
use halcyon_storage::schema::{ce_provider_identity, ce_provider_orders};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

diesel::define_sql_function! {
    #[sql_name = "max"]
    fn max_valid_until(a: BigInt, b: BigInt) -> BigInt;
}

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

fn checked_timestamp(seconds: u64) -> Result<i64, Error> {
    i64::try_from(seconds).map_err(|_| Error::Internal)
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
    db: &mut SqliteConnection,
    player: &str,
    request: Prepare,
    grant: ProviderProduct,
) -> Result<Value, Error> {
    let request_id = request.request_id.unwrap_or_else(Uuid::new_v4).to_string();
    let expires = checked_timestamp(now().checked_add(3600).ok_or(Error::Internal)?)?;
    diesel::insert_into(ce_provider_orders::table)
        .values((
            ce_provider_orders::id.eq(Uuid::new_v4().to_string()),
            ce_provider_orders::player.eq(player),
            ce_provider_orders::request.eq(&request_id),
            ce_provider_orders::product.eq(&request.product),
            ce_provider_orders::item.eq(&grant.item),
            ce_provider_orders::quantity.eq(i64::from(grant.count)),
            ce_provider_orders::expires.eq(expires),
        ))
        .on_conflict((ce_provider_orders::player, ce_provider_orders::request))
        .do_nothing()
        .execute(db)?;
    let (order, product, expiry) = ce_provider_orders::table
        .filter(ce_provider_orders::player.eq(player))
        .filter(ce_provider_orders::request.eq(&request_id))
        .select((
            ce_provider_orders::id,
            ce_provider_orders::product,
            ce_provider_orders::expires,
        ))
        .first::<(String, String, i64)>(db)?;
    if product != request.product {
        return Err(Error::Invalid);
    }
    Ok(serde_json::to_value(PreparedOrder {
        order_id: order,
        product,
        expires_at: u64::try_from(expiry).map_err(|_| Error::Internal)?,
    })?)
}

fn fulfill(
    db: &mut SqliteConnection,
    account: Uuid,
    order: Uuid,
    product: String,
    reference: Vec<u8>,
) -> Result<Value, Error> {
    let order_id = order.to_string();
    let player = account.to_string();
    let (item, quantity, expires, previous) = ce_provider_orders::table
        .find(&order_id)
        .filter(ce_provider_orders::player.eq(&player))
        .filter(ce_provider_orders::product.eq(&product))
        .select((
            ce_provider_orders::item,
            ce_provider_orders::quantity,
            ce_provider_orders::expires,
            ce_provider_orders::receipt,
        ))
        .first::<(String, i64, i64, Option<Vec<u8>>)>(db)
        .optional()?
        .ok_or(Error::Invalid)?;
    let count = u32::try_from(quantity).map_err(|_| Error::Internal)?;
    let expires = u64::try_from(expires).map_err(|_| Error::Internal)?;
    if let Some(previous) = previous {
        if previous != reference {
            return Err(Error::Invalid);
        }
    } else {
        if expires <= now() {
            return Err(Error::Invalid);
        }
        let used = diesel::select(exists(
            ce_provider_orders::table.filter(ce_provider_orders::receipt.eq(&reference)),
        ))
        .get_result::<bool>(db)?;
        if used {
            return Err(Error::Invalid);
        }
        Community::award_item_in_orm_transaction(db, order, account, &item, count)?;
        diesel::update(ce_provider_orders::table.find(&order_id))
            .set(ce_provider_orders::receipt.eq(Some(&reference[..])))
            .execute(db)?;
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
    community: &Community,
    account: Uuid,
    method: RpcMethod,
    args: &[Value],
) -> Result<Value, Error> {
    let player = account.to_string();
    if method == RpcMethod::IsGovernmentIdVerified {
        let current = checked_timestamp(now())?;
        return community
            .orm_read(move |db| {
                let verified = diesel::select(exists(
                    ce_provider_identity::table
                        .find(&player)
                        .filter(ce_provider_identity::valid_until.gt(current)),
                ))
                .get_result::<bool>(db)?;
                Ok(serde_json::to_value(Verification { verified })?)
            })
            .await;
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
        return community
            .orm_try_transaction(move |db| prepare_order(db, &player, request, grant))
            .await;
    }
    let signed: Attestation = input(args)?;
    let claims = verifier.verify(&signed.attestation, account)?;
    match (method, claims.proof) {
        (RpcMethod::VerifyGovernmentId, Proof::Verified { valid_until })
            if valid_until > checked_timestamp(now())? =>
        {
            community
                .orm_try_transaction(move |db| {
                    diesel::insert_into(ce_provider_identity::table)
                        .values((
                            ce_provider_identity::player.eq(&player),
                            ce_provider_identity::valid_until.eq(valid_until),
                        ))
                        .on_conflict(ce_provider_identity::player)
                        .do_update()
                        .set(ce_provider_identity::valid_until.eq(max_valid_until(
                            ce_provider_identity::valid_until,
                            excluded(ce_provider_identity::valid_until),
                        )))
                        .execute(db)?;
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
            community
                .orm_try_transaction(move |db| fulfill(db, account, order, product, reference))
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
    use rusqlite::Connection;

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

    fn test_verifier(dir: &std::path::Path, seed: u8) -> (Verifier, EncodingKey) {
        let path = dir.join("provider.pub");
        let (public, signer) = key(seed);
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
        (verifier, signer)
    }

    fn open_provider_community(db: &std::path::Path) -> Community {
        Community::open(db).unwrap()
    }

    async fn prepare(v: &Verifier, c: &Community, id: Uuid, request: Uuid) -> Value {
        dispatch(
            Some(v),
            c,
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

    fn purchase_arg(sub: Uuid, order: Uuid, receipt: &str, signer: &EncodingKey) -> Value {
        sign(
            &claims(
                sub,
                Proof::Purchase {
                    order,
                    product: "credits".into(),
                    receipt: receipt.into(),
                },
            ),
            signer,
        )
    }

    async fn define_coin(community: &Community) {
        community
            .define_item(ItemDefinition {
                id: "coin".into(),
                name: "Coin".into(),
                slot: "currency".into(),
            })
            .await
            .unwrap();
    }

    fn inventory_balance(conn: &Connection, player: Uuid) -> i64 {
        conn.query_row(
            "SELECT COALESCE(SUM(count), 0) FROM community_inventory WHERE player = ?",
            [player.to_string()],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn signed_grants_are_scoped_replay_safe_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let (verifier, signer) = test_verifier(dir.path(), 17);
        struct NoHost;
        impl halcyon_platform::MatchHost for NoHost {
            fn reserve(
                &self,
                _: halcyon_platform::Reservation,
            ) -> halcyon_platform::HostResult<halcyon_platform::HostedMatch> {
                Err(halcyon_platform::HostError::HostUnavailable)
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
        let community = open_provider_community(&db);
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
        let order = prepare(&verifier, &community, id, request).await;
        assert_eq!(order, prepare(&verifier, &community, id, request).await);
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
                &community,
                id,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed)
            )
            .await
            .is_err()
        );
        define_coin(&community).await;
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
                    &community,
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
                &community,
                id,
                RpcMethod::StoreProcessIAP,
                &[sign(&valid, &key(3).1)]
            )
            .await
            .is_err()
        );
        let args = std::slice::from_ref(&signed);
        let (a, b) = tokio::join!(
            dispatch(
                Some(&verifier),
                &community,
                id,
                RpcMethod::StoreProcessIAP,
                args
            ),
            dispatch(
                Some(&verifier),
                &community,
                id,
                RpcMethod::StoreProcessIAP,
                args
            )
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let second = prepare(&verifier, &community, other, Uuid::new_v4()).await;
        let mut reused = valid.clone();
        reused["sub"] = serde_json::to_value(other).unwrap();
        reused["order"] = second["orderId"].clone();
        assert!(
            dispatch(
                Some(&verifier),
                &community,
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
            dispatch(None, &community, id, get, &[]).await.unwrap()["verified"],
            false
        );
        assert!(matches!(
            dispatch(None, &community, id, RpcMethod::VerifyGovernmentId, &[]).await,
            Err(Error::Unavailable(_))
        ));
        let longer = now() as i64 + 7200;
        let shorter = now() as i64 + 3600;
        let longest = now() as i64 + 10800;
        let proof = claims(
            id,
            Proof::Verified {
                valid_until: longer,
            },
        );
        let signed_id = sign(&proof, &signer);
        assert!(
            dispatch(
                Some(&verifier),
                &community,
                id,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed_id)
            )
            .await
            .is_err()
        );
        dispatch(
            Some(&verifier),
            &community,
            id,
            RpcMethod::VerifyGovernmentId,
            &[signed_id],
        )
        .await
        .unwrap();
        dispatch(
            Some(&verifier),
            &community,
            id,
            RpcMethod::VerifyGovernmentId,
            &[sign(
                &claims(
                    id,
                    Proof::Verified {
                        valid_until: shorter,
                    },
                ),
                &signer,
            )],
        )
        .await
        .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT valid_until FROM ce_provider_identity WHERE player = ?",
                [id.to_string()],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
            longer
        );
        dispatch(
            Some(&verifier),
            &community,
            id,
            RpcMethod::VerifyGovernmentId,
            &[sign(
                &claims(
                    id,
                    Proof::Verified {
                        valid_until: longest,
                    },
                ),
                &signer,
            )],
        )
        .await
        .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT valid_until FROM ce_provider_identity WHERE player = ?",
                [id.to_string()],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
            longest
        );
        let reopened = open_provider_community(&db);
        assert_eq!(
            dispatch(None, &reopened, id, get, &[]).await.unwrap()["verified"],
            true
        );
        assert_eq!(
            dispatch(None, &reopened, other, get, &[]).await.unwrap()["verified"],
            false
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('ce_provider_orders','ce_provider_identity')",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM __diesel_schema_migrations WHERE version = '20261120000005'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn duplicate_receipt_delivery_grants_once_and_rejects_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("duplicate.db");
        let (verifier, signer) = test_verifier(dir.path(), 31);
        let community = open_provider_community(&db);
        let player = Uuid::new_v4();
        let other = Uuid::new_v4();
        community
            .observe_player(player, "buyer".into(), true)
            .await
            .unwrap();
        community
            .observe_player(other, "other".into(), true)
            .await
            .unwrap();
        define_coin(&community).await;

        let first = prepare(&verifier, &community, player, Uuid::new_v4()).await;
        let first_id: Uuid = serde_json::from_value(first["orderId"].clone()).unwrap();
        let same_player_second = prepare(&verifier, &community, player, Uuid::new_v4()).await;
        let same_player_second_id: Uuid =
            serde_json::from_value(same_player_second["orderId"].clone()).unwrap();
        let other_player_order = prepare(&verifier, &community, other, Uuid::new_v4()).await;
        let other_player_order_id: Uuid =
            serde_json::from_value(other_player_order["orderId"].clone()).unwrap();

        let receipt_arg = purchase_arg(player, first_id, "shared-receipt-001", &signer);
        let (c1, c2, c3) = tokio::join!(
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&receipt_arg)
            ),
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&receipt_arg)
            ),
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&receipt_arg)
            ),
        );
        let r1 = c1.unwrap();
        assert_eq!(r1, c2.unwrap());
        assert_eq!(r1, c3.unwrap());

        let sequential = dispatch(
            Some(&verifier),
            &community,
            player,
            RpcMethod::StoreProcessIAP,
            &[receipt_arg],
        )
        .await
        .unwrap();
        assert_eq!(r1, sequential);

        // Same order with a different receipt fails.
        assert!(
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                &[purchase_arg(
                    player,
                    first_id,
                    "different-receipt-002",
                    &signer
                )],
            )
            .await
            .is_err()
        );
        // Same receipt on another order of the same player fails.
        assert!(
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                &[purchase_arg(
                    player,
                    same_player_second_id,
                    "shared-receipt-001",
                    &signer
                )],
            )
            .await
            .is_err()
        );
        // Same receipt on another player's order fails.
        assert!(
            dispatch(
                Some(&verifier),
                &community,
                other,
                RpcMethod::StoreProcessIAP,
                &[purchase_arg(
                    other,
                    other_player_order_id,
                    "shared-receipt-001",
                    &signer
                )],
            )
            .await
            .is_err()
        );

        let conn = Connection::open(&db).unwrap();
        assert_eq!(inventory_balance(&conn, player), 5);
        assert_eq!(inventory_balance(&conn, other), 0);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM community_item_grants", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn failed_award_or_receipt_update_rolls_back_atomically_and_retries_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("rollback.db");
        let (verifier, signer) = test_verifier(dir.path(), 41);
        let community = open_provider_community(&db);
        let player = Uuid::new_v4();
        community
            .observe_player(player, "atomic".into(), true)
            .await
            .unwrap();

        let prepared = prepare(&verifier, &community, player, Uuid::new_v4()).await;
        let order_id: Uuid = serde_json::from_value(prepared["orderId"].clone()).unwrap();
        let signed = purchase_arg(player, order_id, "atomic-receipt-001", &signer);

        // Missing item fails and rolls back cleanly.
        assert!(
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed),
            )
            .await
            .is_err()
        );
        let conn = Connection::open(&db).unwrap();
        assert_eq!(inventory_balance(&conn, player), 0);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM community_item_grants", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let pending_receipt: Option<Vec<u8>> = conn
            .query_row(
                "SELECT receipt FROM ce_provider_orders WHERE id = ?",
                [order_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(pending_receipt.is_none());

        // Define item, then force receipt update failure via SQLite trigger so the
        // reward grant and inventory credit must both roll back.
        define_coin(&community).await;
        conn.execute_batch(
            "CREATE TRIGGER fail_receipt_update BEFORE UPDATE ON ce_provider_orders
             BEGIN
                 SELECT RAISE(ABORT, 'simulated receipt update failure');
             END;",
        )
        .unwrap();

        assert!(
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::StoreProcessIAP,
                std::slice::from_ref(&signed),
            )
            .await
            .is_err()
        );
        assert_eq!(inventory_balance(&conn, player), 0);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM community_item_grants", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let still_pending: Option<Vec<u8>> = conn
            .query_row(
                "SELECT receipt FROM ce_provider_orders WHERE id = ?",
                [order_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(still_pending.is_none());

        conn.execute_batch("DROP TRIGGER fail_receipt_update;")
            .unwrap();
        let fulfilled = dispatch(
            Some(&verifier),
            &community,
            player,
            RpcMethod::StoreProcessIAP,
            &[signed],
        )
        .await
        .unwrap();
        assert_eq!(fulfilled["success"], true);
        assert_eq!(inventory_balance(&conn, player), 5);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM community_item_grants", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let receipt_len: i64 = conn
            .query_row(
                "SELECT length(receipt) FROM ce_provider_orders WHERE id = ?",
                [order_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipt_len, 32);
    }

    #[tokio::test]
    async fn provider_orders_and_identity_survive_restart_across_prepare_rollback_and_fulfill() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("restart.db");
        let (verifier, signer) = test_verifier(dir.path(), 53);
        let player = Uuid::new_v4();
        let req_a = Uuid::new_v4();
        let req_b = Uuid::new_v4();

        let (order_a, order_b) = {
            let community = open_provider_community(&db);
            community
                .observe_player(player, "survivor".into(), true)
                .await
                .unwrap();
            dispatch(
                Some(&verifier),
                &community,
                player,
                RpcMethod::VerifyGovernmentId,
                &[sign(
                    &claims(
                        player,
                        Proof::Verified {
                            valid_until: now() as i64 + 3600,
                        },
                    ),
                    &signer,
                )],
            )
            .await
            .unwrap();
            let prepared_a = prepare(&verifier, &community, player, req_a).await;
            let prepared_b = prepare(&verifier, &community, player, req_b).await;
            let id_a: Uuid = serde_json::from_value(prepared_a["orderId"].clone()).unwrap();
            let id_b: Uuid = serde_json::from_value(prepared_b["orderId"].clone()).unwrap();
            // Trigger a rollback before restart by attempting to fulfill an undefined item.
            assert!(
                dispatch(
                    Some(&verifier),
                    &community,
                    player,
                    RpcMethod::StoreProcessIAP,
                    &[purchase_arg(player, id_a, "restart-receipt-a", &signer)],
                )
                .await
                .is_err()
            );
            (prepared_a, id_b)
        };

        let id_a: Uuid = serde_json::from_value(order_a["orderId"].clone()).unwrap();
        {
            let restarted = open_provider_community(&db);
            assert_eq!(
                dispatch(
                    None,
                    &restarted,
                    player,
                    RpcMethod::IsGovernmentIdVerified,
                    &[]
                )
                .await
                .unwrap()["verified"],
                true
            );
            assert_eq!(prepare(&verifier, &restarted, player, req_a).await, order_a);
            define_coin(&restarted).await;
            let fulfilled_a = dispatch(
                Some(&verifier),
                &restarted,
                player,
                RpcMethod::StoreProcessIAP,
                &[purchase_arg(player, id_a, "restart-receipt-a", &signer)],
            )
            .await
            .unwrap();
            assert_eq!(fulfilled_a["orderId"], id_a.to_string());
        }

        let final_community = open_provider_community(&db);
        assert_eq!(
            dispatch(
                None,
                &final_community,
                player,
                RpcMethod::IsGovernmentIdVerified,
                &[]
            )
            .await
            .unwrap()["verified"],
            true
        );
        // Replay order A idempotently after second restart.
        dispatch(
            Some(&verifier),
            &final_community,
            player,
            RpcMethod::StoreProcessIAP,
            &[purchase_arg(player, id_a, "restart-receipt-a", &signer)],
        )
        .await
        .unwrap();
        // Fulfill order B prepared before both restarts.
        dispatch(
            Some(&verifier),
            &final_community,
            player,
            RpcMethod::StoreProcessIAP,
            &[purchase_arg(player, order_b, "restart-receipt-b", &signer)],
        )
        .await
        .unwrap();

        let conn = Connection::open(&db).unwrap();
        assert_eq!(inventory_balance(&conn, player), 10);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM community_item_grants", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
