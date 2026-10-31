//! Halcyon Fold 地图静态数据

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// 地图常量
pub const MAP_WIDTH: f32 = 128.0;
pub const MAP_HEIGHT: f32 = 128.0;
pub const LANE_WIDTH: f32 = 8.0;
pub const RIVER_WIDTH: f32 = 12.0;

/// 团队基地位置
pub const BLUE_BASE: Vec3 = Vec3::new(-54.0, 0.0, -54.0);
pub const RED_BASE: Vec3 = Vec3::new(54.0, 0.0, 54.0);

/// Vain Crystal 位置
pub const BLUE_VAIN_CRYSTAL: Vec3 = Vec3::new(-56.0, 0.0, -56.0);
pub const RED_VAIN_CRYSTAL: Vec3 = Vec3::new(56.0, 0.0, 56.0);

/// 防御塔位置
pub const TURRET_POSITIONS: &[TurretPosition] = &[
    // Blue 方塔
    TurretPosition { team: Team::Blue, turret_type: TurretType::Outer, position: Vec3::new(-32.0, 0.0, -32.0) }, // Top outer
    TurretPosition { team: Team::Blue, turret_type: TurretType::Inner, position: Vec3::new(-16.0, 0.0, -16.0) }, // Top inner
    TurretPosition { team: Team::Blue, turret_type: TurretType::Base, position: Vec3::new(-8.0, 0.0, -8.0) },   // Top base
    
    TurretPosition { team: Team::Blue, turret_type: TurretType::Outer, position: Vec3::new(-32.0, 0.0, 32.0) }, // Bot outer
    TurretPosition { team: Team::Blue, turret_type: TurretType::Inner, position: Vec3::new(-16.0, 0.0, 16.0) }, // Bot inner
    TurretPosition { team: Team::Blue, turret_type: TurretType::Base, position: Vec3::new(-8.0, 0.0, 8.0) },   // Bot base
    
    // Red 方塔
    TurretPosition { team: Team::Red, turret_type: TurretType::Outer, position: Vec3::new(32.0, 0.0, 32.0) },   // Top outer
    TurretPosition { team: Team::Red, turret_type: TurretType::Inner, position: Vec3::new(16.0, 0.0, 16.0) },   // Top inner
    TurretPosition { team: Team::Red, turret_type: TurretType::Base, position: Vec3::new(8.0, 0.0, 8.0) },     // Top base
    
    TurretPosition { team: Team::Red, turret_type: TurretType::Outer, position: Vec3::new(32.0, 0.0, -32.0) },  // Bot outer
    TurretPosition { team: Team::Red, turret_type: TurretType::Inner, position: Vec3::new(16.0, 0.0, -16.0) },  // Bot inner
    TurretPosition { team: Team::Red, turret_type: TurretType::Base, position: Vec3::new(8.0, 0.0, -8.0) },    // Bot base
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TurretPosition {
    pub team: Team,
    pub turret_type: TurretType,
    pub position: Vec3,
}

/// 野怪营地位置
pub const JUNGLE_CAMPS: &[JungleCamp] = &[
    // Blue 侧野区
    JungleCamp { monster_type: JungleMonsterType::Treant, position: Vec3::new(-48.0, 0.0, -16.0), team: Team::Blue },
    JungleCamp { monster_type: JungleMonsterType::CrystalSentry, position: Vec3::new(-40.0, 0.0, 8.0), team: Team::Blue },
    JungleCamp { monster_type: JungleMonsterType::GoldToad, position: Vec3::new(-24.0, 0.0, 24.0), team: Team::Blue },
    
    // Red 侧野区
    JungleCamp { monster_type: JungleMonsterType::Treant, position: Vec3::new(48.0, 0.0, 16.0), team: Team::Red },
    JungleCamp { monster_type: JungleMonsterType::CrystalSentry, position: Vec3::new(40.0, 0.0, -8.0), team: Team::Red },
    JungleCamp { monster_type: JungleMonsterType::GoldToad, position: Vec3::new(24.0, 0.0, -24.0), team: Team::Red },
    
    // 河道中立
    JungleCamp { monster_type: JungleMonsterType::MinionMine, position: Vec3::new(0.0, 0.0, -40.0), team: Team::Blue }, // 实际上中立
    JungleCamp { monster_type: JungleMonsterType::GoldMine, position: Vec3::new(0.0, 0.0, 40.0), team: Team::Red },   // 实际上中立
    JungleCamp { monster_type: JungleMonsterType::Kraken, position: Vec3::new(0.0, 0.0, 0.0), team: Team::Blue },    // 实际上中立
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct JungleCamp {
    pub monster_type: JungleMonsterType,
    pub position: Vec3,
    pub team: Team, // 初始归属（中立用任意）
}

/// 小兵生成路径点
pub fn minion_paths() -> Vec<MinionPath> {
    vec![
    // Top lane
    MinionPath { team: Team::Blue, waypoints: vec![
        Vec3::new(-48.0, 0.0, -48.0),
        Vec3::new(-32.0, 0.0, -32.0),
        Vec3::new(-16.0, 0.0, -16.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(16.0, 0.0, 16.0),
        Vec3::new(32.0, 0.0, 32.0),
        Vec3::new(48.0, 0.0, 48.0),
    ]},
    MinionPath { team: Team::Red, waypoints: vec![
        Vec3::new(48.0, 0.0, 48.0),
        Vec3::new(32.0, 0.0, 32.0),
        Vec3::new(16.0, 0.0, 16.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(-16.0, 0.0, -16.0),
        Vec3::new(-32.0, 0.0, -32.0),
        Vec3::new(-48.0, 0.0, -48.0),
    ]},
    
    // Bot lane (镜像)
    MinionPath { team: Team::Blue, waypoints: vec![
        Vec3::new(-48.0, 0.0, 48.0),
        Vec3::new(-32.0, 0.0, 32.0),
        Vec3::new(-16.0, 0.0, 16.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(16.0, 0.0, -16.0),
        Vec3::new(32.0, 0.0, -32.0),
        Vec3::new(48.0, 0.0, -48.0),
    ]},
    MinionPath { team: Team::Red, waypoints: vec![
        Vec3::new(48.0, 0.0, -48.0),
        Vec3::new(32.0, 0.0, -32.0),
        Vec3::new(16.0, 0.0, -16.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(-16.0, 0.0, 16.0),
        Vec3::new(-32.0, 0.0, 32.0),
        Vec3::new(-48.0, 0.0, 48.0),
    ]},
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinionPath {
    pub team: Team,
    pub waypoints: Vec<Vec3>,
}

/// 草丛区域（用于隐身/视野）
pub const BUSHES: &[Bush] = &[
    Bush { center: Vec3::new(-24.0, 0.0, -8.0), radius: 4.0 },
    Bush { center: Vec3::new(24.0, 0.0, 8.0), radius: 4.0 },
    Bush { center: Vec3::new(-8.0, 0.0, 24.0), radius: 4.0 },
    Bush { center: Vec3::new(8.0, 0.0, -24.0), radius: 4.0 },
    Bush { center: Vec3::new(0.0, 0.0, -32.0), radius: 6.0 }, // 河道草
    Bush { center: Vec3::new(0.0, 0.0, 32.0), radius: 6.0 },  // 河道草
];

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bush {
    pub center: Vec3,
    pub radius: f32,
}

/// 视野系统常量
pub const HERO_SIGHT_RANGE: f32 = 14.0;
pub const WARD_SIGHT_RANGE: f32 = 10.0;
pub const TURRET_SIGHT_RANGE: f32 = 16.0;

/// 导入组件定义避免循环依赖
use crate::components::{Team, TurretType};
use crate::net::JungleMonsterType;