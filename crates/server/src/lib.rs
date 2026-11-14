//! 服务端模块

pub mod gameplay;
pub mod net_bridge;
pub mod networking;
pub mod objectives;
pub mod simulation;
pub mod spawn;
pub mod systems;
pub mod world;

use bevy::prelude::*;
use game_core::gameplay::clock::{MatchClock, MatchMode};

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ServerState>()
            .add_systems(OnEnter(ServerState::Running), start_match)
            .add_systems(
                Update,
                (apply_match_finished_state, handle_state_transitions).chain(),
            );
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

/// 判胜后把服务端状态机切到 `Finished`（P-A）：此前 `finish()` 只改数据，
/// 状态机仍停在 Running，导致结束后仍按连接数反复回写 Running。
fn apply_match_finished_state(
    clock: Option<Res<MatchClock>>,
    state: Option<Res<State<ServerState>>>,
    next_state: Option<ResMut<NextState<ServerState>>>,
) {
    if !clock.is_some_and(|c| c.is_over()) {
        return;
    }
    if state.is_some_and(|s| *s.get() == ServerState::Finished) {
        return;
    }
    if let Some(mut next) = next_state {
        next.set(ServerState::Finished);
    }
}

/// 开局所需真人数：PVE 单人即可（服务端补 AI），PVP 必须两名真人。
/// 抽成纯函数以便单测锁定，避免状态机里散落魔数。
pub fn required_players(mode: MatchMode) -> usize {
    match mode {
        MatchMode::Pve => 1,
        MatchMode::Pvp => 2,
    }
}

fn handle_state_transitions(
    clock: Option<Res<MatchClock>>,
    mut next_state: ResMut<NextState<ServerState>>,
    net_state: Res<networking::ServerNetworkState>,
) {
    // 对局已结束则冻结状态机，不再因连接数变化回写 Running / Waiting。
    if clock.as_deref().is_some_and(|c| c.is_over()) {
        return;
    }
    // PVE 单人即可开局（服务端补 AI 对手）；PVP 必须两名真人连接后才 Running，
    // 且 `ensure_pve_bot` 依据 `MatchClock.mode == Pve` 决定是否生成 AI。
    let mode = clock.as_deref().map(|c| c.mode).unwrap_or_default();
    if net_state.connected_players < required_players(mode) {
        next_state.set(ServerState::WaitingForPlayers);
    } else {
        next_state.set(ServerState::Running);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pvp_waits_for_a_second_human_player() {
        assert_eq!(required_players(MatchMode::Pve), 1);
        assert_eq!(required_players(MatchMode::Pvp), 2);
        assert_eq!(required_players(MatchMode::default()), 1);
    }

    #[test]
    fn mode_code_roundtrips_and_rejects_unknown() {
        assert_eq!(MatchMode::from_code(MatchMode::Pve.code()), MatchMode::Pve);
        assert_eq!(MatchMode::from_code(MatchMode::Pvp.code()), MatchMode::Pvp);
        assert_eq!(MatchMode::from_code(255), MatchMode::Pve);
    }
}
