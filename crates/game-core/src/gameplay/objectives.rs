//! 中立目标占领模型（`GX-CBT-2026-001` §13）。
//!
//! 全局事件 `ObjectiveCapturedEvent` **只含 objective_id / owner / event_id**，
//! 不含任何实体字段（`VIS-6`、`TC-CBT-10`）。

use crate::components::Team;
use bevy::prelude::*;

/// 每 tick 单人贡献 1 点，多人最多 3 点（§13.1）
pub const MAX_CAPTURE_RATE: i32 = 3;
pub const CAPTURE_RADIUS: f32 = 8.0;

#[derive(Component, Clone, Debug)]
pub struct CapturePoint {
    pub objective_id: u32,
    /// 正值 = Blue 进度，负值 = Red 进度
    pub progress: i32,
    pub threshold: i32,
    pub owner: Option<Team>,
    pub radius: f32,
    pub respawn_ticks: u64,
    pub cooldown_until_tick: u64,
    pub contested: bool,
}

impl CapturePoint {
    pub fn new(objective_id: u32, threshold: i32, respawn_ticks: u64) -> Self {
        Self {
            objective_id,
            progress: 0,
            threshold: threshold.max(1),
            owner: None,
            radius: CAPTURE_RADIUS,
            respawn_ticks,
            cooldown_until_tick: 0,
            contested: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureInput {
    pub blue_count: u32,
    pub red_count: u32,
    pub tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    None,
    Contested,
    Captured(Team),
    OnCooldown,
}

/// 推进一个 tick 的占领进度。纯函数，服务端权威。
pub fn step_capture(cp: &mut CapturePoint, input: CaptureInput) -> CaptureOutcome {
    if input.tick < cp.cooldown_until_tick {
        cp.contested = false;
        return CaptureOutcome::OnCooldown;
    }
    if input.blue_count > 0 && input.red_count > 0 {
        // 争夺时进度冻结
        cp.contested = true;
        return CaptureOutcome::Contested;
    }
    cp.contested = false;

    let delta = input.blue_count as i32 - input.red_count as i32;
    if delta == 0 {
        return CaptureOutcome::None;
    }
    cp.progress = (cp.progress + delta.signum() * delta.abs().min(MAX_CAPTURE_RATE))
        .clamp(-cp.threshold, cp.threshold);

    if cp.progress.abs() >= cp.threshold {
        let team = if cp.progress > 0 {
            Team::Blue
        } else {
            Team::Red
        };
        cp.owner = Some(team);
        cp.progress = 0;
        cp.cooldown_until_tick = input.tick + cp.respawn_ticks;
        CaptureOutcome::Captured(team)
    } else {
        CaptureOutcome::None
    }
}

/// 全局事件（ReliableOrdered + event_id 幂等）。字段集合固定为 3 项，禁止加入实体信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveCapturedEvent {
    pub objective_id: u32,
    pub owner: Team,
    pub event_id: u64,
}

pub const EVENT_FIELD_COUNT: usize = 3;

pub fn capture_event(cp: &CapturePoint, tick: u64) -> Option<ObjectiveCapturedEvent> {
    cp.owner.map(|owner| ObjectiveCapturedEvent {
        objective_id: cp.objective_id,
        owner,
        // 确定性 id：同一目标在同一 tick 重复广播时幂等
        event_id: (tick << 8) | (cp.objective_id as u64 & 0xff),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_cbt_10_contested_freezes_progress() {
        let mut cp = CapturePoint::new(1, 100, 5400);
        assert_eq!(
            step_capture(
                &mut cp,
                CaptureInput {
                    blue_count: 1,
                    red_count: 0,
                    tick: 0
                }
            ),
            CaptureOutcome::None
        );
        let before = cp.progress;
        assert_eq!(
            step_capture(
                &mut cp,
                CaptureInput {
                    blue_count: 2,
                    red_count: 1,
                    tick: 1
                }
            ),
            CaptureOutcome::Contested
        );
        assert_eq!(cp.progress, before);
        assert!(cp.contested);
    }

    #[test]
    fn capture_rate_is_capped_and_owner_goes_on_cooldown() {
        let mut cp = CapturePoint::new(2, 10, 100);
        let outcome = step_capture(
            &mut cp,
            CaptureInput {
                blue_count: 6,
                red_count: 0,
                tick: 0,
            },
        );
        assert_eq!(outcome, CaptureOutcome::None);
        assert_eq!(cp.progress, 3, "rate must be capped at 3/tick");
        let mut ticks = 0;
        loop {
            ticks += 1;
            if step_capture(
                &mut cp,
                CaptureInput {
                    blue_count: 1,
                    red_count: 0,
                    tick: ticks,
                },
            ) == CaptureOutcome::Captured(Team::Blue)
            {
                break;
            }
            assert!(ticks < 20);
        }
        assert_eq!(cp.owner, Some(Team::Blue));
        assert_eq!(
            step_capture(
                &mut cp,
                CaptureInput {
                    blue_count: 1,
                    red_count: 0,
                    tick: ticks + 1
                }
            ),
            CaptureOutcome::OnCooldown
        );
    }

    #[test]
    fn tc_cbt_10_event_has_no_entity_fields() {
        let mut cp = CapturePoint::new(7, 1, 0);
        step_capture(
            &mut cp,
            CaptureInput {
                blue_count: 1,
                red_count: 0,
                tick: 42,
            },
        );
        let event = capture_event(&cp, 42).unwrap();
        assert_eq!(EVENT_FIELD_COUNT, 3);
        assert_eq!(event.objective_id, 7);
        assert_eq!(event.owner, Team::Blue);
        let debug = format!("{event:?}");
        assert!(!debug.contains("NetId"));
        assert!(!debug.contains("Entity"));
        assert!(!debug.contains("position"));
    }

    #[test]
    fn red_capture_uses_negative_progress() {
        let mut cp = CapturePoint::new(3, 2, 0);
        step_capture(
            &mut cp,
            CaptureInput {
                blue_count: 0,
                red_count: 3,
                tick: 0,
            },
        );
        assert_eq!(
            step_capture(
                &mut cp,
                CaptureInput {
                    blue_count: 0,
                    red_count: 3,
                    tick: 1
                }
            ),
            CaptureOutcome::Captured(Team::Red)
        );
    }
}
