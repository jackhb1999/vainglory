//! 数据驱动的技能定义（AbilityDef）与技能注册表。
//!
//! 设计原则：
//! - 技能是纯数据（`AbilityDef`），不携带运行期状态；运行期状态（冷却、蓝耗）存在
//!   英雄身上的 `HeroAbilities` 组件里。
//! - 命中裁决全部在服务端进行（`ability_cast_system` / `projectile_system`），
//!   客户端只负责按快照渲染弹道与受击反馈。

use crate::components::DamageType;

/// 施法类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastType {
    /// 瞬发：在施法者身上立即结算（如自我治疗 / 周身爆发）
    Instant,
    /// 弹道：朝瞄准方向发射一枚弹体，飞行中命中敌人
    Projectile,
    /// 指向：在"施法者 + 瞄准方向 * 射程"处形成范围伤害（AoE）
    PointTarget,
}

/// 单个技能的数据定义（纯数据，不含运行期状态）
#[derive(Clone, Debug)]
pub struct AbilityDef {
    pub id: u8,
    pub name: String,
    /// 冷却（秒）
    pub cooldown: f32,
    /// 蓝耗
    pub mana_cost: f32,
    /// 施法距离 / 弹道射程
    pub range: f32,
    pub cast_type: CastType,
    pub damage_type: DamageType,
    /// 基础伤害
    pub base_damage: f32,
    /// 加成系数：物理吃 attack_damage，法术吃 ability_power
    pub scaling: f32,
    /// 弹道速度（仅 Projectile）
    pub projectile_speed: f32,
    /// 指向/瞬发的作用半径（AoE）
    pub aoe_radius: f32,
    /// 瞬发治疗量（仅 Instant 治疗类）
    pub heal_amount: f32,
}

impl AbilityDef {
    /// 根据施法者属性计算最终伤害
    pub fn damage(&self, attack_damage: f32, ability_power: f32) -> f32 {
        let stat = match self.damage_type {
            DamageType::Magical => ability_power,
            _ => attack_damage,
        };
        self.base_damage + self.scaling * stat
    }
}

/// 按技能 id 获取定义（未注册 id 返回占位定义，避免越界）
pub fn get_ability_def(id: u8) -> AbilityDef {
    match id {
        // 0: 火焰弹 —— 直线弹道物理伤害
        0 => AbilityDef {
            id: 0,
            name: "火焰弹".into(),
            cooldown: 4.0,
            mana_cost: 50.0,
            range: 35.0,
            cast_type: CastType::Projectile,
            damage_type: DamageType::Physical,
            base_damage: 140.0,
            scaling: 0.6,
            projectile_speed: 45.0,
            aoe_radius: 1.5,
            heal_amount: 0.0,
        },
        // 1: 寒冰新星 —— 指向范围魔法伤害
        1 => AbilityDef {
            id: 1,
            name: "寒冰新星".into(),
            cooldown: 8.0,
            mana_cost: 100.0,
            range: 18.0,
            cast_type: CastType::PointTarget,
            damage_type: DamageType::Magical,
            base_damage: 220.0,
            scaling: 0.7,
            projectile_speed: 0.0,
            aoe_radius: 6.0,
            heal_amount: 0.0,
        },
        // 2: 能量光束 —— 高速弹道魔法伤害
        2 => AbilityDef {
            id: 2,
            name: "能量光束".into(),
            cooldown: 6.0,
            mana_cost: 80.0,
            range: 40.0,
            cast_type: CastType::Projectile,
            damage_type: DamageType::Magical,
            base_damage: 180.0,
            scaling: 0.8,
            projectile_speed: 60.0,
            aoe_radius: 1.5,
            heal_amount: 0.0,
        },
        // 3: 自我治疗 —— 瞬发回复生命
        3 => AbilityDef {
            id: 3,
            name: "自我治疗".into(),
            cooldown: 10.0,
            mana_cost: 120.0,
            range: 0.0,
            cast_type: CastType::Instant,
            damage_type: DamageType::True,
            base_damage: 0.0,
            scaling: 0.0,
            projectile_speed: 0.0,
            aoe_radius: 0.0,
            heal_amount: 350.0,
        },
        _ => AbilityDef {
            id,
            name: "未知技能".into(),
            cooldown: 1.0,
            mana_cost: 0.0,
            range: 0.0,
            cast_type: CastType::Instant,
            damage_type: DamageType::True,
            base_damage: 0.0,
            scaling: 0.0,
            projectile_speed: 0.0,
            aoe_radius: 0.0,
            heal_amount: 0.0,
        },
    }
}

/// 英雄默认携带的 4 个技能槽对应的技能 id（与客户端按键 1-4 对应）
pub fn default_hero_abilities() -> [u8; 4] {
    [0, 1, 2, 3]
}
