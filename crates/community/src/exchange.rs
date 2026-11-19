use crate::{Community, CommunityError, Result, economy::add_item, ensure_player};
use rusqlite::{Connection, OptionalExtension, params};
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

pub(super) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS community_redemptions(
        player TEXT NOT NULL REFERENCES community_players(id),
        operation TEXT NOT NULL, sku TEXT NOT NULL, request BLOB NOT NULL,
        PRIMARY KEY(player, request));
        CREATE INDEX IF NOT EXISTS redemptions_by_offer ON community_redemptions(player,operation,sku);")?;
    Ok(())
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
        if operation == "guildCollectJoinBonus" && recipe.limit != Some(1) {
            return Err(CommunityError::Invalid(
                "guild join bonus must have a one-claim limit",
            ));
        }
        self.run(move |tx| {
            let player = player.to_string();
            ensure_player(tx, &player)?;
            if matches!(operation.as_str(), "guildRedeemChest" | "guildCollectJoinBonus") {
                let member = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM community_members WHERE player=? AND kind='guild')",
                    [&player], |row| row.get::<_, bool>(0),
                )?;
                if !member {
                    return Err(CommunityError::Forbidden);
                }
            }
            let prior: Option<(bool, String, String)> = tx.query_row(
                "SELECT request=?1, operation, sku FROM community_redemptions
                 WHERE player=?2 AND request IN (?1,?3) ORDER BY (request=?1) DESC LIMIT 1",
                params![request, player, previous], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
            if let Some((current, old_operation, old_sku)) = prior {
                if old_operation != operation || old_sku != sku {
                    return Err(CommunityError::Conflict("receipt already used"));
                }
                if !current {
                    tx.execute("UPDATE community_redemptions SET request=? WHERE player=? AND request=?",
                        params![request, player, previous])?;
                }
                return Ok(());
            }
            if operation == "guildCollectJoinBonus" && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM community_redemptions WHERE player=? AND operation=?)",
                params![player, operation], |row| row.get::<_, bool>(0),
            )? {
                return Err(CommunityError::Conflict("guild join bonus was already collected"));
            }
            if let Some(limit) = recipe.limit {
                let used: u64 = tx.query_row("SELECT COUNT(*) FROM community_redemptions WHERE player=? AND operation=? AND sku=?",
                    params![player,operation,sku],|r|r.get(0))?;
                if used >= u64::from(limit) { return Err(CommunityError::Conflict("offer limit reached")); }
            }
            for (item, cost) in &recipe.cost {
                let held: Option<u32> = tx.query_row("SELECT count FROM community_inventory WHERE player=? AND item=?",
                    params![player,item],|r|r.get(0)).optional()?;
                match held.unwrap_or(0).checked_sub(*cost) {
                    None => return Err(CommunityError::Conflict("insufficient inventory")),
                    Some(0) => {
                        tx.execute("DELETE FROM community_equipment WHERE player=? AND item=?", params![player,item])?;
                        tx.execute("DELETE FROM community_inventory WHERE player=? AND item=?", params![player,item])?;
                    }
                    Some(remaining) => {
                        tx.execute("UPDATE community_inventory SET count=? WHERE player=? AND item=?",params![remaining,player,item])?;
                    }
                }
            }
            for (item,count) in &recipe.reward {
                add_item(tx, &player, item, *count)?;
            }
            tx.execute("INSERT INTO community_redemptions VALUES(?,?,?,?)",params![player,operation,sku,request])?;
            Ok(())
        }).await
    }
}
