//! Vainglory 服务端入口（Headless）

use bevy::asset::Assets;
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use server::{
    networking::NetworkingPlugin,
    simulation::SimulationPlugin,
    spawn::SpawnPlugin,
    ServerPlugin,
};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    App::new()
        .add_plugins(MinimalPlugins)
        // ServerState 状态机需要 StatesPlugin（MinimalPlugins 不含）
        .add_plugins(bevy::state::app::StatesPlugin)
        .add_plugins(NetworkingPlugin)
        .add_plugins(SimulationPlugin)
        .add_plugins(ServerPlugin)
        .add_plugins(SpawnPlugin)
        // headless 服务端不渲染，但 SpawnPlugin 在 Running 时需要 Mesh / StandardMaterial
        // 资源存储（仅 CPU 侧），故手动初始化，避免 6 人开局生成防御塔时 panic
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .run();
}