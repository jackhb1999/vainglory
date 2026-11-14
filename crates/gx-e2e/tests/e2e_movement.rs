//! 端到端移动链路测试：点击目标 → handle_input → PlayerInputState →
//! 客户端预测 → 上行 InputCmd → 服务端权威模拟 → 快照回传，验证 x/z 两轴都能移动。
//! v1.3 起为纯点击控制，键盘移动已移除（另有护栏断言）。
//!
//! 运行方式：cargo test -p gx-e2e --test e2e_movement -- --nocapture

use bevy::prelude::*;
use client::{
    input::{InputPlugin, MoveTarget, PathState, PlayerInputState},
    networking::NetworkingPlugin,
    prediction::PredictionPlugin,
    rendering::RenderingPlugin,
};
use game_core::components::{LocalPlayer, Predicted};
use game_core::net::PROTOCOL_VERSION;
use renet2::{ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use server::networking::NetworkingPlugin as ServerNetworkingPlugin;
use server::simulation::SimulationPlugin as ServerSimulationPlugin;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn frame_duration() -> Duration {
    Duration::from_secs_f64(1.0 / 60.0)
}

fn spawn_server_app(port: u16) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ServerNetworkingPlugin)
        .add_plugins(ServerSimulationPlugin)
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(server::networking::ServerPort(port));
    app.update(); // 触发 Startup（start_server）
    app
}

fn step_fixed(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(frame_duration());
    app.world_mut().run_schedule(FixedUpdate);
}

/// 用指定测试端口重建客户端的网络资源（默认 Startup 连的是 5000）
fn rehook_client_to_port(app: &mut App, port: u16) {
    let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind client socket");
    let socket = NativeSocket::new(socket).expect("native socket");
    let connection_config = ConnectionConfig::from_shared_channels(DefaultChannel::config());
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let authentication = ClientAuthentication::Unsecure {
        socket_id: 0,
        server_addr,
        client_id: rand::random(),
        user_data: None,
        protocol_id: PROTOCOL_VERSION as u64,
    };
    let client = RenetClient::new(connection_config, false);
    let transport = NetcodeClientTransport::new(current_time, authentication, socket)
        .expect("client transport");
    app.insert_resource(client::networking::RenetClientResource(client));
    app.insert_resource(client::networking::ClientTransportResource(transport));
    app.insert_resource(client::networking::NetworkState::Connecting);
    app.world_mut()
        .resource_mut::<client::networking::ClientLobbyInfo>()
        .my_net_id = None;
}

fn spawn_client_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        // headless：不加载 EguiPlugin（渲染依赖过重）；输入系统对无 egui 上下文已做优雅降级
        // 与服务端 main.rs 同理：手动初始化 CPU 侧资产存储
        .init_resource::<bevy::asset::Assets<bevy::mesh::Mesh>>()
        .init_resource::<bevy::asset::Assets<bevy::pbr::StandardMaterial>>()
        .init_resource::<bevy::asset::Assets<bevy::image::Image>>()
        // headless 无 InputPlugin，手动创建输入资源（handle_input 依赖）
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        // EguiContexts SystemParam 校验需要该资源（正常由 EguiPlugin 注册）
        .init_resource::<bevy_egui::EguiUserTextures>()
        .add_plugins(NetworkingPlugin)
        .add_plugins(InputPlugin)
        .add_plugins(PredictionPlugin)
        .insert_resource(Time::<Fixed>::from_hz(60.0));
    app.update(); // 触发 Startup（connect_to_server 5000）
    app
}

/// 握手：直到本地英雄实体生成且收到世界快照
fn handshake(server: &mut App, client: &mut App) {
    for _ in 0..600 {
        step_fixed(server);
        step_fixed(client);
        let spawned = client
            .world_mut()
            .query_filtered::<(), (With<LocalPlayer>, With<Predicted>)>()
            .single(client.world())
            .is_ok();
        let snapshot_tick = client
            .world()
            .resource::<client::networking::ReceivedSnapshot>()
            .0
            .tick;
        if spawned && snapshot_tick > 0 {
            return;
        }
    }
    panic!("handshake failed: local hero/snapshot never arrived");
}

fn local_hero_pos(client: &mut App) -> Vec3 {
    let mut q = client
        .world_mut()
        .query_filtered::<&Transform, (With<LocalPlayer>, With<Predicted>)>();
    let Ok(t) = q.single(client.world()) else {
        panic!("local hero entity missing");
    };
    t.translation
}

fn move_dir(client: &App) -> Vec2 {
    client.world().resource::<PlayerInputState>().move_dir
}

/// 按下某键（headless 手工维护 ButtonInput<KeyCode>）。
/// v1.3 起仅用于验证「键盘不再驱动移动」。
fn press_key(client: &mut App, key: KeyCode) {
    let mut kb = ButtonInput::<KeyCode>::default();
    kb.press(key);
    client.insert_resource(kb);
}

fn release_keys(client: &mut App) {
    client.insert_resource(ButtonInput::<KeyCode>::default());
}

/// 注入一次点击移动的最终产物（`MoveTarget`）。
/// headless 夹具没有 `Window` / `Camera3d`，无法真实投射鼠标射线，
/// 因此从点击链路的产物层驱动端到端验证；点击 → MoveTarget 的映射由
/// `client/src/input.rs` 单测覆盖。
fn set_click_target(client: &mut App, goal: Vec2) {
    client.insert_resource(MoveTarget(Some(goal)));
    client.insert_resource(PathState::default());
}

/// 按住某键若干帧，返回 (dx, dz)
fn run_frames(client: &mut App, server: &mut App, n: usize) -> (f32, f32) {
    let before = local_hero_pos(client);
    for _ in 0..n {
        step_fixed(server);
        step_fixed(client);
    }
    let after = local_hero_pos(client);
    (after.x - before.x, after.z - before.z)
}

#[test]
fn click_target_moves_hero_in_z() {
    let port = 5300;
    let mut server = spawn_server_app(port);
    let mut client = spawn_client_app();
    rehook_client_to_port(&mut client, port);
    handshake(&mut server, &mut client);

    let start = local_hero_pos(&mut client);
    eprintln!("hero start pos={:?}", start);
    set_click_target(&mut client, Vec2::new(start.x, start.z + 40.0));
    step_fixed(&mut server);
    step_fixed(&mut client);
    let dir = move_dir(&client);
    eprintln!("move_dir after +Z click = {:?}", dir);
    assert!(
        dir.y > 0.5,
        "+Z click should produce +y move_dir, got {dir:?}"
    );

    let (dx, dz) = run_frames(&mut client, &mut server, 60);
    eprintln!("after 60 frames of +Z click: dx={} dz={}", dx, dz);
    assert!(dz > 1.0, "hero should move +Z: dz={}", dz);
    assert!(
        dx.abs() < 1.0,
        "pure Z target must not drift in X: dx={}",
        dx
    );

    let now = local_hero_pos(&mut client);
    set_click_target(&mut client, Vec2::new(now.x, now.z - 40.0));
    let (_, dz2) = run_frames(&mut client, &mut server, 60);
    eprintln!("after 60 frames of -Z click: dz={}", dz2);
    assert!(dz2 < -1.0, "hero should move -Z: dz={}", dz2);
}

#[test]
fn click_target_moves_hero_in_x() {
    let port = 5301;
    let mut server = spawn_server_app(port);
    let mut client = spawn_client_app();
    rehook_client_to_port(&mut client, port);
    handshake(&mut server, &mut client);

    let start = local_hero_pos(&mut client);
    set_click_target(&mut client, Vec2::new(start.x + 40.0, start.z));
    let (dx, dz) = run_frames(&mut client, &mut server, 60);
    eprintln!("after 60 frames of +X click: dx={} dz={}", dx, dz);
    assert!(dx > 1.0, "hero should move +X: dx={}", dx);
    assert!(
        dz.abs() < 1.0,
        "pure X target must not drift in Z: dz={}",
        dz
    );
}

/// v1.3 回归护栏：键盘 W/A/S/D 与方向键不得再驱动英雄移动。
#[test]
fn keyboard_no_longer_moves_hero() {
    let port = 5303;
    let mut server = spawn_server_app(port);
    let mut client = spawn_client_app();
    rehook_client_to_port(&mut client, port);
    handshake(&mut server, &mut client);

    press_key(&mut client, KeyCode::ArrowUp);
    step_fixed(&mut server);
    step_fixed(&mut client);
    assert_eq!(
        move_dir(&client),
        Vec2::ZERO,
        "ArrowUp must not produce move_dir in click-only control"
    );
    let (dx, dz) = run_frames(&mut client, &mut server, 60);
    eprintln!("after 60 frames of ArrowUp: dx={} dz={}", dx, dz);
    assert!(
        dx.abs() < 1.0 && dz.abs() < 1.0,
        "keyboard must not move the hero: dx={dx} dz={dz}"
    );
    release_keys(&mut client);
}

/// 复现真实客户端（main.rs 含 RenderingPlugin）启动时的 B0001 查询冲突崩溃。
///
/// 多个系统同时持有 `&mut Transform`（camera_follow_local_player 的相机、
/// tick_hit_flash / update_target_marker 的标记实体）。它们必须靠 `Without<>`
/// 过滤器在编译期被证明互不相交，否则 Bevy 在调度构建/系统初始化时直接 panic。
/// e2e 的其它用例为 headless 故意不挂 RenderingPlugin，故无法覆盖此路径。
#[test]
fn rendering_plugin_schedule_builds_without_query_conflict() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(RenderingPlugin)
        .init_resource::<bevy::asset::Assets<bevy::mesh::Mesh>>()
        .init_resource::<bevy::asset::Assets<bevy::pbr::StandardMaterial>>()
        .init_resource::<bevy::asset::Assets<bevy::image::Image>>()
        .insert_resource(Time::<Fixed>::from_hz(60.0));
    // 首次 update 会构建调度并初始化系统；若存在查询冲突将在此 panic（B0001）
    app.update();
    app.update();
}
