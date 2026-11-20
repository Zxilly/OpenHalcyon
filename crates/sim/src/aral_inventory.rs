//! ARAL items occupy native slots independently of the shop's modifier ledger.
use crate::{HeldItem, World};

pub const ARAL_ITEM_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AralItem {
    pub template: u32,
    pub instance: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AralInventory([Option<AralItem>; ARAL_ITEM_CAPACITY]);

impl AralInventory {
    pub(super) fn starting() -> Self {
        let mut slots = [None; ARAL_ITEM_CAPACITY];
        slots[0] = Some(AralItem {
            template: 457,
            instance: 2000,
        });
        slots[1] = Some(AralItem {
            template: 526,
            instance: 2001,
        });
        Self(slots)
    }

    pub fn slots(&self) -> &[Option<AralItem>; ARAL_ITEM_CAPACITY] {
        &self.0
    }

    /// Current grants can reproduce the exact native slots only with no internal gap.
    pub fn is_dense(&self) -> bool {
        let first_free = self
            .0
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.0.len());
        self.0[first_free..].iter().all(Option::is_none)
    }

    pub(super) fn with_purchase(&self, consumed: &[HeldItem], granted: HeldItem) -> Option<Self> {
        let mut next = self.clone();
        for item in consumed {
            let slot = next
                .0
                .iter_mut()
                .find(|slot| slot.is_some_and(|owned| owned.instance == item.instance))?;
            *slot = None;
        }
        let empty = next.0.iter_mut().find(|slot| slot.is_none())?;
        *empty = Some(AralItem {
            template: granted.kind.template(),
            instance: granted.instance,
        });
        Some(next)
    }
}

impl World {
    /// A validated active connection may swap two occupied native ARAL positions.
    pub fn reorder_aral_items(&mut self, actor: u32, indices: [u8; 2]) -> bool {
        let Some(index) = self.index(actor) else {
            return false;
        };
        let Some(inventory) = self.entities[index].aral_inventory.as_mut() else {
            return false;
        };
        let [left, right] = indices.map(usize::from);
        if self.winner.is_some()
            || left == right
            || ![left, right]
                .into_iter()
                .all(|slot| inventory.0.get(slot).is_some_and(Option::is_some))
        {
            return false;
        }
        inventory.0.swap(left, right);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BasicItem::*;

    #[test]
    fn starter_items_and_recipe_keep_eight_native_positions() {
        let mut world = World::new(12, 6);
        world.use_six_human_roster().unwrap();
        world.use_aral_lane().unwrap();
        let starter = world.entities[0].aral_inventory.as_ref().unwrap().slots()[..2].to_vec();
        assert_eq!(
            starter
                .iter()
                .flatten()
                .map(|held| held.template)
                .collect::<Vec<_>>(),
            [457, 526]
        );
        world.entities[0].gold = 20_000;
        let crystal = world.purchase(1, CrystalBit).unwrap().granted;
        let swift = world.purchase(1, SwiftShooter).unwrap().granted;
        let boots = world.purchase(1, SprintBoots).unwrap().granted;
        let salvo = world.purchase(1, BlazingSalvo).unwrap();
        assert_eq!(salvo.consumed, [swift]);
        let ac = world.purchase(1, AlternatingCurrent).unwrap();
        assert!(ac.consumed.contains(&crystal));
        assert!(ac.consumed.contains(&salvo.granted));
        let inventory = world.entities[0].aral_inventory.as_ref().unwrap();
        assert!(!inventory.is_dense());
        assert_eq!(inventory.slots()[2].unwrap().instance, ac.granted.instance);
        assert_eq!(inventory.slots()[3], None);
        assert_eq!(inventory.slots()[4].unwrap().instance, boots.instance);
        let before = world.state_digest();
        for denied in [[2, 2], [2, 3], [2, 8], [255, 1]] {
            assert!(!world.reorder_aral_items(1, denied));
        }
        assert_eq!(world.state_digest(), before);
        assert!(world.reorder_aral_items(1, [2, 4]));
        assert_eq!(
            world.entities[0].aral_inventory.as_ref().unwrap().slots()[2]
                .unwrap()
                .instance,
            boots.instance
        );
        assert!(world.reorder_aral_items(1, [2, 4]));
        assert_eq!(world.state_digest(), before);

        let replacement = world.purchase(1, LightArmor).unwrap();
        for _ in 0..3 {
            world.purchase(1, LightShield).unwrap();
        }
        let inventory = world.entities[0].aral_inventory.as_ref().unwrap();
        assert_eq!(inventory.slots()[..2], starter);
        assert_eq!(
            inventory.slots()[3].unwrap().instance,
            replacement.granted.instance
        );
        assert!(inventory.slots().iter().all(Option::is_some));
        assert!(world.reorder_aral_items(1, [6, 5]));
        let cap_state = world.state_digest();
        assert_eq!(world.try_purchase(1, Oakheart).unwrap(), None);
        assert_eq!(world.state_digest(), cap_state);
    }
}
