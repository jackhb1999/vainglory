//! 自动寻路（`GX-CBT-2026-001` §3.3 / `docs/05_COMBAT_SPEC.md` §24.1 `NAV-M2-01`）。
//!
//! 客户端负责路径质量，服务端只校验速度上限与碰撞（§3.3 职责划分）。
//! 网格几何与障碍真相源下沉到 `game_core::navgrid`，两端共用同一个 `NavGrid`：
//! 静态地形由 `map::LANE_WALLS` 烘焙一次，动态障碍（哨塔 / 源核 / 渊卫）只能来自
//! 服务端下发的 `NavObstacleSnapshot` —— 渲染实体经逐玩家视野裁剪，拿不到全图障碍（C1）。
//!
//! 网格按 **2:1 长方形地图**（当前 480×240）派生为 64×32 格、每格 7.5 单位；
//! 旧版单一 `GRID_N` 只在正方形下成立，长方形里 z 轴会整片越界。
//! A* 八邻域、octile 启发式、禁止穿角、4096 节点预算（C5）、起终点钳制（C6）、
//! 线段可见性平滑；纯函数、无 ECS 依赖，便于 `TC-CBT-02` / `TC-NAV-01~03` 直接断言。

use bevy::math::Vec2;
use game_core::navgrid::{
    A_STAR_NODE_BUDGET, NavGrid, cell_in_bounds, cell_to_world, clamp_to_walkable, world_to_cell,
};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub use game_core::navgrid::{
    NAV_CELL_SIZE as CELL_SIZE, NAV_GRID_HALF_X as GRID_HALF_X, NAV_GRID_HALF_Z as GRID_HALF_Z,
    NAV_GRID_NX as GRID_NX, NAV_GRID_NZ as GRID_NZ,
};

const NEIGHBORS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// 兼容旧调用方与单测的「格 + 阻挡列表」判定。
pub fn is_blocked(c: (i32, i32), walls: &[(i32, i32)]) -> bool {
    !cell_in_bounds(c) || walls.contains(&c)
}

/// 兵线 / 野区堤墙的静态阻挡格（与 `NavGrid::new()` 同源，进程内只栅格化一次）。
pub fn lane_wall_cells() -> Vec<(i32, i32)> {
    static CACHE: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    CACHE
        .get_or_init(game_core::navgrid::static_terrain_cells)
        .clone()
}

fn octile(a: (i32, i32), b: (i32, i32)) -> f32 {
    let dx = (a.0 - b.0).abs() as f32;
    let dy = (a.1 - b.1).abs() as f32;
    dx + dy + (std::f32::consts::SQRT_2 - 2.0) * dx.min(dy)
}

fn step_cost(dx: i32, dz: i32) -> f32 {
    if dx != 0 && dz != 0 {
        std::f32::consts::SQRT_2
    } else {
        1.0
    }
}

/// A* 结果；`over_budget` 时 `path` 是调用方传入的上一次有效路径（C5）。
#[derive(Clone, Debug)]
pub struct PathResult {
    pub path: Vec<Vec2>,
    pub expanded: usize,
    pub over_budget: bool,
}

/// 返回从 `start` 到 `goal` 的路径点（世界坐标），末点为精确目标点。
/// 不可达时返回空向量，调用方据此停止移动。
pub fn astar(start: Vec2, goal: Vec2, grid: &NavGrid) -> Vec<Vec2> {
    find_path(start, goal, grid, &[]).path
}

/// `NAV-M2-01-C5`：超预算返回 `fallback` 而非空路径，避免英雄急停。
pub fn find_path(start: Vec2, goal: Vec2, grid: &NavGrid, fallback: &[Vec2]) -> PathResult {
    find_path_with_budget(start, goal, grid, fallback, A_STAR_NODE_BUDGET)
}

/// 抽出可调预算的内部入口，便于 `TC-NAV-05` / C5 用小预算单测回退行为。
fn find_path_with_budget(
    start: Vec2,
    goal: Vec2,
    grid: &NavGrid,
    fallback: &[Vec2],
    budget: usize,
) -> PathResult {
    // C6：起/终点落在障碍格 → 钳制到 ≤3 格内最近可通行格；钳不到才回退。
    let (Some(s), Some(g)) = (
        clamp_to_walkable(world_to_cell(start), grid),
        clamp_to_walkable(world_to_cell(goal), grid),
    ) else {
        return PathResult {
            path: fallback.to_vec(),
            expanded: 0,
            over_budget: true,
        };
    };
    if s == g {
        return PathResult {
            path: vec![goal],
            expanded: 0,
            over_budget: false,
        };
    }

    let mut open: Vec<(i32, i32)> = vec![s];
    let mut closed: HashSet<(i32, i32)> = HashSet::new();
    let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), f32> = HashMap::new();
    let mut f_score: HashMap<(i32, i32), f32> = HashMap::new();
    g_score.insert(s, 0.0);
    f_score.insert(s, octile(s, g));
    let mut expanded = 0usize;

    while !open.is_empty() {
        let best = open
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                f_score
                    .get(a)
                    .unwrap_or(&f32::INFINITY)
                    .partial_cmp(f_score.get(b).unwrap_or(&f32::INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, c)| (i, *c));
        let Some((idx, current)) = best else { break };
        open.remove(idx);

        // C5：节点访问预算，超了立即返回上一次有效路径，禁止阻塞主线程。
        expanded += 1;
        if expanded > budget {
            return PathResult {
                path: fallback.to_vec(),
                expanded,
                over_budget: true,
            };
        }

        if current == g {
            let mut path = vec![current];
            let mut cursor = current;
            while let Some(&prev) = came.get(&cursor) {
                path.push(prev);
                cursor = prev;
            }
            path.reverse();
            let mut world: Vec<Vec2> = path.iter().skip(1).map(|c| cell_to_world(*c)).collect();
            if let Some(last) = world.last_mut() {
                *last = goal;
            } else {
                world.push(goal);
            }
            return PathResult {
                path: smooth(&world, grid),
                expanded,
                over_budget: false,
            };
        }

        closed.insert(current);
        let current_g = *g_score.get(&current).unwrap_or(&f32::INFINITY);
        for (dx, dz) in NEIGHBORS {
            let next = (current.0 + dx, current.1 + dz);
            if grid.is_blocked(next) || closed.contains(&next) {
                continue;
            }
            // 禁止穿角：两个相邻正交格任一被阻挡，则对角不可通行（C6 保持不变）
            if dx != 0
                && dz != 0
                && (grid.is_blocked((current.0 + dx, current.1))
                    || grid.is_blocked((current.0, current.1 + dz)))
            {
                continue;
            }
            let tentative = current_g + step_cost(dx, dz);
            if tentative < *g_score.get(&next).unwrap_or(&f32::INFINITY) {
                came.insert(next, current);
                g_score.insert(next, tentative);
                f_score.insert(next, tentative + octile(next, g));
                if !open.contains(&next) {
                    open.push(next);
                }
            }
        }
    }

    // 真正不可达（与「预算耗尽」区分）：返回空路径，由调用方清目标 / 触发 Stuck。
    PathResult {
        path: Vec::new(),
        expanded,
        over_budget: false,
    }
}

/// 线段可见性平滑：若从当前点到更远路点的直线不穿越障碍格，删除中间点。
fn smooth(path: &[Vec2], grid: &NavGrid) -> Vec<Vec2> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        out.push(path[i]);
        if i + 1 >= path.len() {
            break;
        }
        let mut j = path.len() - 1;
        while j > i + 1 && !line_clear(path[i], path[j], grid) {
            j -= 1;
        }
        // 保证 i 严格递增，避免在末段原地打转
        i = j.max(i + 1);
    }
    out
}

pub fn line_clear(a: Vec2, b: Vec2, grid: &NavGrid) -> bool {
    let steps = ((b - a).length() / (CELL_SIZE * 0.5)).ceil().max(1.0) as usize;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        if grid.is_blocked(world_to_cell(a.lerp(b, t))) {
            return false;
        }
    }
    true
}

#[cfg(test)]
fn path_length(path: &[Vec2], start: Vec2) -> f32 {
    let mut len = 0.0;
    let mut prev = start;
    for p in path {
        len += (*p - prev).length();
        prev = *p;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::navgrid::{NavObstacleSnapshot, ObstacleKind};

    fn nav(turrets: Vec<([f32; 2], bool)>, warden: Option<[f32; 2]>, tick: u64) -> NavGrid {
        let mut grid = NavGrid::new();
        grid.rebuild(&NavObstacleSnapshot {
            turrets,
            source_cores: Vec::new(),
            abyss_warden: warden,
            server_tick: tick,
        });
        grid
    }

    fn no_corner_cut(path: &[Vec2], from: Vec2, grid: &NavGrid) {
        let mut prev = from;
        for p in path {
            assert!(
                line_clear(prev, *p, grid),
                "路径段 {prev:?}→{p:?} 穿越障碍格（含穿角）"
            );
            prev = *p;
        }
    }

    /// 回归护栏：网格必须按 2:1 比例覆盖整张长方形地图。
    #[test]
    fn grid_covers_whole_rectangular_map() {
        let (hx, hz) = (game_core::map::MAP_W_HALF, game_core::map::MAP_H_HALF);
        for p in [
            Vec2::new(-hx + 1.0, -hz + 1.0),
            Vec2::new(hx - 1.0, hz - 1.0),
            Vec2::new(-hx + 1.0, hz - 1.0),
            Vec2::new(hx - 1.0, -hz + 1.0),
        ] {
            assert!(cell_in_bounds(world_to_cell(p)), "地图角点 {p:?} 越界");
        }
        assert_eq!(GRID_NX as f32 * CELL_SIZE, game_core::map::MAP_WIDTH);
        assert_eq!(GRID_NZ as f32 * CELL_SIZE, game_core::map::MAP_HEIGHT);
        assert_eq!(GRID_NX as f32 / GRID_NZ as f32, 2.0);
        assert!(is_blocked(world_to_cell(Vec2::new(0.0, hz + 10.0)), &[]));
        assert!(is_blocked(world_to_cell(Vec2::new(-hx - 10.0, 0.0)), &[]));
    }

    #[test]
    fn straight_path_is_reachable() {
        let grid = NavGrid::new();
        let path = astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &grid);
        assert!(!path.is_empty());
        assert_eq!(path.last().copied(), Some(Vec2::new(20.0, 0.0)));
    }

    #[test]
    fn path_never_enters_blocked_cells() {
        // 在 x≈0 处竖起一道贯穿南北的墙，中间留一个缺口
        let walls: Vec<(i32, i32)> = (0..GRID_NZ)
            .filter(|&z| z != GRID_NZ / 2)
            .map(|z| (GRID_NX / 2, z))
            .collect();
        let grid = NavGrid::with_extra_cells(&walls);
        let path = astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &grid);
        assert!(!path.is_empty(), "must find a detour through the gap");
        for p in &path {
            assert!(!grid.is_blocked(world_to_cell(*p)), "路径进入阻挡格 {p:?}");
        }
    }

    #[test]
    fn unreachable_goal_returns_empty() {
        let walls: Vec<(i32, i32)> = (0..GRID_NZ).map(|z| (GRID_NX / 2, z)).collect();
        let grid = NavGrid::with_extra_cells(&walls);
        assert!(astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &grid).is_empty());
    }

    /// 兵线 / 野区堤墙必须真的进入 A* 障碍集，坡道开口留空。
    #[test]
    fn lane_walls_are_rasterized_with_open_gaps() {
        let walls = lane_wall_cells();
        assert!(!walls.is_empty(), "墙体必须产生阻挡格");
        let c = world_to_cell(Vec2::new(-144.0, game_core::map::WALL_SOUTH_Z));
        assert!(walls.contains(&c), "墙体位置 {c:?} 未被栅格化");
        let gap = world_to_cell(Vec2::new(0.0, game_core::map::WALL_SOUTH_Z));
        assert!(!walls.contains(&gap), "中央坡道 {gap:?} 被堵死");
        let wall_row = gap.1;
        let lane_row = world_to_cell(Vec2::new(0.0, game_core::map::LANE_Z)).1;
        assert!(wall_row > lane_row, "墙必须在兵线南侧");
        let north_row = lane_row - 2;
        assert!(
            !walls
                .iter()
                .any(|&(gx, gz)| gz == north_row && (20..44).contains(&gx)),
            "兵线北侧不得存在墙体阻挡格"
        );
    }

    /// 从兵线去野区不能直穿墙：A* 必须绕到坡道开口，且路径不进入任何阻挡格。
    #[test]
    fn astar_detours_through_wall_gaps() {
        let grid = NavGrid::new();
        let start = Vec2::new(-144.0, game_core::map::LANE_Z);
        let goal = Vec2::new(-144.0, 0.0);
        let path = astar(start, goal, &grid);
        assert!(!path.is_empty(), "必须能经坡道开口到达野区");
        assert_eq!(path.last().copied(), Some(goal));
        for p in &path {
            assert!(!grid.is_blocked(world_to_cell(*p)), "路径进入了墙格 {p:?}");
        }
        assert!(
            path_length(&path, start) > (goal - start).length() * 1.05,
            "路径应当绕墙而非直穿"
        );
        let mid_gap = astar(
            Vec2::new(0.0, game_core::map::LANE_Z),
            Vec2::new(0.0, 0.0),
            &grid,
        );
        assert!(!mid_gap.is_empty(), "中央坡道必须可通行");
    }

    #[test]
    fn diagonal_never_cuts_a_blocked_corner() {
        let cx = GRID_NX / 2;
        let cz = GRID_NZ / 2;
        let grid = NavGrid::with_extra_cells(&[(cx + 1, cz), (cx, cz + 1)]);
        let path = astar(Vec2::new(0.0, 0.0), Vec2::new(12.0, 12.0), &grid);
        assert!(!path.is_empty());
        no_corner_cut(&path, Vec2::new(0.0, 0.0), &grid);
    }

    // ------------------------------------------------------------------
    // TC-NAV-01 ~ 03（§24.1 验收）
    // ------------------------------------------------------------------

    /// `TC-NAV-01`：在网格中心格（长方形地图对应规格 `(32,32)` 的 `(32,16)`）
    /// 置 `Turret` 障碍，A* 返回路径且 `blocked` 格命中数为 0；路径不穿角。
    #[test]
    fn tc_nav_01_turret_obstacle_path_avoids_all_blocked_cells() {
        let center = cell_to_world((32, 16));
        let mut grid = NavGrid::new();
        grid.mark(ObstacleKind::Turret, center, true);
        assert!(grid.is_blocked((32, 16)));

        let start = Vec2::new(-20.0, center.y);
        let goal = Vec2::new(20.0, center.y);
        let path = astar(start, goal, &grid);
        assert!(!path.is_empty(), "哨塔不得把整张图切断");
        assert_eq!(path.last().copied(), Some(goal));
        for p in &path {
            assert!(
                !grid.is_blocked(world_to_cell(*p)),
                "路径命中 blocked 格 {p:?}"
            );
        }
        no_corner_cut(&path, start, &grid);
        // 必须绕行而不是直穿
        assert!(path_length(&path, start) > (goal - start).length());
    }

    /// `TC-NAV-02`：哨塔由存活改为摧毁并 `rebuild` 后，`version` 严格递增，
    /// 原本绕行的路径变为直线。
    #[test]
    fn tc_nav_02_destroyed_turret_straightens_path_and_bumps_version() {
        let center = cell_to_world((32, 16));
        let p = NavObstacleSnapshot::xy(center);
        let mut grid = nav(vec![(p, true)], None, 10);
        let start = Vec2::new(-20.0, center.y);
        let goal = Vec2::new(20.0, center.y);
        let detour = astar(start, goal, &grid);
        assert!(path_length(&detour, start) > (goal - start).length() * 1.001);
        let v0 = grid.version;

        let v1 = grid.rebuild(&NavObstacleSnapshot {
            turrets: vec![(p, false)],
            source_cores: Vec::new(),
            abyss_warden: None,
            server_tick: 11,
        });
        assert!(v1 > v0, "version 必须严格递增");
        let straight = astar(start, goal, &grid);
        assert!(straight.len() <= 2, "平滑后最多保留一个中间路点");
        assert!(
            (path_length(&straight, start) - (goal - start).length()).abs() < 1e-3,
            "障碍消失后路径长度应等于直线距离"
        );
        assert!(path_length(&straight, start) < path_length(&detour, start));
    }

    /// `TC-NAV-03`：渊卫出现 / 消失各一次，`blocked` 格集合与预期半径一致；
    /// 同一 `server_tick` 重复快照 `version` 不变。
    #[test]
    fn tc_nav_03_warden_spawn_despawn_and_duplicate_tick() {
        let center = cell_to_world((40, 20));
        let p = NavObstacleSnapshot::xy(center);
        let mut grid = nav(Vec::new(), Some(p), 100);
        let cr = ObstacleKind::AbyssWarden.cell_radius();
        let (cx, cz) = world_to_cell(center);
        let mut expected = 0;
        for dz in -cr..=cr {
            for dx in -cr..=cr {
                if cell_in_bounds((cx + dx, cz + dz))
                    && grid.is_dynamic_blocked_world(cell_to_world((cx + dx, cz + dz)))
                {
                    expected += 1;
                }
            }
        }
        assert_eq!(
            expected,
            (2 * cr + 1) * (2 * cr + 1),
            "阻挡格数须与半径一致"
        );
        let v_spawn = grid.version;

        // 同一 tick 的重复快照：version 不变（C4）
        assert_eq!(
            grid.rebuild(&NavObstacleSnapshot {
                turrets: Vec::new(),
                source_cores: Vec::new(),
                abyss_warden: Some(p),
                server_tick: 100,
            }),
            v_spawn
        );

        let v_gone = grid.rebuild(&NavObstacleSnapshot {
            turrets: Vec::new(),
            source_cores: Vec::new(),
            abyss_warden: None,
            server_tick: 101,
        });
        assert!(v_gone > v_spawn);
        assert!(!grid.is_dynamic_blocked_world(center), "C7：消失后不再阻挡");
    }

    /// C5：节点预算耗尽时返回上一次有效路径，而不是空路径导致急停。
    #[test]
    fn astar_over_budget_returns_last_valid_path() {
        let grid = NavGrid::new();
        let fallback = vec![Vec2::new(1.0, 1.0), Vec2::new(5.0, 5.0)];
        let r = find_path_with_budget(
            Vec2::new(-20.0, 0.0),
            Vec2::new(20.0, 0.0),
            &grid,
            &fallback,
            1,
        );
        assert!(r.over_budget);
        assert_eq!(r.path, fallback);
    }

    /// C6：起点落在障碍格内时钳制到 ≤3 格的最近可通行格，仍能给出路径。
    #[test]
    fn start_inside_obstacle_is_clamped_instead_of_empty() {
        let center = cell_to_world((32, 16));
        let mut grid = NavGrid::new();
        grid.mark(ObstacleKind::Turret, center, true);
        assert!(grid.is_blocked(world_to_cell(center)));
        let path = astar(center, Vec2::new(20.0, center.y), &grid);
        assert!(!path.is_empty(), "钳制后必须仍可规划");
    }
}
