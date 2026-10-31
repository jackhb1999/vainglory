//! 验证服务端 FixedUpdate 是否真的在跑（移动/攻击逻辑都挂在 FixedUpdate 上）。
//! 若 ServerTick 一直为 0，说明 FixedUpdate 没执行，英雄永远不会移动/攻击。

use bevy::prelude::*;
use bevy::asset::Assets;
use bevy::pbr::StandardMaterial;
use bevy::render::mesh::Mesh;
use game_core::components::*;
use server::networking::{NetworkingPlugin, ServerPort};
use server::simulation::{SimulationPlugin, ServerTick};
use server::spawn::SpawnPlugin;
use server::ServerPlugin;
use std::time::Duration;

fn build_server() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        .add_plugins(NetworkingPlugin)
        .add_plugins(SimulationPlugin)
        .add_plugins(ServerPlugin)
        .add_plugins(SpawnPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        // 用 0 端口避免测试时与本机真实服务冲突
        .insert_resource(ServerPort(0))
        .insert_resource(Time::<Fixed>::from_hz(60.0));
    app
}

#[test]
fn fixed_update_actually_runs() {
    let mut app = build_server();

    // 用真实墙钟时间推进：每帧 sleep 20ms，使 Time<Fixed> 累积到触发阈值
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(20));
        app.update();
    }

    let tick = app.world().resource::<ServerTick>();
    eprintln!("ServerTick after ~40 frames = {}", tick.0);
    assert!(
        tick.0 > 0,
        "服务端 FixedUpdate 没有执行！ServerTick={}（移动/攻击逻辑全部挂在 FixedUpdate 上）",
        tick.0
    );
}

#[test]
fn hero_moves_when_input_applied() {
    let mut app = build_server();

    let net_id = NetId(12345);
    let hero = app
        .world_mut()
        .spawn((
            net_id,
            Team::Blue,
            Transform::from_xyz(0.0, 0.0, 0.0),
            TransformSync {
                position: Vec3::ZERO,
                ..default()
            },
            PlayerInput {
                move_dir: Vec2::new(0.0, -1.0),
                ..default()
            },
            Hero {
                name: "Test".to_string(),
                ..default()
            },
            ServerAuthority,
        ))
        .id();

    let start = app.world().get::<Transform>(hero).unwrap().translation;

    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(20));
        app.update();
    }

    let end = app.world().get::<Transform>(hero).unwrap().translation;
    eprintln!("hero z: {} -> {}", start.z, end.z);
    assert!(
        (end.z - start.z).abs() > 0.1,
        "英雄在输入移动后没有移动：start.z={}, end.z={}",
        start.z,
        end.z
    );
}
