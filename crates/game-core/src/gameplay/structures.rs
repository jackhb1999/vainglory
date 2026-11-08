//! 哨塔护盾、逐塔减伤、脱战回复与仇恨（`GX-CBT-2026-001` §14）。

use bevy::prelude::*;

/// 射程内无己方战偶时的伤害倍率（60% 减伤）
pub const NO_CONSTRUCT_MULTIPLIER: f32 = 0.4;
/// 前置塔未倒时的伤害倍率（50% 减伤）
pub const FRONT_TOWER_MULTIPLIER: f32 = 0.5;
/// 切换目标后的前 2 次攻击伤害倍率
pub const TARGET_SWITCH_ATTACKS: u8 = 2;
pub const TARGET_SWITCH_MULTIPLIER: f32 = 0.5;
/// 脱战 8 s 后每秒回复 2% 最大生命
pub const OUT_OF_COMBAT_TICKS: u32 = 480;
pub const REGEN_PER_TICK: f32 = 0.02 / 60.0;

#[derive(Component, Clone, Debug)]
pub struct StructureDefense {
    pub front_tower_alive: bool,
    pub friendly_constructs_in_range: u32,
    pub out_of_combat_ticks: u32,
    pub max_health: f32,
    pub health: f32,
    /// 连续攻击同一目标的次数（切换目标时清零）
    pub attacks_on_current_target: u8,
    pub is_source_core: bool,
}

impl StructureDefense {
    pub fn new(max_health: f32, is_source_core: bool) -> Self {
        Self {
            front_tower_alive: false,
            friendly_constructs_in_range: 0,
            out_of_combat_ticks: 0,
            max_health,
            health: max_health,
            attacks_on_current_target: 0,
            is_source_core,
        }
    }

    pub fn notify_target_switch(&mut self) {
        self.attacks_on_current_target = 0;
    }
}

/// 入伤倍率（§14.2）：两条减伤规则相乘；源核不受"无战偶"规则影响。
pub fn incoming_multiplier(def: &StructureDefense) -> f32 {
    let mut m = 1.0;
    if !def.is_source_core && def.friendly_constructs_in_range == 0 {
        m *= NO_CONSTRUCT_MULTIPLIER;
    }
    if def.front_tower_alive {
        m *= FRONT_TOWER_MULTIPLIER;
    }
    m
}

/// 结算一次对建筑的伤害；返回实际扣血量。
pub fn apply_incoming(def: &mut StructureDefense, raw: f32) -> f32 {
    let mut dealt = raw.max(0.0) * incoming_multiplier(def);
    if def.attacks_on_current_target < TARGET_SWITCH_ATTACKS {
        dealt *= TARGET_SWITCH_MULTIPLIER;
    }
    def.attacks_on_current_target = def.attacks_on_current_target.saturating_add(1);
    def.out_of_combat_ticks = 0;
    def.health = (def.health - dealt).max(0.0);
    dealt
}

/// 脱战回复；返回回复后的生命值。
pub fn tick_out_of_combat(def: &mut StructureDefense) -> f32 {
    def.out_of_combat_ticks = def.out_of_combat_ticks.saturating_add(1);
    if def.out_of_combat_ticks >= OUT_OF_COMBAT_TICKS {
        def.health = (def.health + def.max_health * REGEN_PER_TICK).min(def.max_health);
    }
    def.health
}

/// 仇恨优先级（§14.3）：1 正在攻击己方英雄的敌方英雄 > 2 最近战偶 > 3 最近敌方英雄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreatClass {
    AttackingFriendlyHero = 0,
    Construct = 1,
    Hero = 2,
}

pub fn choose_threat(candidates: &[(ThreatClass, f32)]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (a.0 as u8)
                .cmp(&(b.0 as u8))
                .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cbt_11_tower_reductions() {
        let mut def = StructureDefense::new(5000.0, false);
        assert_eq!(incoming_multiplier(&def), NO_CONSTRUCT_MULTIPLIER);
        def.friendly_constructs_in_range = 1;
        assert_eq!(incoming_multiplier(&def), 1.0);
        def.front_tower_alive = true;
        assert_eq!(incoming_multiplier(&def), FRONT_TOWER_MULTIPLIER);
        def.friendly_constructs_in_range = 0;
        assert_eq!(incoming_multiplier(&def), 0.2);
    }

    #[test]
    fn tc_cbt_11_source_core_ignores_no_construct_rule() {
        let mut def = StructureDefense::new(12000.0, true);
        assert_eq!(incoming_multiplier(&def), 1.0);
        def.front_tower_alive = true;
        assert_eq!(incoming_multiplier(&def), FRONT_TOWER_MULTIPLIER);
    }

    #[test]
    fn first_two_attacks_after_target_switch_are_halved() {
        let mut def = StructureDefense::new(5000.0, false);
        def.friendly_constructs_in_range = 1;
        let d1 = apply_incoming(&mut def, 100.0);
        let d2 = apply_incoming(&mut def, 100.0);
        let d3 = apply_incoming(&mut def, 100.0);
        assert_eq!(d1, 50.0);
        assert_eq!(d2, 50.0);
        assert_eq!(d3, 100.0);
        def.notify_target_switch();
        assert_eq!(apply_incoming(&mut def, 100.0), 50.0);
    }

    #[test]
    fn out_of_combat_regen_starts_after_480_ticks() {
        let mut def = StructureDefense::new(1000.0, false);
        def.health = 500.0;
        for _ in 0..OUT_OF_COMBAT_TICKS - 1 {
            tick_out_of_combat(&mut def);
        }
        assert_eq!(def.health, 500.0);
        tick_out_of_combat(&mut def);
        assert!(def.health > 500.0);
        assert!(def.health <= 1000.0);
    }

    #[test]
    fn threat_priority_prefers_hero_attacking_friendly() {
        let candidates = [
            (ThreatClass::Hero, 1.0),
            (ThreatClass::Construct, 2.0),
            (ThreatClass::AttackingFriendlyHero, 9.0),
        ];
        assert_eq!(choose_threat(&candidates), Some(2));
    }
}
