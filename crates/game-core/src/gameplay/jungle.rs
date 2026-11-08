//! 野怪 AI 状态机与刷新（`GX-CBT-2026-001` §12.3）。
//!
//! `Idle → Aggro → Returning → Idle`；静止型（移速 0）永不进入 Aggro。

use bevy::math::Vec2;
use bevy::prelude::*;

pub const OUT_OF_COMBAT_REGEN_PER_TICK: f32 = 0.08 / 60.0;
pub const RETURN_ARRIVE_RADIUS: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonsterState {
    Idle,
    Aggro,
    Returning,
}

#[derive(Component, Clone, Debug)]
pub struct MonsterAi {
    pub state: MonsterState,
    pub spawn: Vec2,
    pub aggro_range: f32,
    pub leash_range: f32,
    pub attack_range: f32,
    pub move_speed: f32,
    pub max_health: f32,
    pub health: f32,
    pub out_of_combat_ticks: u32,
}

impl MonsterAi {
    pub fn new(
        spawn: Vec2,
        health: f32,
        aggro_range: f32,
        leash_range: f32,
        move_speed: f32,
    ) -> Self {
        Self {
            state: MonsterState::Idle,
            spawn,
            aggro_range,
            leash_range,
            attack_range: 1.5,
            move_speed,
            max_health: health,
            health,
            out_of_combat_ticks: 0,
        }
    }

    pub fn is_static(&self) -> bool {
        self.move_speed <= f32::EPSILON
    }

    fn regen(&mut self) {
        self.health =
            (self.health + self.max_health * OUT_OF_COMBAT_REGEN_PER_TICK).min(self.max_health);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MonsterInput {
    pub position: Vec2,
    pub nearest_enemy: Option<Vec2>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterOutput {
    pub state: MonsterState,
    pub desired_move_dir: Vec2,
    pub should_attack: bool,
    pub health_after: f32,
}

/// 推进一个 tick。纯函数，服务端系统负责提供 `nearest_enemy` 快照。
pub fn step_monster(ai: &mut MonsterAi, input: &MonsterInput) -> MonsterOutput {
    let enemy = input.nearest_enemy;
    let enemy_dist = enemy
        .map(|e| e.distance(input.position))
        .unwrap_or(f32::INFINITY);
    let spawn_dist = input.position.distance(ai.spawn);

    // 静止型只反击射程内目标，不追击
    if ai.is_static() {
        ai.state = MonsterState::Idle;
        if enemy.is_none() {
            ai.regen();
        } else {
            ai.out_of_combat_ticks = 0;
        }
        return MonsterOutput {
            state: ai.state,
            desired_move_dir: Vec2::ZERO,
            should_attack: enemy_dist <= ai.attack_range,
            health_after: ai.health,
        };
    }

    // 阶段 1：状态迁移（同一 tick 内迁移后立即进入新状态的行为，避免"空转一帧"）
    match ai.state {
        MonsterState::Idle => {
            if enemy.is_some() && enemy_dist <= ai.aggro_range {
                ai.state = MonsterState::Aggro;
            }
        }
        MonsterState::Aggro => {
            // 脱战：超出牵引半径或目标丢失
            if enemy.is_none() || spawn_dist > ai.leash_range {
                ai.state = MonsterState::Returning;
            }
        }
        MonsterState::Returning => {
            if spawn_dist <= RETURN_ARRIVE_RADIUS {
                ai.state = MonsterState::Idle;
            }
        }
    }

    // 阶段 2：按迁移后的状态执行行为
    let mut dir = Vec2::ZERO;
    let mut attack = false;
    match ai.state {
        MonsterState::Idle => ai.regen(),
        MonsterState::Aggro => {
            ai.out_of_combat_ticks = 0;
            if let Some(e) = enemy {
                if enemy_dist > ai.attack_range {
                    dir = (e - input.position).normalize_or_zero();
                } else {
                    attack = true;
                }
            }
        }
        MonsterState::Returning => {
            // 脱战期间不可被再次拉仇恨，且持续回血
            ai.regen();
            dir = (ai.spawn - input.position).normalize_or_zero();
        }
    }

    if ai.state != MonsterState::Aggro {
        ai.out_of_combat_ticks = ai.out_of_combat_ticks.saturating_add(1);
    }

    MonsterOutput {
        state: ai.state,
        desired_move_dir: dir * ai.move_speed,
        should_attack: attack,
        health_after: ai.health,
    }
}

/// 野怪刷新计时（§12.1）：首次刷新 tick 与阵亡后的固定间隔。
#[derive(Component, Clone, Debug)]
pub struct MonsterRespawn {
    pub first_spawn_tick: u64,
    pub respawn_ticks: u64,
    pub alive: bool,
    pub respawn_at_tick: u64,
}

impl MonsterRespawn {
    pub fn new(first_spawn_tick: u64, respawn_ticks: u64) -> Self {
        Self {
            first_spawn_tick,
            respawn_ticks,
            alive: false,
            respawn_at_tick: first_spawn_tick,
        }
    }
}

pub fn on_monster_death(timer: &mut MonsterRespawn, tick: u64) {
    timer.alive = false;
    timer.respawn_at_tick = tick + timer.respawn_ticks;
}

/// 返回 true 表示本 tick 应重新生成该野怪。
pub fn step_monster_respawn(timer: &mut MonsterRespawn, tick: u64) -> bool {
    if timer.alive || tick < timer.respawn_at_tick {
        return false;
    }
    timer.alive = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roamer() -> MonsterAi {
        MonsterAi::new(Vec2::ZERO, 1000.0, 10.0, 15.0, 3.5)
    }

    #[test]
    fn tc_cbt_09_static_monster_never_enters_aggro() {
        let mut ai = MonsterAi::new(Vec2::ZERO, 1000.0, 10.0, 15.0, 0.0);
        let out = step_monster(
            &mut ai,
            &MonsterInput {
                position: Vec2::ZERO,
                nearest_enemy: Some(Vec2::new(1.0, 0.0)),
            },
        );
        assert_eq!(out.state, MonsterState::Idle);
        assert_eq!(out.desired_move_dir, Vec2::ZERO);
        assert!(out.should_attack);
    }

    #[test]
    fn tc_cbt_09_leash_triggers_return_and_regen() {
        let mut ai = roamer();
        ai.health = 500.0;
        ai.state = MonsterState::Aggro;
        let out = step_monster(
            &mut ai,
            &MonsterInput {
                position: Vec2::new(20.0, 0.0), // 超出 leash=15
                nearest_enemy: Some(Vec2::new(21.0, 0.0)),
            },
        );
        assert_eq!(out.state, MonsterState::Returning);
        assert!(!out.should_attack);
        assert!(out.health_after > 500.0, "returning should regen");
    }

    #[test]
    fn tc_cbt_09_returning_ignores_new_threats() {
        let mut ai = roamer();
        ai.state = MonsterState::Returning;
        let out = step_monster(
            &mut ai,
            &MonsterInput {
                position: Vec2::new(5.0, 0.0),
                nearest_enemy: Some(Vec2::new(6.0, 0.0)),
            },
        );
        assert_eq!(out.state, MonsterState::Returning);
        assert!(!out.should_attack);
        assert!(out.desired_move_dir.x < 0.0, "should head back to spawn");
    }

    #[test]
    fn idle_enters_aggro_and_stops_at_attack_range() {
        let mut ai = roamer();
        let out = step_monster(
            &mut ai,
            &MonsterInput {
                position: Vec2::ZERO,
                nearest_enemy: Some(Vec2::new(8.0, 0.0)),
            },
        );
        assert_eq!(out.state, MonsterState::Aggro);
        assert!(out.desired_move_dir.x > 0.0);
        let out = step_monster(
            &mut ai,
            &MonsterInput {
                position: Vec2::new(7.0, 0.0),
                nearest_enemy: Some(Vec2::new(8.0, 0.0)),
            },
        );
        assert!(out.should_attack);
        assert_eq!(out.desired_move_dir, Vec2::ZERO);
    }

    #[test]
    fn respawn_is_single_shot_and_respects_first_spawn() {
        let mut t = MonsterRespawn::new(900, 5400);
        assert!(!step_monster_respawn(&mut t, 899));
        assert!(step_monster_respawn(&mut t, 900));
        assert!(!step_monster_respawn(&mut t, 901));
        on_monster_death(&mut t, 1000);
        assert!(!step_monster_respawn(&mut t, 6399));
        assert!(step_monster_respawn(&mut t, 6400));
    }
}
