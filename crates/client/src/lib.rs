//! 客户端模块：网络、渲染、输入、预测、插值与局内 UI。

pub mod audio;
pub mod entity_visual;
pub mod fog;
pub mod hero_anim;
pub mod input;
pub mod interpolation;
pub mod lobby;
pub mod lobby_ui;
pub mod networking;
pub mod pathfinding;
pub mod prediction;
pub mod rendering;
pub mod ui;
pub mod visual;
pub mod voxel;

use bevy::prelude::*;

/// 客户端应用状态：默认在大厅，点击「进入对局」后进入对局。
#[derive(States, Clone, Copy, Eq, PartialEq, Debug, Hash, Default)]
pub enum AppState {
    #[default]
    Lobby,
    InMatch,
}
