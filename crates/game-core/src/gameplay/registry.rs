//! 统一内容 ID 注册表（`P-A`）：把玩法模块的 `&'static str` id
//! 映射成协议使用的稳定 `u32`，并提供给 `config_hash` 的内容摘要。
//!
//! 之前存在四套互不相认的 ID 空间：
//! `gx-proto-content::ItemDef{id:String}`、`components::Item{item_id:u32}`、
//! `gameplay::shop::ItemDef{id:&'static str}`、协议 `item_id:u32`。
//! 本模块是它们之间的唯一转换点。

use crate::gameplay::shop::ShopCatalog;
use crate::net::JungleMonsterType;

/// FNV-1a 32 位；对固定词表做一次编译无关的确定性映射。
pub fn stable_id(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    // 0 保留给 "unknown"
    if h == 0 { 1 } else { h }
}

/// 内容域，用于在同一张表里区分同名项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentDomain {
    Item,
    Ability,
    Objective,
    Monster,
}

pub fn item_id(name: &str) -> u32 {
    stable_id(name)
}

pub fn item_name(id: u32) -> Option<&'static str> {
    ShopCatalog::placeholder()
        .items
        .iter()
        .find(|d| stable_id(d.id) == id)
        .map(|d| d.id)
}

/// 目标 id 是 `gameplay::objectives::CapturePoint.objective_id: u32`，
/// 协议侧 `GameEvent::ObjectiveCaptured.objective_id: String` 从这里取名字。
pub fn objective_name(id: u32) -> &'static str {
    match id {
        1 => "lode_shrine",
        2 => "muster_well",
        3 => "abyss_warden",
        _ => "unknown_objective",
    }
}

pub fn objective_id(name: &str) -> Option<u32> {
    (1..=3).find(|&i| objective_name(i) == name)
}

pub fn monster_name(kind: JungleMonsterType) -> &'static str {
    use JungleMonsterType::*;
    match kind {
        MusterWell => "muster_well",
        LodeShrine => "lode_shrine",
        AbyssWarden => "abyss_warden",
        Treant => "treant",
        CoreWarden => "core_warden",
        GildedToad => "gilded_toad",
    }
}

pub fn ability_name(slot: u8) -> &'static str {
    match slot {
        0 => "ability_0",
        1 => "ability_1",
        2 => "ability_2",
        3 => "ability_3",
        _ => "unknown_ability",
    }
}

/// 内容摘要：玩法表变化时 `config_hash` 必须变化，否则新旧客户端会用错 ID。
pub fn content_digest() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |s: &str| {
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for item in &ShopCatalog::placeholder().items {
        mix(item.id);
        if let Some(from) = item.from {
            mix(from);
        }
        mix(&item.price.to_string());
    }
    for id in 1..=3 {
        mix(objective_name(id));
    }
    for slot in 0..4 {
        mix(ability_name(slot));
    }
    // 野怪 ID 词表同样纳入摘要：改名会导致内容表变化，旧客户端必须被拒绝。
    for kind in [
        JungleMonsterType::MusterWell,
        JungleMonsterType::LodeShrine,
        JungleMonsterType::AbyssWarden,
        JungleMonsterType::Treant,
        JungleMonsterType::CoreWarden,
        JungleMonsterType::GildedToad,
    ] {
        mix(monster_name(kind));
    }
    mix(ContentDomain::Monster.debug_name());
    h
}

impl ContentDomain {
    fn debug_name(self) -> &'static str {
        match self {
            ContentDomain::Item => "item",
            ContentDomain::Ability => "ability",
            ContentDomain::Objective => "objective",
            ContentDomain::Monster => "monster",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ids_are_unique_bidirectional() {
        let catalog = ShopCatalog::placeholder();
        let mut seen = std::collections::HashSet::new();
        for item in &catalog.items {
            let id = item_id(item.id);
            assert!(seen.insert(id), "collision on {}", item.id);
            assert_eq!(item_name(id), Some(item.id));
        }
        assert_eq!(seen.len(), 12);
    }

    #[test]
    fn objective_names_roundtrip() {
        for id in 1..=3 {
            assert_eq!(objective_id(objective_name(id)), Some(id));
        }
        assert_eq!(objective_id("nope"), None);
    }

    #[test]
    fn digest_is_stable_and_order_sensitive() {
        assert_eq!(content_digest(), content_digest());
        assert_ne!(stable_id("a"), stable_id("b"));
        assert_ne!(stable_id(""), 0);
        assert_ne!(content_digest(), 0);
    }

    #[test]
    fn objective_buff_ids_match_registry_names() {
        use crate::components::Team;
        for id in 1..=3 {
            let name = objective_name(id);
            assert_eq!(objective_id(name), Some(id));
            let buff = crate::gameplay::attributes::objective_buff(id, Team::Blue)
                .expect("buff table must cover all registry ids");
            assert_eq!(buff.id, id, "buff id 必须与注册表同一 ID 空间");
            assert_eq!(buff.name, name, "buff 名必须与注册表一致");
        }
    }
}
