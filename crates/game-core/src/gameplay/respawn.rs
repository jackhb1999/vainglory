//! 死亡、重生与赏金（`GX-CBT-2026-001` §15.1 / §15.2 / §9.2）。

use bevy::math::Vec2;
use bevy::prelude::*;

pub const RESPAWN_MIN_TICKS: u64 = 8 * 60;
pub const RESPAWN_MAX_TICKS: u64 = 40 * 60;
pub const INVULNERABLE_TICKS: u64 = 3 * 60;
pub const HASTE_TICKS: u64 = 3 * 60;
pub const HERO_KILL_BASE_GOLD: u32 = 180;
pub const STREAK_BONUS: u32 = 45;
pub const MAX_STREAK_BONUS_STEPS: u8 = 5;
pub const STREAK_EXPIRE_TICKS: u64 = 90 * 60;
pub const ASSIST_GOLD_PERCENT: u32 = 35;

#[derive(Component, Clone, Debug)]
pub struct RespawnState {
    pub dead: bool,
    pub death_tick: u64,
    pub respawn_tick: u64,
    pub level: u8,
    pub spawn_point: Vec2,
    pub invulnerable_until_tick: u64,
    pub haste_until_tick: u64,
}

impl Default for RespawnState {
    fn default() -> Self {
        Self {
            dead: false,
            death_tick: 0,
            respawn_tick: 0,
            level: 1,
            spawn_point: Vec2::ZERO,
            invulnerable_until_tick: 0,
            haste_until_tick: 0,
        }
    }
}

impl RespawnState {
    pub fn new(level: u8, spawn_point: Vec2) -> Self {
        Self {
            level,
            spawn_point,
            ..Default::default()
        }
    }
}

/// `respawn_ticks = clamp((8 + 2×(L-1)) × 60, 480, 2400)`（§15.2）。
pub fn respawn_ticks(level: u8) -> u64 {
    let seconds = 8u64 + 2u64 * level.saturating_sub(1) as u64;
    (seconds * 60).clamp(RESPAWN_MIN_TICKS, RESPAWN_MAX_TICKS)
}

pub fn mark_dead(state: &mut RespawnState, tick: u64) {
    if state.dead {
        return;
    }
    state.dead = true;
    state.death_tick = tick;
    state.respawn_tick = tick + respawn_ticks(state.level);
}

/// 返回 true 表示本 tick 重生（调用方负责回满生命/能量、清状态、给无敌与加速）。
pub fn step_respawn(state: &mut RespawnState, tick: u64) -> bool {
    if !state.dead || tick < state.respawn_tick {
        return false;
    }
    state.dead = false;
    state.invulnerable_until_tick = tick + INVULNERABLE_TICKS;
    state.haste_until_tick = tick + HASTE_TICKS;
    true
}

/// 无敌/加速在主动攻击或施法时立即移除（§15.2）。
pub fn consume_on_action(state: &mut RespawnState, tick: u64) {
    if tick < state.invulnerable_until_tick {
        state.invulnerable_until_tick = tick;
    }
    if tick < state.haste_until_tick {
        state.haste_until_tick = tick;
    }
}

#[derive(Component, Clone, Debug, Default)]
pub struct KillStreak {
    pub count: u8,
    pub last_kill_tick: u64,
}

/// 当前有效连杀数：超过 90 s 无击杀或阵亡后归零。
pub fn effective_streak(streak: &mut KillStreak, tick: u64) -> u8 {
    if streak.count > 0 && tick.saturating_sub(streak.last_kill_tick) > STREAK_EXPIRE_TICKS {
        streak.count = 0;
    }
    streak.count
}

pub fn kill_bounty(streak: &mut KillStreak, tick: u64) -> u32 {
    let count = effective_streak(streak, tick);
    let steps = count.saturating_sub(1).min(MAX_STREAK_BONUS_STEPS);
    HERO_KILL_BASE_GOLD + steps as u32 * STREAK_BONUS
}

pub fn register_kill(streak: &mut KillStreak, tick: u64) {
    let count = effective_streak(streak, tick);
    streak.count = count.saturating_add(1);
    streak.last_kill_tick = tick;
}

pub fn reset_streak(streak: &mut KillStreak) {
    streak.count = 0;
    streak.last_kill_tick = 0;
}

pub fn assist_gold(kill_gold: u32) -> u32 {
    kill_gold * ASSIST_GOLD_PERCENT / 100
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeathRecord {
    pub victim_net_id: u64,
    pub killer_net_id: u64,
    pub tick: u64,
    pub bounty: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_time_is_clamped_to_8_to_40_seconds() {
        assert_eq!(respawn_ticks(1), 480);
        assert_eq!(respawn_ticks(2), 600);
        assert_eq!(respawn_ticks(12), 1800);
        assert_eq!(respawn_ticks(30), RESPAWN_MAX_TICKS);
    }

    #[test]
    fn death_and_respawn_state_transition() {
        let mut s = RespawnState::new(3, Vec2::new(-56.0, -56.0));
        mark_dead(&mut s, 1000);
        assert!(s.dead);
        assert_eq!(s.respawn_tick, 1000 + 720);
        assert!(!step_respawn(&mut s, 1719));
        assert!(step_respawn(&mut s, 1720));
        assert!(!s.dead);
        assert_eq!(s.invulnerable_until_tick, 1720 + 180);
        assert_eq!(s.haste_until_tick, 1720 + 180);
        assert!(!step_respawn(&mut s, 1721), "respawn fires only once");
    }

    #[test]
    fn action_consumes_spawn_protection() {
        let mut s = RespawnState {
            invulnerable_until_tick: 1000,
            haste_until_tick: 1000,
            ..Default::default()
        };
        consume_on_action(&mut s, 900);
        assert_eq!(s.invulnerable_until_tick, 900);
        assert_eq!(s.haste_until_tick, 900);
    }

    #[test]
    fn bounty_caps_at_405_and_resets_after_death() {
        let mut streak = KillStreak::default();
        let mut gold = 0;
        for _ in 0..8 {
            gold = kill_bounty(&mut streak, 100);
            register_kill(&mut streak, 100);
        }
        assert_eq!(gold, 405);
        reset_streak(&mut streak);
        assert_eq!(kill_bounty(&mut streak, 101), 180);
    }

    #[test]
    fn streak_expires_after_90_seconds() {
        let mut streak = KillStreak::default();
        register_kill(&mut streak, 0);
        register_kill(&mut streak, 1);
        assert_eq!(kill_bounty(&mut streak, 2), 225);
        assert_eq!(kill_bounty(&mut streak, STREAK_EXPIRE_TICKS + 2), 180);
    }

    #[test]
    fn assist_gold_is_thirty_five_percent() {
        assert_eq!(assist_gold(180), 63);
    }
}
