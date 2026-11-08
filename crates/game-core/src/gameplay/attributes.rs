//! 属性聚合层（`P-A`）：`基础值 + 等级成长 + 装备 + buff → 最终属性`。
//!
//! 之前 `shop_purchase_system` 直接对 `Hero` 做 `+=`，一旦引入可过期 buff，
//! 卸下/重装/死亡重生后就无法还原。本模块把所有加成收敛为可重算的来源。

use crate::components::{Hero, Team};
use crate::gameplay::combat::CombatStats;
use crate::gameplay::shop::ItemStats;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StatDelta {
    pub max_health: f32,
    pub max_energy: f32,
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub secondary: CombatStats,
}

impl StatDelta {
    pub fn add(&self, rhs: &Self) -> Self {
        Self {
            max_health: self.max_health + rhs.max_health,
            max_energy: self.max_energy + rhs.max_energy,
            attack_damage: self.attack_damage + rhs.attack_damage,
            ability_power: self.ability_power + rhs.ability_power,
            armor: self.armor + rhs.armor,
            magic_resist: self.magic_resist + rhs.magic_resist,
            attack_speed: self.attack_speed + rhs.attack_speed,
            move_speed: self.move_speed + rhs.move_speed,
            secondary: add_combat(&self.secondary, &rhs.secondary),
        }
    }

    pub fn from_item(s: &ItemStats) -> Self {
        Self {
            max_health: s.max_health,
            attack_damage: s.attack_damage,
            ability_power: s.ability_power,
            armor: s.armor,
            magic_resist: s.magic_resist,
            move_speed: s.move_speed,
            secondary: CombatStats {
                crit_chance: s.crit_chance,
                cooldown_reduction: s.cooldown_reduction,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

fn add_combat(a: &CombatStats, b: &CombatStats) -> CombatStats {
    CombatStats {
        crit_chance: a.crit_chance + b.crit_chance,
        crit_multiplier: a.crit_multiplier.max(b.crit_multiplier),
        flat_armor_pen: a.flat_armor_pen + b.flat_armor_pen,
        pct_armor_pen: a.pct_armor_pen + b.pct_armor_pen,
        flat_mr_pen: a.flat_mr_pen + b.flat_mr_pen,
        pct_mr_pen: a.pct_mr_pen + b.pct_mr_pen,
        lifesteal: a.lifesteal + b.lifesteal,
        cooldown_reduction: a.cooldown_reduction + b.cooldown_reduction,
        damage_amp: a.damage_amp + b.damage_amp,
        damage_reduction: a.damage_reduction + b.damage_reduction,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HeroBaseline {
    pub max_health: f32,
    pub max_energy: f32,
    pub attack_damage: f32,
    pub ability_power: f32,
    pub armor: f32,
    pub magic_resist: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub attack_range: f32,
}

impl HeroBaseline {
    pub fn capture(hero: &Hero) -> Self {
        Self {
            max_health: hero.max_health,
            max_energy: hero.max_energy,
            attack_damage: hero.attack_damage,
            ability_power: hero.ability_power,
            armor: hero.armor,
            magic_resist: hero.magic_resist,
            attack_speed: hero.attack_speed,
            move_speed: hero.move_speed,
            attack_range: hero.attack_range,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BuffInstance {
    pub id: u32,
    pub name: &'static str,
    pub remaining_ticks: u32,
    pub delta: StatDelta,
}

#[derive(Component, Clone, Debug)]
pub struct AttributeSources {
    pub base: HeroBaseline,
    pub level: u8,
    pub equipment: StatDelta,
    pub buffs: Vec<BuffInstance>,
}

impl AttributeSources {
    /// 捕获「1 级基础值」。若实体在挂接聚合层前已经升级过，必须先扣掉已有
    /// 等级成长，否则 `total()` 会二次叠加，base 被污染。
    pub fn new(hero: &Hero) -> Self {
        let mut base = HeroBaseline::capture(hero);
        let grown = level_delta(hero.level);
        base.max_health -= grown.max_health;
        base.max_energy -= grown.max_energy;
        base.attack_damage -= grown.attack_damage;
        base.armor -= grown.armor;
        base.magic_resist -= grown.magic_resist;
        Self {
            base,
            level: hero.level,
            equipment: StatDelta::default(),
            buffs: Vec::new(),
        }
    }

    pub fn total(&self) -> StatDelta {
        let mut t = level_delta(self.level);
        t = t.add(&self.equipment);
        for b in &self.buffs {
            t = t.add(&b.delta);
        }
        t
    }
}

/// 等级成长（与 `level_up_system` 的旧数值保持一致，但改为可重算的增量）。
pub fn level_delta(level: u8) -> StatDelta {
    let n = level.saturating_sub(1) as f32;
    StatDelta {
        max_health: 80.0 * n,
        max_energy: 40.0 * n,
        attack_damage: 3.5 * n,
        armor: 1.5 * n,
        magic_resist: 1.0 * n,
        ..Default::default()
    }
}

/// 把聚合结果写回 `Hero` / `CombatStats`。生命/能量按 max 的变化量补足，不回满。
pub fn recompute(sources: &AttributeSources, hero: &mut Hero, combat: &mut CombatStats) {
    let t = sources.total();
    let new_max_hp = sources.base.max_health + t.max_health;
    let new_max_en = sources.base.max_energy + t.max_energy;
    let hp_diff = new_max_hp - hero.max_health;
    let en_diff = new_max_en - hero.max_energy;

    hero.max_health = new_max_hp;
    hero.max_energy = new_max_en;
    hero.health = (hero.health + hp_diff).clamp(0.0, new_max_hp);
    hero.energy = (hero.energy + en_diff).clamp(0.0, new_max_en);
    hero.attack_damage = sources.base.attack_damage + t.attack_damage;
    hero.ability_power = sources.base.ability_power + t.ability_power;
    hero.armor = sources.base.armor + t.armor;
    hero.magic_resist = sources.base.magic_resist + t.magic_resist;
    hero.attack_speed = sources.base.attack_speed + t.attack_speed;
    hero.move_speed = sources.base.move_speed + t.move_speed;
    hero.attack_range = sources.base.attack_range;

    *combat = t.secondary.clamped();
}

pub fn add_buff(sources: &mut AttributeSources, buff: BuffInstance) {
    if let Some(existing) = sources.buffs.iter_mut().find(|b| b.id == buff.id) {
        *existing = buff; // 同类刷新，不叠加
    } else {
        sources.buffs.push(buff);
    }
}

/// 推进一 tick，返回到期被移除的 buff id（供事件下发）。
pub fn tick_buffs(sources: &mut AttributeSources) -> Vec<u32> {
    let mut expired = Vec::new();
    for b in &mut sources.buffs {
        b.remaining_ticks = b.remaining_ticks.saturating_sub(1);
    }
    sources.buffs.retain(|b| {
        if b.remaining_ticks == 0 {
            expired.push(b.id);
            false
        } else {
            true
        }
    });
    expired
}

pub fn clear_buffs(sources: &mut AttributeSources) {
    sources.buffs.clear();
}

/// 中立目标 buff 表（§13）：占领成功后发给归属方全队。
pub fn objective_buff(objective_id: u32, _team: Team) -> Option<BuffInstance> {
    let (name, duration, delta) = match objective_id {
        1 => (
            "lode_shrine",
            90 * 60,
            StatDelta {
                armor: 20.0,
                magic_resist: 15.0,
                ..Default::default()
            },
        ),
        2 => (
            "muster_well",
            90 * 60,
            StatDelta {
                attack_damage: 15.0,
                attack_speed: 0.15,
                ..Default::default()
            },
        ),
        3 => (
            "abyss_warden",
            120 * 60,
            StatDelta {
                secondary: CombatStats {
                    damage_amp: 0.15,
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        _ => return None,
    };
    Some(BuffInstance {
        id: objective_id,
        name,
        remaining_ticks: duration,
        delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero() -> Hero {
        Hero::default()
    }

    #[test]
    fn equipment_and_buff_are_recomputable() {
        let h = hero();
        let mut src = AttributeSources::new(&h);
        let mut stats = CombatStats::default();
        let mut live = h.clone();

        src.equipment = StatDelta::from_item(&ItemStats {
            attack_damage: 40.0,
            max_health: 120.0,
            crit_chance: 0.15,
            ..Default::default()
        });
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.attack_damage, h.attack_damage + 40.0);
        assert_eq!(live.max_health, h.max_health + 120.0);
        assert_eq!(stats.crit_chance, 0.15);

        add_buff(
            &mut src,
            BuffInstance {
                id: 2,
                name: "muster_well",
                remaining_ticks: 30,
                delta: StatDelta {
                    attack_damage: 15.0,
                    ..Default::default()
                },
            },
        );
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.attack_damage, h.attack_damage + 55.0);

        for _ in 0..30 {
            tick_buffs(&mut src);
        }
        assert!(src.buffs.is_empty());
        recompute(&src, &mut live, &mut stats);
        assert_eq!(
            live.attack_damage,
            h.attack_damage + 40.0,
            "buff must be removable"
        );
    }

    #[test]
    fn same_buff_refreshes_instead_of_stacking() {
        let h = hero();
        let mut src = AttributeSources::new(&h);
        for _ in 0..3 {
            add_buff(
                &mut src,
                BuffInstance {
                    id: 1,
                    name: "lode_shrine",
                    remaining_ticks: 60,
                    delta: StatDelta {
                        armor: 20.0,
                        ..Default::default()
                    },
                },
            );
        }
        assert_eq!(src.buffs.len(), 1);
        assert_eq!(src.total().armor, 20.0);
    }

    #[test]
    fn level_growth_is_idempotent() {
        let h = hero();
        let mut src = AttributeSources::new(&h);
        let mut live = h.clone();
        let mut stats = CombatStats::default();
        src.level = 3;
        recompute(&src, &mut live, &mut stats);
        let hp = live.max_health;
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.max_health, hp);
        assert_eq!(hp, h.max_health + 160.0);
    }

    #[test]
    fn objective_buff_table_covers_all_three() {
        for id in 1..=3 {
            assert!(objective_buff(id, Team::Blue).is_some());
        }
        assert!(objective_buff(99, Team::Blue).is_none());
    }

    #[test]
    fn late_attach_does_not_double_count_level_growth() {
        // 实体在挂接聚合层前已升到 3 级：Hero 已含等级成长，base 必须扣回，
        // 否则 total() 会二次叠加。
        let d = Hero::default();
        let h = Hero {
            level: 3,
            max_health: d.max_health + 160.0,
            max_energy: d.max_energy + 80.0,
            attack_damage: d.attack_damage + 7.0,
            armor: d.armor + 3.0,
            magic_resist: d.magic_resist + 2.0,
            ..Default::default()
        };
        let src = AttributeSources::new(&h);
        let mut live = h.clone();
        let mut stats = CombatStats::default();
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.max_health, h.max_health);
        assert_eq!(live.max_energy, h.max_energy);
        assert_eq!(live.attack_damage, h.attack_damage);
        assert_eq!(live.armor, h.armor);
    }

    #[test]
    fn level_growth_survives_equipment_recompute() {
        let h = Hero::default();
        let mut src = AttributeSources::new(&h);
        let mut stats = CombatStats::default();
        let mut live = h.clone();
        src.level = 4;
        src.equipment = StatDelta::from_item(&ItemStats {
            attack_damage: 40.0,
            ..Default::default()
        });
        recompute(&src, &mut live, &mut stats);
        let expected = h.attack_damage + 40.0 + 3.5 * 3.0;
        assert_eq!(live.attack_damage, expected);
        // 模拟 buff 过期 / 售出后的重算：等级与装备都必须保留
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.attack_damage, expected);
        src.equipment = StatDelta::default();
        recompute(&src, &mut live, &mut stats);
        assert_eq!(live.attack_damage, h.attack_damage + 3.5 * 3.0);
    }
}
