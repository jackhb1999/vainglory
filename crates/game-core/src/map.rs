//! 灰界裂谷（原创地图）静态数据 —— 复刻经典 3v3 竞技场的 **单路 + 野区结构**
//!
//! 该竞技场 **只有一条兵线**（不是三条），布局要点：
//! - 蓝方基地在左下角、红方基地在右上角，两基地沿地图主对角线对峙；
//! - **唯一兵线**：蓝方基地 → 沿左侧边线北上 → 绕过左上角 → 沿顶部边线东进 → 红方基地；
//! - 兵线内侧（地图右下方向）是**野区**，双方野区关于“折叠线”（`z = x` 对角线）镜像对称；
//! - **中央金矿位于地图正中心**，后期被**深渊巨兽**取代；
//! - 每队 **3 座哨塔**（外侧 Outer / 内侧 Inner / 基地塔）+ 基地源核；
//! - 双方各有一座**野区商店**（Jungle Shop）与 3 个野怪营地，全部镜像对称。
//!
//! 数值为自研占位，权威平衡值来自 `gx-proto-content::map`（`DN-23` / `DEV-02`）。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{Team, TurretType};
use crate::net::JungleMonsterType;

/// 地图尺寸（240×240，为“单条长兵线 + 大片野区”留出足够空间）
pub const MAP_WIDTH: f32 = 240.0;
pub const MAP_HEIGHT: f32 = 240.0;
/// 地图半径（以原点为中心的正方形）
pub const MAP_HALF: f32 = MAP_WIDTH * 0.5;
/// 河道半宽（P-V4）：沿 `x = 0` 南北贯穿的浅滩带，`|x| ≤ 6` 为水域，
/// 与 `docs/05_COMBAT_SPEC.md` §2 的 `RIVER_WIDTH = 12` 一致。
/// **纯表现层**：不改变服务端裁决平面 y=0，英雄可涉水通行。
pub const RIVER_HALF_WIDTH: f32 = 6.0;
/// 兵线宽度（3v3 竞技场只有一条很宽的兵线）
pub const LANE_WIDTH: f32 = 13.0;
/// 兵线数量：3v3 竞技场为单路
pub const LANE_COUNT: usize = 1;

/// 团队基地（左下角 / 右上角，沿地图主对角线对峙）
pub const BLUE_BASE: Vec3 = Vec3::new(-96.0, 0.0, 96.0);
pub const RED_BASE: Vec3 = Vec3::new(96.0, 0.0, -96.0);

/// 源核（基地水晶）位置：位于各自基地核心
pub const BLUE_SOURCE_CORE: Vec3 = Vec3::new(-96.0, 0.0, 96.0);
pub const RED_SOURCE_CORE: Vec3 = Vec3::new(96.0, 0.0, -96.0);

/// 中央金矿 / 深渊巨兽坑位：地图正中心（前期金矿，后期深渊巨兽在此刷新）
pub const GOLD_MINE_POS: Vec3 = Vec3::new(0.0, 0.0, 0.0);

/// 训练假人位置（蓝方野区内，避开野怪营地与占点目标）
pub const TRAINING_DUMMY_POS: Vec3 = Vec3::new(-50.0, 0.0, 26.0);

/// 唯一兵线的路点（蓝方视角：基地门前 → 左侧边线 → 左上转角 → 顶部边线 → 红方基地门前）
///
/// 红方直接反向使用同一组路点（同一根兵线，双方对向推进）。
const THE_LANE: &[(f32, f32)] = &[
    (-90.0, 88.0), // 蓝方基地门前
    (-86.0, 62.0),
    (-84.0, 34.0),
    (-84.0, 2.0),
    (-84.0, -30.0),
    (-82.0, -56.0),
    (-72.0, -74.0), // 左上转角（“折叠”处）
    (-52.0, -82.0),
    (-24.0, -84.0),
    (6.0, -84.0),
    (36.0, -84.0),
    (60.0, -82.0),
    (76.0, -76.0),
    (88.0, -90.0), // 红方基地门前
];

/// 生成一方兵线的路点：蓝方沿定义方向推进，红方反向推进。
/// `lane` 参数仅为兼容既有调用点，3v3 竞技场只有一条兵线。
pub fn lane_waypoints(team: Team, _lane: usize) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = THE_LANE
        .iter()
        .map(|(x, z)| Vec3::new(*x, 0.0, *z))
        .collect();
    if team == Team::Red {
        out.reverse();
    }
    out
}

/// 哨塔位置：每队 3 座（外侧 / 内侧 / 基地），沿兵线向野区一侧偏移，避免堵住兵线。
///
/// 蓝方塔在左侧边线（`x = -77`），红方塔在顶部边线（`z = -77`），
/// 两者关于折叠线 `z = x` 镜像（`(x, z) ↔ (z, x)`）。
pub const TURRET_POSITIONS: &[TurretPosition] = &[
    // 蓝方（左侧边线，由外向内）
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Outer,
        position: Vec3::new(-77.0, 0.0, -46.0),
    },
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Inner,
        position: Vec3::new(-77.0, 0.0, 2.0),
    },
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Base,
        position: Vec3::new(-77.0, 0.0, 50.0),
    },
    // 红方（顶部边线，由外向内；与蓝方镜像）
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Outer,
        position: Vec3::new(-46.0, 0.0, -77.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Inner,
        position: Vec3::new(2.0, 0.0, -77.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Base,
        position: Vec3::new(50.0, 0.0, -77.0),
    },
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TurretPosition {
    pub team: Team,
    pub turret_type: TurretType,
    pub position: Vec3,
}

/// 野区营地：双方各 3 个常规营地 + 1 座野区商店，另加地图中心的金矿 / 海妖坑。
///
/// 红方营地与蓝方营地关于折叠线 `z = x` 镜像，保证双方野区完全对等。
pub const JUNGLE_CAMPS: &[JungleCamp] = &[
    // 蓝方野区（兵线内侧、靠近蓝方基地）
    JungleCamp {
        monster_type: JungleMonsterType::Treant,
        position: Vec3::new(-62.0, 0.0, 40.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::CoreWarden,
        position: Vec3::new(-30.0, 0.0, 68.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::GildedToad,
        position: Vec3::new(-66.0, 0.0, 12.0),
        team: Team::Blue,
    },
    // 红方野区（与蓝方镜像）
    JungleCamp {
        monster_type: JungleMonsterType::Treant,
        position: Vec3::new(40.0, 0.0, -62.0),
        team: Team::Red,
    },
    JungleCamp {
        monster_type: JungleMonsterType::CoreWarden,
        position: Vec3::new(68.0, 0.0, -30.0),
        team: Team::Red,
    },
    JungleCamp {
        monster_type: JungleMonsterType::GildedToad,
        position: Vec3::new(12.0, 0.0, -66.0),
        team: Team::Red,
    },
    // 野区商店（Jungle Shop，镜像各一座）
    JungleCamp {
        monster_type: JungleMonsterType::MusterWell,
        position: Vec3::new(-46.0, 0.0, 72.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::LodeShrine,
        position: Vec3::new(72.0, 0.0, -46.0),
        team: Team::Red,
    },
    // 地图正中心：金矿 / 海妖坑（前期金矿，后期被海妖占据）
    JungleCamp {
        monster_type: JungleMonsterType::AbyssWarden,
        position: GOLD_MINE_POS,
        team: Team::Neutral,
    },
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct JungleCamp {
    pub monster_type: JungleMonsterType,
    pub position: Vec3,
    pub team: Team,
}

/// 小地图 / 渲染用的兵线集合（3v3 竞技场只有一条兵线）
pub fn minion_paths() -> Vec<MinionPath> {
    (0..LANE_COUNT)
        .map(|lane| MinionPath {
            team: Team::Blue,
            waypoints: lane_waypoints(Team::Blue, lane),
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinionPath {
    pub team: Team,
    pub waypoints: Vec<Vec3>,
}

/// 草丛区域（一期遮蔽区，VIS-5）：分布在野区内，按折叠线镜像成对。
pub const BUSHES: &[Bush] = &[
    // 保留测试用例使用的草丛位置（半径 4）
    Bush {
        center: Vec3::new(-24.0, 0.0, -8.0),
        radius: 4.0,
    },
    Bush {
        center: Vec3::new(-8.0, 0.0, -24.0),
        radius: 4.0,
    },
    Bush {
        center: Vec3::new(-14.0, 0.0, 54.0),
        radius: 5.0,
    },
    Bush {
        center: Vec3::new(54.0, 0.0, -14.0),
        radius: 5.0,
    },
    Bush {
        center: Vec3::new(-44.0, 0.0, 34.0),
        radius: 5.0,
    },
    Bush {
        center: Vec3::new(34.0, 0.0, -44.0),
        radius: 5.0,
    },
    Bush {
        center: Vec3::new(-70.0, 0.0, 74.0),
        radius: 5.0,
    },
    Bush {
        center: Vec3::new(74.0, 0.0, -70.0),
        radius: 5.0,
    },
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bush {
    pub center: Vec3,
    pub radius: f32,
}

/// 地图“折叠线”端点（左上 → 右下，`z = x` 对角线），
/// 既是双方野区的镜像对称轴，也用于渲染/小地图标注。
pub fn fold_line_endpoints() -> (Vec3, Vec3) {
    (
        Vec3::new(-MAP_HALF, 0.0, -MAP_HALF),
        Vec3::new(MAP_HALF, 0.0, MAP_HALF),
    )
}

/// 视野常量（渲染/AI 用占位；网络裁剪的权威值见 `gx-proto-content::vision`）
pub const HERO_SIGHT_RANGE: f32 = 14.0;
pub const WARD_SIGHT_RANGE: f32 = 10.0;
pub const TURRET_SIGHT_RANGE: f32 = 16.0;

/// 桥面：河道带内兵线穿越河道的全速通行区（`THE_LANE` 在 `z = -84` 处跨越 `x = 0`）。
/// 单位在此不减速，与河道浅滩形成差异化移动效果。
pub const BRIDGE_Z: f32 = -84.0;
pub const BRIDGE_HALF_LEN: f32 = 6.0;

/// 河道移动系数：沿 `x = 0` 南北贯穿的浅滩带（`|x| ≤ RIVER_HALF_WIDTH`）减速为一半，
/// 但兵线穿越河道的「桥面」（`|x| ≤ RIVER_HALF_WIDTH` 且 `|z - BRIDGE_Z| ≤ BRIDGE_HALF_LEN`）
/// 保持全速——即**河道浅滩减速、桥面全速**的差异化移动效果。
/// **仅影响移动速度**，不改裁决平面（y=0）、不阻挡通行、不提供碰撞豁免。
/// 视觉区分：河道为水面（`PixelKind::Water`），桥面为兵线穿越点。
pub fn terrain_speed_mult(pos: Vec3) -> f32 {
    if pos.x.abs() <= RIVER_HALF_WIDTH {
        if (pos.z - BRIDGE_Z).abs() <= BRIDGE_HALF_LEN {
            1.0 // 桥面：全速通行
        } else {
            0.5 // 河道浅滩：减速一半
        }
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn river_slows_movement_but_bridge_is_full_speed() {
        // 河道浅滩（非桥面）减速一半
        assert_eq!(terrain_speed_mult(Vec3::new(0.0, 0.0, 30.0)), 0.5);
        assert_eq!(
            terrain_speed_mult(Vec3::new(RIVER_HALF_WIDTH, 0.0, 0.0)),
            0.5
        );
        // 桥面（兵线穿越点）保持全速
        assert_eq!(terrain_speed_mult(Vec3::new(0.0, 0.0, BRIDGE_Z)), 1.0);
        assert_eq!(
            terrain_speed_mult(Vec3::new(RIVER_HALF_WIDTH, 0.0, BRIDGE_Z + BRIDGE_HALF_LEN)),
            1.0
        );
        // 河道带外、桥面边界外均全速
        assert_eq!(
            terrain_speed_mult(Vec3::new(RIVER_HALF_WIDTH + 0.01, 0.0, 0.0)),
            1.0
        );
        assert_eq!(
            terrain_speed_mult(Vec3::new(0.0, 0.0, BRIDGE_Z + BRIDGE_HALF_LEN + 0.01)),
            0.5
        );
        assert_eq!(terrain_speed_mult(Vec3::new(40.0, 0.0, 40.0)), 1.0);
    }
}
