//! `P-U10` 验收：中央渊卫 / 兵矿收益 / 战争迷雾。
//!
//! 用户明确要求「**地图布置不变**」，本文件的第一个用例锁定地图常量关系；
//! 其余用例覆盖服务端目标链路、协议往返以及「迷雾可见区 == 服务端可见集」。

use bevy::prelude::*;
use client::fog::{ClientVisionSources, FogLevel, FogOfWar, VisionSnapshot, update_fog_of_war};
use game_core::components::{Hero, Team};
use game_core::gameplay::attributes::{AttributeSources, objective_buff};
use game_core::gameplay::clock::{MatchClock, OVERTIME_START_TICKS};
use game_core::gameplay::combat::CombatStats;
use game_core::gameplay::objectives::{CapturePoint, ObjectiveCapturedEvent};
use game_core::gameplay::objectives_economy::{
    ABYSS_WARDEN_FIRST_SPAWN_TICK, ABYSS_WARDEN_RESPAWN_TICKS, AbyssWardenState,
    LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD, LODE_SHRINE_CAPTURE_TEAM_GOLD,
    LODE_SHRINE_INCOME_PERIOD_TICKS, LODE_SHRINE_PERIODIC_GOLD, OBJECTIVE_ABYSS_WARDEN,
    OBJECTIVE_LODE_SHRINE, ObjectiveIncomeState, WARDEN_MINION_HP_MULT, WardenBuff,
    abyss_warden_owner, should_spawn_abyss_warden,
};
use game_core::gameplay::{GameplayEvents, NetEvent};
use game_core::map::{BUSHES, GOLD_MINE_POS, JUNGLE_CAMPS, LANE_WALLS, MAP_HEIGHT, MAP_WIDTH};
use game_core::navgrid::{NavGrid, NavObstacleSnapshot};
use game_core::net::{EntitySnapshot, EntityType, Quatf, ServerMessage, Vec3f, VisibilityPolicy};
use game_core::visibility::{compute_visibility, vision_sources};

fn entity(net_id: u64, team: Team, kind: EntityType, x: f32, z: f32) -> EntitySnapshot {
    EntitySnapshot {
        net_id: game_core::components::NetId(net_id),
        entity_type: kind,
        team,
        position: Vec3f::new(x, 0.0, z),
        rotation: Quatf::identity(),
        velocity: Vec3f::zero(),
        health: 100.0,
        max_health: 100.0,
        energy: 0.0,
        max_energy: 0.0,
        level: 1,
        gold: 0,
        ability_cooldowns: [0.0; 4],
        status_mask: 0,
        shield: 0.0,
    }
}

fn policy() -> VisibilityPolicy {
    VisibilityPolicy {
        vision_radius: 18.0,
        brush_reveal_radius: 6.0,
        constructs_provide_vision: true,
        sentries_provide_vision: true,
    }
}

/// 地图骨架不变：只新增目标 / 迷雾，不动兵线、野区、河道与营地坐标。
#[test]
fn map_layout_is_unchanged_by_objectives_and_fog() {
    assert_eq!(MAP_WIDTH / MAP_HEIGHT, 2.0, "地图仍是 2:1 长方形");
    assert_eq!(GOLD_MINE_POS.x, 0.0, "中央坑位仍在河道中线上");
    assert!(!LANE_WALLS.is_empty() && !BUSHES.is_empty());

    let wardens: Vec<_> = JUNGLE_CAMPS
        .iter()
        .filter(|c| c.monster_type == game_core::net::JungleMonsterType::AbyssWarden)
        .collect();
    assert_eq!(wardens.len(), 1, "全图只有一个中央全局目标营地");
    assert_eq!(wardens[0].position, GOLD_MINE_POS);

    assert_eq!(
        JUNGLE_CAMPS
            .iter()
            .filter(|c| c.monster_type == game_core::net::JungleMonsterType::LodeShrine)
            .count(),
        2,
        "两座兵矿保持原位"
    );
    assert_eq!(
        JUNGLE_CAMPS
            .iter()
            .filter(|c| c.monster_type == game_core::net::JungleMonsterType::MusterWell)
            .count(),
        1
    );
}

/// 渊卫首刷 / 重生 / 加时门禁；刷新后必须立刻进入 NavGrid 动态障碍。
#[test]
fn warden_spawn_gates_and_nav_grid_obstacle() {
    assert!(!should_spawn_abyss_warden(
        ABYSS_WARDEN_FIRST_SPAWN_TICK - 1,
        false,
        0,
        false
    ));
    assert!(should_spawn_abyss_warden(
        ABYSS_WARDEN_FIRST_SPAWN_TICK,
        false,
        0,
        false
    ));
    assert!(!should_spawn_abyss_warden(
        OVERTIME_START_TICKS,
        false,
        0,
        true
    ));

    let mut app = App::new();
    app.insert_resource(MatchClock {
        tick: ABYSS_WARDEN_FIRST_SPAWN_TICK,
        ..default()
    })
    .init_resource::<AbyssWardenState>()
    .init_resource::<GameplayEvents>()
    .add_systems(Update, server::objectives::abyss_warden_spawn_system);
    app.update();
    let state = app.world().resource::<AbyssWardenState>();
    assert!(state.alive);
    assert!(state.entity.is_some());
    assert!(
        app.world()
            .resource::<GameplayEvents>()
            .net
            .iter()
            .any(|e| matches!(e, NetEvent::AbyssWardenSpawn { .. }))
    );

    // 服务端侧用同一份障碍视图重建网格，坑位格必须被动态阻挡
    let mut grid = NavGrid::new();
    let before = grid.blocked_cells();
    grid.rebuild(&NavObstacleSnapshot {
        turrets: Vec::new(),
        source_cores: Vec::new(),
        abyss_warden: Some([GOLD_MINE_POS.x, GOLD_MINE_POS.z]),
        server_tick: 1,
    });
    assert!(grid.blocked_cells() > before, "渊卫存活时坑位应阻挡寻路");
    assert!(grid.is_dynamic_blocked_world(Vec2::new(GOLD_MINE_POS.x, GOLD_MINE_POS.z)));

    // 累计伤害归属 → 120s 全队增益
    assert_eq!(abyss_warden_owner(0.0, 0.0), None);
    assert_eq!(abyss_warden_owner(150.0, 100.0), Some(Team::Blue));
    let buff = objective_buff(OBJECTIVE_ABYSS_WARDEN, Team::Blue).expect("渊卫 buff 必须存在");
    assert_eq!(buff.remaining_ticks, 120 * 60);
    assert_eq!(buff.name, "abyss_warden");

    let mut s = AbyssWardenState::default();
    s.record_damage(Some(Team::Red), 250.0);
    s.record_damage(Some(Team::Blue), 100.0);
    assert_eq!(
        s.on_defeated(ABYSS_WARDEN_FIRST_SPAWN_TICK),
        Some(Team::Red)
    );
    assert_eq!(
        s.next_spawn_tick,
        ABYSS_WARDEN_FIRST_SPAWN_TICK + ABYSS_WARDEN_RESPAWN_TICKS
    );
}

/// 兵矿占领一次性收益 + 归属期间周期收益，只发给归属方。
#[test]
fn gold_mine_capture_and_periodic_income_pipeline() {
    let mut app = App::new();
    app.insert_resource(MatchClock {
        tick: 900,
        ..default()
    })
    .init_resource::<ObjectiveIncomeState>()
    .init_resource::<WardenBuff>()
    .init_resource::<GameplayEvents>();
    let blue = app
        .world_mut()
        .spawn((
            Team::Blue,
            AttributeSources::new(&Hero::default()),
            Hero::default(),
            CombatStats::default(),
        ))
        .id();
    let red = app
        .world_mut()
        .spawn((
            Team::Red,
            AttributeSources::new(&Hero::default()),
            Hero::default(),
            CombatStats::default(),
        ))
        .id();
    let blue_start = app.world().entity(blue).get::<Hero>().unwrap().gold;
    let red_start = app.world().entity(red).get::<Hero>().unwrap().gold;

    // 占领瞬间
    app.world_mut()
        .resource_mut::<GameplayEvents>()
        .captured
        .push(ObjectiveCapturedEvent {
            objective_id: OBJECTIVE_LODE_SHRINE,
            owner: Team::Blue,
            event_id: 1,
        });
    app.add_systems(Update, server::world::objective_buff_system);
    app.update();
    assert_eq!(
        app.world().entity(blue).get::<Hero>().unwrap().gold,
        blue_start + LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD
    );
    assert_eq!(
        app.world().entity(red).get::<Hero>().unwrap().gold,
        red_start
    );
    assert_eq!(
        app.world()
            .resource::<ObjectiveIncomeState>()
            .team_gold_of(Team::Blue),
        LODE_SHRINE_CAPTURE_TEAM_GOLD
    );
    assert!(
        app.world()
            .resource::<GameplayEvents>()
            .net
            .iter()
            .any(|e| matches!(e, NetEvent::LodeShrineCapture { .. }))
    );

    // 归属期间的周期收益（两座兵矿同队 → 两份）
    let pay_tick = 900 + LODE_SHRINE_INCOME_PERIOD_TICKS;
    app.world_mut().resource_mut::<MatchClock>().tick = pay_tick;
    for _ in 0..2 {
        let mut cp = CapturePoint::new(OBJECTIVE_LODE_SHRINE, 100, 5400);
        cp.owner = Some(Team::Blue);
        app.world_mut().spawn(cp);
    }
    app.add_systems(Update, server::objectives::objective_income_system);
    app.update();
    assert_eq!(
        app.world().entity(blue).get::<Hero>().unwrap().gold,
        blue_start + LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD + 2 * LODE_SHRINE_PERIODIC_GOLD
    );
    assert_eq!(
        app.world().entity(red).get::<Hero>().unwrap().gold,
        red_start
    );
    assert!(
        app.world()
            .resource::<GameplayEvents>()
            .net
            .iter()
            .any(|e| matches!(e, NetEvent::ObjectiveIncome { .. }))
    );
}

/// 渊卫归属：全队赏金 + 120s 推进增益只强化归属方新生成的战偶。
#[test]
fn warden_capture_grants_gold_and_strengthens_only_owning_minions() {
    let mut app = App::new();
    app.insert_resource(MatchClock {
        tick: 900,
        ..default()
    })
    .init_resource::<ObjectiveIncomeState>()
    .init_resource::<WardenBuff>()
    .init_resource::<GameplayEvents>()
    .insert_resource(server::networking::NextNetId(1));
    let hero = app
        .world_mut()
        .spawn((
            Team::Blue,
            AttributeSources::new(&Hero::default()),
            Hero::default(),
            CombatStats::default(),
        ))
        .id();
    let gold_before = app.world().entity(hero).get::<Hero>().unwrap().gold;

    app.world_mut()
        .resource_mut::<GameplayEvents>()
        .captured
        .push(ObjectiveCapturedEvent {
            objective_id: OBJECTIVE_ABYSS_WARDEN,
            owner: Team::Blue,
            event_id: 3,
        });
    app.add_systems(Update, server::world::objective_buff_system);
    app.update();
    assert_eq!(
        app.world().entity(hero).get::<Hero>().unwrap().gold,
        gold_before + game_core::gameplay::objectives_economy::ABYSS_WARDEN_CAPTURE_PER_PLAYER_GOLD
    );
    assert!(app.world().resource::<WardenBuff>().active(Team::Blue, 900));

    // 波次生成：蓝方（归属方）战偶生命 = 红方 × 1.5
    app.add_systems(Update, server::world::wave_spawn_system);
    app.update();
    let mut blue_hp: Vec<f32> = Vec::new();
    let mut red_hp: Vec<f32> = Vec::new();
    let mut q = app
        .world_mut()
        .query::<(&Team, &game_core::components::Minion)>();
    for (team, m) in q.iter(app.world()) {
        match team {
            Team::Blue => blue_hp.push(m.max_health),
            Team::Red => red_hp.push(m.max_health),
            Team::Neutral => {}
        }
    }
    assert!(!blue_hp.is_empty() && blue_hp.len() == red_hp.len());
    blue_hp.sort_by(f32::total_cmp);
    red_hp.sort_by(f32::total_cmp);
    for (b, r) in blue_hp.iter().zip(&red_hp) {
        assert!(
            (b - r * WARDEN_MINION_HP_MULT).abs() < 1e-2,
            "归属方战偶应强化 1.5 倍: {b} vs {r}"
        );
    }
}

/// 迷雾遮罩必须与服务端实体裁剪严格同源：可见的敌人所在格亮，被裁剪的敌人所在格暗。
#[test]
fn fog_matches_server_visibility_culling() {
    let entities = vec![
        entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
        entity(2, Team::Red, EntityType::Hero, 5.0, 0.0),
        entity(3, Team::Red, EntityType::Hero, 140.0, 0.0),
    ];
    let p = policy();
    let set = compute_visibility(Team::Blue, &p, &entities);
    assert!(set.contains(2) && !set.contains(3));

    let sources: Vec<Vec2> = vision_sources(Team::Blue, &p, &entities)
        .into_iter()
        .map(|s| Vec2::new(s[0], s[2]))
        .collect();
    assert_eq!(sources.len(), 1, "只有己方英雄是可见源");
    let mut fog = FogOfWar::default();
    fog.refresh(&sources, p.vision_radius, p.brush_reveal_radius);

    let cell_of = |x: f32, z: f32| {
        let ix = ((x + game_core::map::MAP_W_HALF) / client::fog::FOG_CELL_X)
            .floor()
            .clamp(0.0, (client::fog::FOG_NX - 1) as f32) as usize;
        let iz = ((z + game_core::map::MAP_H_HALF) / client::fog::FOG_CELL_Z)
            .floor()
            .clamp(0.0, (client::fog::FOG_NZ - 1) as f32) as usize;
        (ix, iz)
    };
    let (nx, nz) = cell_of(5.0, 0.0);
    let (fx, fz) = cell_of(140.0, 0.0);
    assert_eq!(fog.level(nx, nz), FogLevel::Visible);
    assert_eq!(fog.level(fx, fz), FogLevel::Unexplored);
}

/// `VisionUpdate` 走 bitcode 往返后仍能驱动迷雾，且旧 tick 被丢弃。
#[test]
fn vision_update_wire_roundtrip_drives_fog() {
    let src = FogOfWar::cell_center(32, 16);
    let msg = ServerMessage::VisionUpdate {
        sources: vec![[src.x, src.y], [src.x + 6.0, src.y]],
        vision_radius: 18.0,
        brush_reveal_radius: 6.0,
        server_tick: 7,
    };
    let bytes = bitcode::encode(&msg);
    let decoded: ServerMessage = bitcode::decode(&bytes).expect("报文必须可解码");
    let ServerMessage::VisionUpdate {
        sources,
        vision_radius,
        brush_reveal_radius,
        server_tick,
    } = decoded
    else {
        panic!("解码后必须是 VisionUpdate");
    };

    let mut app = App::new();
    app.init_resource::<ClientVisionSources>()
        .init_resource::<FogOfWar>()
        .add_systems(Update, update_fog_of_war);
    app.world_mut().resource_mut::<ClientVisionSources>().0 = Some(VisionSnapshot {
        sources: sources.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
        vision_radius,
        brush_reveal_radius,
        server_tick,
    });
    app.update();
    assert_eq!(
        app.world().resource::<FogOfWar>().level(32, 16),
        FogLevel::Visible
    );
    assert_eq!(app.world().resource::<FogOfWar>().last_server_tick, 7);
}
