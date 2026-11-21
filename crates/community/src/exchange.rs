use crate::{
    Community, CommunityError, GroupKind, Result,
    economy::store::{add_item, held},
    orm::ensure_player,
    schema::{
        community_equipment as equipment, community_inventory as inventory,
        community_members as members, community_redemptions as receipts,
    },
};
use diesel::{dsl::exists, prelude::*};
use halcyon_types::ce::RpcMethod;
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Costs, rewards and lifetime purchase limits are operator-owned game rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(default)]
    pub cost: BTreeMap<String, u32>,
    pub reward: BTreeMap<String, u32>,
    pub limit: Option<u32>,
}

impl Recipe {
    pub fn validate(&self) -> Result<()> {
        if self.reward.is_empty()
            || self.limit == Some(0)
            || (self.cost.is_empty() && self.limit.is_none())
            || self
                .cost
                .values()
                .chain(self.reward.values())
                .any(|&n| n == 0)
        {
            return Err(CommunityError::Invalid(
                "invalid cost, reward or grant limit",
            ));
        }
        Ok(())
    }
}

impl Community {
    /// Spend and grant atomically. An exact transport retry has no side effects.
    pub async fn redeem(
        &self,
        player: Uuid,
        operation: String,
        sku: String,
        request: [u8; 32],
        recipe: Recipe,
    ) -> Result<()> {
        self.redeem_with_previous_key(player, operation, sku, request, None, recipe)
            .await
    }

    /// Recognize an exact request already committed under an older receipt key.
    pub async fn redeem_with_previous_key(
        &self,
        player: Uuid,
        operation: String,
        sku: String,
        request: [u8; 32],
        previous: Option<[u8; 32]>,
        recipe: Recipe,
    ) -> Result<()> {
        recipe.validate()?;
        let method = RpcMethod::from_wire(&operation);
        if method == RpcMethod::GuildCollectJoinBonus && recipe.limit != Some(1) {
            return Err(CommunityError::Invalid(
                "guild join bonus must have a one-claim limit",
            ));
        }
        self.orm_transaction(move |db| {
            let player = player.to_string();
            ensure_player(db, &player)?;
            if matches!(
                method,
                RpcMethod::GuildRedeemChest | RpcMethod::GuildCollectJoinBonus
            ) {
                let member = diesel::select(exists(
                    members::table
                        .filter(members::player.eq(&player))
                        .filter(members::kind.eq(GroupKind::Guild.as_str())),
                ))
                .get_result::<bool>(db)?;
                if !member {
                    return Err(CommunityError::Forbidden);
                }
            }
            let request = request.as_slice();
            let prior_query = |receipt| {
                receipts::table
                    .find((&player, receipt))
                    .select((receipts::operation, receipts::sku))
            };
            let mut prior = prior_query(request)
                .first::<(String, String)>(db)
                .optional()?
                .map(|row| (true, row));
            if prior.is_none()
                && let Some(previous) = previous.as_ref()
            {
                prior = prior_query(previous.as_slice())
                    .first::<(String, String)>(db)
                    .optional()?
                    .map(|row| (false, row));
            }
            if let Some((current, (old_operation, old_sku))) = prior {
                if old_operation != operation || old_sku != sku {
                    return Err(CommunityError::Conflict("receipt already used"));
                }
                if !current && let Some(previous) = previous.as_ref() {
                    diesel::update(receipts::table.find((&player, previous.as_slice())))
                        .set(receipts::request.eq(request))
                        .execute(db)?;
                }
                return Ok(());
            }
            let offers = receipts::table
                .filter(receipts::player.eq(&player))
                .filter(receipts::operation.eq(&operation));
            if method == RpcMethod::GuildCollectJoinBonus
                && diesel::select(exists(offers)).get_result::<bool>(db)?
            {
                return Err(CommunityError::Conflict(
                    "guild join bonus was already collected",
                ));
            }
            if let Some(limit) = recipe.limit {
                let used = offers
                    .filter(receipts::sku.eq(&sku))
                    .count()
                    .get_result::<i64>(db)?;
                if used >= i64::from(limit) {
                    return Err(CommunityError::Conflict("offer limit reached"));
                }
            }
            for (item, cost) in &recipe.cost {
                match held(db, &player, item)?.checked_sub(*cost) {
                    None => return Err(CommunityError::Conflict("insufficient inventory")),
                    Some(0) => {
                        diesel::delete(
                            equipment::table
                                .filter(equipment::player.eq(&player))
                                .filter(equipment::item.eq(item)),
                        )
                        .execute(db)?;
                        diesel::delete(inventory::table.find((&player, item))).execute(db)?;
                    }
                    Some(remaining) => {
                        diesel::update(inventory::table.find((&player, item)))
                            .set(inventory::count.eq(i64::from(remaining)))
                            .execute(db)?;
                    }
                }
            }
            for (item, count) in &recipe.reward {
                add_item(db, &player, item, *count)?;
            }
            diesel::insert_into(receipts::table)
                .values((
                    receipts::player.eq(&player),
                    receipts::operation.eq(operation),
                    receipts::sku.eq(sku),
                    receipts::request.eq(request),
                ))
                .execute(db)?;
            Ok(())
        })
        .await
    }
}
