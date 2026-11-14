//! 灰界裂谷（原创地图）静态数据 —— 复刻经典单路 3v3 的**横向长方形裂谷**。
//!
//! 经典 3v3 的地图是一张宽约为高两倍的长方形，骨架为「北侧一条横贯左右的兵线
//! + 兵线南侧一整片野区 + 一条把野区切成左右两半的纵向河道」：
//! - 蓝方水晶在**左端**、红方水晶在**右端**，两者由**唯一兵线**沿北侧横向连通；
//! - **兵线与野区之间有一道实体石墙**（只在兵线靠野区的南侧），在对称的几处坡道开口连通；
//!   兵线北侧是地图边界方向，不需要墙。小兵不会跑进野区，
//!   英雄想转线必须走坡道 / 河道交汇处，转线路径因此可被预判；
//! - 兵线南侧是野区，被沿中轴 `x = 0` 的**纵向河道**一分为二：左半属蓝方、右半属红方；
//! - 河道方向 `(0,1)` 与兵线方向 `(1,0)` **正交**，兵线在中点跨河处有一座全速桥面；
//! - **中央矿坑**位于兵线中点正下方、河道之上，是双方共用的必争目标；
//! - **野区商店**在地图正下方中央，双方共用；两座**兵矿**分列左下角与右下角；
//! - 每队 5 座哨塔（3 座沿兵线、2 座守卫水晶），草丛主要沿兵线两端与野区商店分布。
//!
//! **等比放大**：所有地图几何都由一组「基准布局」常量经 `MAP_SCALE` 统一缩放生成
//! （`s(v)` 缩放标量、`p(x,z)` 缩放坐标）。改这一个常量即可把整张地图等比放大 / 缩小，
//! 尺寸、路点、营地、塔、草丛、河道宽度、墙段与开口会一起变，不会破坏相对方位和对称性。
//! 英雄 / 建筑模型尺寸、碰撞半径、攻击距离等**物件尺度**数值不随之缩放，
//! 这样放大地图得到的是「更开阔的空间与更长转线」，而不是把角色一起放大。
//!
//! **公平性：整张地图关于中轴 `x = 0` 左右镜像对称**（`mirror_fold(x,z) = (-x,z)`）。
//! 该镜像把蓝方每个实体映射到红方对应实体、把水平兵线映射到自身（反向），
//! 河道 / 桥面 / 中央矿坑 / 共用商店都落在镜像轴上；南侧分隔墙及全部坡道开口也逐项镜像。
//! 上一版采用「沿对角线 `z = x` 镜像」，那个变换只在正方形里才能把地图映回自身，
//! 因此把尺寸锁死成了 240×240；改为左右镜像后，长方形同样严格对称且更贴合参考布局。
//!
//! 数值为自研占位，权威平衡值来自 `gx-proto-content::map`（`DN-23` / `DEV-02`）。

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{Team, TurretType};
use crate::net::JungleMonsterType;

/// 整图**等比放大系数**（相对 320×160 的基准布局）。
///
/// 1.5 倍 → `480 × 240`。这是唯一的缩放真相源：所有尺寸、坐标、宽度都由它派生。
/// 想要更大 / 更小只改这里，别去逐个改坐标，否则会破坏等比与左右镜像。
pub const MAP_SCALE: f32 = 1.5;

/// 标量等比缩放（地图尺度量：尺寸 / 半宽 / 宽度 / 半径）。
const fn s(v: f32) -> f32 {
    v * MAP_SCALE
}

/// 基准布局坐标 → 世界坐标（x/z 同时缩放，y 恒为裁决平面 0）。
const fn p(x: f32, z: f32) -> Vec3 {
    Vec3::new(s(x), 0.0, s(z))
}

/// 基准布局下兵线所在的 z（真正暴露的 `LANE_Z` 已缩放）。
const BASE_LANE_Z: f32 = -40.0;

/// 地图尺寸：**横向长方形，宽高比 2:1**。
/// 放大后 `x ∈ [-240,240]`、`z ∈ [-120,120]`。
pub const MAP_WIDTH: f32 = s(320.0);
pub const MAP_HEIGHT: f32 = s(160.0);
/// 分轴半径：长方形下 x / z 半径**不同**，所有边界判定必须分别取用。
pub const MAP_W_HALF: f32 = MAP_WIDTH * 0.5;
pub const MAP_H_HALF: f32 = MAP_HEIGHT * 0.5;
/// 兼容别名：仅表示 **x 轴**半宽。z 轴请用 `MAP_H_HALF`；
/// 旧代码把单一 `MAP_HALF` 通吃两轴，正是「地图只能是正方形」的根源。
pub const MAP_HALF: f32 = MAP_W_HALF;

/// 兵线所在的北侧 z 坐标（水平横贯左右）。
pub const LANE_Z: f32 = s(BASE_LANE_Z);
/// 兵线宽度（3v3 竞技场只有一条很宽的兵线）
pub const LANE_WIDTH: f32 = s(13.0);
/// 兵线数量：3v3 竞技场为单路
pub const LANE_COUNT: usize = 1;

/// 河道半宽（P-V4）：到中轴 `x = 0` 的水平距离不超过此值为水域。
/// **纯表现层**：不改变服务端裁决平面 y=0，英雄可涉水通行。
pub const RIVER_HALF_WIDTH: f32 = s(8.0);
/// 河道沿 z 轴的起点（北边界），向南贯穿到 `MAP_H_HALF`。
pub const RIVER_START: f32 = -MAP_H_HALF;
/// 桥面中心（兵线跨河处）与沿河方向的半长：桥上全速，其余河段减速一半。
pub const BRIDGE_Z: f32 = LANE_Z;
pub const BRIDGE_HALF_LEN: f32 = s(10.0);

/// 关于中轴 `x = 0` 的左右镜像：翻转 x、保留 z。红方实体 = 蓝方实体的镜像点。
///
/// 长方形下仍然是地图的自映射（不像 `z = x` 镜像会交换宽高导致越界）。
pub fn mirror_fold(p: Vec3) -> Vec3 {
    Vec3::new(-p.x, p.y, p.z)
}

/// 沿河流轴（z 方向）的坐标。
pub fn river_axis(pos: Vec3) -> f32 {
    pos.z
}

/// 到河流中线 `x = 0` 的水平距离。
pub fn river_cross(pos: Vec3) -> f32 {
    pos.x.abs()
}

/// 是否处于河流水域（不含桥面判定）。
pub fn is_river(pos: Vec3) -> bool {
    river_cross(pos) <= RIVER_HALF_WIDTH && river_axis(pos) >= RIVER_START
}

/// 是否处于兵线跨河的桥面：河道内且靠近 `BRIDGE_Z`。
pub fn is_bridge(pos: Vec3) -> bool {
    is_river(pos) && (river_axis(pos) - BRIDGE_Z).abs() <= BRIDGE_HALF_LEN
}

/// 河道移动系数：纵向浅滩减速一半，兵线跨河的桥面全速。
///
/// 河道方向 `(0,1)` 与兵线方向 `(1,0)` 点积为 0 —— **河道与兵线正交**，
/// 这是经典单路 3v3 的识别特征；`is_river` / `is_bridge` 在 `mirror_fold` 下不变，
/// 保证双方涉水代价完全对等。
/// **仅影响移动速度**，不改裁决平面（y=0）、不阻挡通行、不提供碰撞豁免。
pub fn terrain_speed_mult(pos: Vec3) -> f32 {
    if is_bridge(pos) {
        1.0
    } else if is_river(pos) {
        0.5
    } else {
        1.0
    }
}

/// 每次涉水结算造成的真实伤害（`True` 语义，不经过护甲减免）。
/// 英雄默认 1000 血，50 点/秒意味着长时间泡在水里约 20 秒会被淹死；
/// 正常横穿一次河道只吃约 6 秒 ≈ 300 点，代价明显但不至于劝退。
pub const RIVER_HAZARD_DAMAGE: f32 = 25.0;
/// 涉水结算周期：30 tick = 0.5 s，与既有 `BURN_PERIOD_TICKS` 同节奏。
pub const RIVER_HAZARD_PERIOD_TICKS: u64 = 30;

/// 是否处于**危险水域**：在河道里但不在桥面上。
///
/// 河道是危险的浅滩，桥面（兵线跨河处）是唯一安全通道 —— 这与减速规则一致：
/// `terrain_speed_mult` 也只在非桥面河段减半。服务端用此函数裁决伤害，
/// 客户端用同一函数显示「正在被河水灼烧」的 HUD 警示，两边不会打架。
pub fn is_hazardous_river(pos: Vec3) -> bool {
    is_river(pos) && !is_bridge(pos)
}

// ============================================================================
// 兵线 / 野区分隔墙：服务端权威碰撞、客户端预测、A*、渲染、小地图的唯一真相源
// ============================================================================

/// 石墙半厚（地图尺度量，随 `MAP_SCALE` 等比放大）。
/// 服务端 `collision_system` 只支持圆形 `Collider`，因此长墙由一串半径等于半厚的圆拼成。
pub const WALL_HALF_THICKNESS: f32 = s(1.6);
/// 相邻墙碰撞圆的间距：必须小于 `2 * WALL_HALF_THICKNESS` 才能拼成无缝屏障。
pub const WALL_POST_SPACING: f32 = s(2.6);

/// 分隔墙的中心 z：位于兵线**南侧**（野区一侧），与兵线通道边缘留出 `s(0.5)` 的视觉间隙。
/// 兵线北侧靠地图边界，不需要墙 —— 参考总览图里只有兵线与野区之间有一道堤墙。
pub const WALL_SOUTH_Z: f32 = LANE_Z + (LANE_WIDTH * 0.5 + WALL_HALF_THICKNESS + s(0.5));

/// 一段水平石墙（世界坐标）。`x0 < x1`，`z` 恒为 `WALL_SOUTH_Z`。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LaneWall {
    pub x0: f32,
    pub x1: f32,
    pub z: f32,
}

/// 坡道开口（世界坐标 x 区间）。只在开口处能从兵线走进野区。
///
/// 基准布局共 5 处：左右两端基地入口、左右两处中段坡道、中央河道交汇口。
/// 全部关于 `x = 0` 镜像（中央开口自身镜像），双方转线距离完全对等。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WallGap {
    pub x0: f32,
    pub x1: f32,
}

pub const LANE_WALL_GAPS: &[WallGap] = &[
    WallGap {
        x0: s(-160.0),
        x1: s(-116.0),
    }, // 蓝方基地入口
    WallGap {
        x0: s(-76.0),
        x1: s(-62.0),
    }, // 蓝方中段坡道
    WallGap {
        x0: s(-14.0),
        x1: s(14.0),
    }, // 中央河道 / 桥面交汇口
    WallGap {
        x0: s(62.0),
        x1: s(76.0),
    }, // 红方中段坡道
    WallGap {
        x0: s(116.0),
        x1: s(160.0),
    }, // 红方基地入口
];

/// 分隔墙的实体段：`[-240,240]` 全区间扣除 5 处开口，共 4 段。
/// 红方每一段都是蓝方某一段的 `x = 0` 镜像（z 不变）。
pub const LANE_WALLS: &[LaneWall] = &[
    // 兵线南侧（朝野区）
    LaneWall {
        x0: s(-116.0),
        x1: s(-76.0),
        z: WALL_SOUTH_Z,
    },
    LaneWall {
        x0: s(-62.0),
        x1: s(-14.0),
        z: WALL_SOUTH_Z,
    },
    LaneWall {
        x0: s(14.0),
        x1: s(62.0),
        z: WALL_SOUTH_Z,
    },
    LaneWall {
        x0: s(76.0),
        x1: s(116.0),
        z: WALL_SOUTH_Z,
    },
];

/// 该 x 是否落在坡道开口内（A*、渲染门柱、单测共用）。
pub fn is_lane_wall_gap(x: f32) -> bool {
    LANE_WALL_GAPS.iter().any(|g| x >= g.x0 && x <= g.x1)
}

/// 把每段长墙采样成一串**圆形静态碰撞体** `(圆心, 半径)`。
///
/// 服务端用它生成 `Collider + StaticCollider`，客户端预测用同一份数据做推出，
/// 两边永远一致 —— 否则会出现「客户端看着能过、服务端把人顶回来」的抖动。
/// 间距 < 直径保证圆并集连续，段端也各放一个圆封口。
pub fn wall_colliders() -> Vec<(Vec3, f32)> {
    let mut out = Vec::new();
    for w in LANE_WALLS {
        let span = w.x1 - w.x0;
        let steps = (span / WALL_POST_SPACING).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let x = w.x0 + span * (i as f32) / (steps as f32);
            out.push((Vec3::new(x, 0.0, w.z), WALL_HALF_THICKNESS));
        }
    }
    out
}

/// 团队基地：蓝方左端 / 红方右端，同在北侧兵线上，互为 `x = 0` 镜像。
pub const BLUE_BASE: Vec3 = p(-140.0, BASE_LANE_Z);
pub const RED_BASE: Vec3 = p(140.0, BASE_LANE_Z);

/// 源核（基地水晶）位置：位于各自基地核心
pub const BLUE_SOURCE_CORE: Vec3 = BLUE_BASE;
pub const RED_SOURCE_CORE: Vec3 = RED_BASE;

/// 中央矿坑 / 深渊巨兽坑位：**兵线中点正下方**、河道之上（镜像轴不动点）。
pub const GOLD_MINE_POS: Vec3 = p(0.0, 8.0);

/// 野区商店位置：地图**正下方中央**，双方共用（镜像轴不动点）。
pub const JUNGLE_SHOP_POS: Vec3 = p(0.0, 64.0);

/// 训练假人位置（蓝方野区内；单人调试设施，不参与双方公平性不变量）
pub const TRAINING_DUMMY_POS: Vec3 = p(-60.0, 20.0);

/// 唯一兵线的路点（**基准布局坐标**）：沿 `BASE_LANE_Z` 水平横贯，蓝方左端 → 红方右端。
///
/// 路点集合关于 `x = 0` 镜像对称：第 `i` 个路点的镜像 = 第 `n-1-i` 个路点，
/// 正中路点 `(0, BASE_LANE_Z)` 是镜像不动点（兵线跨河桥面）。
/// 红方反向使用同一组路点，双方行程完全相等。
const THE_LANE: &[(f32, f32)] = &[
    (-140.0, BASE_LANE_Z), // 蓝方基地
    (-112.0, BASE_LANE_Z),
    (-84.0, BASE_LANE_Z),
    (-56.0, BASE_LANE_Z),
    (-28.0, BASE_LANE_Z),
    (0.0, BASE_LANE_Z), // 兵线中点 / 跨河桥面
    (28.0, BASE_LANE_Z),
    (56.0, BASE_LANE_Z),
    (84.0, BASE_LANE_Z),
    (112.0, BASE_LANE_Z),
    (140.0, BASE_LANE_Z), // 红方基地
];

/// 生成一方兵线的路点：蓝方沿定义方向推进，红方反向推进。
/// `lane` 参数仅为兼容既有调用点，3v3 竞技场只有一条兵线。
pub fn lane_waypoints(team: Team, _lane: usize) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = THE_LANE.iter().map(|(x, z)| p(*x, *z)).collect();
    if team == Team::Red {
        out.reverse();
    }
    out
}

/// 哨塔位置：每队 **5 座** —— 3 座沿兵线由内向外、2 座守卫水晶。
/// 线上塔收进兵线通道内（不落在堤墙上），红方每座塔 = 蓝方对应塔的 `x = 0` 镜像。
pub const TURRET_POSITIONS: &[TurretPosition] = &[
    // 蓝方：线上 3 座（由内向外：Base → Inner → Outer）
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Base,
        position: p(-112.0, -36.0),
    },
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Inner,
        position: p(-76.0, -36.0),
    },
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Outer,
        position: p(-36.0, -36.0),
    },
    // 蓝方：守卫水晶的 2 座，分列水晶南北两侧（均在基地坡道开口内）
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Base,
        position: p(-136.0, -56.0),
    },
    TurretPosition {
        team: Team::Blue,
        turret_type: TurretType::Base,
        position: p(-136.0, -24.0),
    },
    // 红方：与蓝方逐座镜像
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Base,
        position: p(112.0, -36.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Inner,
        position: p(76.0, -36.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Outer,
        position: p(36.0, -36.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Base,
        position: p(136.0, -56.0),
    },
    TurretPosition {
        team: Team::Red,
        turret_type: TurretType::Base,
        position: p(136.0, -24.0),
    },
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TurretPosition {
    pub team: Team,
    pub turret_type: TurretType,
    pub position: Vec3,
}

/// 野区营地：左半（蓝）/ 右半（红）各 4 座逐项镜像，另有 2 座位于中轴的共用目标。
///
/// - `Treant` 单个树精、`GildedToad` 小型野怪、`CoreWarden` 后方大型野怪；
/// - `LodeShrine` 兵矿分列左下 / 右下角（参考布局的「两侧兵矿」）；
/// - `MusterWell` 野区商店在**正下方中央**，双方共用；
/// - `AbyssWarden` 中央矿坑在**兵线中点正下方**，河道之上。
///
/// `team` 只标记营地归属哪半边野区；服务端实际把野怪生成为中立（可被任意阵营攻击）。
pub const JUNGLE_CAMPS: &[JungleCamp] = &[
    JungleCamp {
        monster_type: JungleMonsterType::Treant,
        position: p(-44.0, 20.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::CoreWarden,
        position: p(-116.0, 8.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::GildedToad,
        position: p(-76.0, 44.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::LodeShrine,
        position: p(-120.0, 60.0),
        team: Team::Blue,
    },
    JungleCamp {
        monster_type: JungleMonsterType::Treant,
        position: p(44.0, 20.0),
        team: Team::Red,
    },
    JungleCamp {
        monster_type: JungleMonsterType::CoreWarden,
        position: p(116.0, 8.0),
        team: Team::Red,
    },
    JungleCamp {
        monster_type: JungleMonsterType::GildedToad,
        position: p(76.0, 44.0),
        team: Team::Red,
    },
    JungleCamp {
        monster_type: JungleMonsterType::LodeShrine,
        position: p(120.0, 60.0),
        team: Team::Red,
    },
    // 正下方中央：双方共用的野区商店
    JungleCamp {
        monster_type: JungleMonsterType::MusterWell,
        position: JUNGLE_SHOP_POS,
        team: Team::Neutral,
    },
    // 兵线中点正下方：中央矿坑（前期矿工，后期由深渊巨兽占据）
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

/// 草丛区域（一期遮蔽区，VIS-5）：全部关于 `x = 0` 镜像成对，双方遮蔽资源对等。
/// 主要分布在兵线两端、中央矿坑周围与野区商店周围；半径同样是地图尺度量，随缩放放大。
pub const BUSHES: &[Bush] = &[
    // 保留视野裁剪单测使用的草丛位置及其镜像
    Bush {
        center: p(-24.0, -8.0),
        radius: s(4.0),
    },
    Bush {
        center: p(24.0, -8.0),
        radius: s(4.0),
    },
    // 兵线两端
    Bush {
        center: p(-124.0, -24.0),
        radius: s(5.0),
    },
    Bush {
        center: p(124.0, -24.0),
        radius: s(5.0),
    },
    Bush {
        center: p(-124.0, -56.0),
        radius: s(5.0),
    },
    Bush {
        center: p(124.0, -56.0),
        radius: s(5.0),
    },
    // 中央矿坑周围
    Bush {
        center: p(-20.0, 28.0),
        radius: s(5.0),
    },
    Bush {
        center: p(20.0, 28.0),
        radius: s(5.0),
    },
    // 正下方共用商店周围
    Bush {
        center: p(-24.0, 64.0),
        radius: s(5.0),
    },
    Bush {
        center: p(24.0, 64.0),
        radius: s(5.0),
    },
    // 野区中段
    Bush {
        center: p(-76.0, 24.0),
        radius: s(5.0),
    },
    Bush {
        center: p(76.0, 24.0),
        radius: s(5.0),
    },
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bush {
    pub center: Vec3,
    pub radius: f32,
}

/// 河道中线端点（沿 `x = 0`，北边界 → 南边界），用于渲染 / 小地图标注。
pub fn fold_line_endpoints() -> (Vec3, Vec3) {
    (
        Vec3::new(0.0, 0.0, RIVER_START),
        Vec3::new(0.0, 0.0, MAP_H_HALF),
    )
}

/// 视野常量（**物件尺度**，不随 `MAP_SCALE` 放大；渲染/AI 用占位）。
/// 权威网络裁剪值见 `gx-proto-content::vision`。
pub const HERO_SIGHT_RANGE: f32 = 14.0;
pub const WARD_SIGHT_RANGE: f32 = 10.0;
pub const TURRET_SIGHT_RANGE: f32 = 16.0;

#[cfg(test)]
// 这一组测试的目的就是把地图常量（尺寸、路点、营地、草丛、墙）本身锁成回归护栏，
// 断言「常量满足某个不变量」正是意图，不是笔误。
#[allow(clippy::assertions_on_constants)]
mod tests {
    use super::*;

    /// 回归护栏：地图必须是横向长方形、分轴半径正确，且**整体由 MAP_SCALE 等比放大**。
    #[test]
    fn whole_map_is_uniformly_scaled_landscape_rectangle() {
        assert!(MAP_SCALE > 1.0, "本轮要求地图比上一版更大");
        assert!(MAP_WIDTH > MAP_HEIGHT, "参考布局是横向长方形");
        assert_eq!(MAP_WIDTH / MAP_HEIGHT, 2.0, "等比放大不得改变宽高比");
        assert_eq!(MAP_WIDTH, 320.0 * MAP_SCALE);
        assert_eq!(MAP_HEIGHT, 160.0 * MAP_SCALE);
        assert_eq!(MAP_W_HALF, 160.0 * MAP_SCALE);
        assert_eq!(MAP_H_HALF, 80.0 * MAP_SCALE);
        assert_ne!(MAP_W_HALF, MAP_H_HALF);
        // 所有地标都是「基准坐标 × 同一个缩放系数」
        assert_eq!(
            BLUE_BASE,
            Vec3::new(-140.0 * MAP_SCALE, 0.0, -40.0 * MAP_SCALE)
        );
        assert_eq!(
            RED_BASE,
            Vec3::new(140.0 * MAP_SCALE, 0.0, -40.0 * MAP_SCALE)
        );
        assert_eq!(GOLD_MINE_POS, Vec3::new(0.0, 0.0, 8.0 * MAP_SCALE));
        assert_eq!(JUNGLE_SHOP_POS, Vec3::new(0.0, 0.0, 64.0 * MAP_SCALE));
        assert_eq!(RIVER_HALF_WIDTH, 8.0 * MAP_SCALE);
        assert_eq!(LANE_WIDTH, 13.0 * MAP_SCALE);
        assert!(BLUE_BASE.x.abs() < MAP_W_HALF && BLUE_BASE.z.abs() < MAP_H_HALF);
        // 路点也等比：相邻间距 = 28 × scale
        let lane = lane_waypoints(Team::Blue, 0);
        assert_eq!(
            (lane[1].x - lane[0].x).abs(),
            28.0 * MAP_SCALE,
            "所有路点必须同一缩放系数"
        );
    }

    #[test]
    fn longitudinal_river_slows_movement_but_lane_bridge_is_full_speed() {
        // 纵向河道浅滩（远离桥面）减速一半
        assert_eq!(terrain_speed_mult(Vec3::new(0.0, 0.0, s(40.0))), 0.5);
        assert_eq!(
            terrain_speed_mult(Vec3::new(s(3.0), 0.0, LANE_Z - 3.0 * BRIDGE_HALF_LEN)),
            0.5
        );
        // 兵线跨河的桥面全速
        assert_eq!(terrain_speed_mult(Vec3::new(0.0, 0.0, LANE_Z)), 1.0);
        assert_eq!(
            terrain_speed_mult(Vec3::new(s(2.0), 0.0, LANE_Z + s(4.0))),
            1.0
        );
        // 河道之外（左右野区）不减速
        assert_eq!(terrain_speed_mult(Vec3::new(s(40.0), 0.0, 0.0)), 1.0);
        // 镜像点速度一致：双方涉水代价对等
        for v in [
            Vec3::new(0.0, 0.0, s(40.0)),
            Vec3::new(s(6.0), 0.0, -s(70.0)),
            Vec3::new(-s(6.0), 0.0, s(20.0)),
            Vec3::new(s(40.0), 0.0, 0.0),
            Vec3::ZERO,
        ] {
            assert_eq!(
                terrain_speed_mult(v),
                terrain_speed_mult(mirror_fold(v)),
                "镜像点 {v:?} 的涉水代价不对等"
            );
        }
    }

    /// 涉水伤害规则：河道浅滩危险，桥面（兵线跨河处）安全，陆地安全；
    /// 且危险区与减速区严格一致、镜像对称。
    #[test]
    fn hazardous_water_is_the_slowed_river_except_the_bridge() {
        assert!(RIVER_HAZARD_DAMAGE > 0.0);
        assert_eq!(RIVER_HAZARD_PERIOD_TICKS, 30, "0.5 s 结算一次");
        // 河道浅滩（远离桥）：危险
        assert!(is_hazardous_river(Vec3::new(0.0, 0.0, s(40.0))));
        assert!(is_hazardous_river(GOLD_MINE_POS), "中央矿坑在危险水域里");
        // 桥面：安全（这是双方唯一不付出代价的过河点）
        assert!(!is_hazardous_river(Vec3::new(0.0, 0.0, LANE_Z)));
        assert!(!is_hazardous_river(Vec3::new(s(2.0), 0.0, LANE_Z + s(4.0))));
        // 河道之外：安全
        assert!(!is_hazardous_river(Vec3::new(s(40.0), 0.0, 0.0)));
        assert!(!is_hazardous_river(BLUE_BASE));

        // 危险区 == 减速区，且关于中轴镜像（双方涉水代价对等）
        for v in [
            Vec3::new(0.0, 0.0, s(40.0)),
            Vec3::new(s(6.0), 0.0, -s(70.0)),
            Vec3::new(-s(6.0), 0.0, s(20.0)),
            Vec3::new(0.0, 0.0, LANE_Z),
            Vec3::new(s(40.0), 0.0, 0.0),
        ] {
            assert_eq!(
                is_hazardous_river(v),
                terrain_speed_mult(v) < 1.0,
                "危险水域必须与减速区一致：{v:?}"
            );
            assert_eq!(
                is_hazardous_river(v),
                is_hazardous_river(mirror_fold(v)),
                "镜像点 {v:?} 的涉水代价不对等"
            );
        }
    }

    /// 经典 3v3 的识别特征：兵线水平横贯北侧，河道纵向与之正交，中点跨河有桥。
    #[test]
    fn lane_is_horizontal_and_river_is_orthogonal() {
        assert_eq!(LANE_COUNT, 1, "3v3 竞技场只能有一条兵线");
        let lane = lane_waypoints(Team::Blue, 0);
        assert_eq!(lane.first().copied(), Some(BLUE_BASE));
        assert_eq!(lane.last().copied(), Some(RED_BASE));
        for v in &lane {
            assert!((v.z - LANE_Z).abs() < 1e-4, "路点 {v:?} 离开了水平兵线");
            assert!(v.x.abs() <= MAP_W_HALF);
        }
        assert!(lane.contains(&Vec3::new(0.0, 0.0, LANE_Z)));

        // 河道方向 (0,1) 与兵线方向 (1,0) 正交
        let river_dir = Vec2::new(0.0, 1.0);
        let lane_dir = Vec2::new(1.0, 0.0);
        assert!(
            river_dir.dot(lane_dir).abs() < 1e-6,
            "河道必须与兵线走向正交"
        );
        let (a, b) = fold_line_endpoints();
        assert_eq!((a.x, a.z), (0.0, RIVER_START));
        assert_eq!((b.x, b.z), (0.0, MAP_H_HALF));
        assert_eq!(mirror_fold(a), a, "河道中线必须落在镜像轴上");
        assert_eq!(mirror_fold(b), b);
        assert!(is_bridge(Vec3::new(0.0, 0.0, LANE_Z)));
    }

    /// 参考布局的地标方位：共用商店在正下方中央、兵矿在左右下角、
    /// 中央矿坑在兵线中点正下方。
    #[test]
    fn classic_landmarks_are_in_the_right_places() {
        assert_eq!(JUNGLE_SHOP_POS.x, 0.0);
        assert!(
            JUNGLE_SHOP_POS.z > MAP_H_HALF * 0.7,
            "共用商店必须在地图正下方"
        );
        assert_eq!(
            JUNGLE_CAMPS
                .iter()
                .filter(|c| c.monster_type == JungleMonsterType::MusterWell)
                .count(),
            1,
            "野区商店双方共用，只能有一座"
        );
        let shrines: Vec<Vec3> = JUNGLE_CAMPS
            .iter()
            .filter(|c| c.monster_type == JungleMonsterType::LodeShrine)
            .map(|c| c.position)
            .collect();
        assert_eq!(shrines.len(), 2, "左右下角各一座兵矿");
        assert!(shrines.iter().any(|v| v.x < 0.0 && v.z > 0.0));
        assert!(shrines.iter().any(|v| v.x > 0.0 && v.z > 0.0));

        assert_eq!(GOLD_MINE_POS.x, 0.0);
        assert!(
            GOLD_MINE_POS.z > LANE_Z && GOLD_MINE_POS.z < MAP_H_HALF,
            "中央矿坑应在兵线中点正下方的野区里"
        );
        assert!(is_river(GOLD_MINE_POS), "中央矿坑应位于河道之上");
    }

    /// 兵线与野区之间必须真的有墙：**只在南侧**一道堤墙 + 对称坡道开口，
    /// 北侧（靠地图边界）不能有墙，且墙不与既有实体重叠。
    #[test]
    fn lane_walls_are_a_mirrored_barrier_with_passable_gaps() {
        assert_eq!(LANE_WALLS.len(), 4, "只在兵线靠野区的南侧设 4 段墙");
        assert_eq!(
            LANE_WALL_GAPS.len(),
            5,
            "两处基地 + 两处中段坡道 + 中央交汇口"
        );
        assert!(
            WALL_POST_SPACING < 2.0 * WALL_HALF_THICKNESS,
            "碰撞圆间距必须小于直径，否则屏障有缝"
        );
        assert!(WALL_SOUTH_Z > LANE_Z + LANE_WIDTH * 0.5);
        assert!(WALL_SOUTH_Z > RIVER_START && WALL_SOUTH_Z < MAP_H_HALF);
        assert!(
            LANE_WALLS.iter().all(|w| w.z > LANE_Z),
            "所有墙段都必须在兵线南侧，北侧不得有墙"
        );

        for w in LANE_WALLS {
            assert!(w.x0 < w.x1);
            assert!(w.x0 >= -MAP_W_HALF && w.x1 <= MAP_W_HALF);
            assert_eq!(w.z, WALL_SOUTH_Z);
            // 段与段之间必须有镜像配对（z 相同，x 反号）
            assert!(
                LANE_WALLS.iter().any(|o| o.z == w.z
                    && (o.x0 + w.x1).abs() < 1e-4
                    && (o.x1 + w.x0).abs() < 1e-4),
                "墙段 {w:?} 缺少左右镜像"
            );
            // 墙段不得压在任何坡道开口上
            assert!(
                !is_lane_wall_gap((w.x0 + w.x1) * 0.5),
                "墙段 {w:?} 压住了坡道开口"
            );
        }
        for g in LANE_WALL_GAPS {
            assert!(g.x0 < g.x1 && g.x0 >= -MAP_W_HALF && g.x1 <= MAP_W_HALF);
            assert!(
                LANE_WALL_GAPS
                    .iter()
                    .any(|o| (o.x0 + g.x1).abs() < 1e-4 && (o.x1 + g.x0).abs() < 1e-4),
                "坡道开口 {g:?} 缺少左右镜像"
            );
        }
        assert!(is_lane_wall_gap(0.0), "中央河道交汇处必须能从兵线进野区");
        assert!(is_lane_wall_gap(BLUE_BASE.x) && is_lane_wall_gap(RED_BASE.x));

        // 墙不能和塔 / 营地 / 草丛 / 源核重叠，否则静态碰撞体会互相打架。
        let colliders = wall_colliders();
        assert!(!colliders.is_empty());
        let mut fixtures: Vec<(Vec3, f32)> = TURRET_POSITIONS
            .iter()
            .map(|t| (t.position, 2.5))
            .chain(JUNGLE_CAMPS.iter().map(|c| (c.position, 2.0)))
            .chain(BUSHES.iter().map(|b| (b.center, b.radius)))
            .collect();
        fixtures.push((BLUE_SOURCE_CORE, 3.5));
        fixtures.push((RED_SOURCE_CORE, 3.5));
        for (fp, fr) in fixtures {
            for (wp, wr) in &colliders {
                let d = Vec2::new(fp.x - wp.x, fp.z - wp.z).length();
                assert!(d > fr + wr, "墙体与地图实体重叠：{fp:?} / {wp:?}");
            }
        }

        // 坡道开口中点必须没有任何墙碰撞圆
        for g in LANE_WALL_GAPS {
            let mid = (g.x0 + g.x1) * 0.5;
            assert!(
                colliders
                    .iter()
                    .all(|(c, r)| Vec2::new(mid - c.x, WALL_SOUTH_Z - c.z).length() > *r),
                "坡道 x={mid} 被墙体堵死"
            );
        }
    }

    /// 墙碰撞圆必须连续无缝：每段相邻圆心距 ≤ 直径，且端点各自封口。
    #[test]
    fn wall_colliders_form_a_continuous_barrier() {
        for w in LANE_WALLS {
            let mut pts: Vec<f32> = wall_colliders()
                .iter()
                .filter(|(c, _)| c.z == w.z && c.x >= w.x0 && c.x <= w.x1)
                .map(|(c, _)| c.x)
                .collect();
            pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            pts.dedup();
            assert!(pts.len() >= 2, "墙段 {w:?} 至少两个碰撞圆");
            assert!((pts[0] - w.x0).abs() < 1e-3 && (pts[pts.len() - 1] - w.x1).abs() < 1e-3);
            for pair in pts.windows(2) {
                assert!(
                    pair[1] - pair[0] <= 2.0 * WALL_HALF_THICKNESS + 1e-3,
                    "墙段 {w:?} 存在缝隙 {:.3}",
                    pair[1] - pair[0]
                );
            }
        }
    }

    /// 公平性不变量：整张地图关于中轴 `x = 0` **左右镜像对称**。
    #[test]
    fn map_is_mirror_symmetric_about_center_axis() {
        // 1) 基地 / 源核互为镜像
        assert_eq!(mirror_fold(BLUE_BASE), RED_BASE, "红蓝基地必须镜像对称");
        assert_eq!(
            mirror_fold(BLUE_SOURCE_CORE),
            RED_SOURCE_CORE,
            "红蓝源核必须镜像对称"
        );

        // 2) 兵线镜像对称：第 i 个路点的镜像 = 第 (n-1-i) 个路点
        let lane = lane_waypoints(Team::Blue, 0);
        for (i, v) in lane.iter().enumerate() {
            assert_eq!(
                mirror_fold(*v),
                lane[lane.len() - 1 - i],
                "兵线第 {i} 个路点未镜像对称"
            );
        }

        // 3) 哨塔：红塔集合 = 蓝塔集合的镜像（类型也要对应），每队 5 座
        let blues: Vec<(Vec3, TurretType)> = TURRET_POSITIONS
            .iter()
            .filter(|t| t.team == Team::Blue)
            .map(|t| (t.position, t.turret_type))
            .collect();
        let reds: Vec<(Vec3, TurretType)> = TURRET_POSITIONS
            .iter()
            .filter(|t| t.team == Team::Red)
            .map(|t| (t.position, t.turret_type))
            .collect();
        assert_eq!(blues.len(), 5);
        assert_eq!(blues.len(), reds.len(), "红蓝哨塔数量应对等");
        for (v, kind) in &blues {
            assert!(
                reds.iter()
                    .any(|(rv, rk)| *rv == mirror_fold(*v) && rk == kind),
                "蓝方哨塔 {v:?} 缺少镜像红方哨塔"
            );
        }

        // 4) 野怪营地：非中立营地都有阵营相反的镜像；中立营地在中轴上
        for c in JUNGLE_CAMPS {
            if c.team == Team::Neutral {
                assert_eq!(mirror_fold(c.position), c.position, "共用目标应在镜像轴上");
            } else {
                let mirror = mirror_fold(c.position);
                assert!(
                    JUNGLE_CAMPS
                        .iter()
                        .any(|o| o.position == mirror && o.team != c.team),
                    "营地 {c:?} 缺少镜像对称的另一侧营地"
                );
            }
        }

        // 5) 草丛：每丛都有镜像且半径相同
        for b in BUSHES {
            assert!(
                BUSHES.iter().any(|o| {
                    o.center == mirror_fold(b.center) && (o.radius - b.radius).abs() < 1e-3
                }),
                "草丛 {:?} 缺少镜像对称的另一丛",
                b.center
            );
        }

        // 6) 中央目标与共用商店位于镜像轴上，是双方共享的必争点
        assert_eq!(mirror_fold(GOLD_MINE_POS), GOLD_MINE_POS);
        assert_eq!(mirror_fold(JUNGLE_SHOP_POS), JUNGLE_SHOP_POS);
    }
}
