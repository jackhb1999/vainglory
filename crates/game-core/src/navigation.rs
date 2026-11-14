//! 服务端权威移动合法性校验（`docs/05_COMBAT_SPEC.md` §24.3 `NAV-M2-03`）。
//!
//! §3.3 规定「客户端负责路径质量、服务端负责合法性」，此前合法性部分一直空着：
//! `movement_system` 直接按 `Hero.move_speed × dt` 积分并只钳制地图边界，
//! 客户端上报的 `InputCmd` 也未做有限性检查 —— NaN 一旦进入积分就会永久污染
//! 权威位置与之后所有快照。本模块把校验做成**纯函数**，在位置积分后立即调用，
//! 拒绝时回退到上一 tick 的安全位置，并把连续违规计入审计资源。
//!
//! 静态地形仍直接读 `map::wall_colliders()`：它是服务端静态碰撞体、客户端预测、
//! A* 障碍、体素渲染与小地图共用的唯一真相源，圆形碰撞体比粗格更精确。
//! `NAV-M2-01` 落地后，本模块额外接受 `navgrid::NavGrid`，用其**动态障碍层**
//! （哨塔 / 源核 / 渊卫）做粗粒度格判定 —— 静态墙若也用 7.5 单位的格，
//! 会把坡道通行带凭空缩窄两格，属于误判而非更严格。

use crate::map;
use crate::navgrid::NavGrid;
use bevy::math::Vec2;
use bevy::prelude::*;
use std::collections::HashMap;

/// 允许位移的容差系数（§24.3 `NAV-M2-03-C1`）。
pub const SPEED_TOLERANCE: f32 = 1.05;
/// 碰撞线段采样步长上限（单位）。服务端 `CELL_SIZE = 7.5`，取其约 1/7 更保守。
pub const COLLISION_SAMPLE_STEP: f32 = 1.0;
/// 浮点比较余量。
pub const VALIDATION_EPS: f32 = 1e-3;
/// 连续违规达到此次数写 warn 日志（§24.3 `NAV-M2-03-C6`）。
pub const VIOLATION_WARN_THRESHOLD: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 输入含 NaN / Inf，或参数非有限值。
    InvalidInput,
    /// 单次位移超过 `move_speed × dt × 3`：疑似瞬移 / 加速。
    Teleport,
    /// 超出地图边界（回退到钳制后的安全点）。
    OutOfBounds,
    /// 超过 `move_speed × dt × SPEED_TOLERANCE`。
    SpeedExceeded,
    /// 位移线段采样点落在兵线 / 野区堤墙内。
    WallCollision,
}

#[derive(Clone, Copy, Debug)]
pub struct MovementValidation {
    pub accepted: bool,
    /// 接受时为 `proposed`；拒绝时为安全位置（通常是 `prev`）。
    pub corrected_position: Vec2,
    pub reject_reason: Option<RejectReason>,
}

/// 连续违规审计（不改变裁决结果；是否断开由 M3 反作弊策略决定）。
///
/// key 为服务端实体（ECS 实体在对局内稳定），value 为
/// `(连续违规次数, 最后一次违规 tick)`。
#[derive(Clone, Debug, Resource, Default)]
pub struct MovementViolations {
    pub per_entity: HashMap<Entity, (u8, u64)>,
}

impl MovementViolations {
    /// 记一次违规并返回**连续**违规次数（相邻 tick 才算连续，C6 / `TC-NAV-09`）。
    pub fn record(&mut self, entity: Entity, tick: u64) -> u8 {
        let entry = self.per_entity.entry(entity).or_insert((0, tick));
        entry.0 = if tick <= entry.1 + 1 {
            entry.0.saturating_add(1)
        } else {
            1
        };
        entry.1 = tick;
        entry.0
    }
}

/// 输入清洗：`InputCmd.move_dir` 是客户端上报的归一化方向，必须为有限值且长度 ≤ 1。
///
/// 超长向量按方向归一化（不采纳其长度意图）；NaN / Inf 直接归零。
/// 这一步必须在写入 `PlayerInput` **之前**做，否则非有限值会流进位置积分，
/// 之后所有快照的 `position` 都变成 NaN，且无法自愈。
pub fn sanitize_move_dir(dir: Vec2) -> Vec2 {
    if !dir.x.is_finite() || !dir.y.is_finite() {
        return Vec2::ZERO;
    }
    let len = dir.length();
    if len <= 1.0 + VALIDATION_EPS {
        dir
    } else if len > 0.0 {
        dir / len
    } else {
        Vec2::ZERO
    }
}

/// 与服务端 `StaticCollider` **同源**的墙碰撞圆（延迟缓存，避免每 tick 重采样长墙）。
///
/// 不能用 `LANE_WALLS` 的 AABB 近似：服务端 `wall_colliders()` 把长墙采样成连续圆，
/// 段端是圆角；AABB 会把段端外扩成方角，等于悄悄缩窄坡道开口，玩家在坡道口被卡。
fn wall_circles() -> &'static [(f32, f32, f32)] {
    static CACHE: std::sync::OnceLock<Vec<(f32, f32, f32)>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            map::wall_colliders()
                .into_iter()
                .map(|(c, r)| (c.x, c.z, r))
                .collect()
        })
        .as_slice()
}

/// 采样点是否落在兵线 / 野区堤墙的权威碰撞圆内。
pub fn is_wall(p: Vec2) -> bool {
    wall_circles().iter().any(|&(x, z, r)| {
        let dx = p.x - x;
        let dz = p.y - z;
        dx * dx + dz * dz <= r * r
    })
}

fn rejected(prev: Vec2, reason: RejectReason) -> MovementValidation {
    MovementValidation {
        accepted: false,
        corrected_position: prev,
        reject_reason: Some(reason),
    }
}

/// 在服务端 `FixedUpdate`、位置积分之后调用（§24.3）。
///
/// 判定顺序按「危害从大到小」：非有限值 → 瞬移 → 越界 → 超速 → 撞墙。
/// 拒绝时一律回退到 `prev`（越界例外：钳制到边界内的安全点），
/// 由下一次权威快照把修正位置下发给客户端回滚预测，不做静默瞬移。
pub fn validate_movement(
    prev: Vec2,
    proposed: Vec2,
    move_speed: f32,
    dt: f32,
) -> MovementValidation {
    validate_impl(prev, proposed, move_speed, dt, None)
}

/// `NAV-M2-03` + `NAV-M2-01-C8`：额外用服务端 `ServerNavGrid` 的动态障碍层裁决。
/// 静态墙仍由精确圆形碰撞体判断；格判定只看动态障碍，避免坡道误判（`TC-NAV-08`）。
pub fn validate_movement_with_grid(
    prev: Vec2,
    proposed: Vec2,
    move_speed: f32,
    dt: f32,
    grid: &NavGrid,
) -> MovementValidation {
    validate_impl(prev, proposed, move_speed, dt, Some(grid))
}

fn validate_impl(
    prev: Vec2,
    proposed: Vec2,
    move_speed: f32,
    dt: f32,
    grid: Option<&NavGrid>,
) -> MovementValidation {
    if !prev.x.is_finite()
        || !prev.y.is_finite()
        || !proposed.x.is_finite()
        || !proposed.y.is_finite()
        || !move_speed.is_finite()
        || !dt.is_finite()
        || dt <= 0.0
        || move_speed <= 0.0
    {
        return rejected(prev, RejectReason::InvalidInput);
    }

    let delta = proposed - prev;
    let dist = delta.length();
    let max_step = move_speed * dt;

    // C4：单次位移超过 3 倍步长视为瞬移（重连基线补发的豁免留给上层）。
    if dist > max_step * 3.0 + VALIDATION_EPS {
        return rejected(prev, RejectReason::Teleport);
    }

    // C3：越界钳制到边界内的安全点，而不是回退（正常积分已钳制，这里是防御纵深）。
    let cx = proposed
        .x
        .clamp(-map::MAP_W_HALF + 1.0, map::MAP_W_HALF - 1.0);
    let cz = proposed
        .y
        .clamp(-map::MAP_H_HALF + 1.0, map::MAP_H_HALF - 1.0);
    if (cx - proposed.x).abs() > VALIDATION_EPS || (cz - proposed.y).abs() > VALIDATION_EPS {
        return MovementValidation {
            accepted: false,
            corrected_position: Vec2::new(cx, cz),
            reject_reason: Some(RejectReason::OutOfBounds),
        };
    }

    // C1：允许位移 ≤ move_speed × dt × 1.05 + EPS，move_speed 取服务端 Hero 值。
    if dist > max_step * SPEED_TOLERANCE + VALIDATION_EPS {
        return rejected(prev, RejectReason::SpeedExceeded);
    }

    // C2：线段采样（步长 ≤ COLLISION_SAMPLE_STEP），样本落在墙碰撞圆或动态障碍格即拒绝。
    // 自愈分两种，避免「出生在源核格内」变成穿墙许可证：
    // - 起点落在墙碰撞圆内：整段放行（旧快照 / 回滚导致的异常状态）；
    // - 起点落在动态障碍格内：只跳过动态格判定，静态墙的圆判定仍然生效。
    // 否则每 tick 都因「样本在障碍内」被回退，英雄会被永久锁死在出生点。
    if is_wall(prev) {
        return MovementValidation {
            accepted: true,
            corrected_position: proposed,
            reject_reason: None,
        };
    }
    let prev_in_dynamic = grid.is_some_and(|g| g.is_dynamic_blocked_world(prev));
    let blocked = |p: Vec2| {
        is_wall(p) || (!prev_in_dynamic && grid.is_some_and(|g| g.is_dynamic_blocked_world(p)))
    };
    let steps = (dist / COLLISION_SAMPLE_STEP).ceil().max(1.0) as usize;
    for i in 1..=steps {
        let p = prev + delta * (i as f32 / steps as f32);
        if blocked(p) {
            return rejected(prev, RejectReason::WallCollision);
        }
    }

    MovementValidation {
        accepted: true,
        corrected_position: proposed,
        reject_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = crate::systems::FIXED_TIMESTEP;
    const SPEED: f32 = 8.0;

    #[test]
    fn normal_step_is_accepted() {
        let prev = Vec2::new(10.0, 20.0);
        let proposed = prev + Vec2::X * (SPEED * DT);
        let v = validate_movement(prev, proposed, SPEED, DT);
        assert!(v.accepted);
        assert!(v.reject_reason.is_none());
        assert_eq!(v.corrected_position, proposed);
    }

    #[test]
    fn teleport_and_speed_hacks_are_rejected() {
        let prev = Vec2::new(0.0, 0.0);
        // 3 倍步长以上 = 瞬移
        let jump = validate_movement(prev, Vec2::new(100.0, 0.0), SPEED, DT);
        assert_eq!(jump.reject_reason, Some(RejectReason::Teleport));
        assert_eq!(jump.corrected_position, prev);
        // 1.5 倍步长 = 加速但未到瞬移阈值
        let fast = validate_movement(prev, Vec2::new(SPEED * DT * 1.5, 0.0), SPEED, DT);
        assert_eq!(fast.reject_reason, Some(RejectReason::SpeedExceeded));
        assert_eq!(fast.corrected_position, prev);
    }

    #[test]
    fn non_finite_input_never_pollutes_position() {
        assert_eq!(sanitize_move_dir(Vec2::new(f32::NAN, 1.0)), Vec2::ZERO);
        assert_eq!(sanitize_move_dir(Vec2::new(f32::INFINITY, 0.0)), Vec2::ZERO);
        assert_eq!(sanitize_move_dir(Vec2::X * 99.0), Vec2::X);
        assert_eq!(sanitize_move_dir(Vec2::X * 0.5), Vec2::X * 0.5);

        let prev = Vec2::ZERO;
        let v = validate_movement(prev, Vec2::new(f32::NAN, 0.0), SPEED, DT);
        assert_eq!(v.reject_reason, Some(RejectReason::InvalidInput));
        assert_eq!(v.corrected_position, prev);
    }

    #[test]
    fn out_of_bounds_is_clamped_into_map() {
        let prev = Vec2::new(map::MAP_W_HALF - 2.0, 0.0);
        let v = validate_movement(prev, Vec2::new(map::MAP_W_HALF + 50.0, 0.0), SPEED, DT);
        // 越界同时构成瞬移，按危害优先返回 Teleport；两者都不接受
        assert!(!v.accepted);
        assert!(v.corrected_position.x <= map::MAP_W_HALF);
    }

    /// 位移线段进入堤墙碰撞圆必须拒绝；与服务端 `wall_colliders()` 同源，不另建网格。
    #[test]
    fn wall_segment_is_rejected_by_sampling() {
        let &(x, z, r) = &wall_circles()[0];
        let prev = Vec2::new(x, z - r - 0.05);
        let proposed = Vec2::new(x, z - r + 0.05);
        assert!(!is_wall(prev), "起点必须在墙外");
        assert!(is_wall(proposed), "终点必须刚进入墙内");
        let v = validate_movement(prev, proposed, SPEED, DT);
        assert_eq!(v.reject_reason, Some(RejectReason::WallCollision));
        assert_eq!(v.corrected_position, prev);
    }

    /// 起点意外落在墙内时必须能走出来，不能被墙校验永久锁死。
    #[test]
    fn hero_inside_wall_can_self_heal_out() {
        let &(x, z, _) = &wall_circles()[0];
        let center = Vec2::new(x, z);
        assert!(is_wall(center));
        let v = validate_movement(center, center + Vec2::Y * (SPEED * DT), SPEED, DT);
        assert!(v.accepted, "自愈位移不应被拒绝");
    }

    /// `TC-NAV-07`：正常速度 `accepted = true`；`1.06×` 超限返回
    /// `SpeedExceeded` 且 `corrected_position == prev`。
    #[test]
    fn tc_nav_07_speed_tolerance_boundary() {
        let prev = Vec2::new(10.0, 20.0);
        let normal = validate_movement(prev, prev + Vec2::X * (SPEED * DT), SPEED, DT);
        assert!(normal.accepted);
        assert_eq!(normal.corrected_position, prev + Vec2::X * (SPEED * DT));

        let over = validate_movement(prev, prev + Vec2::X * (SPEED * DT * 1.06), SPEED, DT);
        assert_eq!(over.reject_reason, Some(RejectReason::SpeedExceeded));
        assert_eq!(over.corrected_position, prev);
        assert!(!over.accepted);
    }

    /// `TC-NAV-08`：穿越 `Turret` 障碍格的线段被拒；沿障碍边缘擦过的合法路径不误判。
    #[test]
    fn tc_nav_08_turret_cell_crossing_vs_edge_grazing() {
        use crate::navgrid::{ObstacleKind, cell_to_world};

        // 中心格 (32,16) 远离静态堤墙（墙在 gz≈9/10），隔离出纯动态障碍场景。
        let center = cell_to_world((32, 16));
        let mut grid = NavGrid::new();
        grid.mark(ObstacleKind::Turret, center, true);

        // 格 (31,16) 的西边界 x = -7.5：从格外 0.05 跨进格内 0.05。
        let crossing_from = Vec2::new(-7.55, center.y);
        let crossing_to = Vec2::new(-7.45, center.y);
        assert!(!grid.is_dynamic_blocked_world(crossing_from));
        assert!(grid.is_dynamic_blocked_world(crossing_to));
        let crossing = validate_movement_with_grid(crossing_from, crossing_to, SPEED, DT, &grid);
        assert_eq!(crossing.reject_reason, Some(RejectReason::WallCollision));
        assert_eq!(crossing.corrected_position, crossing_from);

        // 沿阻挡区南边缘（gz=14，未阻挡）擦过：同一格内、不进入任何障碍格。
        let graze_from = Vec2::new(-7.55, -7.6);
        let graze_to = Vec2::new(-7.45, -7.6);
        assert!(!grid.is_dynamic_blocked_world(graze_from));
        assert!(!grid.is_dynamic_blocked_world(graze_to));
        let graze = validate_movement_with_grid(graze_from, graze_to, SPEED, DT, &grid);
        assert!(graze.accepted, "沿障碍边缘擦过不得被误判");
        assert_eq!(graze.corrected_position, graze_to);
    }

    /// `TC-NAV-09` 的一部分：连续 3 次违规计数为 3（驱动 warn 日志），
    /// 中间断一 tick 后重新从 1 开始。
    #[test]
    fn tc_nav_09_three_consecutive_violations() {
        let mut v = MovementViolations::default();
        let mut world = World::new();
        let e = world.spawn_empty().id();
        assert_eq!(v.record(e, 10), 1);
        assert_eq!(v.record(e, 11), 2);
        let count = v.record(e, 12);
        assert_eq!(count, 3);
        assert!(
            count >= VIOLATION_WARN_THRESHOLD,
            "第 3 次必须触发 warn 日志"
        );
        assert_eq!(v.per_entity[&e], (3, 12));
        assert_eq!(v.record(e, 20), 1, "非相邻 tick 不算连续");
    }
}
