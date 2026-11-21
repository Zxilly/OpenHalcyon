//! Optional game content and local item exchanges. Only a configured recipe
//! can change inventory. The client supplies an identifier, never a price.
use crate::RpcMethod;
use crate::{Error, Platform};
use halcyon_community::{
    Action, ItemDefinition, QuestDefinition, Recipe,
    views::{Inventory, InventoryItem},
};
use halcyon_platform::PlayerProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    #[serde(rename = "talentProgress")]
    talent_progress: BTreeMap<String, super::talents::Binding>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum EmptyManifest {
    CardBoxManifest([(); 0]),
    ForgeManifest([(); 0]),
    BuffManifest([(); 0]),
    SeasonModels([(); 0]),
    RewardChests([(); 0]),
    RewardChestDefinitions([(); 0]),
    ProgressiveChests([(); 0]),
    HeroChests([(); 0]),
    PlayerAscensionData([(); 0]),
    DailyPicker([(); 0]),
    ActiveCampaigns([(); 0]),
    Events([(); 0]),
}

fn empty_manifest(method: RpcMethod) -> Option<EmptyManifest> {
    Some(match method {
        RpcMethod::GetCardBoxManifest | RpcMethod::RefreshCardBoxManifest => {
            EmptyManifest::CardBoxManifest([])
        }
        RpcMethod::GetForgeManifest => EmptyManifest::ForgeManifest([]),
        RpcMethod::GetBuffManifest => EmptyManifest::BuffManifest([]),
        RpcMethod::GetSeasonRewardsManifest => EmptyManifest::SeasonModels([]),
        RpcMethod::GetRewardsManifest => EmptyManifest::RewardChests([]),
        RpcMethod::GetRewardChestDefinitions => EmptyManifest::RewardChestDefinitions([]),
        RpcMethod::GetProgressiveChestDescriptions => EmptyManifest::ProgressiveChests([]),
        RpcMethod::GetHeroChestData => EmptyManifest::HeroChests([]),
        RpcMethod::GetAscensionDisplayData => EmptyManifest::PlayerAscensionData([]),
        RpcMethod::GetDailyPicker => EmptyManifest::DailyPicker([]),
        RpcMethod::OffersGetData => EmptyManifest::ActiveCampaigns([]),
        RpcMethod::GetLiveEventData => EmptyManifest::Events([]),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum InventoryClass {
    Card,
    Skin,
    Mastery,
    Trophy,
    Badge,
}

impl InventoryClass {
    fn from_method(method: RpcMethod) -> Option<Self> {
        Some(match method {
            RpcMethod::GetPlayerCardInventory => Self::Card,
            RpcMethod::GetPlayerSkinProgress | RpcMethod::GetSkinProgress => Self::Skin,
            RpcMethod::GetHeroMastery => Self::Mastery,
            RpcMethod::GetTrophyCase => Self::Trophy,
            RpcMethod::GetPlayerEsportsBadgeData => Self::Badge,
            _ => return None,
        })
    }

    fn slot(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Skin => "skin",
            Self::Mastery => "mastery",
            Self::Trophy => "trophy",
            Self::Badge => "badge",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum InventoryField {
    OwnedCards(Vec<InventoryItem>),
    SkinProgress(Vec<InventoryItem>),
    HeroMastery(Vec<InventoryItem>),
    TrophyCase(Vec<InventoryItem>),
    EsportsBadges(Vec<InventoryItem>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventoryProjection {
    #[serde(flatten)]
    inventory: InventoryField,
    #[serde(skip_serializing_if = "Option::is_none")]
    hero_stats: Option<Value>,
}

#[derive(Serialize)]
struct ReceiptInventory {
    owned: Vec<InventoryItem>,
    success: bool,
}

#[derive(Serialize)]
struct TargetingCurrency<'a> {
    currency: &'a crate::auth::profile::Currency,
}

fn is_exchange(m: RpcMethod) -> bool {
    matches!(
        m,
        RpcMethod::PurchaseDailyPick
            | RpcMethod::DecraftSkinBlueprint
            | RpcMethod::UpgradeTalent
            | RpcMethod::PurchaseCardPack
            | RpcMethod::ForgeCard
            | RpcMethod::ForgeEssence
            | RpcMethod::WeaveHeroSkin
            | RpcMethod::OpenRewardChest
            | RpcMethod::OpenInventoryChest
            | RpcMethod::CollectProgressiveChest
            | RpcMethod::CollectHeroChest
            | RpcMethod::AttemptRedeemAscensionChest
            | RpcMethod::AttemptRedeemAscensionRankUpChest
            | RpcMethod::AttemptRedeemAscensionSeasonEndChest
            | RpcMethod::AttemptRedeemSeasonalAscensionChest
            | RpcMethod::BuyAscensionSeasonalBundle
            | RpcMethod::OffersRedeemSku
            | RpcMethod::StoreRequestPurchaseSKU
            | RpcMethod::GuildRedeemChest
            | RpcMethod::GuildCollectJoinBonus
    )
}

impl Content {
    pub fn handles(m: RpcMethod) -> bool {
        is_exchange(m) || empty_manifest(m).is_some() || InventoryClass::from_method(m).is_some()
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
                .any(|(m, v)| empty_manifest(RpcMethod::from_wire(m)).is_none() || !v.is_object())
        {
            return Err(Error::Config("duplicate item or invalid content view"));
        }
        for (method, offers) in &data.exchanges {
            if !is_exchange(RpcMethod::from_wire(method)) {
                return Err(Error::Config("unknown content exchange method"));
            }
            for (id, recipe) in offers {
                recipe.validate()?;
                if RpcMethod::from_wire(method) == RpcMethod::GuildCollectJoinBonus
                    && recipe.limit != Some(1)
                {
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
        this.talents.configure(data.talent_progress, &ids)?;
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
        this.talents.install_match_authority(&this.app)?;
        Ok(self)
    }

    pub(crate) async fn content_rpc(
        &self,
        profile: &PlayerProfile,
        m: RpcMethod,
        params: &[Value],
    ) -> Result<Value, Error> {
        if let Some(default) = empty_manifest(m) {
            let canonical = if m == RpcMethod::RefreshCardBoxManifest {
                RpcMethod::GetCardBoxManifest
            } else {
                m
            };
            let view = match self.content.views.get(canonical.as_str()) {
                Some(view) => view.clone(),
                None => serde_json::to_value(default)?,
            };
            let fields = self.meta.player_fields(profile.player_id).await?;
            let mut info = crate::auth::profile::player_info(profile, Default::default());
            let target_currency = serde_json::to_value(TargetingCurrency {
                currency: &fields.currency,
            })?;
            info.as_object_mut()
                .expect("profile")
                .extend(view.as_object().expect("manifest fields").clone());
            let authority = serde_json::to_value(fields)?;
            info.as_object_mut()
                .expect("profile")
                .extend(authority.as_object().expect("profile activity").clone());
            // Configured targeting fields are opaque. Currency remains authoritative.
            if let Some(targeting) = info.get_mut("targeting").and_then(Value::as_object_mut) {
                targeting.extend(
                    target_currency
                        .as_object()
                        .expect("targeting currency")
                        .clone(),
                );
            }
            return Ok(info);
        }
        if let Some(class) = InventoryClass::from_method(m) {
            let owned = Inventory::deserialize(
                self.community
                    .execute(profile.player_id, Action::Inventory)
                    .await?,
            )?;
            let items = owned
                .items
                .into_iter()
                .filter(|item| item.slot == class.slot())
                .collect();
            let (inventory, hero_stats) = match class {
                InventoryClass::Card => (InventoryField::OwnedCards(items), None),
                InventoryClass::Skin => (InventoryField::SkinProgress(items), None),
                InventoryClass::Mastery => (
                    InventoryField::HeroMastery(items),
                    Some(self.meta.hero_stats(profile.player_id).await?),
                ),
                InventoryClass::Trophy => (InventoryField::TrophyCase(items), None),
                InventoryClass::Badge => (InventoryField::EsportsBadges(items), None),
            };
            let mut info = self.project_player_info(profile).await?;
            let patch = serde_json::to_value(InventoryProjection {
                inventory,
                hero_stats,
            })?;
            info.as_object_mut()
                .expect("player profile")
                .extend(patch.as_object().expect("inventory projection").clone());
            return Ok(info);
        }
        let offers = self
            .content
            .exchanges
            .get(m.as_str())
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
        let original_receipt = Sha256::digest(serde_json::to_vec(&(m, params))?).into();
        let (receipt, previous) = if m == RpcMethod::UpgradeTalent {
            let key = match first.and_then(|arg| arg.get("operationId")) {
                None => original_receipt,
                Some(value) => {
                    let id = value
                        .as_str()
                        .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
                        .filter(|id| !id.is_nil())
                        .ok_or(Error::Invalid)?;
                    // Local extension. Reusing the operation ID for a different SKU
                    // reaches the storage transaction's existing conflict check.
                    Sha256::digest(serde_json::to_vec(&("talent-upgrade-operation", id))?).into()
                }
            };
            // Recognize an existing bearer-independent receipt once. Moving it to
            // the new key frees subsequent native actions for the same talent.
            (
                key,
                Some(Sha256::digest(serde_json::to_vec(&(m, &params[1..]))?).into()),
            )
        } else {
            (original_receipt, None)
        };
        self.community
            .redeem_with_previous_key(
                profile.player_id,
                m.as_str().into(),
                sku.into(),
                receipt,
                previous,
                recipe.clone(),
            )
            .await?;
        if matches!(
            m,
            RpcMethod::GuildRedeemChest | RpcMethod::GuildCollectJoinBonus
        ) {
            return crate::social::community::guild_state(&self.community, profile.player_id).await;
        }
        let owned = Inventory::deserialize(
            self.community
                .execute(profile.player_id, Action::Inventory)
                .await?,
        )?;
        let mut info = self.project_player_info(profile).await?;
        let patch = serde_json::to_value(ReceiptInventory {
            owned: owned.items,
            success: true,
        })?;
        info.as_object_mut()
            .expect("player profile")
            .extend(patch.as_object().expect("receipt inventory").clone());
        Ok(info)
    }
}
