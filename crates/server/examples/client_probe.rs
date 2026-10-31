//! 探测：连接正在运行的真实服务端二进制（127.0.0.1:5000），验证链路。
use game_core::net::{ClientMessage, PROTOCOL_VERSION, ServerMessage};
use renet2::{ClientId, ConnectionConfig, DefaultChannel, RenetClient};
use renet2_netcode::{ClientAuthentication, NativeSocket, NetcodeClientTransport};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let server_addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
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
    let mut client = RenetClient::new(connection_config, false);
    let mut transport =
        NetcodeClientTransport::new(current_time, authentication, socket).expect("transport");

    let delta = Duration::from_secs_f64(1.0 / 60.0);
    for i in 0..900 {
        client.update(delta);
        if let Err(e) = transport.update(delta, &mut client) {
            eprintln!("transport err @{}: {:?}", i, e);
        }
        while let Some(bytes) = client.receive_message(DefaultChannel::ReliableOrdered) {
            if let Ok(msg) = bitcode::decode::<ServerMessage>(bytes.as_ref()) {
                match msg {
                    ServerMessage::Welcome { your_net_id, .. } => {
                        println!("GOT WELCOME net_id={:?} after {} ticks", your_net_id, i);
                        return;
                    }
                    ServerMessage::Event(game_core::net::GameEvent::HeroSpawn { net_id, .. }) => {
                        println!("GOT HeroSpawn net_id={:?} after {} ticks", net_id, i);
                    }
                    _ => {}
                }
            }
        }
        if i % 60 == 0 {
            // 顺便发一帧输入，验证上行链路
            let bytes = bitcode::encode(&ClientMessage::Input(game_core::net::InputCmd {
                tick: 0,
                move_dir: game_core::net::Vec2f::new(1.0, 0.0),
                aim_dir: game_core::net::Vec2f::zero(),
                ability_mask: 0,
                ping: 0,
            }));
            client.send_message(DefaultChannel::Unreliable, bytes);
            let _ = transport.send_packets(&mut client);
        }
        std::thread::sleep(delta);
    }
    println!("NO WELCOME RECEIVED (server likely not running or not responding)");
}
