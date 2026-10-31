//! 服务端模块

pub mod networking;
pub mod simulation;
pub mod spawn;
pub mod systems;

use bevy::prelude::*;

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ServerState>()
            .add_systems(OnEnter(ServerState::Running), start_match)
            .add_systems(Update, handle_state_transitions);
    }
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ServerState {
    #[default]
    WaitingForPlayers,
    Loading,
    Running,
    Paused,
    Finished,
}

fn start_match() {
    tracing::info!("Match started on server");
}

fn handle_state_transitions(
    mut next_state: ResMut<NextState<ServerState>>,
    net_state: Res<networking::ServerNetworkState>,
) {
    // 只要有玩家连接即开始对局（便于单人/双人测试训练假人等）。
    // 正式 3v3 可在此提高阈值（例如 connected_players == 6 才 Running）。
    match net_state.connected_players {
        0 => next_state.set(ServerState::WaitingForPlayers),
        _ => next_state.set(ServerState::Running),
    }
}