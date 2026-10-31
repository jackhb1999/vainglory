//! 服务端网络模块
//!
//! 使用 renet2 作为权威服务器：监听 0.0.0.0:5000，接收客户端连接、
//! 分配 NetId、生成英雄实体，并将世界快照与游戏事件下发给所有客户端。

use bevy::prelude::*;
use std::collections::HashMap;
use renet2::{ClientId, ConnectionConfig, DefaultChannel, RenetServer};
use renet2_netcode::{
    NativeSocket, NetcodeServerTransport, ServerAuthentication, ServerSetupConfig,
};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use game_core::net::{
    ClientMessage, EntitySnapshot, EntityType, GameEvent, InputCmd, PROTOCOL_VERSION,
    ServerMessage, Vec3f, Quatf, WorldSnapshot,
};
use game_core::components::*;
use bitcode;

/// 待处理的客户端输入（键为英雄的 net id）
#[derive(Resource, Default)]
pub struct PendingInputs {
    pub inputs: HashMap<u64, InputCmd>,
}

/// renet 连接 id -> 该玩家在服务端分配的 (NetId, Team)
#[derive(Resource, Default)]
pub struct ClientNetIds(pub HashMap<ClientId, (NetId, Team)>);

/// 下一个待分配的 NetId
#[derive(Resource, Default)]
pub struct NextNetId(pub u64);

/// 服务端默认监听端口
pub const DEFAULT_SERVER_PORT: u16 = 5000;

/// 服务端监听端口（可通过资源覆盖，便于测试/部署）
#[derive(Resource)]
pub struct ServerPort(pub u16);

impl Default for ServerPort {
    fn default() -> Self {
        ServerPort(DEFAULT_SERVER_PORT)
    }
}

/// 比赛是否已开始（用于只下发一次 MatchStart）
#[derive(Resource, Default)]
pub struct MatchStarted(pub bool);

/// 服务端累积的待广播游戏事件（如 AbilityCast），每个广播周期清空
#[derive(Resource, Default)]
pub struct ServerEvents(pub Vec<GameEvent>);

/// 训练假人的生成信息（net id / 阵营 / 名称 / 位置），用于把假人的 HeroSpawn 事件
/// 下发给每个连接（含后续加入）的客户端，使其能渲染假人。
#[derive(Resource, Default)]
pub struct DummyRegistry(pub Vec<GameEvent>);

/// 当前网络状态摘要（供服务端状态机读取）
#[derive(Resource, Default)]
pub struct ServerNetworkState {
    pub connected_players: usize,
    pub blue_count: usize,
    pub red_count: usize,
}

#[derive(Resource)]
pub struct RenetServerResource(pub RenetServer);

#[derive(Resource)]
pub struct ServerTransportResource(pub NetcodeServerTransport);

/// 用真实墙钟驱动 netcode 时钟。
///
/// `ScheduleRunnerPlugin` 的默认忙循环（无延迟）会让 `Time::delta()` 近似为 0，
/// 若每帧用固定 1/60 的 delta 推进 `NetcodeServerTransport`，其 `current_time`
/// 会以远超真实时间的速度飞奔，导致客户端（以真实时间推进时钟）的连接请求
/// 时间戳始终落在服务器校验窗口之外而被拒绝。因此这里用 `Instant` 计算真实
/// 流逝时间喂给 transport，使其 `current_time` 与客户端保持同步。
#[derive(Resource)]
pub struct NetcodeClock {
    last: Instant,
}

impl Default for NetcodeClock {
    fn default() -> Self {
        NetcodeClock {
            last: Instant::now(),
        }
    }
}

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingInputs>()
            .init_resource::<ClientNetIds>()
            .init_resource::<NextNetId>()
            .init_resource::<MatchStarted>()
            .init_resource::<ServerNetworkState>()
            .init_resource::<ServerPort>()
            .init_resource::<NetcodeClock>()
            .init_resource::<ServerEvents>()
            .init_resource::<DummyRegistry>()
            .add_systems(Startup, start_server)
            .add_systems(Update, (poll_connections, receive_inputs, broadcast_snapshot).chain());
    }
}

/// 启动服务器：创建 UDP socket 并监听配置端口（默认 0.0.0.0:5000）
pub fn start_server(mut commands: Commands, port: Res<ServerPort>) {
    let server_addr: SocketAddr = format!("0.0.0.0:{}", port.0)
        .parse()
        .expect("Invalid server address");
    let socket = match UdpSocket::bind(server_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "无法绑定 {}：{e}。\n端口可能已被另一个服务端实例占用，请先关闭/结束旧的 vainglory-server 进程再启动。",
                server_addr
            );
            std::process::exit(1);
        }
    };
    let socket = NativeSocket::new(socket).expect("Failed to create native socket");

    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let server_config = ServerSetupConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_VERSION as u64,
        // 连接令牌中公布的地址需与客户端实际连接地址一致（本地回环 127.0.0.1）。
        // 如需局域网联机，请改为宿主机的实际 IP。
        socket_addresses: vec![vec![
            format!("127.0.0.1:{}", port.0)
                .parse::<SocketAddr>()
                .unwrap(),
        ]],
        authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket)
        .expect("Failed to create server transport");
    let server = RenetServer::new(ConnectionConfig::from_shared_channels(DefaultChannel::config()));

    commands.insert_resource(RenetServerResource(server));
    commands.insert_resource(ServerTransportResource(transport));
    info!("Server listening on {}", server_addr);
}

/// 处理连接/断开、接受/发送数据包
fn poll_connections(
    mut commands: Commands,
    mut server_res: Option<ResMut<RenetServerResource>>,
    mut transport_res: Option<ResMut<ServerTransportResource>>,
    mut client_net_ids: ResMut<ClientNetIds>,
    mut next_net_id: ResMut<NextNetId>,
    mut net_state: ResMut<ServerNetworkState>,
    mut clock: ResMut<NetcodeClock>,
    dummy_registry: Res<DummyRegistry>,
    heroes: Query<(Entity, &NetId, &Team)>,
) {
    let (Some(mut server_res), Some(mut transport_res)) = (server_res, transport_res) else {
        return;
    };

    // 用真实墙钟流逝时间推进 netcode，避免使用固定 delta 在忙循环下导致时钟飞奔
    let now = Instant::now();
    let delta = now.duration_since(clock.last);
    clock.last = now;
    server_res.0.update(delta);
    let _ = transport_res.0.update(delta, &mut server_res.0);

    // 处理新连接
    for client_id in server_res.0.clients_id() {
        if client_net_ids.0.contains_key(&client_id) {
            continue;
        }
        // 基于已分配的队伍记录来交替分配，避免使用延迟生成的英雄实体（commands 尚未 apply）导致计数滞后
        let blue_count = client_net_ids
            .0
            .values()
            .filter(|(_, t)| *t == Team::Blue)
            .count();
        let red_count = client_net_ids
            .0
            .values()
            .filter(|(_, t)| *t == Team::Red)
            .count();
        let team = if blue_count <= red_count {
            Team::Blue
        } else {
            Team::Red
        };

        let net_id = NetId(next_net_id.0);
        next_net_id.0 += 1;

        info!("Client {:?} connected -> net id {:?} ({:?})", client_id, net_id, team);

        // 生成服务端权威英雄实体
        let spawn_pos = match team {
            Team::Blue => bevy::math::Vec3::new(-15.0, 0.0, 0.0),
            Team::Red => bevy::math::Vec3::new(15.0, 0.0, 0.0),
            Team::Neutral => bevy::math::Vec3::ZERO,
        };
        let hero_entity = commands.spawn((
            net_id,
            team,
            Transform::from_translation(spawn_pos),
            Hero {
                name: "Ringo".to_string(),
                move_speed: game_core::components::HERO_BASE_MOVE_SPEED,
                ..default()
            },
            TransformSync {
                position: spawn_pos,
                ..default()
            },
            PlayerInput::default(),
            HeroAbilities {
                cooldowns: [0.0; 4],
                ability_ids: game_core::abilities::default_hero_abilities(),
            },
            ServerAuthority,
        )).id();
        info!("Spawned hero entity {:?} for net {:?}", hero_entity, net_id);

        client_net_ids.0.insert(client_id, (net_id, team));

        let welcome = ServerMessage::Welcome {
            your_net_id: net_id,
            assigned_team: team,
            match_id: 0,
            server_tick: 0,
        };
        server_res
            .0
            .send_message(client_id, DefaultChannel::ReliableOrdered, bitcode::encode(&welcome));

        let hero_spawn = ServerMessage::Event(GameEvent::HeroSpawn {
            net_id,
            team,
            hero_name: "Ringo".to_string(),
            position: Vec3f::from_vec3(spawn_pos),
        });
        server_res
            .0
            .send_message(client_id, DefaultChannel::ReliableOrdered, bitcode::encode(&hero_spawn));

        // 把已存在的训练假人 HeroSpawn 一并下发给新客户端
        for dummy_event in &dummy_registry.0 {
            server_res.0.send_message(
                client_id,
                DefaultChannel::ReliableOrdered,
                bitcode::encode(&ServerMessage::Event(dummy_event.clone())),
            );
        }
    }

    // 处理断开连接
    for client_id in server_res.0.disconnections_id() {
        if let Some((net_id, _team)) = client_net_ids.0.remove(&client_id) {
            for (entity, e_net_id, _) in heroes.iter() {
                if *e_net_id == net_id {
                    commands.entity(entity).despawn();
                    break;
                }
            }
            // 通知其余客户端移除该英雄实体，否则其客户端会残留僵尸实体
            for other in server_res.0.clients_id() {
                if other == client_id {
                    continue;
                }
                let _ = server_res.0.send_message(
                    other,
                    DefaultChannel::ReliableOrdered,
                    bitcode::encode(&ServerMessage::Event(GameEvent::HeroDespawn { net_id })),
                );
            }
            info!("Client {:?} (net {:?}) disconnected", client_id, net_id);
        }
    }

    let _ = transport_res.0.send_packets(&mut server_res.0);

    net_state.connected_players = client_net_ids.0.len();
    net_state.blue_count = client_net_ids
        .0
        .values()
        .filter(|(_, t)| *t == Team::Blue)
        .count();
    net_state.red_count = client_net_ids
        .0
        .values()
        .filter(|(_, t)| *t == Team::Red)
        .count();
}

/// 接收客户端输入，按 client_id -> net id 映射后写入 PendingInputs
fn receive_inputs(
    mut server_res: Option<ResMut<RenetServerResource>>,
    mut pending: ResMut<PendingInputs>,
    client_net_ids: Res<ClientNetIds>,
) {
    let Some(mut server) = server_res else {
        return;
    };

    for client_id in server.0.clients_id() {
        let net_id = match client_net_ids.0.get(&client_id) {
            Some(id) => *id,
            None => continue,
        };
        while let Some(bytes) = server.0.receive_message(client_id, DefaultChannel::Unreliable) {
            if let Ok(ClientMessage::Input(cmd)) = bitcode::decode::<ClientMessage>(bytes.as_ref()) {
                pending.inputs.insert(net_id.0 .0, cmd);
            }
        }
    }
}

/// 收集英雄状态并广播世界快照；比赛开始时下发一次 MatchStart
fn broadcast_snapshot(
    mut server_res: Option<ResMut<RenetServerResource>>,
    mut transport_res: Option<ResMut<ServerTransportResource>>,
    heroes: Query<(&NetId, &Team, &Hero, &TransformSync, &HeroAbilities)>,
    projectiles: Query<(&NetId, &Team, &TransformSync, &Projectile)>,
    mut server_events: ResMut<ServerEvents>,
    tick: Res<crate::simulation::ServerTick>,
    mut match_started: ResMut<MatchStarted>,
) {
    let (Some(mut server), Some(mut transport)) = (server_res, transport_res) else {
        return;
    };

    let mut entities: Vec<EntitySnapshot> = heroes
        .iter()
        .map(|(net_id, team, hero, ts, abilities)| EntitySnapshot {
            net_id: *net_id,
            entity_type: EntityType::Hero,
            team: *team,
            position: Vec3f::from_vec3(ts.position),
            rotation: Quatf::new(ts.rotation.x, ts.rotation.y, ts.rotation.z, ts.rotation.w),
            velocity: Vec3f::from_vec3(ts.velocity),
            health: hero.health,
            max_health: hero.max_health,
            energy: hero.energy,
            max_energy: hero.max_energy,
            level: hero.level,
            gold: hero.gold,
            ability_cooldowns: abilities.cooldowns,
        })
        .collect();

    // 弹道实体也随快照下发，供客户端渲染
    for (net_id, team, ts, proj) in projectiles.iter() {
        entities.push(EntitySnapshot {
            net_id: *net_id,
            entity_type: EntityType::Projectile,
            team: *team,
            position: Vec3f::from_vec3(ts.position),
            rotation: Quatf::default(),
            velocity: Vec3f::from_vec3(proj.direction * proj.speed),
            health: 0.0,
            max_health: 0.0,
            energy: 0.0,
            max_energy: 0.0,
            level: 0,
            gold: 0,
            ability_cooldowns: [0.0; 4],
        });
    }

    let events = std::mem::take(&mut server_events.0);
    let snapshot = WorldSnapshot {
        tick: tick.0,
        server_time: 0.0,
        entities,
        events,
    };

    for client_id in server.0.clients_id() {
        server.0.send_message(
            client_id,
            DefaultChannel::ReliableOrdered,
            bitcode::encode(&ServerMessage::WorldSnapshot(snapshot.clone())),
        );
    }

    // 当蓝红两队都至少各有一名玩家时，下发一次 MatchStart
    if !match_started.0 {
        let mut blue = Vec::new();
        let mut red = Vec::new();
        for (net_id, team, hero, _, _) in heroes.iter() {
            let entry = (*net_id, hero.name.clone());
            match team {
                Team::Blue => blue.push(entry),
                Team::Red => red.push(entry),
                Team::Neutral => {} // 中立实体（如训练假人）不计入双方英雄列表
            }
        }
        if !blue.is_empty() && !red.is_empty() {
            let blue_count = blue.len();
            let red_count = red.len();
            let event = ServerMessage::Event(GameEvent::MatchStart {
                blue_heroes: blue,
                red_heroes: red,
            });
            for client_id in server.0.clients_id() {
                server.0.send_message(
                    client_id,
                    DefaultChannel::ReliableOrdered,
                    bitcode::encode(&event),
                );
            }
            match_started.0 = true;
            info!("Match started with {} vs {}", blue_count, red_count);
        }
    }

    let _ = transport.0.send_packets(&mut server.0);
}
