//! 客户端网络模块（M1-b / M1-d）
//!
//! 会话 Hello / 重连、双版本协商、增量快照重建、幂等关键事件、真实 RTT。

use crate::AppState;
use crate::rendering::{CastEffect, DamageFlash, HitFlash};
use crate::voxel;
use bevy::image::Image;
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use game_core::components::*;
use game_core::net::{
    ClientMessage, ConfigHash, EntityDelta, EntitySnapshot, EventDeduplicator, GameEvent, InputCmd,
    MatchState, PROTOCOL_VERSION, ServerMessage, SessionHello, SessionToken, VersionHello,
    WorldSnapshot, apply_delta,
};
use renet2::{ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 网络连接状态
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

/// 最近一次重建出的服务端世界快照，供预测/插值系统使用
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

/// 增量快照的完整实体状态缓存（客户端重建基线）
#[derive(Resource, Default)]
pub struct EntityStateCache(pub HashMap<u64, EntityDelta>);

/// 关键事件去重器（`FR-53`）
#[derive(Resource)]
pub struct EventDedup(pub EventDeduplicator);

impl Default for EventDedup {
    fn default() -> Self {
        Self(EventDeduplicator::new(256))
    }
}

/// 会话状态：令牌跨重连保持，RTT 由快照头回显计算
#[derive(Resource)]
pub struct SessionState {
    pub token: SessionToken,
    pub config_hash: ConfigHash,
    pub authenticated: bool,
    pub has_session: bool,
    pub was_connected: bool,
    pub seq: u32,
    pub rtt_ms: u32,
    pub dropped_inputs: u16,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            token: rand::random::<u64>()
                .to_le_bytes()
                .iter()
                .copied()
                .chain(rand::random::<u64>().to_le_bytes())
                .collect::<Vec<u8>>()
                .try_into()
                .unwrap_or([0u8; 16]),
            config_hash: gx_config::ConfigSet::load_default_with_gameplay(
                game_core::gameplay::registry::content_digest(),
            )
            .map(|c| c.content_hash)
            .unwrap_or([0u8; 32]),
            authenticated: false,
            has_session: false,
            was_connected: false,
            seq: 0,
            rtt_ms: 0,
            dropped_inputs: 0,
        }
    }
}

#[derive(Resource)]
pub struct RenetClientResource(pub RenetClient);

#[derive(Resource)]
pub struct ClientTransportResource(pub NetcodeClientTransport);

/// NetId -> 客户端实体 映射，用于英雄生成去重与断线清理
#[derive(Resource, Default)]
pub struct HeroRegistry(pub HashMap<NetId, Entity>);

/// 当前本地英雄实体
#[derive(Resource, Default)]
pub struct LocalHeroEntity(pub Option<Entity>);

/// 断线后自动重连的冷却计时（秒）
#[derive(Resource, Default)]
pub struct ReconnectTimer(pub f32);

/// 用真实墙钟驱动客户端 netcode 时钟
#[derive(Resource, Default)]
pub struct ClientNetcodeClock {
    last: Option<Instant>,
}

const RECONNECT_INTERVAL: f32 = 3.0;

fn server_address() -> SocketAddr {
    let host = std::env::var("GX_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("GX_SERVER_PORT").unwrap_or_else(|_| "5000".into());
    format!("{host}:{port}")
        .parse()
        .expect("Invalid server address")
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkState>()
            // 大厅选择 / 命令队列 / 回执 / 图鉴开关：UI 与网络共用，双插件幂等注册。
            .init_resource::<crate::lobby::LobbySelection>()
            .init_resource::<crate::lobby::ClientCommandQueue>()
            .init_resource::<crate::lobby::ShopToast>()
            .init_resource::<crate::lobby::CodexOpen>()
            .init_resource::<ClientLobbyInfo>()
            .init_resource::<ReceivedSnapshot>()
            .init_resource::<EntityStateCache>()
            .init_resource::<EventDedup>()
            .init_resource::<SessionState>()
            .init_resource::<HeroRegistry>()
            .init_resource::<LocalHeroEntity>()
            .init_resource::<ReconnectTimer>()
            .init_resource::<ClientNetcodeClock>()
            // 连接推迟到玩家在大厅点击「进入对局」后（OnEnter(InMatch)）才建立，
            // 避免一启动就直接对局。
            .add_systems(OnEnter(AppState::InMatch), connect_to_server)
            .add_systems(
                Update,
                (
                    poll_connection,
                    send_hello,
                    send_client_commands,
                    receive_server_messages,
                    send_input,
                    // 自动重连只在对局状态生效：大厅里绝不能偷偷连服务器开打
                    try_reconnect.run_if(in_state(AppState::InMatch)),
                )
                    .chain(),
            );
    }
}

/// 连接服务器。**令牌跨重连保持不变**（`DEV-08`）
pub fn connect_to_server(
    mut commands: Commands,
    mut net_state: ResMut<NetworkState>,
    mut local_hero: ResMut<LocalHeroEntity>,
    mut hero_registry: ResMut<HeroRegistry>,
    mut session: ResMut<SessionState>,
) {
    if let Some(e) = local_hero.0.take() {
        commands.entity(e).despawn();
        hero_registry.0.retain(|_, &mut v| v != e);
    }

    let addr = server_address();
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind client socket");
    let socket = NativeSocket::new(socket).expect("Failed to create native socket");
    let connection_config = ConnectionConfig::from_shared_channels(DefaultChannel::config());
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let authentication = ClientAuthentication::Unsecure {
        socket_id: 0,
        server_addr: addr,
        client_id: rand::random(),
        user_data: None,
        protocol_id: PROTOCOL_VERSION as u64,
    };

    let client = RenetClient::new(connection_config, false);
    let transport = NetcodeClientTransport::new(current_time, authentication, socket)
        .expect("Failed to create client transport");

    commands.insert_resource(RenetClientResource(client));
    commands.insert_resource(ClientTransportResource(transport));
    session.was_connected = false;
    session.authenticated = false;
    *net_state = NetworkState::Connecting;
    info!("Connecting to server at {}", addr);
}

/// 连接刚建立时发送 Hello / ReconnectRequest，携带双版本号（`FR-57` / `FR-62`）
fn send_hello(
    client_res: Option<ResMut<RenetClientResource>>,
    transport_res: Option<ResMut<ClientTransportResource>>,
    mut session: ResMut<SessionState>,
    selection: Res<crate::lobby::LobbySelection>,
) {
    let (Some(mut client), Some(mut transport)) = (client_res, transport_res) else {
        return;
    };
    let connected = client.0.is_connected();
    if connected && !session.was_connected {
        let hello = VersionHello {
            protocol_version: PROTOCOL_VERSION,
            config_hash: session.config_hash,
        };
        let msg = if session.has_session {
            ClientMessage::ReconnectRequest {
                token: session.token,
                hello,
            }
        } else {
            ClientMessage::Hello(SessionHello {
                token: session.token,
                hello,
            })
        };
        client
            .0
            .send_message(DefaultChannel::ReliableOrdered, bitcode::encode(&msg));
        // 同批携带大厅模式选择：服务端 `handle_session_messages` 会在同一帧 drain
        // 这两条 ReliableOrdered 消息，保证任何 `FixedUpdate` 的 PVE AI 生成
        // （`ensure_pve_bot` 默认按 PVE 判断）之前就已确定模式。
        client.0.send_message(
            DefaultChannel::ReliableOrdered,
            bitcode::encode(&ClientMessage::SelectMode {
                mode: selection.mode.code(),
            }),
        );
        if let Err(e) = transport.0.send_packets(&mut client.0) {
            warn!("send_packets error: {:?}", e);
        }
    }
    session.was_connected = connected;
}

/// 大厅 / 商店上行命令的唯一发送入口：UI 只入队，集中在这里编码并给出回执。
/// 连接尚未建立时队列保留，重连后自动补发当前大厅模式。
fn send_client_commands(
    client_res: Option<ResMut<RenetClientResource>>,
    transport_res: Option<ResMut<ClientTransportResource>>,
    selection: Res<crate::lobby::LobbySelection>,
    mut queue: ResMut<crate::lobby::ClientCommandQueue>,
    mut toast: ResMut<crate::lobby::ShopToast>,
    mut mode_sent: Local<bool>,
) {
    let (Some(mut client), Some(mut transport)) = (client_res, transport_res) else {
        *mode_sent = false;
        return;
    };
    if !client.0.is_connected() {
        *mode_sent = false;
        return;
    }
    let mut dirty = false;
    if !*mode_sent {
        *mode_sent = true;
        let msg = ClientMessage::SelectMode {
            mode: selection.mode.code(),
        };
        client
            .0
            .send_message(DefaultChannel::ReliableOrdered, bitcode::encode(&msg));
        dirty = true;
    }
    for cmd in std::mem::take(&mut queue.0) {
        let msg = match cmd {
            crate::lobby::ClientCommand::SelectMode(m) => {
                ClientMessage::SelectMode { mode: m.code() }
            }
            crate::lobby::ClientCommand::Purchase(item_id) => {
                let name = game_core::gameplay::registry::item_name(item_id).unwrap_or("?");
                let label = crate::lobby::item_profile(name)
                    .map(|p| p.name_zh)
                    .unwrap_or(name);
                toast.set(
                    format!("已提交购买请求：{label}（等待服务端裁决）"),
                    format!("Purchase sent: {name} (server decides)"),
                );
                ClientMessage::PurchaseItem { item_id }
            }
            crate::lobby::ClientCommand::Sell(slot) => {
                toast.set(
                    format!("已提交售出请求：栏位 {}", slot + 1),
                    format!("Sell sent: slot {}", slot + 1),
                );
                ClientMessage::SellItem { slot }
            }
        };
        client
            .0
            .send_message(DefaultChannel::ReliableOrdered, bitcode::encode(&msg));
        dirty = true;
    }
    if dirty && let Err(e) = transport.0.send_packets(&mut client.0) {
        warn!("send_packets error: {:?}", e);
    }
}

fn try_reconnect(
    net_state: ResMut<NetworkState>,
    commands: Commands,
    mut timer: ResMut<ReconnectTimer>,
    time: Res<Time>,
    local_hero: ResMut<LocalHeroEntity>,
    hero_registry: ResMut<HeroRegistry>,
    session: ResMut<SessionState>,
) {
    timer.0 -= time.delta_secs();
    if *net_state == NetworkState::Disconnected && timer.0 <= 0.0 {
        timer.0 = RECONNECT_INTERVAL;
        connect_to_server(commands, net_state, local_hero, hero_registry, session);
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
    if let Err(e) = transport_res.0.send_packets(&mut client_res.0) {
        warn!("send_packets error: {:?}", e);
    }
    if !client_res.0.is_connected()
        && (*net_state == NetworkState::InGame || *net_state == NetworkState::Connected)
    {
        *net_state = NetworkState::Disconnected;
        timer.0 = RECONNECT_INTERVAL;
    }
}

/// 接收服务端消息（可靠通道 + 不可靠通道的增量快照）
#[allow(clippy::too_many_arguments)]
fn receive_server_messages(
    mut commands: Commands,
    client_res: Option<ResMut<RenetClientResource>>,
    mut net_state: ResMut<NetworkState>,
    mut lobby_info: ResMut<ClientLobbyInfo>,
    mut hero_registry: ResMut<HeroRegistry>,
    mut local_hero: ResMut<LocalHeroEntity>,
    mut received_snapshot: ResMut<ReceivedSnapshot>,
    mut cache: ResMut<EntityStateCache>,
    mut dedup: ResMut<EventDedup>,
    mut session: ResMut<SessionState>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut combat_fx: Option<ResMut<crate::rendering::CombatFxQueue>>,
    mut event_fx: Option<ResMut<crate::rendering::EventFxQueue>>,
    mut pool: Option<ResMut<crate::visual::VoxelMaterialPool>>,
) {
    let Some(mut client) = client_res else { return };

    // 增量状态快照走 Unreliable 通道
    while let Some(bytes) = client.0.receive_message(DefaultChannel::Unreliable) {
        let Ok(ServerMessage::Snapshot(snap)) = bitcode::decode::<ServerMessage>(bytes.as_ref())
        else {
            continue;
        };
        if let Some(baseline) = snap.baseline {
            cache.0.clear();
            for d in baseline {
                cache.0.insert(d.net_id, d);
            }
        }
        for d in snap.deltas {
            if let Some(base) = cache.0.get_mut(&d.net_id) {
                apply_delta(base, &d);
            } else {
                cache.0.insert(d.net_id, d);
            }
        }
        for id in snap.removed {
            cache.0.remove(&id);
        }
        session.config_hash = snap.header.config_hash;
        let now = now_micros();
        if snap.header.client_time_us > 0 && now >= snap.header.client_time_us {
            session.rtt_ms =
                ((now - snap.header.client_time_us) / 1000).min(u32::MAX as u64) as u32;
        }
        let entities: Vec<EntitySnapshot> =
            cache.0.values().map(EntitySnapshot::from_delta).collect();
        received_snapshot.0 = WorldSnapshot {
            tick: snap.header.tick,
            server_time: 0.0,
            entities,
            events: Vec::new(),
        };
        if lobby_info.my_net_id.is_some() && *net_state != NetworkState::InGame {
            *net_state = NetworkState::InGame;
        }
    }

    // 可靠有序通道
    while let Some(bytes) = client.0.receive_message(DefaultChannel::ReliableOrdered) {
        let Ok(msg) = bitcode::decode::<ServerMessage>(bytes.as_ref()) else {
            continue;
        };
        match msg {
            ServerMessage::Welcome {
                your_net_id,
                assigned_team,
                session_token,
                ..
            } => {
                lobby_info.my_net_id = Some(your_net_id);
                lobby_info.assigned_team = Some(assigned_team);
                session.token = session_token;
                session.authenticated = true;
                session.has_session = true;
                *net_state = NetworkState::Connected;
                info!(
                    "Connected! net id = {:?}, team = {:?}",
                    your_net_id, assigned_team
                );
            }
            ServerMessage::SessionResume {
                net_id,
                team,
                snapshot,
            } => {
                lobby_info.my_net_id = Some(net_id);
                lobby_info.assigned_team = Some(team);
                session.authenticated = true;
                session.has_session = true;
                cache.0.clear();
                for e in &snapshot.entities {
                    cache.0.insert(e.net_id.0, e.to_delta());
                }
                received_snapshot.0 = snapshot;
                dedup.0.clear();
                *net_state = NetworkState::InGame;
                info!("Session resumed with net id {:?}", net_id);
            }
            ServerMessage::ConfigSetOffer { config_hash, .. } => {
                session.config_hash = config_hash;
            }
            ServerMessage::Reject { reason } => {
                warn!("Connection rejected by server: {:?}", reason);
                session.authenticated = false;
            }
            ServerMessage::ReliableEvent(envelope) => {
                if !dedup.0.accept(envelope.event_id) {
                    continue;
                }
                if let Ok(event) = bitcode::decode::<GameEvent>(&envelope.payload) {
                    handle_game_event(
                        event,
                        &mut commands,
                        &mut lobby_info,
                        &mut images,
                        &mut meshes,
                        &mut materials,
                        &mut hero_registry,
                        &mut local_hero,
                        combat_fx.as_deref_mut(),
                        event_fx.as_deref_mut(),
                        pool.as_deref_mut(),
                    );
                }
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
                        combat_fx.as_deref_mut(),
                        event_fx.as_deref_mut(),
                        pool.as_deref_mut(),
                    );
                }
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
                    combat_fx.as_deref_mut(),
                    event_fx.as_deref_mut(),
                    pool.as_deref_mut(),
                );
            }
            ServerMessage::MatchStateChange { state } => {
                *net_state = match state {
                    MatchState::Lobby | MatchState::Loading => NetworkState::Connecting,
                    MatchState::InProgress | MatchState::Paused => NetworkState::InGame,
                    MatchState::Finished => NetworkState::Disconnected,
                };
            }
            ServerMessage::Error { message, .. } => warn!("Server error: {}", message),
            // 增量快照已在 Unreliable 通道处理
            ServerMessage::Snapshot(_) => {}
        }
    }
}

/// 队伍语义色（统一调色板，不在事件分支里散落颜色常量）。
fn team_kind(team: game_core::components::Team) -> voxel::PixelKind {
    match team {
        game_core::components::Team::Blue => voxel::PixelKind::Blue,
        game_core::components::Team::Red => voxel::PixelKind::Red,
        game_core::components::Team::Neutral => voxel::PixelKind::Gold,
    }
}

/// 处理单个游戏事件
#[allow(clippy::too_many_arguments)]
fn handle_game_event(
    event: GameEvent,
    commands: &mut Commands,
    lobby_info: &mut ClientLobbyInfo,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    hero_registry: &mut HeroRegistry,
    local_hero: &mut LocalHeroEntity,
    combat_fx: Option<&mut crate::rendering::CombatFxQueue>,
    mut event_fx: Option<&mut crate::rendering::EventFxQueue>,
    mut pool: Option<&mut crate::visual::VoxelMaterialPool>,
) {
    use crate::visual::FxShape;

    // 事件视觉统一推入队列，由渲染层系统消费：集中做池化、确定性排布与数量裁剪。
    let mut push_fx = |kind: voxel::PixelKind,
                       shape: crate::visual::FxShape,
                       pos: Vec3,
                       shards: u8,
                       seed: u32| {
        if let Some(q) = event_fx.as_mut() {
            q.0.push(crate::rendering::EventFxRequest {
                kind,
                shape,
                pos,
                shards,
                seed,
            });
        }
    };
    match event {
        GameEvent::HeroSpawn {
            net_id,
            team,
            hero_name,
            position,
            skin,
        } => {
            if hero_registry.0.contains_key(&net_id) {
                return;
            }
            let is_local = lobby_info.my_net_id == Some(net_id);
            let pos = position.to_vec3();
            // Minecraft 式分层方块人：约 13~19 个 Cuboid 子实体 + 2 个肩枢轴，
            // 几何/纹理全部代码生成（PRES-6）；配色 / 头饰 / 武器 / 体型随 skin 变化，
            // 便于不依赖血条区分角色。造型规范见 `docs/06_VISUAL_SPEC.md`。
            let entity_id =
                voxel::spawn_voxel_hero(commands, meshes, materials, images, team, skin, pos);
            commands.entity(entity_id).insert((
                net_id,
                team,
                Hero {
                    name: hero_name.clone(),
                    skin,
                    move_speed: HERO_BASE_MOVE_SPEED,
                    ..default()
                },
                TransformSync::default(),
                Selectable {
                    radius: 1.5,
                    height: 3.0,
                },
            ));
            if is_local {
                commands.entity(entity_id).insert((LocalPlayer, Predicted));
            } else {
                commands.entity(entity_id).insert(Interpolated);
            }
            hero_registry.0.insert(net_id, entity_id);
            if is_local {
                if let Some(old) = local_hero.0
                    && old != entity_id
                {
                    commands.entity(old).despawn();
                    hero_registry.0.retain(|_, &mut v| v != old);
                }
                local_hero.0 = Some(entity_id);
            }
            info!(
                "Hero spawned: {} ({:?}) local={}",
                hero_name, net_id, is_local
            );
        }
        GameEvent::HeroDespawn { net_id } => {
            if let Some(e) = hero_registry.0.remove(&net_id) {
                commands.entity(e).despawn();
                if local_hero.0 == Some(e) {
                    local_hero.0 = None;
                }
            }
        }
        GameEvent::AbilityCast {
            caster,
            ability_index,
            target_position,
            ..
        } => {
            // P-V2：施法者进入 Cast 动画；P-V3：`ability_index` 决定技能视觉。
            if let Some(&e) = hero_registry.0.get(&caster) {
                commands.entity(e).insert(crate::hero_anim::CastAnim {
                    t: 0.45,
                    ability_index,
                });
            }
            let v = crate::visual::ability_visual(ability_index);
            // `ability_index` 是**技能槽位**，不是 ability_id；英雄换装 / 升级后
            // `ability_ids` 会变，必须经 `default_hero_abilities()` 换算再查数值表。
            let ability_id = game_core::abilities::default_hero_abilities()
                .get(ability_index as usize)
                .copied()
                .unwrap_or(0);
            let def = game_core::abilities::get_ability_def(ability_id);
            // 尺寸 / 时长读技能数值表，风格读 `AbilityVisual`，避免第二套平衡数值。
            let radius = if def.aoe_radius > 0.0 {
                def.aoe_radius.min(6.0)
            } else {
                1.2
            };
            let max = (0.6 + def.range / def.projectile_speed.max(1.0) * 0.1).clamp(0.6, 1.2);
            let to = target_position.to_vec3();
            // 起点锚在**落点正上方**：旧版锚在落点后方 `range*0.4`，技能看起来
            // 「从别处冒出来」。垂直下落既保留五阶段序列，又保证视觉与权威落点一致。
            // 不查询 / 不回写任何裁决组件。
            let from = to + Vec3::Y * 2.0;
            // 遵循 §9.3 复用规则：技能主体材质也走池（测试夹具无池时回退）。
            let seed = 61 + u32::from(ability_index);
            let fx = if let Some(p) = pool.as_mut() {
                p.fx_material(images, materials, v.kind, seed)
            } else {
                voxel::voxel_material(materials, images, v.kind, seed, true)
            };
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.6, 0.3, 1.6))),
                MeshMaterial3d(fx),
                Transform::from_translation(to + Vec3::Y * 0.3),
                CastEffect {
                    t: max,
                    max,
                    radius,
                    ability_index,
                    from,
                    to,
                    burst_done: false,
                },
            ));
        }
        GameEvent::DamageDealt {
            attacker,
            target,
            damage,
            is_critical,
        } => {
            // 攻击表现：交由渲染层按 attacker / target 位置播放“挥击 → 刀光/弹道 → 命中”序列
            if let Some(fx) = combat_fx {
                fx.0.push(crate::rendering::CombatFxRequest {
                    attacker,
                    target,
                    damage,
                    crit: is_critical,
                });
            }
            if let Some(&e) = hero_registry.0.get(&target) {
                // 连续受击时刷新硬直与闪光，避免状态机卡在旧动画。
                commands
                    .entity(e)
                    .insert((HitFlash { t: 0.25 }, crate::hero_anim::HitAnim { t: 0.28 }));
                let fx = voxel::voxel_material(materials, images, voxel::PixelKind::Red, 62, true);
                commands.entity(e).with_child((
                    Mesh3d(meshes.add(Cuboid::new(1.2, 0.3, 1.2))),
                    MeshMaterial3d(fx),
                    Transform::from_translation(Vec3::Y * 4.0),
                    DamageFlash { ttl: 1.0 },
                ));
            }
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
            push_fx(team_kind(winning_team), FxShape::Ring, Vec3::ZERO, 12, 81);
        }
        GameEvent::HeroDeath { victim, .. } => {
            if let Some(&e) = hero_registry.0.get(&victim) {
                commands
                    .entity(e)
                    .insert(crate::hero_anim::DeathAnim { t: 1.2 });
            }
            push_fx(voxel::PixelKind::Gold, FxShape::Orb, Vec3::ZERO, 10, 82);
        }
        // --- P-V3 关键事件视觉（无坐标事件以地图中心为表现锚点）---
        GameEvent::TurretDestroyed { .. } => {
            push_fx(voxel::PixelKind::Stone, FxShape::Orb, Vec3::ZERO, 8, 83);
        }
        GameEvent::JungleMonsterDeath { .. } | GameEvent::AbyssWardenDeath { .. } => {
            push_fx(voxel::PixelKind::Brush, FxShape::Orb, Vec3::ZERO, 6, 84);
        }
        GameEvent::LodeShrineCapture { team }
        | GameEvent::ObjectiveCaptured { owner: team, .. } => {
            push_fx(team_kind(team), FxShape::Ring, Vec3::ZERO, 8, 85);
        }
        GameEvent::WaveSpawned { wave_index, .. } => {
            push_fx(
                voxel::PixelKind::Blue,
                FxShape::Pulse,
                Vec3::ZERO,
                4,
                86 + (wave_index as u32 % 7),
            );
        }
        GameEvent::OvertimeStarted { .. } => {
            push_fx(voxel::PixelKind::Effect, FxShape::Ring, Vec3::ZERO, 12, 93);
        }
        GameEvent::MatchFinished { winning_team, .. }
        | GameEvent::EncounterSettled { winning_team, .. } => {
            push_fx(team_kind(winning_team), FxShape::Ring, Vec3::ZERO, 12, 94);
        }
        GameEvent::SourceCoreDestroyed { .. } => {
            push_fx(voxel::PixelKind::Core, FxShape::Orb, Vec3::ZERO, 12, 95);
        }
        GameEvent::ShieldChanged { value, .. } if value > 0.0 => {
            push_fx(voxel::PixelKind::Marker, FxShape::Ring, Vec3::ZERO, 6, 96);
        }
        GameEvent::StatusApplied { kind, .. } => {
            push_fx(
                voxel::PixelKind::Core,
                FxShape::Pulse,
                Vec3::ZERO,
                4,
                97 + u32::from(kind),
            );
        }
        _ => {}
    }
}

/// 发送本地输入：带序号、时间戳、内容 hash、校验和与真实 RTT（`DN-13` / `FR-56`）
fn send_input(
    client_res: Option<ResMut<RenetClientResource>>,
    transport_res: Option<ResMut<ClientTransportResource>>,
    input: Res<crate::input::PlayerInputState>,
    pending: Option<ResMut<crate::input::PendingCast>>,
    mut session: ResMut<SessionState>,
) {
    let (Some(mut client_res), Some(mut transport_res)) = (client_res, transport_res) else {
        return;
    };
    if !client_res.0.is_connected() {
        return;
    }

    let mut mask = 0u8;
    let mut aim = input.aim_dir;
    if let Some(mut pending) = pending
        && let Some(slot) = pending.slot
    {
        mask |= 1u8 << (slot + 1);
        aim = pending.aim;
        pending.frames = pending.frames.saturating_sub(1);
        if pending.frames == 0 {
            pending.slot = None;
        }
    }

    session.seq = session.seq.wrapping_add(1);
    let cmd = InputCmd {
        tick: 0,
        seq: session.seq,
        client_time_us: now_micros(),
        move_dir: input.move_dir.into(),
        aim_dir: aim.into(),
        ability_mask: mask,
        target_net_id: input.target_net_id.map(|n| n.0),
        ping: session.rtt_ms,
        config_hash: session.config_hash,
        checksum: 0,
    }
    .with_checksum();

    let bytes = bitcode::encode(&ClientMessage::Input(cmd));
    client_res.0.send_message(DefaultChannel::Unreliable, bytes);
    if let Err(e) = transport_res.0.send_packets(&mut client_res.0) {
        warn!("send_packets error: {:?}", e);
    }
}

/// 状态机转换
pub fn handle_state_transitions(
    lobby_info: Res<ClientLobbyInfo>,
    mut net_state: ResMut<NetworkState>,
) {
    if *net_state == NetworkState::Connecting && lobby_info.my_net_id.is_some() {
        *net_state = NetworkState::Connected;
    }
}
