//! 双端同源的动态障碍寻路网格（`docs/05_COMBAT_SPEC.md` §24.1 `NAV-M2-01`）。
//!
//! 规格的 `NavGrid` 放在客户端、`ServerNavGrid` 放在服务端，要求「同一 blocked 数据」。
//! 这里把网格类型与几何常量**下沉到 `game-core`**，两端使用同一个 `NavGrid`：
//! 静态地形由 `map::LANE_WALLS` 烘焙一次，动态障碍（哨塔 / 源核 / 渊卫）只能来自
//! 服务端下发的 `NavObstacleSnapshot`（C1），客户端禁止从渲染实体推断（渲染实体经
//! 逐玩家视野裁剪，拿不到全图障碍）。这样「同源」由类型系统保证，而不是靠两处实现对齐。
//!
//! 网格尺寸按当前 **2:1 长方形地图**（480×240）派生为 64×32、格宽 7.5；
//! 规格里的 `NAV_GRID_N = 64` 写于正方形地图时期，若照搬会让 z 轴整片越界。

use crate::map;
use bevy::math::Vec2;
use bevy::prelude::*;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 寻路网格分轴尺寸；`CELL_SIZE = MAP_WIDTH / NX = 7.5`。
pub const NAV_GRID_NX: i32 = 64;
pub const NAV_GRID_NZ: i32 = 32;
pub const NAV_CELL_SIZE: f32 = map::MAP_WIDTH / NAV_GRID_NX as f32;
pub const NAV_GRID_HALF_X: f32 = map::MAP_W_HALF;
pub const NAV_GRID_HALF_Z: f32 = map::MAP_H_HALF;
pub const NAV_CELL_COUNT: usize = (NAV_GRID_NX * NAV_GRID_NZ) as usize;
/// 网格重建周期（tick），与 20Hz 快照对齐（C4）。
pub const NAV_REBUILD_INTERVAL_TICKS: u64 = 30;
/// A* 单次节点访问上限（§24.1 C5 / §24.2 C5）。
pub const A_STAR_NODE_BUDGET: usize = 4096;
/// 起/终点落在障碍格时的最大钳制半径（格，C6）。
pub const NAV_CLAMP_RADIUS_CELLS: i32 = 3;

/// 世界坐标 `(x, z)` → 网格坐标。
pub fn world_to_cell(p: Vec2) -> (i32, i32) {
    (
        ((p.x + NAV_GRID_HALF_X) / NAV_CELL_SIZE).floor() as i32,
        ((p.y + NAV_GRID_HALF_Z) / NAV_CELL_SIZE).floor() as i32,
    )
}

/// 网格坐标 → 格中心世界坐标。
pub fn cell_to_world(c: (i32, i32)) -> Vec2 {
    Vec2::new(
        (c.0 as f32 + 0.5) * NAV_CELL_SIZE - NAV_GRID_HALF_X,
        (c.1 as f32 + 0.5) * NAV_CELL_SIZE - NAV_GRID_HALF_Z,
    )
}

pub fn cell_in_bounds(c: (i32, i32)) -> bool {
    c.0 >= 0 && c.0 < NAV_GRID_NX && c.1 >= 0 && c.1 < NAV_GRID_NZ
}

fn cell_index(c: (i32, i32)) -> usize {
    (c.1 * NAV_GRID_NX + c.0) as usize
}

/// 障碍类型；半径是世界单位，`cell_radius = ceil(radius / CELL_SIZE)`（C2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObstacleKind {
    StaticTerrain,
    Turret,
    SourceCore,
    AbyssWarden,
}

impl ObstacleKind {
    pub const fn radius(self) -> f32 {
        match self {
            ObstacleKind::StaticTerrain => 0.0,
            ObstacleKind::Turret => 3.0,
            ObstacleKind::SourceCore => 3.0,
            ObstacleKind::AbyssWarden => 4.0,
        }
    }

    /// 半径 → 格数取 `ceil(radius / CELL_SIZE)`，至少 1 格（C2）。
    pub fn cell_radius(self) -> i32 {
        (self.radius() / NAV_CELL_SIZE).ceil().max(1.0) as i32
    }
}

/// 服务端下发的**全局**动态障碍视图（不经视野裁剪）。
///
/// 用 `[f32; 2]` 而不是 `bevy::Vec2`，因为 `ServerMessage` 要 bitcode / serde 编解码。
/// `turrets` 的 bool 是存活位：哨塔被摧毁后实体仍以 `health = 0` 常驻 ECS 并继续下发，
/// 必须由服务端显式给出存活状态，客户端按 `health` 猜既不可靠也违反 C1。
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct NavObstacleSnapshot {
    pub turrets: Vec<([f32; 2], bool)>,
    pub source_cores: Vec<[f32; 2]>,
    pub abyss_warden: Option<[f32; 2]>,
    pub server_tick: u64,
}

impl NavObstacleSnapshot {
    /// 障碍集合是否相同（忽略 `server_tick`），用于「变化立即重建、无变化按 30 tick 节流」。
    pub fn same_obstacles(&self, other: &Self) -> bool {
        self.turrets == other.turrets
            && self.source_cores == other.source_cores
            && self.abyss_warden == other.abyss_warden
    }

    pub fn pos(p: [f32; 2]) -> Vec2 {
        Vec2::new(p[0], p[1])
    }

    pub fn xy(p: Vec2) -> [f32; 2] {
        [p.x, p.y]
    }
}

/// 静态地形烘焙（进程内一次）。
///
/// - x 方向按墙半厚外扩，与服务端圆形 `StaticCollider` 口径一致，坡道开口保持可通行；
/// - z 方向外扩半格，保证墙至少占满一整行格，A* 无法从格间缝隙斜穿。
fn bake_static_terrain() -> Vec<bool> {
    let mut blocked = vec![false; NAV_CELL_COUNT];
    let half_t = map::WALL_HALF_THICKNESS;
    for w in map::LANE_WALLS {
        for gx in 0..NAV_GRID_NX {
            let cx = (gx as f32 + 0.5) * NAV_CELL_SIZE - NAV_GRID_HALF_X;
            if cx < w.x0 - half_t || cx > w.x1 + half_t {
                continue;
            }
            for gz in 0..NAV_GRID_NZ {
                let cz = (gz as f32 + 0.5) * NAV_CELL_SIZE - NAV_GRID_HALF_Z;
                if (cz - w.z).abs() <= half_t + NAV_CELL_SIZE * 0.5 {
                    blocked[cell_index((gx, gz))] = true;
                }
            }
        }
    }
    blocked
}

fn static_mask() -> &'static [bool] {
    static CACHE: OnceLock<Vec<bool>> = OnceLock::new();
    CACHE.get_or_init(bake_static_terrain).as_slice()
}

/// 静态地形阻挡格（兼容旧 A* 的 `&[(i32,i32)]` 调用方与既有单测）。
pub fn static_terrain_cells() -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for gz in 0..NAV_GRID_NZ {
        for gx in 0..NAV_GRID_NX {
            if static_mask()[cell_index((gx, gz))] {
                out.push((gx, gz));
            }
        }
    }
    out
}

fn mark_into(mask: &mut [bool], kind: ObstacleKind, center: Vec2) {
    let cr = kind.cell_radius();
    let (cx, cz) = world_to_cell(center);
    for dz in -cr..=cr {
        for dx in -cr..=cr {
            let c = (cx + dx, cz + dz);
            if cell_in_bounds(c) {
                mask[cell_index(c)] = true;
            }
        }
    }
}

/// 寻路 / 校验共用的网格（`Resource`，两端同类型）。
///
/// `blocked = static_blocked | dynamic_blocked`；`version` 只在 `blocked` **实际变化**
/// 时 `saturating_add(1)`（u32 单调、不取模，C3）。`dynamic_blocked` 单独留一份，
/// 供 `NAV-M2-03` 做粗粒度动态障碍校验 —— 静态墙有精确圆形碰撞体，用格判定会把
/// 通行带凭空缩窄两格（详见 `navigation::validate_movement_with_grid`）。
#[derive(Clone, Debug, Resource)]
pub struct NavGrid {
    pub blocked: Vec<bool>,
    pub version: u32,
    static_blocked: Vec<bool>,
    dynamic_blocked: Vec<bool>,
    last_obstacles: Option<NavObstacleSnapshot>,
    last_server_tick: u64,
    last_rebuild_tick: u64,
}

impl Default for NavGrid {
    fn default() -> Self {
        let static_blocked = static_mask().to_vec();
        Self {
            blocked: static_blocked.clone(),
            version: 0,
            static_blocked,
            dynamic_blocked: vec![false; NAV_CELL_COUNT],
            last_obstacles: None,
            last_server_tick: 0,
            last_rebuild_tick: 0,
        }
    }
}

impl NavGrid {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试 / 兼容用：在静态地形之上再标一批阻挡格。
    pub fn with_extra_cells(cells: &[(i32, i32)]) -> Self {
        let mut g = Self::new();
        for &c in cells {
            if cell_in_bounds(c) {
                g.blocked[cell_index(c)] = true;
                g.dynamic_blocked[cell_index(c)] = true;
            }
        }
        g
    }

    pub fn is_blocked(&self, c: (i32, i32)) -> bool {
        !cell_in_bounds(c) || self.blocked[cell_index(c)]
    }

    pub fn is_blocked_world(&self, p: Vec2) -> bool {
        self.is_blocked(world_to_cell(p))
    }

    /// 仅动态障碍（C7 / `NAV-M2-03`）：静态墙由精确圆形碰撞体裁决，不用粗格。
    pub fn is_dynamic_blocked_world(&self, p: Vec2) -> bool {
        let c = world_to_cell(p);
        cell_in_bounds(c) && self.dynamic_blocked[cell_index(c)]
    }

    pub fn blocked_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for gz in 0..NAV_GRID_NZ {
            for gx in 0..NAV_GRID_NX {
                if self.blocked[cell_index((gx, gz))] {
                    out.push((gx, gz));
                }
            }
        }
        out
    }

    /// 标一个动态障碍；`alive = false` 表示已摧毁 / 未刷新，不写入阻挡（C7）。
    pub fn mark(&mut self, kind: ObstacleKind, center: Vec2, alive: bool) {
        if alive {
            mark_into(&mut self.dynamic_blocked, kind, center);
        }
        self.refresh_union();
    }

    /// 重算 `blocked` 并**只在集合真的变化时**递增 `version`（C3）。
    fn refresh_union(&mut self) -> bool {
        let next: Vec<bool> = self
            .static_blocked
            .iter()
            .zip(&self.dynamic_blocked)
            .map(|(s, d)| *s || *d)
            .collect();
        let changed = next != self.blocked;
        self.blocked = next;
        if changed {
            self.version = self.version.saturating_add(1);
        }
        changed
    }

    /// 从服务端权威障碍视图重建；返回当前 `version`（C3/C4/C7）。
    ///
    /// - 同一个 `server_tick` 的重复快照直接跳过，绝不重复 `rebuild`；
    /// - 障碍集合变化 → **当 tick** 重建并递增 `version`（C3 的即时性优先于节流）；
    /// - 集合未变化 → 仅每 `NAV_REBUILD_INTERVAL_TICKS`（30 tick）做一次周期性重建；
    /// - `blocked` 没变则 `version` 不变，避免客户端无谓重算路径。
    pub fn rebuild(&mut self, snap: &NavObstacleSnapshot) -> u32 {
        if snap.server_tick == self.last_server_tick {
            return self.version;
        }
        self.last_server_tick = snap.server_tick;
        let changed = self
            .last_obstacles
            .as_ref()
            .is_none_or(|prev| !prev.same_obstacles(snap));
        let periodic =
            snap.server_tick.saturating_sub(self.last_rebuild_tick) >= NAV_REBUILD_INTERVAL_TICKS;
        if !changed && !periodic {
            return self.version;
        }

        let mut dynamic = vec![false; NAV_CELL_COUNT];
        for &(p, alive) in &snap.turrets {
            if alive {
                mark_into(
                    &mut dynamic,
                    ObstacleKind::Turret,
                    NavObstacleSnapshot::pos(p),
                );
            }
        }
        for &p in &snap.source_cores {
            mark_into(
                &mut dynamic,
                ObstacleKind::SourceCore,
                NavObstacleSnapshot::pos(p),
            );
        }
        if let Some(p) = snap.abyss_warden {
            mark_into(
                &mut dynamic,
                ObstacleKind::AbyssWarden,
                NavObstacleSnapshot::pos(p),
            );
        }

        self.dynamic_blocked = dynamic;
        self.refresh_union();
        self.last_obstacles = Some(snap.clone());
        self.last_rebuild_tick = snap.server_tick;
        self.version
    }

    /// 生成用于广播的全局障碍视图（`server_tick` 由服务端权威时钟盖戳）。
    pub fn obstacle_snapshot(&self, server_tick: u64) -> NavObstacleSnapshot {
        self.last_obstacles
            .clone()
            .map(|mut s| {
                s.server_tick = server_tick;
                s
            })
            .unwrap_or(NavObstacleSnapshot {
                server_tick,
                ..Default::default()
            })
    }
}

/// C6：起/终点落在障碍格时钳制到最近可通行格，搜索半径 ≤ 3 格。
pub fn clamp_to_walkable(c: (i32, i32), grid: &NavGrid) -> Option<(i32, i32)> {
    if !grid.is_blocked(c) {
        return Some(c);
    }
    for r in 1..=NAV_CLAMP_RADIUS_CELLS {
        let mut best: Option<((i32, i32), f32)> = None;
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let n = (c.0 + dx, c.1 + dz);
                if grid.is_blocked(n) {
                    continue;
                }
                let d = (dx * dx + dz * dz) as f32;
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((n, d));
                }
            }
        }
        if let Some((n, _)) = best {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: u64 = 100;

    fn snap(
        turrets: Vec<([f32; 2], bool)>,
        warden: Option<[f32; 2]>,
        tick: u64,
    ) -> NavObstacleSnapshot {
        NavObstacleSnapshot {
            turrets,
            source_cores: Vec::new(),
            abyss_warden: warden,
            server_tick: tick,
        }
    }

    /// 2:1 长方形地图：四角必须落在界内，`CELL_SIZE` 由地图宽度派生。
    #[test]
    fn grid_covers_rectangular_map() {
        for p in [
            Vec2::new(-NAV_GRID_HALF_X + 1.0, -NAV_GRID_HALF_Z + 1.0),
            Vec2::new(NAV_GRID_HALF_X - 1.0, NAV_GRID_HALF_Z - 1.0),
        ] {
            assert!(cell_in_bounds(world_to_cell(p)));
        }
        assert_eq!(NAV_GRID_NX as f32 * NAV_CELL_SIZE, map::MAP_WIDTH);
        assert_eq!(NAV_GRID_NZ as f32 * NAV_CELL_SIZE, map::MAP_HEIGHT);
        assert!(NavGrid::new().is_blocked(world_to_cell(Vec2::new(0.0, map::MAP_H_HALF + 10.0))));
    }

    /// C2：半径 → 格数 `ceil(r / CELL_SIZE)`，且占位为 `(2r+1)^2` 格。
    #[test]
    fn radius_to_cells_and_mark_footprint() {
        assert_eq!(ObstacleKind::Turret.cell_radius(), 1);
        assert_eq!(ObstacleKind::AbyssWarden.cell_radius(), 1);
        let mut grid = NavGrid::new();
        let center = Vec2::new(0.0, 0.0);
        let before = grid.blocked_cells().len();
        grid.mark(ObstacleKind::AbyssWarden, center, true);
        let added: Vec<_> = grid
            .blocked_cells()
            .into_iter()
            .filter(|c| !NavGrid::new().is_blocked(*c))
            .collect();
        assert_eq!(added.len(), 9, "半径 1 格 = 3×3 占位");
        assert!(grid.is_dynamic_blocked_world(center));
        assert!(grid.blocked_cells().len() > before);
    }

    /// C3/C7：哨塔存活 → 摧毁，同一 tick 内 `version` 严格递增且不再阻挡。
    #[test]
    fn destroyed_turret_stops_blocking_and_bumps_version() {
        let mut grid = NavGrid::new();
        let p = [0.0, 0.0];
        let v0 = grid.rebuild(&snap(vec![(p, true)], None, TICK));
        assert_eq!(v0, 1);
        assert!(grid.is_dynamic_blocked_world(Vec2::ZERO));

        let v1 = grid.rebuild(&snap(vec![(p, false)], None, TICK + 1));
        assert!(v1 > v0, "障碍集合变化必须当 tick 递增 version");
        assert!(
            !grid.is_dynamic_blocked_world(Vec2::ZERO),
            "C7：死塔不再阻挡"
        );
    }

    /// C4：同一 `server_tick` 重复快照不得重复 rebuild；version 不变。
    #[test]
    fn duplicate_server_tick_does_not_rebuild() {
        let mut grid = NavGrid::new();
        let v = grid.rebuild(&snap(vec![([0.0, 0.0], true)], None, TICK));
        for _ in 0..5 {
            assert_eq!(grid.rebuild(&snap(vec![([0.0, 0.0], true)], None, TICK)), v);
        }
        assert_eq!(grid.version, v);
    }

    /// C3/C4：渊卫出现 / 消失各一次，版本两次递增； Brush（草丛）不进网格。
    #[test]
    fn abyss_warden_spawn_and_despawn() {
        let mut grid = NavGrid::new();
        let v0 = grid.version;
        let v1 = grid.rebuild(&snap(Vec::new(), Some([0.0, 12.0]), TICK));
        assert_eq!(v1, v0 + 1);
        assert!(grid.is_dynamic_blocked_world(Vec2::new(0.0, 12.0)));
        let v2 = grid.rebuild(&snap(Vec::new(), None, TICK + 30));
        assert_eq!(v2, v1 + 1);
        assert!(!grid.is_dynamic_blocked_world(Vec2::new(0.0, 12.0)));
        // 周期性重建但集合未变 → version 保持不变
        let v3 = grid.rebuild(&snap(Vec::new(), None, TICK + 60));
        assert_eq!(v3, v2);
    }

    /// C6：起点落在障碍格时钳制到 ≤3 格内的最近可通行格。
    #[test]
    fn clamp_to_walkable_respects_three_cell_radius() {
        let grid = NavGrid::with_extra_cells(&[(32, 16)]);
        assert!(grid.is_blocked((32, 16)));
        let c = clamp_to_walkable((32, 16), &grid).unwrap();
        assert!(!grid.is_blocked(c));
        assert!((c.0 - 32).abs() <= NAV_CLAMP_RADIUS_CELLS);
        assert!((c.1 - 16).abs() <= NAV_CLAMP_RADIUS_CELLS);
    }
}
