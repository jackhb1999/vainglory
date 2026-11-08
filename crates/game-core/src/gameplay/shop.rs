//! 装备商店与购买事务（`GX-CBT-2026-001` §10）。
//!
//! `try_buy` 是**原子事务**：全部前置校验通过后才写入金币与栏位。
//! 服务端单 tick 内完成，客户端只提交意图、只接收结果。

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

pub const INVENTORY_SLOTS: usize = 6;
pub const ACTIVE_ITEM_SLOTS: usize = 1;
pub const SHOP_ZONE_RADIUS: f32 = 12.0;
pub const SELL_REFUND_PERCENT: u32 = 60;
/// 阵亡后仍可购买的宽限 tick（5 s）
pub const DEAD_PURCHASE_GRACE_TICKS: u64 = 300;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemStats {
    pub attack_damage: f32,
    pub ability_power: f32,
    pub max_health: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub crit_chance: f32,
    pub cooldown_reduction: f32,
    pub move_speed: f32,
}

impl ItemStats {
    pub fn sub(&self, other: &Self) -> Self {
        Self {
            attack_damage: self.attack_damage - other.attack_damage,
            ability_power: self.ability_power - other.ability_power,
            max_health: self.max_health - other.max_health,
            armor: self.armor - other.armor,
            magic_resist: self.magic_resist - other.magic_resist,
            crit_chance: self.crit_chance - other.crit_chance,
            cooldown_reduction: self.cooldown_reduction - other.cooldown_reduction,
            move_speed: self.move_speed - other.move_speed,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ItemDef {
    pub id: &'static str,
    pub price: u32,
    /// 合成前置：持有 `from` 时只支付差价
    pub from: Option<&'static str>,
    pub stats: ItemStats,
    pub active: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ShopCatalog {
    pub items: Vec<ItemDef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShopDefect {
    ZeroPrice(&'static str),
    DanglingUpgrade {
        item: &'static str,
        from: &'static str,
    },
    Cycle(&'static str),
    DuplicateId(&'static str),
}

impl ShopCatalog {
    /// §10.1 的完整商品表（自研占位数值）。
    pub fn placeholder() -> Self {
        let s = |attack_damage, ability_power, max_health, armor, magic_resist| ItemStats {
            attack_damage,
            ability_power,
            max_health,
            armor,
            magic_resist,
            crit_chance: 0.0,
            cooldown_reduction: 0.0,
            move_speed: 0.0,
        };
        Self {
            items: vec![
                ItemDef {
                    id: "item_edge_shard",
                    price: 300,
                    from: None,
                    stats: s(12.0, 0.0, 0.0, 0.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_arcane_shard",
                    price: 300,
                    from: None,
                    stats: s(0.0, 12.0, 0.0, 0.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_guard_shard",
                    price: 300,
                    from: None,
                    stats: s(0.0, 0.0, 120.0, 0.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_swift_boots",
                    price: 600,
                    from: None,
                    stats: ItemStats {
                        move_speed: 0.6,
                        ..Default::default()
                    },
                    active: false,
                },
                ItemDef {
                    id: "item_edge_blade",
                    price: 1200,
                    from: Some("item_edge_shard"),
                    stats: s(40.0, 0.0, 0.0, 0.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_arcane_staff",
                    price: 1200,
                    from: Some("item_arcane_shard"),
                    stats: s(0.0, 45.0, 0.0, 0.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_bulwark",
                    price: 1100,
                    from: Some("item_guard_shard"),
                    stats: s(0.0, 0.0, 0.0, 35.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_lumin_charm",
                    price: 1000,
                    from: Some("item_guard_shard"),
                    stats: s(0.0, 0.0, 0.0, 0.0, 25.0),
                    active: false,
                },
                ItemDef {
                    id: "item_rift_cleaver",
                    price: 3200,
                    from: Some("item_edge_blade"),
                    stats: ItemStats {
                        attack_damage: 90.0,
                        crit_chance: 0.15,
                        ..Default::default()
                    },
                    active: false,
                },
                ItemDef {
                    id: "item_star_core",
                    price: 3200,
                    from: Some("item_arcane_staff"),
                    stats: ItemStats {
                        ability_power: 110.0,
                        cooldown_reduction: 0.2,
                        ..Default::default()
                    },
                    active: false,
                },
                ItemDef {
                    id: "item_warden_wall",
                    price: 2800,
                    from: Some("item_bulwark"),
                    stats: s(0.0, 0.0, 400.0, 45.0, 0.0),
                    active: false,
                },
                ItemDef {
                    id: "item_blink_cloak",
                    price: 2400,
                    from: None,
                    stats: ItemStats::default(),
                    active: true,
                },
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<&ItemDef> {
        self.items.iter().find(|i| i.id == id)
    }

    /// TC-CBT-08：价格 > 0、id 唯一、前置存在、合成图为 DAG。
    pub fn validate(&self) -> Result<(), ShopDefect> {
        let mut ids: HashSet<&'static str> = HashSet::new();
        for item in &self.items {
            if item.price == 0 {
                return Err(ShopDefect::ZeroPrice(item.id));
            }
            if !ids.insert(item.id) {
                return Err(ShopDefect::DuplicateId(item.id));
            }
        }
        let by_id: HashMap<&str, &ItemDef> = self.items.iter().map(|i| (i.id, i)).collect();
        for item in &self.items {
            if let Some(from) = item.from
                && !by_id.contains_key(from)
            {
                return Err(ShopDefect::DanglingUpgrade {
                    item: item.id,
                    from,
                });
            }
        }
        // DFS 三色标记检测环
        let mut state: HashMap<&str, u8> = HashMap::new();
        for item in &self.items {
            if detect_cycle(item.id, &by_id, &mut state) {
                return Err(ShopDefect::Cycle(item.id));
            }
        }
        Ok(())
    }

    /// 沿 `from` 链累加**实际支付过的差价**（用于售出返还）。
    /// 例如碎片 300 → 长刃补 900 → 巨刃补 2000，总投入 3200 而非三件标价之和。
    pub fn total_invested(&self, id: &str) -> u32 {
        let mut total = 0;
        let mut cursor = Some(id);
        let mut guard = 0;
        while let Some(cur) = cursor {
            guard += 1;
            if guard > 32 {
                break;
            }
            let Some(def) = self.get(cur) else { break };
            let base_price = def
                .from
                .and_then(|f| self.get(f))
                .map(|d| d.price)
                .unwrap_or(0);
            total += def.price.saturating_sub(base_price);
            cursor = def.from;
        }
        total
    }
}

fn detect_cycle(
    id: &'static str,
    by_id: &HashMap<&str, &ItemDef>,
    state: &mut HashMap<&'static str, u8>,
) -> bool {
    if state.get(id) == Some(&2) {
        return false;
    }
    if state.get(id) == Some(&1) {
        return true;
    }
    state.insert(id, 1);
    if let Some(def) = by_id.get(id)
        && let Some(from) = def.from
        && detect_cycle(from, by_id, state)
    {
        return true;
    }
    state.insert(id, 2);
    false
}

#[derive(Component, Clone, Debug)]
pub struct Inventory {
    pub slots: Vec<Option<String>>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; INVENTORY_SLOTS],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ShopRequest {
    pub item_id: &'static str,
    pub distance_to_source: f32,
    pub is_dead: bool,
    pub death_tick: u64,
    pub current_tick: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShopReject {
    UnknownItem,
    DeadTooLong,
    NotInShopZone,
    InsufficientGold,
    MissingPrerequisite,
    InventoryFull,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShopReceipt {
    pub item_id: &'static str,
    pub paid: u32,
    pub removed: Option<&'static str>,
    pub slot: usize,
    pub delta: ItemStats,
}

/// 购买事务（§10.3）：任何校验失败都不得改动金币或栏位。
pub fn try_buy(
    catalog: &ShopCatalog,
    inventory: &mut Inventory,
    gold: &mut u32,
    request: &ShopRequest,
) -> Result<ShopReceipt, ShopReject> {
    let item = catalog
        .get(request.item_id)
        .ok_or(ShopReject::UnknownItem)?;

    if request.is_dead
        && request.current_tick.saturating_sub(request.death_tick) > DEAD_PURCHASE_GRACE_TICKS
    {
        return Err(ShopReject::DeadTooLong);
    }
    if request.distance_to_source > SHOP_ZONE_RADIUS {
        return Err(ShopReject::NotInShopZone);
    }

    // 找到持有的前置装备；升级只支付差价，否则付全价
    let prerequisite_slot = item.from.and_then(|from| {
        inventory
            .slots
            .iter()
            .position(|s| s.as_deref() == Some(from))
    });
    let paid = match item.from {
        Some(from) => {
            let base = catalog.get(from).ok_or(ShopReject::MissingPrerequisite)?;
            if prerequisite_slot.is_none() {
                return Err(ShopReject::MissingPrerequisite);
            }
            item.price.saturating_sub(base.price)
        }
        None => item.price,
    };
    if *gold < paid {
        return Err(ShopReject::InsufficientGold);
    }

    let slot = prerequisite_slot
        .or_else(|| inventory.slots.iter().position(|s| s.is_none()))
        .ok_or(ShopReject::InventoryFull)?;

    let removed = inventory.slots[slot]
        .as_deref()
        .and_then(|id| catalog.get(id).map(|d| d.id));
    let delta = match removed {
        Some(id) => item
            .stats
            .sub(&catalog.get(id).map(|d| d.stats).unwrap_or_default()),
        None => item.stats,
    };

    // ---- 事务提交点：以下写入全部发生在校验通过之后 ----
    *gold -= paid;
    inventory.slots[slot] = Some(item.id.to_string());
    Ok(ShopReceipt {
        item_id: item.id,
        paid,
        removed,
        slot,
        delta,
    })
}

/// 售出：返还总投入价的 60%（§9.3）。
pub fn try_sell(
    catalog: &ShopCatalog,
    inventory: &mut Inventory,
    gold: &mut u32,
    slot: usize,
) -> Result<u32, ShopReject> {
    let Some(Some(id)) = inventory.slots.get(slot).cloned() else {
        return Err(ShopReject::UnknownItem);
    };
    let refund = catalog.total_invested(&id) * SELL_REFUND_PERCENT / 100;
    inventory.slots[slot] = None;
    *gold = gold.saturating_add(refund);
    Ok(refund)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive_request(id: &'static str) -> ShopRequest {
        ShopRequest {
            item_id: id,
            distance_to_source: 0.0,
            is_dead: false,
            death_tick: 0,
            current_tick: 1000,
        }
    }

    #[test]
    fn tc_cbt_08_catalog_is_a_valid_dag() {
        let catalog = ShopCatalog::placeholder();
        assert_eq!(catalog.validate(), Ok(()));
        assert_eq!(catalog.items.len(), 12);
        assert!(catalog.items.iter().all(|i| i.price > 0));
    }

    #[test]
    fn tc_cbt_08_detects_cycles_and_dangling_upgrades() {
        let mut catalog = ShopCatalog::placeholder();
        catalog.items[0].from = Some("item_rift_cleaver");
        assert!(matches!(catalog.validate(), Err(ShopDefect::Cycle(_))));

        let mut catalog = ShopCatalog::placeholder();
        catalog.items[0].from = Some("does_not_exist");
        assert_eq!(
            catalog.validate(),
            Err(ShopDefect::DanglingUpgrade {
                item: "item_edge_shard",
                from: "does_not_exist"
            })
        );
    }

    #[test]
    fn tc_cbt_07_purchase_is_atomic_on_rejection() {
        let catalog = ShopCatalog::placeholder();
        let mut inv = Inventory::default();
        let mut gold = 100u32;

        let cases = [
            (
                ShopRequest {
                    distance_to_source: 20.0,
                    ..alive_request("item_edge_shard")
                },
                ShopReject::NotInShopZone,
            ),
            (alive_request("not_an_item"), ShopReject::UnknownItem),
            (
                alive_request("item_swift_boots"),
                ShopReject::InsufficientGold,
            ),
            (
                alive_request("item_edge_blade"),
                ShopReject::MissingPrerequisite,
            ),
        ];
        for (req, expected) in cases {
            assert_eq!(try_buy(&catalog, &mut inv, &mut gold, &req), Err(expected));
            assert_eq!(gold, 100, "gold must be unchanged on rejection");
            assert!(inv.slots.iter().all(|s| s.is_none()));
        }
    }

    #[test]
    fn tc_cbt_07_upgrade_pays_only_the_difference() {
        let catalog = ShopCatalog::placeholder();
        let mut inv = Inventory::default();
        let mut gold = 3000u32;
        let r = try_buy(
            &catalog,
            &mut inv,
            &mut gold,
            &alive_request("item_edge_shard"),
        )
        .unwrap();
        assert_eq!(r.paid, 300);
        assert_eq!(gold, 2700);
        let r = try_buy(
            &catalog,
            &mut inv,
            &mut gold,
            &alive_request("item_edge_blade"),
        )
        .unwrap();
        assert_eq!(r.paid, 900);
        assert_eq!(r.removed, Some("item_edge_shard"));
        assert_eq!(r.slot, 0);
        assert_eq!(r.delta.attack_damage, 28.0);
        assert_eq!(inv.slots[0].as_deref(), Some("item_edge_blade"));
    }

    #[test]
    fn inventory_full_is_rejected_before_any_write() {
        let catalog = ShopCatalog::placeholder();
        let mut inv = Inventory {
            slots: vec![Some("item_edge_shard".to_string()); INVENTORY_SLOTS],
        };
        let mut gold = 10000;
        assert_eq!(
            try_buy(
                &catalog,
                &mut inv,
                &mut gold,
                &alive_request("item_guard_shard")
            ),
            Err(ShopReject::InventoryFull)
        );
        assert_eq!(gold, 10000);
    }

    #[test]
    fn sell_refunds_sixty_percent_of_total_investment() {
        let catalog = ShopCatalog::placeholder();
        let mut inv = Inventory::default();
        let mut gold = 0u32;
        inv.slots[0] = Some("item_rift_cleaver".to_string());
        // 300 (碎片) + 900 (升级长刃) + 2000 (升级巨刃) = 3200
        assert_eq!(catalog.total_invested("item_rift_cleaver"), 3200);
        assert_eq!(try_sell(&catalog, &mut inv, &mut gold, 0), Ok(1920));
        assert!(inv.slots[0].is_none());
    }

    #[test]
    fn dead_hero_can_buy_within_grace_but_not_after() {
        let catalog = ShopCatalog::placeholder();
        let mut inv = Inventory::default();
        let mut gold = 1000;
        let mut req = alive_request("item_edge_shard");
        req.is_dead = true;
        req.death_tick = 100;
        req.current_tick = 100 + DEAD_PURCHASE_GRACE_TICKS;
        assert!(try_buy(&catalog, &mut inv, &mut gold, &req).is_ok());
        req.current_tick += 1;
        assert_eq!(
            try_buy(&catalog, &mut inv, &mut gold, &req),
            Err(ShopReject::DeadTooLong)
        );
    }
}
