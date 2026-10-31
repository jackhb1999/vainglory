//! 端到端网络握手测试：在进程内启动服务端 App，并模拟 renet2 客户端连接，
//! 验证客户端能收到 Welcome / HeroSpawn / MatchStart，且服务端生成了对应英雄实体。

use bevy::prelude::*;
use game_core::net::{ClientMessage, GameEvent, InputCmd, ServerMessage, Vec2f, PROTOCOL_VERSION};
use renet2::{ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use server::networking::NetworkingPlugin;
use server::simulation::SimulationPlugin;
use server::systems::*;
use game_core::components::*;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestClient {
    client: RenetClient,
    transport: NetcodeClientTransport,
}

fn spawn_server_app(port: u16) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(NetworkingPlugin)
        .add_plugins(SimulationPlugin)
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(server::networking::ServerPort(port));
    app
}

fn connect_client(port: u16) -> TestClient {
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
    TestClient { client, transport }
}

/// 推进客户端一帧；返回该帧收到的所有 ServerMessage。
fn step_client(c: &mut TestClient, delta: Duration) -> Vec<ServerMessage> {
    c.client.update(delta);
    let _ = c.transport.update(delta, &mut c.client);
    let mut msgs = Vec::new();
    while let Some(bytes) = c.client.receive_message(DefaultChannel::ReliableOrdered) {
        if let Ok(msg) = bitcode::decode::<ServerMessage>(bytes.as_ref()) {
            msgs.push(msg);
        }
    }
    let _ = c.transport.send_packets(&mut c.client);
    msgs
}

/// 推进服务端一帧。
fn step_server(app: &mut App) {
    app.update();
}

/// 客户端向服务端发送一帧移动输入（不可靠通道，与正式客户端一致）。
fn send_input(c: &mut TestClient, dir: Vec2f) {
    let cmd = InputCmd {
        tick: 0,
        move_dir: dir,
        aim_dir: Vec2f::zero(),
        ability_mask: 0,
        ping: 0,
    };
    let bytes = bitcode::encode(&ClientMessage::Input(cmd));
    c.client.send_message(DefaultChannel::Unreliable, bytes);
    let _ = c.transport.send_packets(&mut c.client);
}

/// 带技能掩码与瞄准方向的输入发送。
fn send_input_full(c: &mut TestClient, dir: Vec2f, aim: Vec2f, mask: u8) {
    let cmd = InputCmd {
        tick: 0,
        move_dir: dir,
        aim_dir: aim,
        ability_mask: mask,
        ping: 0,
    };
    let bytes = bitcode::encode(&ClientMessage::Input(cmd));
    c.client.send_message(DefaultChannel::Unreliable, bytes);
    let _ = c.transport.send_packets(&mut c.client);
}

/// 推进固定步长模拟（与 hero_moves_with_input 一致）：先 update 接收输入，再手动推进 FixedUpdate。
fn step_sim(server: &mut App, delta: Duration) {
    step_server(server);
    server
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(delta);
    server.world_mut().run_schedule(FixedUpdate);
}

#[test]
fn client_connects_and_match_starts() {
    let mut server = spawn_server_app(5000);
    // 首次 update 触发 Startup（start_server），随后 Update 系统开始运行
    step_server(&mut server);

    let delta = Duration::from_secs_f64(1.0 / 60.0);

    let mut c1 = connect_client(5000);
    let mut c2 = connect_client(5000);

    let mut c1_welcome = false;
    let mut c1_spawn = false;
    let mut c1_match = false;
    let mut c2_welcome = false;
    let mut c2_match = false;

    for _ in 0..600 {
        step_server(&mut server);
        for m in step_client(&mut c1, delta) {
            match m {
                ServerMessage::Welcome { .. } => c1_welcome = true,
                ServerMessage::Event(GameEvent::HeroSpawn { .. }) => c1_spawn = true,
                ServerMessage::Event(GameEvent::MatchStart { .. }) => c1_match = true,
                _ => {}
            }
        }
        for m in step_client(&mut c2, delta) {
            match m {
                ServerMessage::Welcome { .. } => c2_welcome = true,
                ServerMessage::Event(GameEvent::MatchStart { .. }) => c2_match = true,
                _ => {}
            }
        }
        if c1_welcome && c1_spawn && c1_match && c2_welcome && c2_match {
            break;
        }
    }

    eprintln!(
        "FLAGS c1_welcome={} c1_spawn={} c1_match={} c2_welcome={} c2_match={}",
        c1_welcome, c1_spawn, c1_match, c2_welcome, c2_match
    );

    assert!(c1_welcome, "client1 should receive Welcome");
    assert!(c1_spawn, "client1 should receive HeroSpawn");
    assert!(c1_match, "client1 should receive MatchStart");
    assert!(c2_welcome, "client2 should receive Welcome");
    assert!(c2_match, "client2 should receive MatchStart");

    // 服务端应为两名玩家各分配一个 NetId（连接建立时即生成对应英雄实体）
    let net_ids = server.world().resource::<server::networking::ClientNetIds>();
    assert_eq!(net_ids.0.len(), 2, "server should track 2 connected clients");
}

/// 验证「客户端输入 → 服务端权威模拟移动 → 快照」的链路：
/// 持续发送 +X 输入，确认服务端英雄的 TransformSync 位置确实发生了位移。
#[test]
fn hero_moves_with_input() {
    // 使用独立端口，避免与握手测试并行时端口冲突
    let mut server = spawn_server_app(5001);
    step_server(&mut server);

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    let mut c = connect_client(5001);

    let mut my_net_id: Option<game_core::components::NetId> = None;
    let mut spawn_x = 0.0f32;

    // 完成握手，记录本地英雄的 net id 与出生点
    for _ in 0..600 {
        step_server(&mut server);
        for m in step_client(&mut c, delta) {
            if let ServerMessage::Welcome { your_net_id, .. } = m {
                my_net_id = Some(your_net_id);
            }
            if let ServerMessage::Event(GameEvent::HeroSpawn { net_id, position, .. }) = m {
                if Some(net_id) == my_net_id {
                    spawn_x = position.x;
                }
            }
        }
        if my_net_id.is_some() {
            break;
        }
    }
    let net_id = my_net_id.expect("client should receive Welcome");

    // 持续向 +X 方向移动；手动固定每帧步长，确保 FixedUpdate 模拟使用一致的 dt
    for _ in 0..150 {
        send_input(&mut c, Vec2f::new(1.0, 0.0));
        // 必须驱动客户端 transport，否则输入包不会被真正发出
        step_client(&mut c, delta);
        step_server(&mut server);
        // 手动推进一帧固定步长的权威模拟
        server
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(1.0 / 60.0));
        server.world_mut().run_schedule(FixedUpdate);
    }

    // 读取服务端英雄的最终位置
    let mut world = server.world_mut();
    let mut hero_query = world.query::<(&game_core::components::NetId, &game_core::components::TransformSync)>();
    let mut final_x = None;
    for (nid, sync) in hero_query.iter(&world) {
        if *nid == net_id {
            final_x = Some(sync.position.x);
        }
    }
    let final_x = final_x.expect("hero entity should exist on server");

    eprintln!("hero spawn_x={} final_x={}", spawn_x, final_x);
    assert!(
        final_x > spawn_x + 1.0,
        "hero should move +X under input: spawn_x={} final_x={}",
        spawn_x,
        final_x
    );
}

/// 与上个测试对称：持续发送 +Z 输入，确认服务端权威模拟在 z 轴（move_dir.y）上也会位移。
#[test]
fn hero_moves_in_z() {
    let mut server = spawn_server_app(5002);
    step_server(&mut server);

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    let mut c = connect_client(5002);

    let mut my_net_id: Option<game_core::components::NetId> = None;
    let mut spawn_z = 0.0f32;

    for _ in 0..600 {
        step_server(&mut server);
        for m in step_client(&mut c, delta) {
            if let ServerMessage::Welcome { your_net_id, .. } = m {
                my_net_id = Some(your_net_id);
            }
            if let ServerMessage::Event(GameEvent::HeroSpawn { net_id, position, .. }) = m {
                if Some(net_id) == my_net_id {
                    spawn_z = position.z;
                }
            }
        }
        if my_net_id.is_some() {
            break;
        }
    }
    let net_id = my_net_id.expect("client should receive Welcome");

    // 持续向 +Z 方向移动（move_dir.y 对应 world z）
    for _ in 0..150 {
        send_input(&mut c, Vec2f::new(0.0, 1.0));
        step_client(&mut c, delta);
        step_server(&mut server);
        server
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(1.0 / 60.0));
        server.world_mut().run_schedule(FixedUpdate);
    }

    let mut world = server.world_mut();
    let mut hero_query = world.query::<(&game_core::components::NetId, &game_core::components::TransformSync)>();
    let mut final_z = None;
    for (nid, sync) in hero_query.iter(&world) {
        if *nid == net_id {
            final_z = Some(sync.position.z);
        }
    }
    let final_z = final_z.expect("hero entity should exist on server");

    eprintln!("hero spawn_z={} final_z={}", spawn_z, final_z);
    assert!(
        final_z > spawn_z + 1.0,
        "hero should move +Z under input: spawn_z={} final_z={}",
        spawn_z,
        final_z
    );
}

/// 验证「客户端释放技能 → 服务端权威命中结算 → 目标掉血」链路：
/// 把两名英雄摆到彼此附近，蓝方朝红方释放弹道技能（slot0 火焰弹），
/// 确认红方英雄最终血量低于满血。
#[test]
fn ability_projectile_deals_damage() {
    let mut server = spawn_server_app(5003);
    step_server(&mut server);

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    let mut c1 = connect_client(5003);
    let mut c2 = connect_client(5003);

    let mut c1_id = None;
    let mut c2_id = None;
    for _ in 0..600 {
        step_server(&mut server);
        for m in step_client(&mut c1, delta) {
            if let ServerMessage::Welcome { your_net_id, .. } = m {
                c1_id = Some(your_net_id);
            }
        }
        for m in step_client(&mut c2, delta) {
            if let ServerMessage::Welcome { your_net_id, .. } = m {
                c2_id = Some(your_net_id);
            }
        }
        if c1_id.is_some() && c2_id.is_some() {
            break;
        }
    }
    let c1_id = c1_id.expect("c1 welcome");
    let c2_id = c2_id.expect("c2 welcome");

    // 将两名英雄摆到彼此附近：蓝 (0,0,0)，红 (0,0,5)
    {
        let mut world = server.world_mut();
        let mut q = world
            .query::<(&game_core::components::NetId, &mut Transform, &mut game_core::components::TransformSync)>();
        for (nid, mut tf, mut sync) in q.iter_mut(&mut world) {
            if *nid == c1_id {
                tf.translation = Vec3::new(0.0, 0.0, 0.0);
                sync.position = Vec3::new(0.0, 0.0, 0.0);
            } else if *nid == c2_id {
                tf.translation = Vec3::new(0.0, 0.0, 5.0);
                sync.position = Vec3::new(0.0, 0.0, 5.0);
            }
        }
    }

    // 蓝方持续朝 +z（红方方向）释放 slot0（bit1 = 0b0010）
    for _ in 0..120 {
        send_input_full(&mut c1, Vec2f::zero(), Vec2f::new(0.0, 1.0), 0b0010);
        step_client(&mut c1, delta);
        step_sim(&mut server, delta);
    }

    // 读取红方英雄血量
    let mut world = server.world_mut();
    let mut q = world.query::<(&game_core::components::NetId, &game_core::components::Hero)>();
    let mut red_health = None;
    for (nid, hero) in q.iter(&world) {
        if *nid == c2_id {
            red_health = Some(hero.health);
        }
    }
    let red_health = red_health.expect("red hero should exist on server");
    eprintln!("red hero health after ability = {}", red_health);
    assert!(
        red_health < 1000.0,
        "red hero should take damage from projectile ability: health={}",
        red_health
    );
}

/// 中立训练假人应能被技能弹体命中并掉血（且不会因血量归零被销毁）。
#[test]
fn training_dummy_takes_damage() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_systems(
        Update,
        (
            server::systems::projectile_system,
            server::systems::damage_system,
        ),
    );

    // 中立训练假人，放在原点，高血量
    let dummy = app
        .world_mut()
        .spawn((
            Team::Neutral,
            Transform::from_translation(Vec3::ZERO),
            TransformSync {
                position: Vec3::ZERO,
                ..default()
            },
            Hero {
                name: "训练假人".to_string(),
                health: 5000.0,
                max_health: 5000.0,
                ..default()
            },
            TrainingDummy,
        ))
        .id();

    // 一枚蓝方弹体，紧贴假人，超大命中半径以排除单帧跨步（tunneling）干扰
    let proj_e = app
        .world_mut()
        .spawn((
            Team::Blue,
            Transform::from_translation(Vec3::ZERO),
            TransformSync {
                position: Vec3::ZERO,
                ..default()
            },
            Projectile {
                owner: NetId(999),
                team: Team::Blue,
                damage: 100.0,
                damage_type: DamageType::Physical,
                direction: Vec3::new(0.0, 0.0, 1.0),
                speed: 10.0,
                traveled: 0.0,
                max_range: 50.0,
                radius: 50.0,
                dead: false,
            },
        ))
        .id();

    // 跑多帧：第 1 帧弹体命中并插入 AttackCommand（延迟命令在帧末 flush），
    // 第 2 帧 damage_system 才能读到并结算伤害。
    for _ in 0..5 {
        app.update();
    }

    let hero = app.world().get::<Hero>(dummy).expect("dummy should have Hero");
    assert!(
        hero.health < 5000.0,
        "中立训练假人应被弹体命中掉血, health={}",
        hero.health
    );

    // 继续多次结算，确认低血量也不会被销毁（训练假人不应消失）
    for _ in 0..20 {
        app.update();
    }
    assert!(
        app.world().get_entity(dummy).is_ok(),
        "训练假人在掉血后不应被销毁"
    );
}

/// 用完整服务端插件（含 ServerPlugin / SpawnPlugin / 状态机）启动，模拟一个真实客户端连接，
/// 验证：不产生 panic、客户端能收到 Welcome / WorldSnapshot / 训练假人 HeroSpawn。
/// 任何服务端 panic 都会在此暴露。
#[test]
fn full_server_connects_and_spawns_dummies() {
    use bevy::asset::Assets;
    use bevy::pbr::StandardMaterial;
    use bevy::render::mesh::Mesh;
    use bevy::state::app::StatesPlugin;

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(StatesPlugin)
        .add_plugins(NetworkingPlugin)
        .add_plugins(SimulationPlugin)
        .add_plugins(server::ServerPlugin)
        .add_plugins(server::spawn::SpawnPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(server::networking::ServerPort(5055));

    // 首次 update 触发 Startup（start_server）
    app.update();

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    let mut client = connect_client(5055);

    let mut got_welcome = false;
    let mut got_snapshot = false;
    let mut got_dummy = false;
    let mut got_match_start = false;

    for _ in 0..240 {
        let msgs = step_client(&mut client, delta);
        for m in msgs {
            match m {
                ServerMessage::Welcome { .. } => got_welcome = true,
                ServerMessage::WorldSnapshot(_) => got_snapshot = true,
                ServerMessage::Event(GameEvent::HeroSpawn {
                    team: Team::Neutral,
                    ..
                }) => got_dummy = true,
                ServerMessage::Event(GameEvent::MatchStart { .. }) => got_match_start = true,
                _ => {}
            }
        }
        app.update();
    }

    eprintln!(
        "welcome={} snapshot={} dummy={} match_start={}",
        got_welcome, got_snapshot, got_dummy, got_match_start
    );

    assert!(got_welcome, "客户端应收到 Welcome");
    assert!(
        got_snapshot,
        "客户端应收到 WorldSnapshot（否则无法进入 InGame，导致无法移动/释放技能）"
    );
    assert!(got_dummy, "客户端应收到训练假人 HeroSpawn");
    // 单人时红队为空，MatchStart 不会下发；仅作信息提示
    eprintln!("match_start received = {}", got_match_start);
}
