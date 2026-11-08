//! GX-01 内容数据表（`DEV-02` / `FR-90~92` / `ARCH §7.3`）。
//!
//! **所有数值均为本项目自研占位值**，不是任何第三方作品的数值表；
//! 按 `GX-PRD-2026-001` §11.1 标注为外部依赖（需平衡验证）。
//! 禁止引用 `LEGAL/` 下的第三方遥测字典或数据文件（`DN-29`）。

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ContentTables {
    pub schema_version: String,
    pub heroes: Vec<HeroDef>,
    pub abilities: Vec<AbilityDef>,
    pub items: Vec<ItemDef>,
    pub item_edges: Vec<ItemEdge>,
    pub constructs: Vec<ConstructDef>,
    pub sentries: Vec<SentryDef>,
    pub objectives: Vec<ObjectiveDef>,
    pub map: MapDef,
    pub economy: EconomyDef,
    pub modes: ModeDef,
    pub vision: VisionDef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct HeroDef {
    pub id: String,
    pub display_name: String,
    pub role_tags: Vec<String>,
    pub base_health: f32,
    pub base_energy: f32,
    pub base_attack: f32,
    pub base_armor: f32,
    pub base_shield: f32,
    pub move_speed: f32,
    pub attack_range: f32,
    pub attack_speed: f32,
    pub growth_health: f32,
    pub growth_attack: f32,
    pub ability_ids: Vec<String>,
    pub build_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct AbilityDef {
    pub id: String,
    pub display_name: String,
    pub cooldown: f32,
    pub energy_cost: f32,
    pub range: f32,
    pub cast_windup: f32,
    pub damage: f32,
    /// "self" | "direction" | "point" | "entity" | "projectile"
    pub target_kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ItemDef {
    pub id: String,
    pub display_name: String,
    pub price: u32,
    pub attack: f32,
    pub arcane: f32,
    pub health: f32,
    pub build_path: String,
}

/// 合成图有向边：`from` + `from` -> `to`。校验器要求为 DAG。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct ItemEdge {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ConstructDef {
    pub id: String,
    pub display_name: String,
    pub health: f32,
    pub attack: f32,
    pub move_speed: f32,
    pub gold_reward: u32,
    pub wave_interval_ticks: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct SentryDef {
    pub id: String,
    pub health: f32,
    pub attack: f32,
    pub shielded_without_constructs: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ObjectiveDef {
    pub id: String,
    pub display_name: String,
    /// "lode_shrine" | "muster_well" | "abyss_warden"
    pub kind: String,
    pub spawn_tick: u64,
    pub respawn_ticks: u64,
    pub capture_threshold: u16,
    pub team_reward: u32,
    pub per_player_reward: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct MapDef {
    pub id: String,
    pub display_name: String,
    pub width: f32,
    pub height: f32,
    pub causeway_half_width: f32,
    pub source_core_blue: [f32; 3],
    pub source_core_red: [f32; 3],
    pub vision_sources: Vec<[f32; 3]>,
    pub brush_zones: Vec<BrushZone>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct BrushZone {
    pub center: [f32; 3],
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct EconomyDef {
    pub construct_last_hit_gold: u32,
    pub hero_kill_gold: u32,
    pub passive_gold_per_tick: f32,
    pub xp_to_level_2: u32,
    pub xp_growth: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ModeDef {
    pub team_size: u8,
    pub room_size: u8,
    pub target_match_ticks: u64,
    pub hard_limit_ticks: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct VisionDef {
    pub vision_radius: f32,
    pub brush_reveal_radius: f32,
    pub constructs_provide_vision: bool,
    pub sentries_provide_vision: bool,
}

impl Default for ContentTables {
    fn default() -> Self {
        Self::placeholder()
    }
}

impl ContentTables {
    /// 自研占位表。仅用于 M0+M1 的配置框架验证与自动化测试，
    /// **不是可上线平衡数值**（`AS-04` / `PRD §11.1`）。
    pub fn placeholder() -> Self {
        let abilities = vec![
            AbilityDef {
                id: "ab_ashwarden_q".into(),
                display_name: "余烬突刺".into(),
                cooldown: 6.0,
                energy_cost: 40.0,
                range: 8.0,
                cast_windup: 0.15,
                damage: 80.0,
                target_kind: "direction".into(),
            },
            AbilityDef {
                id: "ab_ashwarden_w".into(),
                display_name: "灰幕".into(),
                cooldown: 12.0,
                energy_cost: 60.0,
                range: 10.0,
                cast_windup: 0.25,
                damage: 40.0,
                target_kind: "point".into(),
            },
            AbilityDef {
                id: "ab_ashwarden_r".into(),
                display_name: "守望裁决".into(),
                cooldown: 45.0,
                energy_cost: 120.0,
                range: 14.0,
                cast_windup: 0.4,
                damage: 220.0,
                target_kind: "projectile".into(),
            },
            AbilityDef {
                id: "ab_frostwarden_q".into(),
                display_name: "霜痕".into(),
                cooldown: 7.0,
                energy_cost: 35.0,
                range: 9.0,
                cast_windup: 0.2,
                damage: 70.0,
                target_kind: "entity".into(),
            },
        ];

        Self {
            schema_version: "1.0.0-gx01".into(),
            heroes: vec![HeroDef {
                id: "hero_ashwarden".into(),
                display_name: "灰烬守望".into(),
                role_tags: vec!["GUARD".into(), "SPEAR".into()],
                base_health: 720.0,
                base_energy: 260.0,
                base_attack: 54.0,
                base_armor: 22.0,
                base_shield: 18.0,
                move_speed: 3.8,
                attack_range: 5.5,
                attack_speed: 1.0,
                growth_health: 68.0,
                growth_attack: 4.2,
                ability_ids: vec![
                    "ab_ashwarden_q".into(),
                    "ab_ashwarden_w".into(),
                    "ab_ashwarden_r".into(),
                ],
                build_paths: vec!["BLADE".into(), "ARCANE".into()],
            }],
            abilities,
            items: vec![
                ItemDef {
                    id: "item_edge_shard".into(),
                    display_name: "棱刃碎片".into(),
                    price: 300,
                    attack: 12.0,
                    arcane: 0.0,
                    health: 0.0,
                    build_path: "BLADE".into(),
                },
                ItemDef {
                    id: "item_arcane_shard".into(),
                    display_name: "秘纹碎片".into(),
                    price: 300,
                    attack: 0.0,
                    arcane: 12.0,
                    health: 0.0,
                    build_path: "ARCANE".into(),
                },
                ItemDef {
                    id: "item_edge_blade".into(),
                    display_name: "棱锋长刃".into(),
                    price: 1200,
                    attack: 40.0,
                    arcane: 0.0,
                    health: 0.0,
                    build_path: "BLADE".into(),
                },
            ],
            item_edges: vec![ItemEdge {
                from: "item_edge_shard".into(),
                to: "item_edge_blade".into(),
            }],
            constructs: vec![ConstructDef {
                id: "construct_line".into(),
                display_name: "战偶".into(),
                health: 240.0,
                attack: 18.0,
                move_speed: 2.4,
                gold_reward: 22,
                wave_interval_ticks: 1500,
            }],
            sentries: vec![SentryDef {
                id: "sentry_standard".into(),
                health: 5000.0,
                attack: 120.0,
                shielded_without_constructs: true,
            }],
            objectives: vec![
                ObjectiveDef {
                    id: "lode_shrine".into(),
                    display_name: "辉矿".into(),
                    kind: "lode_shrine".into(),
                    spawn_tick: 900,
                    respawn_ticks: 5400,
                    capture_threshold: 100,
                    team_reward: 120,
                    per_player_reward: 40,
                },
                ObjectiveDef {
                    id: "muster_well".into(),
                    display_name: "征募井".into(),
                    kind: "muster_well".into(),
                    spawn_tick: 900,
                    respawn_ticks: 5400,
                    capture_threshold: 100,
                    team_reward: 0,
                    per_player_reward: 0,
                },
                ObjectiveDef {
                    id: "abyss_warden".into(),
                    display_name: "渊卫".into(),
                    kind: "abyss_warden".into(),
                    spawn_tick: 54000,
                    respawn_ticks: 7200,
                    capture_threshold: 200,
                    team_reward: 300,
                    per_player_reward: 100,
                },
            ],
            map: MapDef {
                id: "ashrift".into(),
                display_name: "灰界裂谷".into(),
                width: 100.0,
                height: 100.0,
                causeway_half_width: 6.0,
                source_core_blue: [-76.0, 0.0, 20.0],
                source_core_red: [76.0, 0.0, 20.0],
                vision_sources: vec![[-40.0, 0.0, 0.0], [0.0, 0.0, 0.0], [40.0, 0.0, 0.0]],
                brush_zones: vec![BrushZone {
                    center: [-20.0, 0.0, 24.0],
                    radius: 6.0,
                }],
            },
            economy: EconomyDef {
                construct_last_hit_gold: 22,
                hero_kill_gold: 180,
                passive_gold_per_tick: 0.05,
                xp_to_level_2: 280,
                xp_growth: 1.25,
            },
            modes: ModeDef {
                team_size: 3,
                room_size: 6,
                // 18 分钟 @60Hz = 64,800 tick；25 分钟硬上限 = 90,000 tick
                target_match_ticks: 64_800,
                hard_limit_ticks: 90_000,
            },
            vision: VisionDef {
                vision_radius: 18.0,
                brush_reveal_radius: 6.0,
                constructs_provide_vision: true,
                sentries_provide_vision: true,
            },
        }
    }
}
