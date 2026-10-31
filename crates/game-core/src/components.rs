//! 游戏实体组件定义

use bevy::prelude::*;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use crate::net::JungleMonsterType;

/// 阵营
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub enum Team {
    Blue,
    Red,
    /// 中立（如训练假人）：双方均可攻击
    Neutral,
}

/// 伤害类型（决定减免公式）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageType {
    /// 物理：受护甲减免
    Physical,
    /// 魔法：受魔抗减免
    Magical,
    /// 真实：无视减免
    True,
}

/// 英雄基础移速（服务端与客户端预测必须保持一致，否则预测回滚会反复硬修正）
pub const HERO_BASE_MOVE_SPEED: f32 = 8.0;

/// 英雄角色组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Hero {
    pub name: String,
    pub level: u8,
    pub experience: u32,
    pub gold: u32,
    pub max_health: f32,
    pub health: f32,
    pub max_energy: f32,
    pub energy: f32,
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub attack_range: f32,
}

impl Default for Hero {
    fn default() -> Self {
        Self {
            name: "Hero".to_string(),
            level: 1,
            experience: 0,
            gold: 0,
            max_health: 1000.0,
            health: 1000.0,
            max_energy: 500.0,
            energy: 500.0,
            attack_damage: 60.0,
            ability_power: 0.0,
            armor: 20.0,
            magic_resist: 30.0,
            attack_speed: 1.0,
            move_speed: 3.5,
            attack_range: 5.5,
        }
    }
}

/// 小兵组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Minion {
    pub minion_type: MinionType,
    pub max_health: f32,
    pub health: f32,
    pub attack_damage: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub move_speed: f32,
    pub attack_range: f32,
    pub gold_reward: u32,
    pub exp_reward: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinionType {
    Melee,
    Ranged,
    Siege,
    Super,
}

/// 防御塔组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Turret {
    pub turret_type: TurretType,
    pub max_health: f32,
    pub health: f32,
    pub attack_damage: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_range: f32,
    pub attack_speed: f32,
    pub last_attack_tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq,Serialize, Deserialize)]
pub enum TurretType {
    Outer,
    Inner,
    Base,
    VainCrystal,
}

/// 野怪组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct JungleMonster {
    pub monster_type: JungleMonsterType,
    pub max_health: f32,
    pub health: f32,
    pub attack_damage: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub move_speed: f32,
    pub attack_range: f32,
    pub gold_reward: u32,
    pub exp_reward: u32,
    pub aggro_range: f32,
    pub leash_range: f32,
    pub spawn_tick: u64,
    pub last_attacker: Option<NetId>,
}




/// 技能组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub ability_type: AbilityType,
    pub cooldown: f32,
    pub current_cooldown: f32,
    pub mana_cost: f32,
    pub range: f32,
    pub damage: f32,
    pub scaling: AbilityScaling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbilityType {
    Basic,
    Heroic,
    Ultimate,
    Talent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbilityScaling {
    pub attack_damage_ratio: f32,
    pub ability_power_ratio: f32,
}

/// 英雄的技能槽运行状态（冷却、所携带技能）。技能静态数据来自 `game_core::abilities`。
#[derive(Component, Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeroAbilities {
    /// 各槽位剩余冷却（秒）
    pub cooldowns: [f32; 4],
    /// 各槽位对应的技能 id（来自 `default_hero_abilities` 或升级/换装后变更）
    pub ability_ids: [u8; 4],
}

/// 弹道实体（服务端权威，命中裁决在服务端）
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub owner: NetId,
    pub team: Team,
    pub damage: f32,
    pub damage_type: DamageType,
    /// 单位方向（已归一化，y=0）
    pub direction: Vec3,
    pub speed: f32,
    /// 已飞行距离
    pub traveled: f32,
    /// 最大射程
    pub max_range: f32,
    /// 命中判定半径
    pub radius: f32,
    /// 标记待销毁（下一帧由 projectile_system 回收）
    pub dead: bool,
}

/// 待结算的指向/瞬发 AoE 伤害请求（挂在被命中实体上，由 resolve_pending_hits 统一结算）
#[derive(Component, Clone, Debug)]
pub struct PendingHit {
    pub attacker: Entity,
    pub damage: f32,
    pub damage_type: DamageType,
}

/// 训练假人标记：可被任意阵营技能命中，用于单人测试，不会真正死亡。
#[derive(Component, Clone, Debug, Default)]
pub struct TrainingDummy;

/// 物品组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub item_id: u32,
    pub name: String,
    pub cost: u32,
    pub stats: ItemStats,
    pub passive: Option<String>,
    pub active: Option<ActiveItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ItemStats {
    pub health: f32,
    pub energy: f32,
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub cooldown_reduction: f32,
    pub lifesteal: f32,
    pub spell_vamp: f32,
    pub critical_chance: f32,
    pub critical_damage: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveItem {
    pub cooldown: f32,
    pub current_cooldown: f32,
    pub effect: String,
}

/// 玩家输入组件（客户端预测用）
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct PlayerInput {
    pub move_dir: Vec2,
    pub aim_dir: Vec2,
    pub ability_mask: u8, // bitmask for abilities 0-3
    pub tick: u64,
}

impl Default for PlayerInput {
    fn default() -> Self {
        Self {
            move_dir: Vec2::ZERO,
            aim_dir: Vec2::X,
            ability_mask: 0,
            tick: 0,
        }
    }
}

/// 网络实体 ID（服务端权威分配）
#[derive(Component,  Encode, Decode,Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetId(pub u64);

/// 位置同步组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct TransformSync {
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub last_update_tick: u64,
}

impl Default for TransformSync {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            last_update_tick: 0,
        }
    }
}

/// 仇恨组件
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Aggro {
    pub target: Option<NetId>,
    pub threat_table: Vec<(NetId, f32)>,
    pub last_update_tick: u64,
}

impl Default for Aggro {
    fn default() -> Self {
        Self {
            target: None,
            threat_table: Vec::new(),
            last_update_tick: 0,
        }
    }
}

/// 选择器（用于点击选中）
#[derive(Component, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Selectable {
    pub radius: f32,
    pub height: f32,
}

/// 标记：本地玩家控制的实体
#[derive(Component, Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct LocalPlayer;

/// 标记：服务端权威实体
#[derive(Component, Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ServerAuthority;

/// 标记：需要插值的实体
#[derive(Component, Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Interpolated;

/// 标记：预测实体（客户端预测）
#[derive(Component, Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Predicted;