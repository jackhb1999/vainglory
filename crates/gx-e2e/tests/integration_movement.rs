//! 端到端验证：真实 server + 真实 client 同进程跑，注入移动输入，
//! 检查客户端本地英雄是否真的移动（覆盖 网络 → 服务端 → 快照 → 客户端预测/回滚 全链路）。
//! 若本地英雄没拿到 LocalPlayer，或网络链路断了，这里会暴露。

use bevy::asset::AssetPlugin;
use bevy::image::Image;
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use client::input::PlayerInputState;
use client::networking::{ClientLobbyInfo, NetworkingPlugin as ClientNet, ReceivedSnapshot};
use client::prediction::PredictionPlugin;
use game_core::components::*;
use server::ServerPlugin;
use server::networking::{NetworkingPlugin as ServerNet, ServerPort};
use server::simulation::SimulationPlugin as ServerSim;
use server::spawn::SpawnPlugin as ServerSpawn;
use std::time::Duration;

/// 该测试与真实游戏进程共用默认 5000 端口会冲突，改用专用端口；
/// 客户端 `server_address()` 读取 `GX_SERVER_PORT`，因此两端都指向同一端口。
const TEST_PORT: u16 = 5099;

fn main_app() -> (App, App) {
    // SAFETY: 该测试二进制只有一个测试、单线程运行，且在此之后不再读写环境变量；
    // 其他测试各自是独立进程，不会被这里的修改影响。
    unsafe {
        std::env::set_var("GX_SERVER_PORT", TEST_PORT.to_string());
    }
    let mut server = App::new();
    server
        .add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        .add_plugins(ServerNet)
        .add_plugins(ServerSim)
        .add_plugins(ServerPlugin)
        .add_plugins(ServerSpawn)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(ServerPort(TEST_PORT))
        .insert_resource(Time::<Fixed>::from_hz(60.0));

    let mut client = App::new();
    client
        .add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        // 状态机：连接只在对局状态建立（大厅不应偷偷连服务器），
        // 因此 headless 客户端需显式进入 InMatch
        .add_plugins(bevy::state::app::StatesPlugin)
        .insert_state(client::AppState::InMatch)
        .add_plugins(ClientNet)
        .add_plugins(PredictionPlugin)
        .init_resource::<PlayerInputState>()
        .init_resource::<ClientLobbyInfo>()
        .init_resource::<ReceivedSnapshot>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>();

    (server, client)
}

#[test]
fn end_to_end_local_hero_moves() {
    let (mut server, mut client) = main_app();

    let mut start_z: Option<f32> = None;
    let mut end_z: Option<f32> = None;
    let mut saw_local = false;

    for _ in 0..600 {
        std::thread::sleep(Duration::from_millis(8));
        server.update();

        // 一旦拿到自己的 net_id，就持续注入“向上(W/上 = -z)”移动输入
        let connected = client
            .world()
            .get_resource::<ClientLobbyInfo>()
            .map(|l| l.my_net_id.is_some())
            .unwrap_or(false);
        if connected {
            client
                .world_mut()
                .resource_mut::<PlayerInputState>()
                .move_dir = Vec2::new(0.0, -1.0);
        }

        client.update();

        // 查询客户端本地英雄
        let world = client.world_mut();
        let mut q = QueryState::<&Transform, With<LocalPlayer>>::new(world);
        for t in q.iter(world) {
            saw_local = true;
            if start_z.is_none() {
                start_z = Some(t.translation.z);
            }
            end_z = Some(t.translation.z);
        }
    }

    eprintln!(
        "saw_local={}, start_z={:?}, end_z={:?}",
        saw_local, start_z, end_z
    );
    assert!(
        saw_local,
        "客户端从未生成带 LocalPlayer 的本地英雄（is_local 逻辑疑似失效）"
    );
    let (s, e) = (start_z.unwrap(), end_z.unwrap());
    assert!(
        (e - s).abs() > 0.5,
        "本地英雄在持续移动输入下没有移动：start_z={}, end_z={}（网络链路或预测/回滚失效）",
        s,
        e
    );
}
