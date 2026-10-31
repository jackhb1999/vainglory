//! 客户端模块
//! \\n
 //!
 //! 提供客户端游戏的插件和状态管理。

pub mod networking;
pub mod rendering;
pub mod input;
pub mod prediction;
pub mod ui;

mod interpolation;

use bevy::prelude::*;

/** 客户端模块，管理客户端游戏的插件和状态管理。 */

pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    /** \\n 构建客户端插件，注册默认 Bevy 插件和网络连接。 */
    fn build(&self, app: &mut App) {
        // 添加默认的 Bevy 插件（窗口、渲染、输入等）。
        app.add_plugins(bevy::DefaultPlugins);

        // 初始化网络状态资源
        app.init_resource::<networking::NetworkState>()

        // 【关键】在启动时，通过资源直接将 NetworkState 设为 Connecting，
        // 从而启动连接服务器的流程
        .add_systems(Startup, setup_network_connection)

        // 在 Connecting 状态进入时连接服务器
        .add_systems(OnEnter(networking::NetworkState::Connecting), networking::connect_to_server)

        // 在 InGame 状态进入时 spawn 游戏场景
        .add_systems(OnEnter(networking::NetworkState::InGame), rendering::spawn_game_scene)

        // 每帧更新网络状态轮询
        .add_systems(Update, networking::handle_state_transitions);
    }
}

fn setup_network_connection(mut net_state: ResMut<networking::NetworkState>) {
    info!(">>> 客户端启动，开始连接服务器...");
    *net_state = networking::NetworkState::Connecting;
}