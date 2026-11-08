//! 权威对局时钟与阶段（`GX-CBT-2026-001` §1.2 / §15.3）。
//!
//! 所有玩法判定只认 `MatchClock.tick`（由服务端 `ServerTick` 同步），
//! 不使用 `Res<Time>` 墙钟，保证回放可复现。

use crate::components::Team;
use bevy::prelude::*;

pub const TICKS_PER_MINUTE: u64 = 60 * 60;
/// 目标时长 18 分钟
pub const MATCH_TARGET_TICKS: u64 = 18 * TICKS_PER_MINUTE;
/// 加时衰竭起点 22 分钟
pub const OVERTIME_START_TICKS: u64 = 22 * TICKS_PER_MINUTE;
/// 硬上限 25 分钟，到点强制按源核剩余生命比例判胜
pub const MATCH_HARD_CAP_TICKS: u64 = 25 * TICKS_PER_MINUTE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// 单人也能开局：服务端补 AI 对手与 PvE 内容
    #[default]
    Pve,
    /// 需要真人对手
    Pvp,
}

impl MatchMode {
    /// 协议编码：0 = PVE，1 = PVP；未知值一律回退 PVE。
    pub const fn code(self) -> u8 {
        match self {
            Self::Pve => 0,
            Self::Pvp => 1,
        }
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Pvp,
            _ => Self::Pve,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatchPhase {
    #[default]
    Warmup,
    InProgress,
    Overtime,
    Finished,
}

#[derive(Resource, Clone, Debug)]
pub struct MatchClock {
    /// 对局内 tick，恒等于 `ServerTick - start_tick`（P-A 唯一时钟基准）。
    pub tick: u64,
    /// 本局开局瞬间的权威 `ServerTick`，在 `ServerState::Running` 首帧记录。
    pub start_tick: u64,
    pub mode: MatchMode,
    pub phase: MatchPhase,
    pub winner: Option<Team>,
    /// 胜负已定后是否停止玩法模拟（冻结语义由 `match_running` 读取）。
    pub frozen: bool,
}

impl Default for MatchClock {
    fn default() -> Self {
        Self {
            tick: 0,
            start_tick: 0,
            mode: MatchMode::Pve,
            phase: MatchPhase::Warmup,
            winner: None,
            frozen: false,
        }
    }
}

impl MatchClock {
    pub fn new(mode: MatchMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn start(&mut self) {
        self.tick = 0;
        self.phase = MatchPhase::InProgress;
        self.winner = None;
        self.frozen = false;
    }

    /// 在开局帧以权威 `ServerTick` 为基准启动，之后只允许 `sync` 推进。
    pub fn start_at(&mut self, server_tick: u64) {
        self.start();
        self.start_tick = server_tick;
    }

    /// 由权威 `ServerTick` 同步（生产唯一路径）：`tick` 不再独立自增，
    /// 彻底消除与 `ServerTick` / `SimTime` 的偏移漂移。
    pub fn sync(&mut self, server_tick: u64) -> bool {
        self.advance_to(server_tick.saturating_sub(self.start_tick))
    }

    /// 离线推演 / 单测用：按本地 tick 前进一步。生产禁止调用。
    pub fn step(&mut self) -> bool {
        let next = self.tick.saturating_add(1);
        self.advance_to(next)
    }

    fn advance_to(&mut self, tick: u64) -> bool {
        if self.phase == MatchPhase::Finished {
            return false;
        }
        self.tick = tick;
        if self.phase == MatchPhase::Warmup {
            self.phase = MatchPhase::InProgress;
        }
        if tick >= OVERTIME_START_TICKS && self.phase == MatchPhase::InProgress {
            self.phase = MatchPhase::Overtime;
            return true;
        }
        false
    }

    /// 加时衰竭：真实伤害、无视护盾与减伤（§5.3 / §1.2）。
    pub fn exhaustion_active(&self) -> bool {
        matches!(self.phase, MatchPhase::Overtime)
    }

    /// 18 分钟目标时长（UI 展示 / 未来阶段目标用）。
    pub fn target_reached(&self) -> bool {
        self.tick >= MATCH_TARGET_TICKS
    }

    pub fn hard_cap_reached(&self) -> bool {
        self.tick >= MATCH_HARD_CAP_TICKS
    }

    /// 幂等：胜负已定后不再覆盖 winner，防止两处判胜路径重复播报。
    pub fn finish(&mut self, winner: Team) {
        if self.is_over() {
            return;
        }
        self.phase = MatchPhase::Finished;
        self.winner = Some(winner);
        self.frozen = true;
    }

    pub fn is_over(&self) -> bool {
        self.phase == MatchPhase::Finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_enters_overtime_at_22_minutes() {
        let mut c = MatchClock::new(MatchMode::Pve);
        c.start();
        c.tick = OVERTIME_START_TICKS - 2;
        assert!(!c.step());
        assert!(!c.exhaustion_active());
        assert!(c.step());
        assert_eq!(c.phase, MatchPhase::Overtime);
        assert!(c.exhaustion_active());
        assert!(!c.step(), "overtime must only trigger once");
    }

    #[test]
    fn finish_freezes_clock() {
        let mut c = MatchClock::new(MatchMode::Pve);
        c.start();
        c.finish(Team::Blue);
        let t = c.tick;
        c.step();
        assert_eq!(c.tick, t);
        assert_eq!(c.winner, Some(Team::Blue));
        assert!(c.is_over());
    }

    #[test]
    fn pve_is_default_for_single_player() {
        assert_eq!(MatchClock::default().mode, MatchMode::Pve);
        assert!(!MatchClock::default().is_over());
    }

    #[test]
    fn sync_tracks_authoritative_server_tick() {
        let mut c = MatchClock::new(MatchMode::Pve);
        c.start_at(100);
        for step in 1..=600u64 {
            assert!(!c.sync(100 + step));
            assert_eq!(c.tick, step, "tick 必须恒等于 ServerTick - start_tick");
        }
        assert_eq!(c.phase, MatchPhase::InProgress);
        assert!(!c.target_reached());
        assert!(!c.hard_cap_reached());
    }

    #[test]
    fn warmup_first_frame_does_not_lose_a_tick() {
        let mut c = MatchClock::default();
        assert_eq!(c.phase, MatchPhase::Warmup);
        c.start_at(50);
        c.sync(50);
        assert_eq!(c.tick, 0);
        c.sync(51);
        assert_eq!(c.tick, 1, "开局后第一帧即应推进到 tick=1");
        assert_eq!(c.phase, MatchPhase::InProgress);
    }

    #[test]
    fn finish_is_idempotent_and_freezes() {
        let mut c = MatchClock::new(MatchMode::Pve);
        c.start_at(0);
        c.sync(10);
        c.finish(Team::Blue);
        c.finish(Team::Red);
        assert_eq!(c.winner, Some(Team::Blue), "重复判胜不得覆盖 winner");
        assert!(c.frozen);
        assert!(!c.sync(999), "冻结后不再播报加时");
        assert_eq!(c.tick, 10, "冻结后 tick 不再变化");
    }

    #[test]
    fn phase_thresholds_are_ordered_and_target_is_used() {
        const {
            assert!(MATCH_TARGET_TICKS < OVERTIME_START_TICKS);
            assert!(OVERTIME_START_TICKS < MATCH_HARD_CAP_TICKS);
        }
        let mut c = MatchClock::new(MatchMode::Pve);
        c.start_at(0);
        c.sync(MATCH_TARGET_TICKS);
        assert!(c.target_reached());
        assert!(!c.hard_cap_reached());
        assert!(c.sync(OVERTIME_START_TICKS), "22 分钟应触发一次加时");
        assert!(c.exhaustion_active());
        assert!(!c.sync(OVERTIME_START_TICKS + 1), "加时只播报一次");
        c.sync(MATCH_HARD_CAP_TICKS);
        assert!(c.hard_cap_reached());
    }
}
