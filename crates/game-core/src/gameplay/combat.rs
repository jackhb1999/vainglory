//! 属性扩展、状态效果与完整伤害结算（`GX-CBT-2026-001` §4 / §5 / §8）。
//!
//! 本模块全部为纯逻辑（除 Bevy `Component` 派生外无 ECS 依赖），
//! 由服务端系统驱动，客户端不做任何裁决。

use crate::components::{DamageType, Hero};
use bevy::prelude::*;

pub const EPS: f32 = 1e-4;
/// 控制递减窗口：8 s = 480 tick（§8.2）
pub const CONTROL_WINDOW_TICKS: u64 = 480;
/// 第 1/2/3/4 次及以后控制的时长系数（§8.2）
pub const CONTROL_DR: [f32; 4] = [1.0, 0.65, 0.42, 0.28];
/// 灼烧每 0.5 s 结算一次
pub const BURN_PERIOD_TICKS: u32 = 30;

// ---------------------------------------------------------------------------
// 二级属性 / 生命池 / 回复
// ---------------------------------------------------------------------------

/// 二级属性：由装备与状态提供，默认全部为 0（`crit_multiplier` 默认 2.0）。
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CombatStats {
    pub crit_chance: f32,
    pub crit_multiplier: f32,
    pub flat_armor_pen: f32,
    pub pct_armor_pen: f32,
    pub flat_mr_pen: f32,
    pub pct_mr_pen: f32,
    pub lifesteal: f32,
    pub cooldown_reduction: f32,
    pub damage_amp: f32,
    pub damage_reduction: f32,
}

impl Default for CombatStats {
    fn default() -> Self {
        Self {
            crit_chance: 0.0,
            crit_multiplier: 2.0,
            flat_armor_pen: 0.0,
            pct_armor_pen: 0.0,
            flat_mr_pen: 0.0,
            pct_mr_pen: 0.0,
            lifesteal: 0.0,
            cooldown_reduction: 0.0,
            damage_amp: 0.0,
            damage_reduction: 0.0,
        }
    }
}

impl CombatStats {
    pub fn clamp(&mut self) {
        self.crit_chance = self.crit_chance.clamp(0.0, 1.0);
        self.pct_armor_pen = self.pct_armor_pen.clamp(0.0, 0.4);
        self.pct_mr_pen = self.pct_mr_pen.clamp(0.0, 0.4);
        self.lifesteal = self.lifesteal.clamp(0.0, 0.4);
        self.cooldown_reduction = self.cooldown_reduction.clamp(0.0, 0.4);
        self.damage_amp = self.damage_amp.clamp(-0.5, 0.5);
        self.damage_reduction = self.damage_reduction.clamp(0.0, 0.5);
        self.crit_multiplier = self.crit_multiplier.max(1.0);
    }

    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

/// 护盾池：先于生命吸收伤害（§5.2 步骤 5~7）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShieldPool {
    pub value: f32,
}

/// 每秒回复（§4.1）。
#[derive(Component, Clone, Copy, Debug)]
pub struct RegenStats {
    pub hp_regen: f32,
    pub en_regen: f32,
}

impl Default for RegenStats {
    fn default() -> Self {
        Self {
            hp_regen: 4.0,
            en_regen: 6.0,
        }
    }
}

/// 单 tick 回复量（60 Hz）。生命/能量按成长量补足，不回满（§4.4）。
pub fn apply_regen(hero: &mut Hero, regen: &RegenStats) {
    hero.health = (hero.health + regen.hp_regen / 60.0).min(hero.max_health);
    hero.energy = (hero.energy + regen.en_regen / 60.0).min(hero.max_energy);
}

// ---------------------------------------------------------------------------
// 状态效果（§8）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusKind {
    Stun,
    Root,
    Slow,
    Silence,
    Blind,
    Shield,
    Invulnerable,
    Burn,
}

impl StatusKind {
    /// 快照 `CH_STATUS` 位序（协议 v3，客户端按同一顺序还原图标）。
    pub const fn as_bit(self) -> u8 {
        self as u8
    }

    /// 参与控制递减的硬控（软控 Slow 不参与）。
    pub fn is_control(self) -> bool {
        matches!(self, Self::Stun | Self::Root | Self::Silence | Self::Blind)
    }

    /// 驱散优先级数值越小越先被移除（§8.2）。
    pub fn dispel_priority(self) -> u8 {
        match self {
            Self::Invulnerable => 0,
            Self::Shield => 1,
            Self::Stun | Self::Root | Self::Silence | Self::Blind => 2,
            Self::Slow => 3,
            Self::Burn => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    pub kind: StatusKind,
    pub strength: f32,
    pub remaining_ticks: u32,
    pub source_net_id: u64,
}

#[derive(Component, Clone, Debug, Default)]
pub struct StatusEffects {
    pub effects: Vec<EffectInstance>,
    /// 当前 8 s 窗口内已施加的控制次数
    pub control_chain: u8,
    pub last_control_tick: u64,
}

impl StatusEffects {
    /// 压缩成 `CH_STATUS` 的位掩码，供快照下发。
    pub fn status_mask(&self) -> u32 {
        self.effects
            .iter()
            .fold(0u32, |m, e| m | (1u32 << e.kind.as_bit()))
    }

    pub fn has(&self, kind: StatusKind) -> bool {
        self.effects.iter().any(|e| e.kind == kind)
    }

    /// 同类效果不叠乘，取强度最大者。
    pub fn strength(&self, kind: StatusKind) -> f32 {
        self.effects
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.strength)
            .fold(0.0, f32::max)
    }

    pub fn is_stunned(&self) -> bool {
        self.has(StatusKind::Stun)
    }
    pub fn is_rooted(&self) -> bool {
        self.has(StatusKind::Stun) || self.has(StatusKind::Root)
    }
    pub fn is_silenced(&self) -> bool {
        self.has(StatusKind::Stun) || self.has(StatusKind::Silence)
    }
    pub fn is_blind(&self) -> bool {
        self.has(StatusKind::Blind)
    }
    pub fn is_invulnerable(&self) -> bool {
        self.has(StatusKind::Invulnerable)
    }

    /// 施加效果：同类取最强 + 时长刷新（不累加），硬控按控制递减缩时长。
    /// 返回实际生效的时长（tick）。
    pub fn apply(&mut self, mut effect: EffectInstance, tick: u64) -> u32 {
        if effect.kind.is_control() {
            if tick.saturating_sub(self.last_control_tick) > CONTROL_WINDOW_TICKS {
                self.control_chain = 0;
            }
            let factor = CONTROL_DR[(self.control_chain as usize).min(3)];
            effect.remaining_ticks =
                (effect.remaining_ticks as f32 * factor).ceil().max(1.0) as u32;
            self.control_chain = self.control_chain.saturating_add(1);
            self.last_control_tick = tick;
        }

        if let Some(existing) = self.effects.iter_mut().find(|e| e.kind == effect.kind) {
            existing.strength = existing.strength.max(effect.strength);
            existing.remaining_ticks = existing.remaining_ticks.max(effect.remaining_ticks);
            existing.source_net_id = effect.source_net_id;
            existing.remaining_ticks
        } else {
            let ticks = effect.remaining_ticks;
            self.effects.push(effect);
            ticks
        }
    }

    /// 推进一 tick；返回本 tick 需要结算的灼烧效果（复制），并移除到期效果。
    pub fn tick(&mut self, tick: u64) -> Vec<EffectInstance> {
        let mut burns = Vec::new();
        for e in &mut self.effects {
            e.remaining_ticks = e.remaining_ticks.saturating_sub(1);
            if e.kind == StatusKind::Burn
                && e.remaining_ticks > 0
                && e.remaining_ticks % BURN_PERIOD_TICKS == 0
            {
                burns.push(e.clone());
            }
        }
        self.effects.retain(|e| e.remaining_ticks > 0);
        if tick.saturating_sub(self.last_control_tick) > CONTROL_WINDOW_TICKS {
            self.control_chain = 0;
        }
        burns
    }

    /// 一次净化只移除一层，按驱散优先级（§8.2）。
    pub fn dispel_one(&mut self) -> Option<StatusKind> {
        let idx = self
            .effects
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.kind.dispel_priority())
            .map(|(i, _)| i)?;
        Some(self.effects.remove(idx).kind)
    }

    pub fn clear(&mut self) {
        self.effects.clear();
        self.control_chain = 0;
        self.last_control_tick = 0;
    }
}

// ---------------------------------------------------------------------------
// 完整伤害结算（§5）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DamageRequest {
    /// 已由调用方按 `base + Σ(scaling × stat)` 算好的原始伤害
    pub raw: f32,
    pub damage_type: DamageType,
    pub can_crit: bool,
    /// 服务端确定性 PRNG 产生的 `[0,1)` 值，客户端不得参与
    pub crit_roll: f32,
    pub target_armor: f32,
    pub target_magic_resist: f32,
    pub shield: f32,
    pub health: f32,
    /// 吸血目标系数：英雄 1.0 / 战偶 0.33 / 野怪 0.33 / 建筑 0
    pub lifesteal_factor: f32,
    /// 加时衰竭：真实伤害、无视护盾与减伤（§1.2 / §5.3）
    pub exhaustion: bool,
}

impl DamageRequest {
    pub fn new(raw: f32, damage_type: DamageType) -> Self {
        Self {
            raw,
            damage_type,
            can_crit: false,
            crit_roll: 1.0,
            target_armor: 0.0,
            target_magic_resist: 0.0,
            shield: 0.0,
            health: 0.0,
            lifesteal_factor: 1.0,
            exhaustion: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageOutcome {
    pub raw: f32,
    pub mitigation: f32,
    pub crit: f32,
    pub amplified: f32,
    pub shield_hit: f32,
    pub dealt: f32,
    pub lifesteal: f32,
    pub lethal: bool,
    pub health_after: f32,
    pub shield_after: f32,
}

impl DamageOutcome {
    fn immune(health: f32, shield: f32) -> Self {
        Self {
            raw: 0.0,
            mitigation: 0.0,
            crit: 1.0,
            amplified: 0.0,
            shield_hit: 0.0,
            dealt: 0.0,
            lifesteal: 0.0,
            lethal: false,
            health_after: health,
            shield_after: shield,
        }
    }
}

pub fn compute_raw(base: f32, ad: f32, ap: f32, scaling_ad: f32, scaling_ap: f32) -> f32 {
    base + scaling_ad * ad + scaling_ap * ap
}

/// 有效护甲/魔抗：`max(0, x - flat_pen) × (1 - clamp(pct_pen, 0, 0.4))`（§4.3）。
fn effective_resist(base: f32, flat_pen: f32, pct_pen: f32) -> f32 {
    (base - flat_pen).max(0.0) * (1.0 - pct_pen.clamp(0.0, 0.4))
}

/// 服务端唯一裁决点（§5.2 步骤 1~9）。纯函数，可回放复现。
pub fn resolve_damage(
    req: &DamageRequest,
    attacker: &CombatStats,
    target: &CombatStats,
    invulnerable: bool,
) -> DamageOutcome {
    if invulnerable && !req.exhaustion {
        return DamageOutcome::immune(req.health, req.shield);
    }

    let raw = req.raw.max(0.0);
    let mitigation = if req.exhaustion {
        1.0
    } else {
        match req.damage_type {
            DamageType::Physical => {
                100.0
                    / (100.0
                        + effective_resist(
                            req.target_armor,
                            attacker.flat_armor_pen,
                            attacker.pct_armor_pen,
                        ))
            }
            DamageType::Magical => {
                100.0
                    / (100.0
                        + effective_resist(
                            req.target_magic_resist,
                            attacker.flat_mr_pen,
                            attacker.pct_mr_pen,
                        ))
            }
            DamageType::True => 1.0,
        }
    };
    let crit = if !req.exhaustion && req.can_crit && req.crit_roll < attacker.crit_chance {
        attacker.crit_multiplier.max(1.0)
    } else {
        1.0
    };
    let amplified = if req.exhaustion {
        raw
    } else {
        raw * mitigation
            * crit
            * (1.0 + attacker.damage_amp.max(-0.5))
            * (1.0 - target.damage_reduction.clamp(0.0, 0.5))
    }
    .clamp(0.0, 1e6);

    let shield_hit = if req.exhaustion {
        0.0
    } else {
        req.shield.max(0.0).min(amplified)
    };
    let dealt = amplified - shield_hit;
    let health_after = (req.health - dealt).max(0.0);

    DamageOutcome {
        raw,
        mitigation,
        crit,
        amplified,
        shield_hit,
        dealt,
        lifesteal: dealt * attacker.lifesteal.clamp(0.0, 0.4) * req.lifesteal_factor.max(0.0),
        lethal: health_after <= EPS && dealt > 0.0,
        health_after,
        shield_after: req.shield.max(0.0) - shield_hit,
    }
}

/// 桥接现有 `Hero` 组件的结算入口；保持与旧 `apply_mitigated_damage` 在默认属性下等价。
pub fn apply_hero_damage(
    hero: &mut Hero,
    attacker: &CombatStats,
    target_stats: &CombatStats,
    shield: &mut ShieldPool,
    status: &StatusEffects,
    mut req: DamageRequest,
) -> DamageOutcome {
    req.target_armor = hero.armor;
    req.target_magic_resist = hero.magic_resist;
    req.shield = shield.value;
    req.health = hero.health;
    let outcome = resolve_damage(&req, attacker, target_stats, status.is_invulnerable());
    hero.health = outcome.health_after;
    shield.value = outcome.shield_after;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(raw: f32, dtype: DamageType) -> DamageRequest {
        DamageRequest::new(raw, dtype)
    }

    #[test]
    fn tc_cbt_03_armor_mitigation() {
        let mut r = req(100.0, DamageType::Physical);
        r.target_armor = 22.0;
        let out = resolve_damage(&r, &CombatStats::default(), &CombatStats::default(), false);
        assert!((out.dealt - 81.9672).abs() < 1e-3, "dealt={}", out.dealt);
    }

    #[test]
    fn tc_cbt_03_penetration_is_bounded() {
        let mut r = req(100.0, DamageType::Physical);
        r.target_armor = 100.0;
        let atk = CombatStats {
            flat_armor_pen: 500.0,
            pct_armor_pen: 0.9,
            ..default()
        };
        let out = resolve_damage(&r, &atk, &CombatStats::default(), false);
        assert!((out.dealt - 100.0).abs() < EPS, "dealt={}", out.dealt);
        assert!(out.mitigation.is_finite() && out.mitigation <= 1.0 + EPS);
    }

    #[test]
    fn tc_cbt_04_shield_absorbed_first() {
        let mut r = req(100.0, DamageType::Physical);
        r.shield = 40.0;
        r.health = 500.0;
        let out = resolve_damage(&r, &CombatStats::default(), &CombatStats::default(), false);
        assert_eq!(out.shield_hit, 40.0);
        assert_eq!(out.shield_after, 0.0);
        assert_eq!(out.health_after, 440.0);
    }

    #[test]
    fn tc_cbt_04_true_damage_ignores_resist_but_not_reduction() {
        let mut r = req(100.0, DamageType::True);
        r.target_armor = 500.0;
        r.health = 500.0;
        let target = CombatStats {
            damage_reduction: 0.5,
            ..default()
        };
        let out = resolve_damage(&r, &CombatStats::default(), &target, false);
        assert_eq!(out.dealt, 50.0);
    }

    #[test]
    fn tc_cbt_06_control_diminishing_returns() {
        let mut s = StatusEffects::default();
        let expected = [60, 39, 26, 17, 17];
        for (i, want) in expected.iter().enumerate() {
            let ticks = s.apply(
                EffectInstance {
                    kind: StatusKind::Stun,
                    strength: 1.0,
                    remaining_ticks: 60,
                    source_net_id: 7,
                },
                i as u64 * 10,
            );
            assert_eq!(ticks, *want, "control #{i}");
            // 只清除效果实例，保留窗口内的控制递减状态
            s.effects.clear();
        }
    }

    #[test]
    fn tc_cbt_06_slow_does_not_stack() {
        let mut s = StatusEffects::default();
        s.apply(
            EffectInstance {
                kind: StatusKind::Slow,
                strength: 0.3,
                remaining_ticks: 60,
                source_net_id: 1,
            },
            0,
        );
        s.apply(
            EffectInstance {
                kind: StatusKind::Slow,
                strength: 0.5,
                remaining_ticks: 90,
                source_net_id: 2,
            },
            1,
        );
        assert_eq!(s.effects.len(), 1);
        assert_eq!(s.strength(StatusKind::Slow), 0.5);
        assert_eq!(s.effects[0].remaining_ticks, 90);
    }

    #[test]
    fn invulnerable_blocks_damage_but_exhaustion_bypasses() {
        let mut r = req(100.0, DamageType::True);
        r.health = 200.0;
        r.shield = 50.0;
        assert_eq!(
            resolve_damage(&r, &CombatStats::default(), &CombatStats::default(), true).dealt,
            0.0
        );
        r.exhaustion = true;
        let out = resolve_damage(&r, &CombatStats::default(), &CombatStats::default(), true);
        assert_eq!(out.dealt, 100.0);
        assert_eq!(out.shield_after, 50.0);
    }

    #[test]
    fn crit_and_lifesteal_use_server_roll() {
        let mut r = req(100.0, DamageType::True);
        r.can_crit = true;
        r.crit_roll = 0.1;
        r.health = 1000.0;
        let atk = CombatStats {
            crit_chance: 0.5,
            lifesteal: 0.2,
            ..default()
        };
        let out = resolve_damage(&r, &atk, &CombatStats::default(), false);
        assert_eq!(out.crit, 2.0);
        assert_eq!(out.dealt, 200.0);
        assert_eq!(out.lifesteal, 40.0);
    }

    #[test]
    fn dispel_respects_priority() {
        let mut s = StatusEffects::default();
        for kind in [StatusKind::Burn, StatusKind::Invulnerable, StatusKind::Stun] {
            s.effects.push(EffectInstance {
                kind,
                strength: 1.0,
                remaining_ticks: 60,
                source_net_id: 0,
            });
        }
        assert_eq!(s.dispel_one(), Some(StatusKind::Invulnerable));
        assert_eq!(s.dispel_one(), Some(StatusKind::Stun));
        assert_eq!(s.dispel_one(), Some(StatusKind::Burn));
    }

    #[test]
    fn regen_never_exceeds_max() {
        let mut hero = Hero::default();
        hero.health = hero.max_health - 0.001;
        hero.energy = hero.max_energy - 0.001;
        apply_regen(&mut hero, &RegenStats::default());
        assert_eq!(hero.health, hero.max_health);
        assert_eq!(hero.energy, hero.max_energy);
    }
}
