//! Optional game content and local item exchanges. Only a configured recipe
//! can change inventory. The client supplies an identifier, never a price.
use crate::{Error, Platform, player_info};
use halcyon_community::{Action, ItemDefinition, QuestDefinition, Recipe};
use halcyon_platform::PlayerProfile;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    items: Vec<ItemDefinition>,
    quests: Vec<QuestDefinition>,
    views: BTreeMap<String, Value>,
    exchanges: BTreeMap<String, BTreeMap<String, Recipe>>,
}

#[derive(Default)]
pub(crate) struct Content {
    views: BTreeMap<String, Value>,
    exchanges: BTreeMap<String, BTreeMap<String, Recipe>>,
}

fn view_field(method: &str) -> Option<&'static str> {
    Some(match method {
        "getCardBoxManifest" | "refreshCardBoxManifest" => "cardBoxManifest",
        "getForgeManifest" => "forgeManifest",
        "getBuffManifest" => "buffManifest",
        "getSeasonRewardsManifest" => "seasonModels",
        "getRewardsManifest" => "rewardChests",
        "getRewardChestDefinitions" => "rewardChestDefinitions",
        "getProgressiveChestDescriptions" => "progressiveChests",
        "getHeroChestData" => "heroChests",
        "getAscensionDisplayData" => "playerAscensionData",
        "getDailyPicker" => "dailyPicker",
        "offersGetData" => "activeCampaigns",
        "getLiveEventData" => "events",
        _ => return None,
    })
}

fn inventory_field(method: &str) -> Option<(&'static str, &'static str)> {
    Some(match method {
        "getPlayerCardInventory" => ("ownedCards", "card"),
        "getPlayerSkinProgress" | "getSkinProgress" => ("skinProgress", "skin"),
        "getHeroMastery" => ("heroMastery", "mastery"),
        "getTrophyCase" => ("trophyCase", "trophy"),
        "getPlayerEsportsBadgeData" => ("esportsBadges", "badge"),
        _ => return None,
    })
}

fn is_exchange(m: &str) -> bool {
    matches!(
        m,
        "purchaseDailyPick"
            | "decraftSkinBlueprint"
            | "upgradeTalent"
            | "purchaseCardPack"
            | "forgeCard"
            | "forgeEssence"
            | "weaveHeroSkin"
            | "openRewardChest"
            | "openInventoryChest"
            | "collectProgressiveChest"
            | "collectHeroChest"
            | "attemptRedeemAscensionChest"
            | "attemptRedeemAscensionRankUpChest"
            | "attemptRedeemAscensionSeasonEndChest"
            | "attemptRedeemSeasonalAscensionChest"
            | "buyAscensionSeasonalBundle"
            | "offersRedeemSku"
            | "storeRequestPurchaseSKU"
            | "guildRedeemChest"
            | "guildCollectJoinBonus"
    )
}

impl Content {
    pub fn handles(m: &str) -> bool {
        is_exchange(m) || view_field(m).is_some() || inventory_field(m).is_some()
    }
}

impl Platform {
    /// Install trusted definitions before publishing the HTTP router.
    pub async fn with_content(mut self: Arc<Self>, path: &Path) -> Result<Arc<Self>, Error> {
        let data: File = serde_json::from_reader(std::fs::File::open(path)?)?;
        let ids: BTreeSet<_> = data.items.iter().map(|i| &i.id).collect();
        if ids.len() != data.items.len()
            || data
                .views
                .iter()
                .any(|(m, v)| view_field(m).is_none() || !v.is_object())
        {
            return Err(Error::Config("duplicate item or invalid content view"));
        }
        for (method, offers) in &data.exchanges {
            if !is_exchange(method) {
                return Err(Error::Config("unknown content exchange method"));
            }
            for (id, recipe) in offers {
                recipe.validate()?;
                if method == "guildCollectJoinBonus" && recipe.limit != Some(1) {
                    return Err(Error::Config("guild join bonus must have limit 1"));
                }
                if id.is_empty()
                    || recipe
                        .cost
                        .keys()
                        .chain(recipe.reward.keys())
                        .any(|item| !ids.contains(item))
                {
                    return Err(Error::Config("invalid SKU or undefined content item"));
                }
            }
        }
        let this = Arc::get_mut(&mut self)
            .ok_or(Error::Config("content must be installed before serving"))?;
        for item in data.items {
            this.community.define_item(item).await?;
        }
        for quest in data.quests {
            this.community.define_quest(quest).await?;
        }
        this.content = Content {
            views: data.views,
            exchanges: data.exchanges,
        };
        Ok(self)
    }

    pub(crate) async fn content_rpc(
        &self,
        profile: &PlayerProfile,
        m: &str,
        params: &[Value],
    ) -> Result<Value, Error> {
        let mut info = player_info(profile);
        if let Some(field) = view_field(m) {
            let canonical = if m == "refreshCardBoxManifest" {
                "getCardBoxManifest"
            } else {
                m
            };
            if let Some(view) = self.content.views.get(canonical) {
                info.as_object_mut()
                    .unwrap()
                    .extend(view.as_object().unwrap().clone());
            } else {
                info[field] = json!([]);
            }
            return Ok(info);
        }
        if let Some((field, slot)) = inventory_field(m) {
            let owned = self
                .community
                .execute(profile.player_id, Action::Inventory)
                .await?;
            info[field] = owned["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["slot"] == slot)
                .cloned()
                .collect();
            if m == "getHeroMastery" {
                info["heroStats"] = self.meta.hero_stats(profile.player_id).await?;
            }
            return Ok(info);
        }
        let offers = self
            .content
            .exchanges
            .get(m)
            .ok_or(Error::Unavailable("offer unavailable"))?;
        let first = params.get(1);
        let resource = first.and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
                .or_else(|| {
                    ["sku", "product_id", "resourceId", "id", "chestId"]
                        .iter()
                        .find_map(|k| v.get(k)?.as_str().map(str::to_owned))
                })
        });
        let sku = resource.as_deref().unwrap_or("default");
        let recipe = offers
            .get(sku)
            .ok_or(Error::Unavailable("offer unavailable"))?;
        let receipt = Sha256::digest(serde_json::to_vec(&(m, params))?).into();
        self.community
            .redeem(
                profile.player_id,
                m.into(),
                sku.into(),
                receipt,
                recipe.clone(),
            )
            .await?;
        if matches!(m, "guildRedeemChest" | "guildCollectJoinBonus") {
            return crate::community::guild_state(&self.community, profile.player_id).await;
        }
        let owned = self
            .community
            .execute(profile.player_id, Action::Inventory)
            .await?;
        info["owned"] = owned["items"].clone();
        info["success"] = json!(true);
        Ok(info)
    }
}
