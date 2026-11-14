//! `TC-NAV-01~09` 完整验收：**跨 crate 链路**层。
//!
//! 纯函数级用例分布：
//! | 用例 | 位置 |
//! |---|---|
//! | `TC-NAV-01/02/03` | `client/src/pathfinding.rs::tests::tc_nav_01..03_*` |
//! | `TC-NAV-04/05/06` | `client/src/input.rs::tests::tc_nav_04..06_*` |
//! | `TC-NAV-07/08` | `game-core/src/navigation.rs::tests::tc_nav_07/08_*` |
//! | `TC-NAV-09`（违规计数） | `game-core/src/navigation.rs::tests::tc_nav_09_*` |
//!
//! 本文件补的是**接线**：服务端权威 ECS → `NavGrid::rebuild` → bitcode 报文 →
//! 客户端 `NavGrid` → `astar` / `should_recompute` / `validate_movement_with_grid`，
//! 以及 `TC-NAV-09` 的抓包与视野裁剪断言。

use bevy::math::Vec2;
use client::input::{ClientNavObstacles, PathState, RecomputeReason, should_recompute};
use client::pathfinding::astar;
use game_core::navgrid::{
    NavGrid, NavObstacleSnapshot, ObstacleKind, cell_in_bounds, cell_to_world, world_to_cell,
};
use game_core::navigation::{MovementViolations, validate_movement_with_grid};
use game_core::net::{EntityDelta, EntitySnapshot, EntityType, ServerMessage, Team};
use game_core::visibility::compute_visibility;
use gx_net::{Snapshot, SnapshotHeader, VisibilityPolicy};

/// 模拟 `broadcast_nav_grid` 的编码 / 客户端 `receive_server_messages` 的解码。
fn roundtrip(snapshot: NavObstacleSnapshot, version: u32) -> (NavObstacleSnapshot, u32) {
    let msg = ServerMessage::NavGridUpdate { snapshot, version };
    let bytes = bitcode::encode(&msg);
    match bitcode::decode::<ServerMessage>(&bytes).expect("NavGridUpdate must decode") {
        ServerMessage::NavGridUpdate { snapshot, version } => (snapshot, version),
        _ => panic!("wrong ServerMessage variant"),
    }
}

fn path_len(path: &[Vec2], start: Vec2) -> f32 {
    let mut len = 0.0;
    let mut prev = start;
    for p in path {
        len += (*p - prev).length();
        prev = *p;
    }
    len
}

/// `TC-NAV-01/02/03` 接线版：服务端网格 → 报文 → 客户端网格 → A*，
/// 覆盖哨塔摧毁、渊卫出现/消失与同 tick 幂等。
#[test]
fn tc_nav_e2e_server_grid_packet_to_client_astar() {
    let center = cell_to_world((32, 16));
    let p = NavObstacleSnapshot::xy(center);
    let start = Vec2::new(-20.0, center.y);
    let goal = Vec2::new(20.0, center.y);

    // --- 哨塔存活：服务端重建 → 下发 → 客户端重建 → A* 绕行 ---
    let mut server_grid = NavGrid::new();
    let v1 = server_grid.rebuild(&NavObstacleSnapshot {
        turrets: vec![(p, true)],
        source_cores: Vec::new(),
        abyss_warden: None,
        server_tick: 10,
    });
    let (wire, version) = roundtrip(server_grid.obstacle_snapshot(10), v1);
    assert_eq!(version, v1);

    // 客户端版本由 `NavGrid::rebuild` 依据障碍集合自行裁决，不采信报文里的数值（C8）。
    let mut client_grid = NavGrid::new();
    assert!(client_grid.rebuild(&wire) > 0);
    assert_eq!(
        client_grid.blocked_cells(),
        server_grid.blocked_cells(),
        "双端同源"
    );
    let detour = astar(start, goal, &client_grid);
    assert!(!detour.is_empty(), "TC-NAV-01：哨塔不得把地图切断");
    assert!(path_len(&detour, start) > (goal - start).length());

    // --- 哨塔摧毁：version 严格递增，路径变直线 ---
    let v2 = server_grid.rebuild(&NavObstacleSnapshot {
        turrets: vec![(p, false)],
        source_cores: Vec::new(),
        abyss_warden: None,
        server_tick: 11,
    });
    assert!(v2 > v1, "TC-NAV-02：version 严格递增");
    let (wire2, _) = roundtrip(server_grid.obstacle_snapshot(11), v2);
    client_grid.rebuild(&wire2);
    let straight = astar(start, goal, &client_grid);
    assert!(path_len(&straight, start) < path_len(&detour, start));
    assert!((path_len(&straight, start) - (goal - start).length()).abs() < 1e-3);

    // --- 渊卫出现：阻挡格集合与半径一致；同 tick 重复快照 version 不变 ---
    let warden = NavObstacleSnapshot::xy(cell_to_world((40, 20)));
    let mut warden_grid = NavGrid::new();
    let spawn_v = warden_grid.rebuild(&NavObstacleSnapshot {
        turrets: Vec::new(),
        source_cores: Vec::new(),
        abyss_warden: Some(warden),
        server_tick: 100,
    });
    let cr = ObstacleKind::AbyssWarden.cell_radius();
    let (cx, cz) = world_to_cell(NavObstacleSnapshot::pos(warden));
    let mut count = 0;
    for dz in -cr..=cr {
        for dx in -cr..=cr {
            let c = (cx + dx, cz + dz);
            if cell_in_bounds(c) && warden_grid.is_dynamic_blocked_world(cell_to_world(c)) {
                count += 1;
            }
        }
    }
    assert_eq!(count, (2 * cr + 1) * (2 * cr + 1), "TC-NAV-03：半径一致");
    assert_eq!(
        warden_grid.rebuild(&NavObstacleSnapshot {
            turrets: Vec::new(),
            source_cores: Vec::new(),
            abyss_warden: Some(warden),
            server_tick: 100,
        }),
        spawn_v,
        "TC-NAV-03：同一 server_tick 重复快照 version 不变"
    );
}

/// `TC-NAV-04/05` 接线版：网络障碍视图引起网格版本变化 → `GridChanged` 立即重算。
#[test]
fn tc_nav_e2e_grid_change_triggers_immediate_recompute() {
    let mut path = PathState::default();
    // 待命 60 tick 不触发周期重算
    for tick in 0..60 {
        assert_eq!(
            should_recompute(&path, None, false, false, tick),
            RecomputeReason::None
        );
    }
    assert_eq!(path.recompute_calls, 0);
    // 进入「有活跃路径」状态，否则 GridChanged 没有可重算的目标
    path.goal = Some(Vec2::new(20.0, 0.0));

    // 模拟 sync_nav_grid：把服务端报文写进 ClientNavObstacles 再重建客户端网格
    let obstacles = ClientNavObstacles(Some(NavObstacleSnapshot {
        turrets: vec![([0.0, 0.0], true)],
        source_cores: Vec::new(),
        abyss_warden: None,
        server_tick: 61,
    }));
    if let Some(snap) = &obstacles.0 {
        path.grid.rebuild(snap);
    }
    assert_eq!(
        should_recompute(&path, None, false, false, 61),
        RecomputeReason::GridChanged,
        "网格版本变化必须立即重算（优先于 Periodic）"
    );
    let before = path.recompute_calls;
    path.process_requests(&[], Vec2::ZERO, 61);
    assert_eq!(path.recompute_calls - before, 1, "单 tick 最多一次 A*");
}

/// `TC-NAV-09`：抓包断言拒绝帧的 `corrected_position` 随快照下发；
/// 视野外实体位置零下发；连续 3 次违规计数为 3。
#[test]
fn tc_nav_09_corrected_position_packet_and_visibility() {
    const DT: f32 = game_core::systems::FIXED_TIMESTEP;
    const SPEED: f32 = 8.0;

    // 1) 构造一次被拒的移动，拿到 corrected_position
    let center = cell_to_world((32, 16));
    let mut grid = NavGrid::new();
    grid.mark(ObstacleKind::Turret, center, true);
    let from = Vec2::new(-7.55, center.y);
    let to = Vec2::new(-7.45, center.y);
    let validation = validate_movement_with_grid(from, to, SPEED, DT, &grid);
    assert!(!validation.accepted);
    assert_eq!(validation.corrected_position, from);

    // 2) 把它当作服务端权威位置写进 EntityDelta，编码成真实 Snapshot 报文再解包
    let corrected = validation.corrected_position;
    let mut delta = EntityDelta::empty(42);
    delta.entity_type = EntityType::Hero as u8;
    delta.team = 0;
    delta.position = [corrected.x, 0.0, corrected.y];
    delta.health = 100.0;
    let packet = Snapshot {
        header: SnapshotHeader {
            tick: 77,
            baseline_tick: 77,
            config_hash: Default::default(),
            view_seq: 1,
            client_time_us: 0,
        },
        deltas: Vec::new(),
        removed: Vec::new(),
        baseline: Some(vec![delta.clone()]),
    };
    let bytes = bitcode::encode(&ServerMessage::Snapshot(packet));
    let decoded = match bitcode::decode::<ServerMessage>(&bytes).unwrap() {
        ServerMessage::Snapshot(s) => s,
        _ => panic!("wrong ServerMessage variant"),
    };
    let got = &decoded.baseline.expect("基线必须随报文下发")[0];
    assert_eq!(got.net_id, 42);
    assert_eq!(
        got.position[0], corrected.x,
        "抓包：corrected_position.x 必须存在"
    );
    assert_eq!(
        got.position[2], corrected.y,
        "抓包：corrected_position.z 必须存在"
    );
    let rebuilt = EntitySnapshot::from_delta(got);
    assert_eq!(rebuilt.position.x, corrected.x);

    // 3) 视野裁剪：远处敌方英雄零下发
    let policy = VisibilityPolicy {
        vision_radius: 18.0,
        brush_reveal_radius: 4.0,
        constructs_provide_vision: false,
        sentries_provide_vision: false,
    };
    let mut friend = EntityDelta::empty(1);
    friend.entity_type = EntityType::Hero as u8;
    friend.team = 0;
    friend.position = [0.0, 0.0, 0.0];
    friend.health = 100.0;
    let mut enemy = EntityDelta::empty(2);
    enemy.entity_type = EntityType::Hero as u8;
    enemy.team = 1;
    enemy.position = [1000.0, 0.0, 1000.0];
    enemy.health = 100.0;
    let entities = vec![
        EntitySnapshot::from_delta(&friend),
        EntitySnapshot::from_delta(&enemy),
    ];
    let set = compute_visibility(Team::Blue, &policy, &entities);
    let delivered: Vec<_> = entities
        .iter()
        .filter(|e| set.contains(e.net_id.0))
        .collect();
    assert_eq!(delivered.len(), 1, "视野外实体零下发");
    assert_eq!(delivered[0].net_id.0, 1);
    assert!(!delivered.iter().any(|e| e.position.x > 100.0));

    // 4) 连续 3 次违规后计数为 3（驱动 warn 日志）
    let mut violations = MovementViolations::default();
    let mut world = bevy::prelude::World::new();
    let e = world.spawn_empty().id();
    for tick in 10..13 {
        violations.record(e, tick);
    }
    assert_eq!(violations.per_entity[&e].0, 3);
}
