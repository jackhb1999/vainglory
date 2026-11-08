//! 兵线战偶波次定义（§11）与行军路点。
//!
//! 数据 + 纯函数；ECS 驱动系统位于 `server::gameplay::world`。

use crate::components::{MinionType, Team};
use bevy::math::Vec2;
use bevy::prelude::*;

pub const FIRST_WAVE_TICK: u64 = 900; // 开局 15 s
pub const WAVE_INTERVAL_TICKS: u64 = 1500; // 每 25 s 一波
pub const MINIONS_PER_LANE: usize = 3;

/// 战偶到达路点判定的半径
pub const ARRIVE_RADIUS: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveDef {
    pub first_wave_tick: u64,
    pub interval_ticks: u64,
}

impl Default for WaveDef {
    fn default() -> Self {
        Self {
            first_wave_tick: FIRST_WAVE_TICK,
            interval_ticks: WAVE_INTERVAL_TICKS,
        }
    }
}

impl WaveDef {
    /// 本 tick 是否应生成新一波，返回波次序号（从 0 开始）。
    pub fn wave_at(&self, tick: u64) -> Option<u64> {
        if tick < self.first_wave_tick {
            return None;
        }
        let elapsed = tick - self.first_wave_tick;
        if !elapsed.is_multiple_of(self.interval_ticks) {
            return None;
        }
        Some(elapsed / self.interval_ticks)
    }
}

/// 单波单路构成：随波次成长，每 3 波加一个攻城单位。
pub fn wave_composition(wave_index: u64) -> Vec<MinionType> {
    let mut out = vec![MinionType::Melee; MINIONS_PER_LANE];
    out.push(MinionType::Ranged);
    if wave_index > 0 && wave_index.is_multiple_of(3) {
        out.push(MinionType::Siege);
    }
    if wave_index >= 6 {
        out.push(MinionType::Super);
    }
    out
}

/// 该波的属性成长系数（§11.1）
pub fn wave_scale(wave_index: u64) -> f32 {
    1.0 + (wave_index as f32) * 0.06
}

/// 生成一方兵线的路点：与地图数据保持同源，避免小地图与实际行军脱节。
pub fn lane_waypoints(team: Team, lane: usize) -> Vec<Vec2> {
    crate::map::lane_waypoints(team, lane)
        .into_iter()
        .map(|p| Vec2::new(p.x, p.z))
        .collect()
}

/// 兵线 AI 组件：沿路点推进，进入射程后停下来攻击。
#[derive(Component, Clone, Debug)]
pub struct MinionAi {
    pub waypoints: Vec<Vec2>,
    pub index: usize,
    pub lane: usize,
    pub attack_cooldown: f32,
    pub attack_range: f32,
    pub move_speed: f32,
    pub attack_damage: f32,
    pub finished: bool,
}

impl MinionAi {
    pub fn new(team: Team, lane: usize, move_speed: f32, attack_damage: f32) -> Self {
        Self {
            waypoints: lane_waypoints(team, lane),
            index: 0,
            lane,
            attack_cooldown: 0.0,
            attack_range: 2.5,
            move_speed,
            attack_damage,
            finished: false,
        }
    }

    /// 返回本 tick 的位移方向；已到终点返回零向量并置 `finished`。
    pub fn step(&mut self, pos: Vec2, dt: f32) -> Vec2 {
        self.attack_cooldown = (self.attack_cooldown - dt).max(0.0);
        let Some(&wp) = self.waypoints.get(self.index) else {
            self.finished = true;
            return Vec2::ZERO;
        };
        let to = wp - pos;
        if to.length() <= ARRIVE_RADIUS {
            self.index += 1;
            if self.index >= self.waypoints.len() {
                self.finished = true;
            }
            return Vec2::ZERO;
        }
        to.normalize()
    }

    pub fn can_attack(&self) -> bool {
        self.attack_cooldown <= 0.0
    }

    pub fn mark_attack(&mut self, attack_speed: f32) {
        self.attack_cooldown = 1.0 / attack_speed.max(0.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_start_at_900_and_repeat_every_1500() {
        let d = WaveDef::default();
        assert_eq!(d.wave_at(899), None);
        assert_eq!(d.wave_at(900), Some(0));
        assert_eq!(d.wave_at(901), None);
        assert_eq!(d.wave_at(2400), Some(1));
        assert_eq!(d.wave_at(3900), Some(2));
    }

    #[test]
    fn composition_grows_with_wave_index() {
        assert!(!wave_composition(0).contains(&MinionType::Siege));
        assert!(wave_composition(3).contains(&MinionType::Siege));
        assert!(wave_composition(6).contains(&MinionType::Super));
        assert!(wave_scale(5) > wave_scale(0));
    }

    #[test]
    fn blue_and_red_lanes_are_reverse_paths() {
        for lane in 0..crate::map::LANE_COUNT {
            let blue = lane_waypoints(Team::Blue, lane);
            let red = lane_waypoints(Team::Red, lane);
            assert_eq!(blue.len(), red.len());
            // 红方路点应与蓝方完全反向，保证两侧兵线对称
            assert_eq!(blue.first(), red.last());
            assert_eq!(blue.last(), red.first());
        }
    }

    #[test]
    fn minion_reaches_end_of_path() {
        for lane in 0..crate::map::LANE_COUNT {
            let mut ai = MinionAi::new(Team::Blue, lane, 3.5, 20.0);
            let mut pos = *ai.waypoints.first().unwrap();
            for _ in 0..20_000 {
                let dir = ai.step(pos, 1.0 / 60.0);
                if ai.finished {
                    break;
                }
                pos += dir * ai.move_speed / 60.0;
            }
            assert!(ai.finished, "lane {lane} minion must reach enemy core");
        }
    }
}
