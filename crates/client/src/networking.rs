//! 客户端网络模块
//!
//! 负责连接服务器、接收 ServerMessage（Welcome/WorldSnapshot/Event）并驱动
//! 英雄实体的生成与本地预测。

use bevy::prelude::*;
use bevy::image::Image;
use bevy::render::mesh::Mesh;
use bevy::pbr::StandardMaterial;
use renet2::{ClientId, ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use game_core::net::{
    ClientMessage, GameEvent, InputCmd, MatchState, PROTOCOL_VERSION, ServerMessage, WorldSnapshot,
};
use game_core::components::*;
use crate::rendering::make_hero_texture;
use bitcode;

/// 网络连接状态（同时是 Resource 与 States，供 lib.rs 的 OnEnter 调度使用）
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Hash, Debug, States)]
pub enum NetworkState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    InGame,
    Reconnecting,
}

/// 大厅信息（本地玩家的 net id、所属队伍、双方英雄列表）
#[derive(Resource, Default)]
pub struct ClientLobbyInfo {
    pub my_net_id: Option<NetId>,
    pub assigned_team: Option<Team>,
    pub blue_heroes: Vec<(NetId, String)>,
    pub red_heroes: Vec<(NetId, String)>,
}

/// 最近一次收到的服务端世界快照，供预测/插值系统使用
#[derive(Resource)]
pub struct ReceivedSnapshot(pub WorldSnapshot);

impl Default for ReceivedSnapshot {
    fn default() -> Self {
        Self(WorldSnapshot {
            tick: 0,
            server_time: 0.0,
            entities: Vec::new(),
            events: Vec::new(),
        })
    }
}

#[derive(Resource)]
pub struct RenetClientResource(pub RenetClient);

#[derive(Resource)]
pub struct ClientTransportResource(pub NetcodeClientTransport);

/// NetId -> 客户端实体 映射，用于英雄生成去重与断线清理。
/// 没有它，每次收到 HeroSpawn（连接 / 重连 / 快照事件）都会新建实体，
/// 实体数量会瞬间爆炸并撑爆 GPU 顶点缓冲（OOM）。
#[derive(Resource, Default)]
pub struct HeroRegistry(pub HashMap<NetId, Entity>);

/// 当前本地英雄实体（重连换 net id 时清理旧的，保证只有一个“我控制的英雄”）。
#[derive(Resource, Default)]
pub struct LocalHeroEntity(pub Option<Entity>);

/// 断线后自动重连的冷却计时（秒），避免断线后每帧疯狂重连
/// —— 每次重连都会在服务端生成全新身份 / 英雄实体，不加冷却会把客户端刷爆。
#[derive(Resource, Default)]
pub struct ReconnectTimer(pub f32);

/// 用真实墙钟驱动客户端 netcode 时钟，与服务端的 `NetcodeClock` 保持一致。
///
/// 不能用 Bevy 的 `Time::delta()`：窗口化运行时首次帧/卡顿帧 `delta()` 可能被
/// 拉长，导致 netcode 时钟相对服务端飞奔或滞后，使服务端/客户端把彼此的数据包
/// 判定为超时（connection timed out）。因此这里用 `Instant` 计算真实流逝时间。
#[derive(Resource, Default)]
pub struct ClientNetcodeClock {
    last: Option<Instant>,
}

/// 断线后多久尝试一次重连（秒）。
const RECONNECT_INTERVAL: f32 = 3.0;

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkState>()
            .init_resource::<ClientLobbyInfo>()
            .init_resource::<ReceivedSnapshot>()
            .init_resource::<HeroRegistry>()
            .init_resource::<LocalHeroEntity>()
            .init_resource::<ReconnectTimer>()
            .init_resource::<ClientNetcodeClock>()
            .add_systems(Startup, connect_to_server)
            .add_systems(
                Update,
                (poll_connection, receive_server_messages, send_input, try_reconnect).chain(),
            );
    }
}

/// 连接服务器（端口与 server 保持一致：5000）
///
/// 每次连接都用全新的随机 client_id。重连前清理上一次本地英雄实体，
/// 避免残留多个“我控制的英雄”。不使用固定 client_id，以免 renet 在
/// 重连复用同一 id 时把连接判为失效（connection timed out）。
pub fn connect_to_server(
    mut commands: Commands,
    mut net_state: ResMut<NetworkState>,
    mut local_hero: ResMut<LocalHeroEntity>,
    mut hero_registry: ResMut<HeroRegistry>,
) {
    // 清理上一次本地英雄实体，避免重连换 net id 后残留多个“我控制的英雄”
    if let Some(e) = local_hero.0.take() {
        commands.entity(e).despawn();
        hero_registry.0.retain(|_, &mut v| v != e);
    }

    let server_addr: SocketAddr = "127.0.0.1:5000".parse().expect("Invalid server address");
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind client socket");
    let socket = NativeSocket::new(socket).expect("Failed to create native socket");

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
    let transport =
        NetcodeClientTransport::new(current_time, authentication, socket)
            .expect("Failed to create client transport");

    commands.insert_resource(RenetClientResource(client));
    commands.insert_resource(ClientTransportResource(transport));
    *net_state = NetworkState::Connecting;
    info!("Connecting to server at {}", server_addr);
}

/// 断线后自动重连：仅在状态为 Disconnected 且冷却结束时重建连接。
/// 冷却避免断线后每帧疯狂重连——每次重连都会在服务端生成全新英雄实体，
/// 若不加冷却会把客户端实体刷爆并导致 GPU OOM。
fn try_reconnect(
    mut net_state: ResMut<NetworkState>,
    mut commands: Commands,
    mut timer: ResMut<ReconnectTimer>,
    time: Res<Time>,
    mut local_hero: ResMut<LocalHeroEntity>,
    mut hero_registry: ResMut<HeroRegistry>,
) {
    timer.0 -= time.delta_secs();
    if *net_state == NetworkState::Disconnected && timer.0 <= 0.0 {
        timer.0 = RECONNECT_INTERVAL;
        connect_to_server(commands, net_state, local_hero, hero_registry);
    }
}

fn poll_connection(
    client_res: Option<ResMut<RenetClientResource>>,
    transport_res: Option<ResMut<ClientTransportResource>>,
    mut net_state: ResMut<NetworkState>,
    mut clock: ResMut<ClientNetcodeClock>,
    mut timer: ResMut<ReconnectTimer>,
) {
    let (Some(mut client_res), Some(mut transport_res)) = (client_res, transport_res) else {
        return;
    };

    let now = Instant::now();
    let delta = match clock.last {
        Some(last) => now.duration_since(last),
        None => Duration::ZERO,
    };
    clock.last = Some(now);
    client_res.0.update(delta);
    if let Err(e) = transport_res.0.update(delta, &mut client_res.0) {
        warn!("Transport update error: {:?}", e);
    }
    // 每帧务必把待发数据包（握手 / ACK / 心跳）发往服务器。
    // 否则在进入 InGame 之前客户端完全不发包，服务器收不到 ACK 会超时断开，
    // 导致英雄无法移动、技能无法释放。
    if let Err(e) = transport_res.0.send_packets(&mut client_res.0) {
        warn!("send_packets error: {:?}", e);
    }
    // 仅在已建立连接（InGame/Connected）之后、传输层报告断开时才标记 Disconnected。
    // 避免握手阶段 is_connected()==false 把状态错误清零为 Disconnected，
    // 否则一旦抖动就会永久卡死、无法恢复。
    if !client_res.0.is_connected()
        && (*net_state == NetworkState::InGame || *net_state == NetworkState::Connected)
    {
        *net_state = NetworkState::Disconnected;
        // 进入 Disconnected 即重置重连冷却，使 try_reconnect 在等待 RECONNECT_INTERVAL 后重连一次
        timer.0 = RECONNECT_INTERVAL;
    }
}

/// 接收服务端消息：Welcome / WorldSnapshot / Event
fn receive_server_messages(
    mut commands: Commands,
    client_res: Option<ResMut<RenetClientResource>>,
    mut net_state: ResMut<NetworkState>,
    mut lobby_info: ResMut<ClientLobbyInfo>,
    mut hero_registry: ResMut<HeroRegistry>,
    mut local_hero: ResMut<LocalHeroEntity>,
    mut received_snapshot: ResMut<ReceivedSnapshot>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut client) = client_res else {
        return;
    };

    while let Some(bytes) = client.0.receive_message(DefaultChannel::ReliableOrdered) {
        let Ok(msg) = bitcode::decode::<ServerMessage>(bytes.as_ref()) else {
            continue;
        };
        match msg {
            ServerMessage::Welcome {
                your_net_id,
                assigned_team,
                ..
            } => {
                lobby_info.my_net_id = Some(your_net_id);
                lobby_info.assigned_team = Some(assigned_team);
                *net_state = NetworkState::Connected;
                info!("Connected! Your net id = {:?}, team = {:?}", your_net_id, assigned_team);
            }
            ServerMessage::WorldSnapshot(snap) => {
                received_snapshot.0 = snap.clone();
                for ev in &snap.events {
                    handle_game_event(
                        ev.clone(),
                        &mut commands,
                        &mut lobby_info,
                        &mut images,
                        &mut meshes,
                        &mut materials,
                        &mut hero_registry,
                        &mut local_hero,
                    );
                }
                // 只要已拿到自己的 net_id 且收到快照，即视为进入对局，
                // 从任意非 InGame 状态（含握手后的 Connected 或被抖动误判的 Disconnected）都能回到 InGame。
                if lobby_info.my_net_id.is_some() && *net_state != NetworkState::InGame {
                    *net_state = NetworkState::InGame;
                }
            }
            ServerMessage::Event(event) => {
                handle_game_event(
                    event,
                    &mut commands,
                    &mut lobby_info,
                    &mut images,
                    &mut meshes,
                    &mut materials,
                    &mut hero_registry,
                    &mut local_hero,
                );
            }
            ServerMessage::MatchStateChange { state } => {
                *net_state = match state {
                    MatchState::Lobby => NetworkState::Connecting,
                    MatchState::Loading => NetworkState::Connecting,
                    MatchState::InProgress => NetworkState::InGame,
                    MatchState::Paused => NetworkState::InGame,
                    MatchState::Finished => NetworkState::Disconnected,
                };
            }
            ServerMessage::Error { message, .. } => {
                warn!("Server error: {}", message);
            }
        }
    }
}

/// 处理单个游戏事件（英雄生成 / 阵亡 / 离场 / 比赛开始 / 结束）
fn handle_game_event(
    event: GameEvent,
    commands: &mut Commands,
    lobby_info: &mut ClientLobbyInfo,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    hero_registry: &mut HeroRegistry,
    local_hero: &mut LocalHeroEntity,
) {
    match event {
        GameEvent::HeroSpawn {
            net_id,
            team,
            hero_name,
            position,
        } => {
            // 去重：同一 NetId 只生成一个实体，避免重复生成导致实体爆炸（GPU OOM）
            if hero_registry.0.contains_key(&net_id) {
                return;
            }
            let is_local = lobby_info.my_net_id == Some(net_id);
            let base_color = match team {
                Team::Blue => Color::srgb(0.2, 0.4, 1.0),
                Team::Red => Color::srgb(1.0, 0.3, 0.2),
                Team::Neutral => Color::srgb(0.7, 0.7, 0.7),
            };
            // 程序化生成英雄贴图（队伍色圆盘）
            let texture = make_hero_texture(images, base_color);
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                base_color: base_color,
                perceptual_roughness: 0.6,
                ..default()
            });
            let pos = position.to_vec3();
            let mut ec = commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(1.0, 3.0))),
                MeshMaterial3d(material),
                Transform::from_translation(pos + Vec3::Y * 1.5),
                net_id,
                team,
                Hero {
                    name: hero_name.clone(),
                    move_speed: game_core::components::HERO_BASE_MOVE_SPEED,
                    ..default()
                },
                TransformSync::default(),
                Selectable {
                    radius: 1.5,
                    height: 3.0,
                },
            ));
            // 本地英雄走客户端预测，远端英雄走插值
            if is_local {
                ec.insert((LocalPlayer, Predicted));
            } else {
                ec.insert(Interpolated);
            }
            let entity_id = ec.id();
            hero_registry.0.insert(net_id, entity_id);
            // 若之前已有本地英雄（重连换 net id），清理旧的，保证只有一个“我控制的英雄”
            if is_local {
                if let Some(old) = local_hero.0 {
                    if old != entity_id {
                        commands.entity(old).despawn();
                        hero_registry.0.retain(|_, &mut v| v != old);
                    }
                }
                local_hero.0 = Some(entity_id);
            }
            info!(
                "Hero spawned: {} ({:?}) local={}",
                hero_name, net_id, is_local
            );
        }
        GameEvent::HeroDespawn { net_id } => {
            // 服务端在英雄离场（断线 / 阵亡且非假人）时通知其它客户端移除实体
            if let Some(e) = hero_registry.0.remove(&net_id) {
                commands.entity(e).despawn();
                if local_hero.0 == Some(e) {
                    local_hero.0 = None;
                }
            }
        }
        GameEvent::HeroDeath {
            victim,
            killer,
            assist,
        } => {
            info!(
                "Hero died: {:?} killed by {:?} (assist {:?})",
                victim,
                killer,
                assist
            );
        }
        GameEvent::MatchStart {
            blue_heroes,
            red_heroes,
        } => {
            lobby_info.blue_heroes = blue_heroes;
            lobby_info.red_heroes = red_heroes;
            info!("Match started!");
        }
        GameEvent::MatchEnd { winning_team } => {
            info!("Match ended! Winner: {:?}", winning_team);
        }
        _ => {}
    }
}

/// 发送本地输入到服务器
fn send_input(
    client_res: Option<ResMut<RenetClientResource>>,
    transport_res: Option<ResMut<ClientTransportResource>>,
    input: Res<crate::input::PlayerInputState>,
) {
    let (Some(mut client_res), Some(mut transport_res)) = (client_res, transport_res) else {
        return;
    };

    // 只要传输层已连接就持续上报输入，不依赖 net_state 的 InGame。
    // 否则状态一旦短暂抖动（is_connected() 误判）就会停止发送，导致英雄无法移动、技能失效。
    if !client_res.0.is_connected() {
        return;
    }

    let cmd = InputCmd {
        tick: 0,
        move_dir: input.move_dir.into(),
        aim_dir: input.aim_dir.into(),
        ability_mask: input.ability_mask,
        ping: 0,
    };

    let bytes = bitcode::encode(&ClientMessage::Input(cmd));
    client_res.0.send_message(DefaultChannel::Unreliable, bytes);

    if let Err(e) = transport_res.0.send_packets(&mut client_res.0) {
        warn!("send_packets error: {:?}", e);
    }
}

/// 状态机转换（供 lib.rs 的 ClientPlugin 使用；主入口未启用时亦不影响编译）
pub fn handle_state_transitions(
    lobby_info: Res<ClientLobbyInfo>,
    mut net_state: ResMut<NetworkState>,
) {
    if *net_state == NetworkState::Connecting && lobby_info.my_net_id.is_some() {
        *net_state = NetworkState::Connected;
    }
}
