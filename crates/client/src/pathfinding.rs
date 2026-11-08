//! 自动寻路（`GX-CBT-2026-001` §3.3）。
//!
//! 客户端负责路径质量，服务端只校验速度上限与碰撞（§3.3 职责划分）。
//! 地图 128×128，按 2×2 单位网格离散为 64×64 格；A* 八邻域、octile 启发式、
//! 禁止穿角。纯函数、无 ECS 依赖，便于 `TC-CBT-02` 直接断言。

use bevy::math::Vec2;
use std::collections::{HashMap, HashSet};

pub const CELL_SIZE: f32 = 2.0;
pub const GRID_HALF: f32 = 64.0;
pub const GRID_N: i32 = 64;

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

/// 世界坐标 `(x, z)` → 网格坐标。
pub fn world_to_cell(p: Vec2) -> (i32, i32) {
    (
        ((p.x + GRID_HALF) / CELL_SIZE).floor() as i32,
        ((p.y + GRID_HALF) / CELL_SIZE).floor() as i32,
    )
}

/// 网格坐标 → 格中心世界坐标。
pub fn cell_to_world(c: (i32, i32)) -> Vec2 {
    Vec2::new(
        (c.0 as f32 + 0.5) * CELL_SIZE - GRID_HALF,
        (c.1 as f32 + 0.5) * CELL_SIZE - GRID_HALF,
    )
}

fn in_bounds(c: (i32, i32)) -> bool {
    c.0 >= 0 && c.0 < GRID_N && c.1 >= 0 && c.1 < GRID_N
}

pub fn is_blocked(c: (i32, i32), walls: &[(i32, i32)]) -> bool {
    !in_bounds(c) || walls.contains(&c)
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

/// 返回从 `start` 到 `goal` 的路径点（世界坐标），末点为精确目标点。
/// 不可达时返回空向量，调用方据此停止移动。
pub fn astar(start: Vec2, goal: Vec2, walls: &[(i32, i32)]) -> Vec<Vec2> {
    let s = world_to_cell(start);
    let g = world_to_cell(goal);
    if is_blocked(s, walls) || is_blocked(g, walls) {
        return Vec::new();
    }
    if s == g {
        return vec![goal];
    }

    let mut open: Vec<(i32, i32)> = vec![s];
    let mut closed: HashSet<(i32, i32)> = HashSet::new();
    let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), f32> = HashMap::new();
    let mut f_score: HashMap<(i32, i32), f32> = HashMap::new();
    g_score.insert(s, 0.0);
    f_score.insert(s, octile(s, g));

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
            return smooth(&world, walls);
        }

        closed.insert(current);
        let current_g = *g_score.get(&current).unwrap_or(&f32::INFINITY);
        for (dx, dz) in NEIGHBORS {
            let next = (current.0 + dx, current.1 + dz);
            if is_blocked(next, walls) || closed.contains(&next) {
                continue;
            }
            // 禁止穿角：两个相邻正交格任一被阻挡，则对角不可通行
            if dx != 0
                && dz != 0
                && (is_blocked((current.0 + dx, current.1), walls)
                    || is_blocked((current.0, current.1 + dz), walls))
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
    Vec::new()
}

/// 线段可见性平滑：若从当前点到更远路点的直线不穿越障碍格，删除中间点。
fn smooth(path: &[Vec2], walls: &[(i32, i32)]) -> Vec<Vec2> {
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
        while j > i + 1 && !line_clear(path[i], path[j], walls) {
            j -= 1;
        }
        // 保证 i 严格递增，避免在末段原地打转
        i = j.max(i + 1);
    }
    out
}

fn line_clear(a: Vec2, b: Vec2, walls: &[(i32, i32)]) -> bool {
    let steps = ((b - a).length() / (CELL_SIZE * 0.5)).ceil().max(1.0) as usize;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let cell = world_to_cell(a.lerp(b, t));
        if is_blocked(cell, walls) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_path_is_reachable() {
        let path = astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &[]);
        assert!(!path.is_empty());
        assert_eq!(path.last().copied(), Some(Vec2::new(20.0, 0.0)));
    }

    #[test]
    fn path_never_enters_blocked_cells() {
        // 在 x≈0 处竖起一道墙，中间留一个缺口
        let mut walls = Vec::new();
        for z in 0..GRID_N {
            if z != 32 {
                walls.push((32, z));
            }
        }
        let path = astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &walls);
        assert!(!path.is_empty(), "must find a detour through the gap");
        for p in &path {
            let c = world_to_cell(*p);
            assert!(!walls.contains(&c), "path entered a blocked cell {c:?}");
        }
    }

    #[test]
    fn unreachable_goal_returns_empty() {
        let walls: Vec<(i32, i32)> = (0..GRID_N).map(|z| (32, z)).collect();
        assert!(astar(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), &walls).is_empty());
    }

    #[test]
    fn diagonal_never_cuts_a_blocked_corner() {
        let walls = vec![(33, 32), (32, 33)];
        let path = astar(Vec2::new(0.0, 0.0), Vec2::new(4.0, 4.0), &walls);
        for p in &path {
            let c = world_to_cell(*p);
            assert!(!walls.contains(&c));
        }
    }
}
