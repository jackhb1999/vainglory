//! `TC-NET-04`：`FR-55` / `DEV-08` 状态恢复式重连。
//!
//! 新连接用同一会话令牌发起 `ReconnectRequest`，服务端必须恢复原 NetId / 阵营，
//! 且**不生成第二个英雄实体**、不产生第二条会话记录。

use bevy::prelude::*;
use game_core::components::NetId;
use game_core::net::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionHello, SessionToken, VersionHello,
};
use renet2::{ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use server::networking::{NetworkingPlugin as ServerNet, ServerPort, SessionRegistry};
use server::simulation::SimulationPlugin as ServerSim;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Probe {
    client: RenetClient,
    transport: NetcodeClientTransport,
    pending: Option<ClientMessage>,
}

impl Probe {
    fn new(port: u16, first_message: ClientMessage) -> Self {
        let server_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let socket = UdpSocket::bind("0.0.0.0:0").expect("bind client socket");
        let socket = NativeSocket::new(socket).expect("native socket");
        let config = ConnectionConfig::from_shared_channels(DefaultChannel::config());
        let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let authentication = ClientAuthentication::Unsecure {
            socket_id: 0,
            server_addr,
            client_id: rand::random(),
            user_data: None,
            protocol_id: PROTOCOL_VERSION as u64,
        };
        let client = RenetClient::new(config, false);
        let transport = NetcodeClientTransport::new(current_time, authentication, socket)
            .expect("client transport");
        Self {
            client,
            transport,
            pending: Some(first_message),
        }
    }

    fn step(&mut self, delta: Duration) -> Vec<ServerMessage> {
        self.client.update(delta);
        let _ = self.transport.update(delta, &mut self.client);
        if self.client.is_connected()
            && let Some(msg) = self.pending.take()
        {
            self.client
                .send_message(DefaultChannel::ReliableOrdered, bitcode::encode(&msg));
        }
        let mut msgs = Vec::new();
        while let Some(bytes) = self.client.receive_message(DefaultChannel::ReliableOrdered) {
            if let Ok(msg) = bitcode::decode::<ServerMessage>(bytes.as_ref()) {
                msgs.push(msg);
            }
        }
        let _ = self.transport.send_packets(&mut self.client);
        msgs
    }
}

#[test]
fn tc_net_04_reconnect_restores_identity_without_duplicate_hero() {
    let port = 5401;
    let mut server = App::new();
    server
        .add_plugins(MinimalPlugins)
        .add_plugins(ServerNet)
        .add_plugins(ServerSim)
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(ServerPort(port));
    server.update();

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    let token: SessionToken = [42u8; 16];
    let hello = VersionHello {
        protocol_version: PROTOCOL_VERSION,
        config_hash: gx_config::ConfigSet::load_default_with_gameplay(
            game_core::gameplay::registry::content_digest(),
        )
        .unwrap()
        .content_hash,
    };

    let mut first = Probe::new(port, ClientMessage::Hello(SessionHello { token, hello }));
    let mut first_id = None;
    for _ in 0..600 {
        server.update();
        for msg in first.step(delta) {
            if let ServerMessage::Welcome { your_net_id, .. } = msg {
                first_id = Some(your_net_id);
            }
        }
        if first_id.is_some() {
            break;
        }
    }
    let first_id = first_id.expect("initial Hello should yield Welcome");

    // 模拟重连：新连接、同一令牌
    let mut resumed_client = Probe::new(port, ClientMessage::ReconnectRequest { token, hello });
    let mut resumed_id = None;
    for _ in 0..600 {
        server.update();
        for msg in resumed_client.step(delta) {
            if let ServerMessage::SessionResume { net_id, .. } = msg {
                resumed_id = Some(net_id);
            }
        }
        if resumed_id.is_some() {
            break;
        }
    }

    assert_eq!(
        resumed_id,
        Some(first_id),
        "TC-NET-04: reconnect must restore the original NetId"
    );
    assert_eq!(
        server.world().resource::<SessionRegistry>().0.len(),
        1,
        "reconnect must not create a second session record"
    );

    let world = server.world_mut();
    let mut query = world.query::<&NetId>();
    let hero_count = query.iter(world).filter(|n| n.0 == first_id.0).count();
    assert_eq!(hero_count, 1, "reconnect must not spawn a duplicate hero");
}
