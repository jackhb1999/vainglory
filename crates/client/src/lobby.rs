//! 大厅数据目录：英雄档案、道具图鉴、模式选择与上行命令队列。
//!
//! 全部为**纯展示层数据**，不改协议握手 hash、不引入外部资源。
//! 英雄索引与 `visual::HERO_VISUALS` 一一对应（同一套 6 剪影原型），
//! 道具索引与 `game_core::gameplay::shop::ShopCatalog` 的 12 个 `id` 一一对应（单测锁定）。

use bevy::prelude::*;
use game_core::gameplay::clock::MatchMode;
use game_core::gameplay::shop::ItemStats;

/// 英雄档案。**不伪造数值**：用五项定性评分描述定位，
/// 局内真实血量/攻击由服务端快照的 `Hero` 组件显示，避免大厅与对局口径冲突。
#[derive(Clone, Copy, Debug)]
pub struct HeroProfile {
    pub index: u8,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub role_zh: &'static str,
    pub role_en: &'static str,
    pub description_zh: &'static str,
    pub description_en: &'static str,
    pub appearance_zh: &'static str,
    pub appearance_en: &'static str,
    /// 生存 / 输出 / 控制 / 机动 / 上手难度，取值 1..5
    pub ratings: [u8; 5],
    pub tip_zh: &'static str,
    pub tip_en: &'static str,
    /// 推荐出装（商品 id，与 `ITEM_PROFILES` 对齐）
    pub build: &'static [&'static str],
}

pub const RATING_LABELS_ZH: [&str; 5] = ["生存", "输出", "控制", "机动", "难度"];
pub const RATING_LABELS_EN: [&str; 5] = ["SURV", "DMG", "CTRL", "MOBI", "DIFF"];
pub const SKILL_KEYS: [char; 4] = ['Q', 'E', 'R', 'F'];

/// 6 位英雄，索引即 `skin`，与 `visual::HERO_VISUALS` / `hero_visual(skin)` 一致。
pub const HERO_PROFILES: [HeroProfile; 6] = [
    HeroProfile {
        index: 0,
        name_zh: "磐盾守卫",
        name_en: "BULWARK",
        role_zh: "重装前排",
        role_en: "FRONTLINE",
        description_zh: "宽厚重甲与巨盾，顶在兵线最前方吸收伤害，为队友创造输出空间。",
        description_en: "Wide plated guard that soaks damage at the front of the lane.",
        appearance_zh: "金冠 / 厚甲 / 宽肩 / 蓝色主调",
        appearance_en: "CROWN / PLATE / BROAD / BLUE",
        ratings: [5, 2, 3, 1, 1],
        tip_zh: "先手贴脸，用寒冰新星留人，再让队友跟进。",
        tip_en: "Engage first, lock targets with the nova, then let allies follow.",
        build: &["item_guard_shard", "item_bulwark", "item_warden_wall"],
    },
    HeroProfile {
        index: 1,
        name_zh: "赤刃剑士",
        name_en: "BLADE",
        role_zh: "近战刺杀",
        role_en: "MELEE ASSASSIN",
        description_zh: "披风长剑的高机动近战，靠火焰弹起手、贴身爆发收割残血。",
        description_en: "Caped duelist that opens with a fire bolt and bursts down low-health foes.",
        appearance_zh: "双角 / 长剑 / 披风 / 红色主调",
        appearance_en: "HORNS / SWORD / CAPE / RED",
        ratings: [2, 5, 2, 4, 2],
        tip_zh: "等对方交完控制再切入，优先秒掉后排。",
        tip_en: "Dive after enemy control is spent; burst the back line first.",
        build: &["item_edge_shard", "item_edge_blade", "item_rift_cleaver"],
    },
    HeroProfile {
        index: 2,
        name_zh: "幽林游侠",
        name_en: "RANGER",
        role_zh: "远程持续输出",
        role_en: "RANGED CARRY",
        description_zh: "兜帽长柄的修长射手，靠射程与走位持续点射，能量光束可穿线消耗。",
        description_en: "Hooded marksman that keeps distance and pokes with the energy beam.",
        appearance_zh: "兜帽 / 长柄武器 / 披风 / 紫色主调",
        appearance_en: "HOOD / POLEARM / CAPE / VIOLET",
        ratings: [2, 5, 1, 3, 3],
        tip_zh: "保持最大射程，用草丛隐藏视野后再开火。",
        tip_en: "Hold max range; fire from brush to stay hidden.",
        build: &["item_edge_shard", "item_swift_boots", "item_rift_cleaver"],
    },
    HeroProfile {
        index: 3,
        name_zh: "霜环法师",
        name_en: "MAGE",
        role_zh: "法术爆发 / 控制",
        role_en: "BURST MAGE",
        description_zh: "尖顶白面的法术核心，寒冰新星团控配合法术装备打出成片爆发。",
        description_en: "Spike-crowned caster that groups enemies with a frost nova, then bursts.",
        appearance_zh: "尖顶 / 法杖 / 瘦削 / 白石主调",
        appearance_en: "SPIRE / STAFF / SLIM / WHITE",
        ratings: [2, 4, 5, 2, 3],
        tip_zh: "先控后打，寒冰新星的落点尽量覆盖两人以上。",
        tip_en: "Control first; land the nova on two or more targets.",
        build: &["item_arcane_shard", "item_arcane_staff", "item_star_core"],
    },
    HeroProfile {
        index: 4,
        name_zh: "青芽医者",
        name_en: "HEALER",
        role_zh: "辅助 / 续航",
        role_en: "SUPPORT",
        description_zh: "带光环的兜帽治疗者，用自我治疗与护盾装备维系整条兵线的续航。",
        description_en: "Haloed support that sustains the lane with self-heal and defensive items.",
        appearance_zh: "光环兜帽 / 短杖 / 披风 / 草绿主调",
        appearance_en: "HALO / SHORT STAFF / CAPE / GREEN",
        ratings: [4, 2, 3, 2, 2],
        tip_zh: "跟紧前排，治疗留给血线过半的队友而不是自己。",
        tip_en: "Stay behind the frontline; save the heal for allies, not yourself.",
        build: &["item_guard_shard", "item_lumin_charm", "item_bulwark"],
    },
    HeroProfile {
        index: 5,
        name_zh: "破城巨像",
        name_en: "SIEGE",
        role_zh: "攻城 / 拆塔",
        role_en: "SIEGE",
        description_zh: "极宽厚的巨锤单位，专精推进兵线与强拆哨塔，代价是机动极低。",
        description_en: "Massive hammer unit built to push the lane and break turrets.",
        appearance_zh: "双角盔 / 巨锤 / 极宽肩 / 金色主调",
        appearance_en: "HORNED HELM / HAMMER / HUGE / GOLD",
        ratings: [5, 4, 1, 1, 4],
        tip_zh: "抱团推进，别单独追人；拆塔优先于击杀。",
        tip_en: "Push as a group; objectives before kills.",
        build: &["item_guard_shard", "item_edge_blade", "item_warden_wall"],
    },
];

pub fn hero_profile(index: u8) -> HeroProfile {
    HERO_PROFILES[(index as usize) % HERO_PROFILES.len()]
}

/// 技能说明（全英雄共用同一套 `default_hero_abilities()`，名称与数值仍从
/// `game_core::abilities::get_ability_def` 读取，此处只补描述文案，避免第二套数值）。
pub const ABILITY_NOTES_ZH: [&str; 4] = [
    "直线弹道，命中造成单体伤害；适合起手消耗与补刀。",
    "指定落点的范围控制，按作用半径展开阶梯方块环。",
    "沿瞄准线的连续能量束，射程远，适合穿线消耗。",
    "瞬发自我治疗，配合护盾装备可大幅提升续航。",
];
pub const ABILITY_NOTES_EN: [&str; 4] = [
    "Single-target projectile for poke and last-hitting.",
    "Ground-targeted AoE control that expands as a stepped voxel ring.",
    "Long-range continuous beam along the aim line.",
    "Instant self-heal; pairs well with defensive items.",
];

/// 道具分类（用于商店 / 图鉴分组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemCategory {
    Attack,
    Magic,
    Defense,
    Mobility,
    Active,
}

impl ItemCategory {
    pub const fn label(self, cjk: bool) -> &'static str {
        match (self, cjk) {
            (Self::Attack, true) => "物理攻击",
            (Self::Attack, false) => "ATTACK",
            (Self::Magic, true) => "法术强度",
            (Self::Magic, false) => "ABILITY",
            (Self::Defense, true) => "防御生存",
            (Self::Defense, false) => "DEFENSE",
            (Self::Mobility, true) => "移动",
            (Self::Mobility, false) => "MOBILITY",
            (Self::Active, true) => "主动装备",
            (Self::Active, false) => "ACTIVE",
        }
    }
}

pub const ITEM_CATEGORIES: [ItemCategory; 5] = [
    ItemCategory::Attack,
    ItemCategory::Magic,
    ItemCategory::Defense,
    ItemCategory::Mobility,
    ItemCategory::Active,
];

/// 道具展示元数据：补齐 `ShopCatalog::ItemDef` 缺失的中文名、分类与说明。
#[derive(Clone, Copy, Debug)]
pub struct ItemProfile {
    pub id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub category: ItemCategory,
    pub description_zh: &'static str,
    pub description_en: &'static str,
}

pub const ITEM_PROFILES: [ItemProfile; 12] = [
    ItemProfile {
        id: "item_edge_shard",
        name_zh: "锋刃碎片",
        name_en: "EDGE SHARD",
        category: ItemCategory::Attack,
        description_zh: "基础物理组件，提供少量攻击力，可合成长刃与大剑。",
        description_en: "Basic attack component; builds into blade and cleaver.",
    },
    ItemProfile {
        id: "item_arcane_shard",
        name_zh: "奥术碎片",
        name_en: "ARCANE SHARD",
        category: ItemCategory::Magic,
        description_zh: "基础法术组件，提供少量法强，是法杖与星核的前置。",
        description_en: "Basic ability component; builds into staff and star core.",
    },
    ItemProfile {
        id: "item_guard_shard",
        name_zh: "守护碎片",
        name_en: "GUARD SHARD",
        category: ItemCategory::Defense,
        description_zh: "提供额外生命值，可合成壁垒、辉光护符与守望之墙。",
        description_en: "Grants bonus health; builds into bulwark, charm and warden wall.",
    },
    ItemProfile {
        id: "item_swift_boots",
        name_zh: "疾行靴",
        name_en: "SWIFT BOOTS",
        category: ItemCategory::Mobility,
        description_zh: "提升移动速度，适合需要拉扯与追击的远程英雄。",
        description_en: "Movement speed for kiting and chasing.",
    },
    ItemProfile {
        id: "item_edge_blade",
        name_zh: "锋锐长刃",
        name_en: "EDGE BLADE",
        category: ItemCategory::Attack,
        description_zh: "由锋刃碎片升级，大幅提升物理攻击。",
        description_en: "Upgrade of Edge Shard with a large attack boost.",
    },
    ItemProfile {
        id: "item_arcane_staff",
        name_zh: "奥术法杖",
        name_en: "ARCANE STAFF",
        category: ItemCategory::Magic,
        description_zh: "由奥术碎片升级，大幅提升法术强度。",
        description_en: "Upgrade of Arcane Shard with a large ability boost.",
    },
    ItemProfile {
        id: "item_bulwark",
        name_zh: "磐石壁垒",
        name_en: "BULWARK",
        category: ItemCategory::Defense,
        description_zh: "由守护碎片升级，提供高额护甲，克制物理输出。",
        description_en: "Upgrade of Guard Shard; heavy armor against physical damage.",
    },
    ItemProfile {
        id: "item_lumin_charm",
        name_zh: "辉光护符",
        name_en: "LUMIN CHARM",
        category: ItemCategory::Defense,
        description_zh: "由守护碎片升级，提供魔抗，克制法术爆发。",
        description_en: "Upgrade of Guard Shard; magic resist against burst.",
    },
    ItemProfile {
        id: "item_rift_cleaver",
        name_zh: "裂谷巨斧",
        name_en: "RIFT CLEAVER",
        category: ItemCategory::Attack,
        description_zh: "物理终极装：巨额攻击力并附带暴击几率。",
        description_en: "Physical capstone: huge attack plus critical chance.",
    },
    ItemProfile {
        id: "item_star_core",
        name_zh: "星核法器",
        name_en: "STAR CORE",
        category: ItemCategory::Magic,
        description_zh: "法术终极装：巨额法强并缩短技能冷却。",
        description_en: "Ability capstone: huge power plus cooldown reduction.",
    },
    ItemProfile {
        id: "item_warden_wall",
        name_zh: "守望之墙",
        name_en: "WARDEN WALL",
        category: ItemCategory::Defense,
        description_zh: "防御终极装：大量生命值与护甲，前排核心。",
        description_en: "Defense capstone: large health and armor for frontliners.",
    },
    ItemProfile {
        id: "item_blink_cloak",
        name_zh: "闪现披风",
        name_en: "BLINK CLOAK",
        category: ItemCategory::Active,
        description_zh: "主动装备：不提供属性，但可在关键时刻位移脱离或开团。",
        description_en: "Active item: no stats, but a clutch blink engage or escape.",
    },
];

pub fn item_profile(id: &str) -> Option<ItemProfile> {
    ITEM_PROFILES.iter().find(|p| p.id == id).copied()
}

/// 把 `ItemStats` 转成可读的属性加成行（数值仍来自唯一数值表）。
pub fn stats_text(s: &ItemStats, cjk: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: f32, zh: &str, en: &str, pct: bool| {
        if v.abs() > 1e-4 {
            out.push(if pct {
                format!("{} +{:.0}%", if cjk { zh } else { en }, v * 100.0)
            } else {
                format!("{} +{:.0}", if cjk { zh } else { en }, v)
            });
        }
    };
    push(s.attack_damage, "攻击", "AD", false);
    push(s.ability_power, "法强", "AP", false);
    push(s.max_health, "生命", "HP", false);
    push(s.armor, "护甲", "ARM", false);
    push(s.magic_resist, "魔抗", "MR", false);
    push(s.crit_chance, "暴击", "CRIT", true);
    push(s.cooldown_reduction, "冷却缩减", "CDR", true);
    push(s.move_speed, "移速", "MS", false);
    out
}

/// 大厅选择：英雄索引 + 对局模式。进入对局后仍由 HUD / 技能栏读取。
#[derive(Resource, Debug, Clone, Copy)]
pub struct LobbySelection {
    pub hero_index: u8,
    pub mode: MatchMode,
}

impl Default for LobbySelection {
    fn default() -> Self {
        Self {
            hero_index: 0,
            mode: MatchMode::Pve,
        }
    }
}

/// 本地扩展 trait：`MatchMode` 定义在 `game-core`，本 crate 不能写 inherent impl，
/// 但可以为它补展示层文案；编码 / 解码由 `game-core` 的权威实现提供。
pub trait MatchModeExt: Sized {
    fn label(self, cjk: bool) -> &'static str;
    fn detail(self, cjk: bool) -> &'static str;
}

impl MatchModeExt for MatchMode {
    fn label(self, cjk: bool) -> &'static str {
        match (self, cjk) {
            (MatchMode::Pve, true) => "PVE 训练",
            (MatchMode::Pve, false) => "PVE TRAINING",
            (MatchMode::Pvp, true) => "PVP 对战",
            (MatchMode::Pvp, false) => "PVP MATCH",
        }
    }

    fn detail(self, cjk: bool) -> &'static str {
        match (self, cjk) {
            (MatchMode::Pve, true) => "单人即可开局，服务端补 AI 对手；适合熟悉操作与出装。",
            (MatchMode::Pve, false) => "Solo ready; server adds an AI opponent. Practice mode.",
            (MatchMode::Pvp, true) => "需两名真人玩家连接后开局，不生成 AI 对手。",
            (MatchMode::Pvp, false) => "Requires two human players; no AI opponent.",
        }
    }
}

/// 上行命令队列：UI 只入队，由 `networking` 统一编码发送（避免 UI 层碰网络资源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    SelectMode(MatchMode),
    Purchase(u32),
    Sell(u8),
}

#[derive(Resource, Default, Debug)]
pub struct ClientCommandQueue(pub Vec<ClientCommand>);

/// 商店操作反馈（由发送系统写入、UI 显示，独立计时系统递减）。
#[derive(Resource, Default, Debug)]
pub struct ShopToast {
    pub text_zh: String,
    pub text_en: String,
    pub ttl: f32,
}

impl ShopToast {
    pub fn set(&mut self, zh: impl Into<String>, en: impl Into<String>) {
        self.text_zh = zh.into();
        self.text_en = en.into();
        self.ttl = 2.5;
    }
}

/// 大厅「道具图鉴」窗口开关（跨系统共享，不能用 `Local`）。
#[derive(Resource, Default, Debug)]
pub struct CodexOpen(pub bool);

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::gameplay::shop::ShopCatalog;
    use std::collections::HashSet;

    #[test]
    fn hero_catalog_matches_six_visual_profiles() {
        assert_eq!(HERO_PROFILES.len(), crate::visual::HERO_VISUALS.len());
        let mut names = HashSet::new();
        for (i, h) in HERO_PROFILES.iter().enumerate() {
            assert_eq!(h.index as usize, i);
            assert!(names.insert(h.name_zh));
            assert!(names.insert(h.name_en));
            assert!(h.ratings.iter().all(|&r| (1..=5).contains(&r)));
            assert!(!h.build.is_empty());
            for id in h.build {
                assert!(item_profile(id).is_some(), "推荐出装 {id} 必须在图鉴中");
            }
        }
    }

    #[test]
    fn item_catalog_exactly_covers_shop_catalog() {
        let shop = ShopCatalog::placeholder();
        assert_eq!(shop.items.len(), ITEM_PROFILES.len());
        let shop_ids: HashSet<_> = shop.items.iter().map(|d| d.id).collect();
        let meta_ids: HashSet<_> = ITEM_PROFILES.iter().map(|p| p.id).collect();
        assert_eq!(shop_ids, meta_ids, "图鉴与数值表必须一一对应");
        for d in &shop.items {
            let p = item_profile(d.id).expect("每个道具都要有展示信息");
            assert!(!p.name_zh.is_empty() && !p.description_zh.is_empty());
        }
    }

    #[test]
    fn stats_text_reflects_all_nonzero_stats() {
        let d = ShopCatalog::placeholder()
            .get("item_rift_cleaver")
            .unwrap()
            .clone();
        let text = stats_text(&d.stats, true).join(" ");
        assert!(text.contains("攻击"), "{text}");
        assert!(text.contains("暴击"), "{text}");
        let empty = stats_text(&ItemStats::default(), false);
        assert!(empty.is_empty());
    }

    #[test]
    fn mode_codes_roundtrip_and_fall_back() {
        assert_eq!(MatchMode::from_code(MatchMode::Pve.code()), MatchMode::Pve);
        assert_eq!(MatchMode::from_code(MatchMode::Pvp.code()), MatchMode::Pvp);
        assert_eq!(MatchMode::from_code(99), MatchMode::Pve);
    }

    #[test]
    fn selection_defaults_to_pve_first_hero() {
        let s = LobbySelection::default();
        assert_eq!(s.hero_index, 0);
        assert_eq!(s.mode, MatchMode::Pve);
        assert_eq!(hero_profile(9).index, 3, "越界索引安全回绕");
    }
}
