//! 服务端网络模块（M1-b / M1-d）
//!
//! - 会话认证与双版本协商（`FR-57` / `FR-62`）
//! - 服务端权威视野裁剪，序列化前执行（`FR-60` / `CR-05` / VIS-1~9）
//! - 增量快照 + 通道分级（`FR-52` / `FR-53` / `DEV-06`）
//! - 状态恢复式重连（`FR-55` / `DEV-08`）
//! - 输入序号/校验和校验与真实 RTT 回显（`FR-56` / `FR-61`）
//!
//! 传输层仍使用 netcode 的 `Unsecure` 握手；应用层令牌校验在此实现。
//! connect token / DTLS 属 M3 外部依赖（见 `GX-PRD-2026-001` §11.1）。

use bevy::prelude::*;
use game_core::components::*;
use game_core::gameplay::clock::{MatchClock, MatchMode, MatchPhase};
use game_core::gameplay::{combat, registry, shop};
use game_core::net::{
    BASELINE_INTERVAL_TICKS, ClientMessage, ConfigHash, EntityDelta, EntitySnapshot, EntityType,
    EventEnvelope, GameEvent, InputCmd, PROTOCOL_VERSION, Quatf, RejectReason,
    SNAPSHOT_INTERVAL_TICKS, ServerMessage, SessionToken, Snapshot, SnapshotHeader, Vec3f,
    WorldSnapshot, negotiate,
};
use game_core::visibility::{compute_visibility, event_visible, vision_sources};
use gx_config::ConfigSet;
use gx_net::VisibilityPolicy;
use renet2::{ClientId, ConnectionConfig, DefaultChannel, RenetServer};
use renet2_netcode::{
    NativeSocket, NetcodeServerTransport, ServerAuthentication, ServerSetupConfig,
};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 待处理的客户端输入（键为英雄的 net id）
#[derive(Resource, Default)]
pub struct PendingInputs {
    pub inputs: HashMap<u64, InputCmd>,
}

/// renet 连接 id -> 该玩家在服务端分配的 (NetId, Team)。**仅已认证会话**才有条目。
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
        let port = std::env::var("GX_SERVER_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_SERVER_PORT);
        ServerPort(port)
    }
}

/// 对外公布的地址（DN-08：不再硬编码）
#[derive(Resource)]
pub struct ServerHost(pub String);

impl Default for ServerHost {
    fn default() -> Self {
        ServerHost(std::env::var("GX_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".into()))
    }
}

/// 比赛是否已开始（用于只下发一次 MatchStart）
#[derive(Resource, Default)]
pub struct MatchStarted(pub bool);

/// 服务端累积的待广播游戏事件，每个广播周期清空
#[derive(Resource, Default)]
pub struct ServerEvents(pub Vec<GameEvent>);

/// 训练假人的生成信息，用于把假人的 HeroSpawn 事件下发给每个（含后续加入）客户端
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

/// 本进程绑定的 ConfigSet。一局内不变（`DN-25`）。
#[derive(Resource)]
pub struct ServerConfig(pub ConfigSet);

/// 会话令牌 -> 身份。断线后保留，供状态恢复式重连。
#[derive(Resource, Default)]
pub struct SessionRegistry(pub HashMap<SessionToken, (NetId, Team)>);

/// 连接 -> 会话令牌
#[derive(Resource, Default)]
pub struct ClientSessions(pub HashMap<ClientId, SessionToken>);

/// 每个连接的增量快照状态。
#[derive(Clone, Default)]
pub struct ClientView {
    pub last_sent: HashMap<u64, EntityDelta>,
    pub last_baseline_tick: u64,
    pub view_seq: u32,
}

#[derive(Resource, Default)]
pub struct ClientViews(pub HashMap<ClientId, ClientView>);

/// 每个英雄已接受的最大输入序号（去重 / 迟到丢弃，`FR-61`）
#[derive(Resource, Default)]
pub struct InputSeqs(pub HashMap<u64, u32>);

/// 单调递增的关键事件 id（`FR-53`）
#[derive(Resource, Default)]
pub struct EventCursor(pub u64);

/// 每个英雄最近一次上行时间戳，用于 RTT 回显（`FR-56`）
#[derive(Resource, Default)]
pub struct ClientTimestamps(pub HashMap<u64, u64>);

/// 用真实墙钟驱动 netcode 时钟（避免忙循环下时钟飞奔）。
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
            .init_resource::<ServerHost>()
            .init_resource::<NetcodeClock>()
            .init_resource::<ServerEvents>()
            .init_resource::<DummyRegistry>()
            .init_resource::<SessionRegistry>()
            .init_resource::<ClientSessions>()
            .init_resource::<ClientViews>()
            .init_resource::<InputSeqs>()
            .init_resource::<EventCursor>()
            .init_resource::<ClientTimestamps>()
            .init_resource::<crate::gameplay::SellIntents>()
            .insert_resource(ServerConfig(
                ConfigSet::load_default_with_gameplay(
                    game_core::gameplay::registry::content_digest(),
                )
                .expect("placeholder ConfigSet must validate"),
            ))
            .add_systems(Startup, start_server)
            .add_systems(
                Update,
                (
                    poll_connections,
                    handle_session_messages,
                    receive_inputs,
                    handle_gameplay_messages,
                    broadcast_snapshot,
                    // 全局动态障碍视图必须在同一 Update 里跟在快照之后下发
                    broadcast_nav_grid,
                )
                    .chain(),
            );
    }
}

/// 启动服务器：创建 UDP socket 并监听配置端口
pub fn start_server(mut commands: Commands, port: Res<ServerPort>, host: Res<ServerHost>) {
    let server_addr: SocketAddr = format!("0.0.0.0:{}", port.0)
        .parse()
        .expect("Invalid server address");
    let socket = match UdpSocket::bind(server_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "无法绑定 {}：{e}。\n端口可能已被另一个服务端实例占用，请先关闭/结束旧的 gx-battle 进程再启动。",
                server_addr
            );
            std::process::exit(1);
        }
    };
    let socket = NativeSocket::new(socket).expect("Failed to create native socket");

    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let published: SocketAddr = format!("{}:{}", host.0, port.0)
        .parse()
        .expect("Invalid published server address");
    let server_config = ServerSetupConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_VERSION as u64,
        socket_addresses: vec![vec![published]],
        authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket)
        .expect("Failed to create server transport");
    let server = RenetServer::new(ConnectionConfig::from_shared_channels(
        DefaultChannel::config(),
    ));

    commands.insert_resource(RenetServerResource(server));
    commands.insert_resource(ServerTransportResource(transport));
    info!("Server listening on {}", server_addr);
}

fn send_reliable(server: &mut RenetServer, client_id: ClientId, msg: &ServerMessage) {
    server.send_message(
        client_id,
        DefaultChannel::ReliableOrdered,
        bitcode::encode(msg),
    );
}

/// 处理连接层事件；断线时**保留英雄实体与会话令牌**，供重连恢复（`DEV-08`）。
fn poll_connections(
    server_res: Option<ResMut<RenetServerResource>>,
    transport_res: Option<ResMut<ServerTransportResource>>,
    mut client_net_ids: ResMut<ClientNetIds>,
    mut client_sessions: ResMut<ClientSessions>,
    mut views: ResMut<ClientViews>,
    mut net_state: ResMut<ServerNetworkState>,
    mut clock: ResMut<NetcodeClock>,
) {
    let (Some(mut server_res), Some(mut transport_res)) = (server_res, transport_res) else {
        return;
    };

    let now = Instant::now();
    let delta = now.duration_since(clock.last);
    clock.last = now;
    server_res.0.update(delta);
    let _ = transport_res.0.update(delta, &mut server_res.0);

    let live: HashSet<ClientId> = server_res.0.clients_id().into_iter().collect();
    let disconnected: Vec<ClientId> = server_res.0.disconnections_id().into_iter().collect();

    let stale: Vec<ClientId> = client_net_ids
        .0
        .keys()
        .copied()
        .filter(|id| !live.contains(id))
        .collect();
    for client_id in disconnected.iter().copied().chain(stale) {
        if client_net_ids.0.remove(&client_id).is_some() {
            // 不 despawn：恢复重连时复用原 NetId / 英雄实体
            client_sessions.0.remove(&client_id);
            views.0.remove(&client_id);
            info!(
                "Client {:?} disconnected; session kept for resume",
                client_id
            );
        }
    }

    transport_res.0.send_packets(&mut server_res.0);

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

/// 会话建立 / 恢复：校验协议版本与内容 hash，无令牌连接不分配身份（`FR-62`）。
/// 大厅模式选择的唯一写入口。只在 Warmup（开局前）接受，局中改模式会破坏
/// 人数门槛与 AI 生成，因此一律忽略；未知 `mode` 由 `from_code` 回退 PVE。
fn apply_match_mode(clock: &mut Option<ResMut<MatchClock>>, mode: u8) {
    let Some(c) = clock.as_mut() else {
        return;
    };
    if c.phase != MatchPhase::Warmup {
        return;
    }
    let next = MatchMode::from_code(mode);
    if c.mode != next {
        c.mode = next;
        info!("[lobby] match mode -> {next:?}");
    }
}

fn handle_session_messages(
    mut commands: Commands,
    server_res: Option<ResMut<RenetServerResource>>,
    mut clock: Option<ResMut<MatchClock>>,
    config: Res<ServerConfig>,
    mut client_net_ids: ResMut<ClientNetIds>,
    mut next_net_id: ResMut<NextNetId>,
    mut sessions: ResMut<SessionRegistry>,
    mut client_sessions: ResMut<ClientSessions>,
    mut views: ResMut<ClientViews>,
    dummy_registry: Res<DummyRegistry>,
    heroes: Query<(&NetId, &Team, &Hero, &TransformSync, &HeroAbilities)>,
) {
    let Some(mut server) = server_res else { return };
    let expected_hash: ConfigHash = config.0.content_hash;

    for client_id in server.0.clients_id() {
        if client_net_ids.0.contains_key(&client_id) {
            continue;
        }
        let mut messages = Vec::new();
        while let Some(bytes) = server
            .0
            .receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            if let Ok(msg) = bitcode::decode::<ClientMessage>(bytes.as_ref()) {
                messages.push(msg);
            }
        }
        for msg in messages {
            match msg {
                // 与 Hello 同批抵达的模式选择：会话系统会 drain 掉全部 ReliableOrdered
                // 消息，未认证客户端的 SelectMode 必须在这里处理，否则会被 `_ => {}` 吞掉。
                ClientMessage::SelectMode { mode } => apply_match_mode(&mut clock, mode),
                ClientMessage::Hello(session) => {
                    if let Err(reason) = negotiate(&session.hello, expected_hash) {
                        send_reliable(&mut server.0, client_id, &ServerMessage::Reject { reason });
                        continue;
                    }
                    let token = session.token;
                    if let Some(&identity) = sessions.0.get(&token) {
                        // 重复 Hello：恢复已有身份，不生成新英雄
                        resume_identity(
                            &mut server.0,
                            client_id,
                            token,
                            identity,
                            &mut client_net_ids,
                            &mut client_sessions,
                            &mut views,
                            &heroes,
                            &config,
                            &dummy_registry,
                        );
                        continue;
                    }
                    let blue_count = sessions
                        .0
                        .values()
                        .filter(|(_, t)| *t == Team::Blue)
                        .count();
                    let red_count = sessions.0.values().filter(|(_, t)| *t == Team::Red).count();
                    let team = if blue_count <= red_count {
                        Team::Blue
                    } else {
                        Team::Red
                    };
                    let net_id = NetId(next_net_id.0);
                    next_net_id.0 += 1;
                    sessions.0.insert(token, (net_id, team));
                    client_net_ids.0.insert(client_id, (net_id, team));
                    client_sessions.0.insert(client_id, token);
                    views.0.remove(&client_id);

                    // 从对应阵营的基地出生，并略微错开避免同队英雄叠在一起
                    let spawn_pos = match team {
                        Team::Blue => {
                            game_core::map::BLUE_BASE + bevy::math::Vec3::new(4.0, 0.0, 4.0)
                        }
                        Team::Red => {
                            game_core::map::RED_BASE + bevy::math::Vec3::new(-4.0, 0.0, -4.0)
                        }
                        Team::Neutral => bevy::math::Vec3::ZERO,
                    };
                    commands.spawn((
                        net_id,
                        team,
                        Transform::from_translation(spawn_pos),
                        Hero {
                            name: "Ashwarden".to_string(),
                            move_speed: HERO_BASE_MOVE_SPEED,
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
                        Collider { radius: 1.3 },
                        LocalPlayer,
                    ));

                    send_reliable(
                        &mut server.0,
                        client_id,
                        &ServerMessage::Welcome {
                            your_net_id: net_id,
                            assigned_team: team,
                            match_id: 0,
                            server_tick: 0,
                            session_token: token,
                        },
                    );
                    offer_config(&mut server.0, client_id, &config);
                    send_reliable(
                        &mut server.0,
                        client_id,
                        &ServerMessage::Event(GameEvent::HeroSpawn {
                            net_id,
                            team,
                            hero_name: "Ashwarden".to_string(),
                            position: Vec3f::from_vec3(spawn_pos),
                            // 同一阵营内也按 net_id 分配不同外观，避免只能靠血条辨认
                            skin: match team {
                                Team::Blue => (net_id.0 % 6) as u8,
                                Team::Red => 2 + (net_id.0 % 4) as u8,
                                Team::Neutral => 9,
                            },
                        }),
                    );
                    for dummy_event in &dummy_registry.0 {
                        send_reliable(
                            &mut server.0,
                            client_id,
                            &ServerMessage::Event(dummy_event.clone()),
                        );
                    }
                    info!(
                        "Session established for {:?} -> net {:?} ({:?})",
                        client_id, net_id, team
                    );
                }
                ClientMessage::ReconnectRequest { token, hello } => {
                    if let Err(reason) = negotiate(&hello, expected_hash) {
                        send_reliable(&mut server.0, client_id, &ServerMessage::Reject { reason });
                        continue;
                    }
                    let Some(&identity) = sessions.0.get(&token) else {
                        send_reliable(
                            &mut server.0,
                            client_id,
                            &ServerMessage::Reject {
                                reason: RejectReason::InvalidSessionToken,
                            },
                        );
                        continue;
                    };
                    resume_identity(
                        &mut server.0,
                        client_id,
                        token,
                        identity,
                        &mut client_net_ids,
                        &mut client_sessions,
                        &mut views,
                        &heroes,
                        &config,
                        &dummy_registry,
                    );
                }
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resume_identity(
    server: &mut RenetServer,
    client_id: ClientId,
    token: SessionToken,
    identity: (NetId, Team),
    client_net_ids: &mut ClientNetIds,
    client_sessions: &mut ClientSessions,
    views: &mut ClientViews,
    heroes: &Query<(&NetId, &Team, &Hero, &TransformSync, &HeroAbilities)>,
    config: &ServerConfig,
    dummy_registry: &DummyRegistry,
) {
    let (net_id, team) = identity;
    client_net_ids.0.insert(client_id, identity);
    client_sessions.0.insert(client_id, token);
    views.0.remove(&client_id);

    let entities: Vec<EntitySnapshot> = heroes
        .iter()
        .map(|(id, t, hero, ts, abilities)| EntitySnapshot {
            net_id: *id,
            entity_type: EntityType::Hero,
            team: *t,
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
            status_mask: 0,
            shield: 0.0,
        })
        .collect();

    send_reliable(
        server,
        client_id,
        &ServerMessage::SessionResume {
            net_id,
            team,
            snapshot: WorldSnapshot {
                tick: 0,
                server_time: 0.0,
                entities,
                events: Vec::new(),
            },
        },
    );
    offer_config(server, client_id, config);
    for dummy_event in &dummy_registry.0 {
        send_reliable(
            server,
            client_id,
            &ServerMessage::Event(dummy_event.clone()),
        );
    }
    info!("Session resumed for {:?} -> net {:?}", client_id, net_id);
}

fn offer_config(server: &mut RenetServer, client_id: ClientId, config: &ServerConfig) {
    send_reliable(
        server,
        client_id,
        &ServerMessage::ConfigSetOffer {
            version: config.0.version.clone(),
            config_hash: config.0.content_hash,
        },
    );
}

/// 接收客户端输入：仅已认证会话；校验序号、校验和与内容 hash（`FR-61` / `FR-57`）。
fn receive_inputs(
    server_res: Option<ResMut<RenetServerResource>>,
    mut pending: ResMut<PendingInputs>,
    mut input_seqs: ResMut<InputSeqs>,
    mut timestamps: ResMut<ClientTimestamps>,
    client_net_ids: Res<ClientNetIds>,
    config: Res<ServerConfig>,
) {
    let Some(mut server) = server_res else { return };

    for client_id in server.0.clients_id() {
        let Some(&(net_id, _)) = client_net_ids.0.get(&client_id) else {
            continue;
        };
        while let Some(bytes) = server
            .0
            .receive_message(client_id, DefaultChannel::Unreliable)
        {
            let Ok(ClientMessage::Input(cmd)) = bitcode::decode::<ClientMessage>(bytes.as_ref())
            else {
                continue;
            };
            if !cmd.checksum_valid() || cmd.config_hash != config.0.content_hash {
                continue;
            }
            let last = input_seqs.0.entry(net_id.0).or_insert(0);
            if cmd.seq <= *last {
                continue;
            }
            *last = cmd.seq;
            timestamps.0.insert(net_id.0, cmd.client_time_us);
            pending.inputs.insert(net_id.0, cmd);
        }
    }
}

/// 已认证客户端的玩法消息（购买 / 售出）。
///
/// `handle_session_messages` 只处理**未认证**连接，`receive_inputs` 只解 Unreliable
/// 通道上的 `Input`；此前 `PurchaseItem` 落在两处 `_ => {}` 里被静默丢弃。
/// 本系统补上 ReliableOrdered 通道的玩法 opcode，并把协议 `u32` 反查为玩法 `&str` id。
fn handle_gameplay_messages(
    server_res: Option<ResMut<RenetServerResource>>,
    client_net_ids: Res<ClientNetIds>,
    mut intents: ResMut<crate::gameplay::ShopIntents>,
    mut sells: ResMut<crate::gameplay::SellIntents>,
    tick: Res<crate::simulation::ServerTick>,
    mut clock: Option<ResMut<MatchClock>>,
) {
    let Some(mut server) = server_res else {
        return;
    };
    for (client_id, (net_id, _team)) in client_net_ids.0.iter() {
        while let Some(bytes) = server
            .0
            .receive_message(*client_id, DefaultChannel::ReliableOrdered)
        {
            let Ok(msg) = bitcode::decode::<ClientMessage>(bytes.as_ref()) else {
                continue;
            };
            match msg {
                // 已在认证之后（或重连后）单独抵达的模式选择。
                ClientMessage::SelectMode { mode } => apply_match_mode(&mut clock, mode),
                ClientMessage::PurchaseItem { item_id } => {
                    let Some(name) = registry::item_name(item_id) else {
                        continue;
                    };
                    intents.0.push((
                        net_id.0,
                        shop::ShopRequest {
                            item_id: name,
                            // 距离/阵亡/tick 一律由服务端重算，绝不采信客户端
                            distance_to_source: 0.0,
                            is_dead: false,
                            death_tick: 0,
                            current_tick: tick.0,
                        },
                    ));
                }
                ClientMessage::SellItem { slot } => {
                    sells.0.push((net_id.0, slot));
                }
                _ => {}
            }
        }
    }
}

/// 协议 v3：英雄快照携带 `CH_STATUS` / `CH_SHIELD`，并把哨塔与兵线一并下发，
/// 使客户端能显示状态/护盾以及可交互的世界实体。
#[allow(clippy::type_complexity)]
fn collect_entities(
    heroes: &Query<(
        &NetId,
        &Team,
        &Hero,
        &TransformSync,
        &HeroAbilities,
        Option<&combat::StatusEffects>,
        Option<&combat::ShieldPool>,
    )>,
    projectiles: &Query<(&NetId, &Team, &TransformSync, &Projectile)>,
    turrets: &Query<(&NetId, &Team, &TransformSync, &Turret)>,
    // 兵线与野怪共用一条可选查询，避免再给 broadcast_snapshot 增加一个系统参数。
    // 野怪此前完全没进快照，客户端 `sync_monsters` 永远收不到实体 → 看不到、点不中、打不了。
    #[allow(clippy::type_complexity)] minions: &Query<
        (
            &NetId,
            &Team,
            &TransformSync,
            Option<&Minion>,
            Option<&JungleMonster>,
        ),
        Or<(With<Minion>, With<JungleMonster>)>,
    >,
) -> Vec<EntitySnapshot> {
    let mut entities: Vec<EntitySnapshot> = heroes
        .iter()
        .map(
            |(net_id, team, hero, ts, abilities, status, shield)| EntitySnapshot {
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
                status_mask: status.map(|s| s.status_mask()).unwrap_or(0),
                shield: shield.map(|s| s.value).unwrap_or(0.0),
            },
        )
        .collect();

    for (net_id, team, ts, tur) in turrets.iter() {
        entities.push(EntitySnapshot {
            net_id: *net_id,
            entity_type: EntityType::Turret,
            team: *team,
            position: Vec3f::from_vec3(ts.position),
            rotation: Quatf::default(),
            velocity: Vec3f::zero(),
            health: tur.health,
            max_health: tur.max_health,
            energy: 0.0,
            max_energy: 0.0,
            level: 0,
            gold: 0,
            ability_cooldowns: [0.0; 4],
            status_mask: 0,
            shield: 0.0,
        });
    }

    for (net_id, team, ts, minion, monster) in minions.iter() {
        let Some((health, max_health, entity_type)) = minion
            .map(|m| (m.health, m.max_health, EntityType::Minion))
            .or_else(|| monster.map(|m| (m.health, m.max_health, EntityType::JungleMonster)))
        else {
            continue;
        };
        entities.push(EntitySnapshot {
            net_id: *net_id,
            entity_type,
            team: *team,
            position: Vec3f::from_vec3(ts.position),
            rotation: Quatf::default(),
            velocity: Vec3f::zero(),
            health,
            max_health,
            energy: 0.0,
            max_energy: 0.0,
            level: 0,
            gold: 0,
            ability_cooldowns: [0.0; 4],
            status_mask: 0,
            shield: 0.0,
        });
    }

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
            status_mask: 0,
            shield: 0.0,
        });
    }
    entities
}

/// `NAV-M2-01-C8`：把权威**全局**动态障碍视图 + 网格版本下发给所有客户端。
///
/// - 变化立即发；无变化每 30 tick 补发一次（覆盖新加入 / 断线重连的客户端）；
/// - 走 ReliableOrdered 且**不经逐玩家视野裁剪**：视野外的哨塔同样必须进 A* walls，
///   否则客户端只能从（被裁剪过的）渲染实体推断障碍，正是 C1 禁止的做法；
/// - 版本由服务端唯一裁决，客户端不上报任何网格数据。
#[allow(clippy::too_many_arguments)]
fn broadcast_nav_grid(
    nav: Option<Res<game_core::navgrid::NavGrid>>,
    server_res: Option<ResMut<RenetServerResource>>,
    tick: Option<Res<crate::simulation::ServerTick>>,
    clients: Option<Res<ClientNetIds>>,
    mut last_version: Local<u32>,
) {
    let (Some(nav), Some(mut server), Some(tick), Some(clients)) = (nav, server_res, tick, clients)
    else {
        return;
    };
    let due = tick
        .0
        .is_multiple_of(game_core::navgrid::NAV_REBUILD_INTERVAL_TICKS);
    if nav.version == *last_version && !due {
        return;
    }
    *last_version = nav.version;
    let msg = ServerMessage::NavGridUpdate {
        snapshot: nav.obstacle_snapshot(tick.0),
        version: nav.version,
    };
    let bytes = bitcode::encode(&msg);
    for client_id in clients.0.keys() {
        server
            .0
            .send_message(*client_id, DefaultChannel::ReliableOrdered, bytes.clone());
    }
}

/// 视野裁剪 + 增量编码 + 通道分级；状态走 Unreliable，关键事件走 ReliableOrdered 且幂等。
#[allow(clippy::too_many_arguments)]
fn broadcast_snapshot(
    server_res: Option<ResMut<RenetServerResource>>,
    transport_res: Option<ResMut<ServerTransportResource>>,
    #[allow(clippy::type_complexity)] heroes: Query<(
        &NetId,
        &Team,
        &Hero,
        &TransformSync,
        &HeroAbilities,
        Option<&combat::StatusEffects>,
        Option<&combat::ShieldPool>,
    )>,
    projectiles: Query<(&NetId, &Team, &TransformSync, &Projectile)>,
    turrets: Query<(&NetId, &Team, &TransformSync, &Turret)>,
    #[allow(clippy::type_complexity)] minions: Query<
        (
            &NetId,
            &Team,
            &TransformSync,
            Option<&Minion>,
            Option<&JungleMonster>,
        ),
        Or<(With<Minion>, With<JungleMonster>)>,
    >,
    mut server_events: ResMut<ServerEvents>,
    tick: Res<crate::simulation::ServerTick>,
    config: Res<ServerConfig>,
    client_net_ids: Res<ClientNetIds>,
    mut views: ResMut<ClientViews>,
    mut cursor: ResMut<EventCursor>,
    timestamps: Res<ClientTimestamps>,
    mut match_started: ResMut<MatchStarted>,
) {
    let (Some(mut server), Some(mut transport)) = (server_res, transport_res) else {
        return;
    };

    let entities = collect_entities(&heroes, &projectiles, &turrets, &minions);
    let vision = &config.0.tables.vision;
    let policy = VisibilityPolicy {
        vision_radius: vision.vision_radius,
        brush_reveal_radius: vision.brush_reveal_radius,
        constructs_provide_vision: vision.constructs_provide_vision,
        sentries_provide_vision: vision.sentries_provide_vision,
    };

    // 每个玩家独立可见集（VIS-1）
    let mut per_client: Vec<(ClientId, NetId, Team, gx_net::VisibilitySet)> = Vec::new();
    for (client_id, (net_id, team)) in client_net_ids.0.iter() {
        let set = compute_visibility(*team, &policy, &entities);
        per_client.push((*client_id, *net_id, *team, set));
    }

    // 关键事件：过滤后带 event_id 幂等下发（FR-53 / VIS-6/7）
    let events = std::mem::take(&mut server_events.0);
    for ev in &events {
        cursor.0 += 1;
        let envelope = EventEnvelope {
            event_id: cursor.0,
            payload: bitcode::encode(ev),
        };
        for (client_id, _, _, set) in &per_client {
            if event_visible(ev, set) {
                send_reliable(
                    &mut server.0,
                    *client_id,
                    &ServerMessage::ReliableEvent(envelope.clone()),
                );
            }
        }
    }

    // 状态快照：20Hz 增量；每 2s 或首次/重连补基线
    if tick.0.is_multiple_of(SNAPSHOT_INTERVAL_TICKS) || !events.is_empty() {
        for (client_id, net_id, team, set) in &per_client {
            let visible_entities: Vec<EntitySnapshot> = entities
                .iter()
                .filter(|e| set.contains(e.net_id.0))
                .cloned()
                .collect();
            let visible_ids: HashSet<u64> = visible_entities.iter().map(|e| e.net_id.0).collect();
            let full: Vec<EntityDelta> = visible_entities.iter().map(|e| e.to_delta()).collect();

            let view = views.0.entry(*client_id).or_default();
            let need_baseline = view.last_sent.is_empty()
                || tick.0.saturating_sub(view.last_baseline_tick) >= BASELINE_INTERVAL_TICKS;

            let removed: Vec<u64> = view
                .last_sent
                .keys()
                .filter(|id| !visible_ids.contains(id))
                .copied()
                .collect();
            let mut deltas = Vec::new();
            let mut baseline = None;
            if need_baseline {
                view.last_sent.clear();
                for d in &full {
                    view.last_sent.insert(d.net_id, d.clone());
                }
                view.last_baseline_tick = tick.0;
                baseline = Some(full);
            } else {
                for d in &full {
                    match view.last_sent.get(&d.net_id) {
                        Some(prev) => {
                            let diff = gx_net::diff_entity(prev, d);
                            if diff.changed_mask != 0 {
                                deltas.push(diff);
                            }
                        }
                        None => deltas.push(d.clone()),
                    }
                }
                for d in &full {
                    view.last_sent.insert(d.net_id, d.clone());
                }
                for id in &removed {
                    view.last_sent.remove(id);
                }
            }

            view.view_seq = view.view_seq.wrapping_add(1);
            let snapshot = Snapshot {
                header: SnapshotHeader {
                    tick: tick.0,
                    baseline_tick: view.last_baseline_tick,
                    config_hash: config.0.content_hash,
                    view_seq: view.view_seq,
                    client_time_us: *timestamps.0.get(&net_id.0).unwrap_or(&0),
                },
                deltas,
                removed,
                baseline,
            };
            server.0.send_message(
                *client_id,
                DefaultChannel::Unreliable,
                bitcode::encode(&ServerMessage::Snapshot(snapshot)),
            );

            // 战争迷雾：把**该玩家**的可见源与有效半径一起下发。
            // 与上面的实体裁剪共用 `vision_sources` / `policy`，遮罩不会和可见集打架；
            // 只在 20Hz 快照 tick 发送，事件触发的额外快照不重复发。
            if tick.0.is_multiple_of(SNAPSHOT_INTERVAL_TICKS) {
                let sources: Vec<[f32; 2]> = vision_sources(*team, &policy, &entities)
                    .into_iter()
                    .map(|p| [p[0], p[2]])
                    .collect();
                server.0.send_message(
                    *client_id,
                    DefaultChannel::Unreliable,
                    bitcode::encode(&ServerMessage::VisionUpdate {
                        sources,
                        vision_radius: policy.vision_radius,
                        brush_reveal_radius: policy.brush_reveal_radius,
                        server_tick: tick.0,
                    }),
                );
            }
        }
    }

    // 双方各至少 1 人时下发一次 MatchStart
    if !match_started.0 {
        let mut blue = Vec::new();
        let mut red = Vec::new();
        for (net_id, team, hero, _, _, _, _) in heroes.iter() {
            let entry = (*net_id, hero.name.clone());
            match team {
                Team::Blue => blue.push(entry),
                Team::Red => red.push(entry),
                Team::Neutral => {}
            }
        }
        if !blue.is_empty() && !red.is_empty() {
            let event = ServerMessage::Event(GameEvent::MatchStart {
                blue_heroes: blue,
                red_heroes: red,
            });
            for (client_id, _, _, _) in &per_client {
                send_reliable(&mut server.0, *client_id, &event);
            }
            match_started.0 = true;
            info!("Match started");
        }
    }

    transport.0.send_packets(&mut server.0);
}
