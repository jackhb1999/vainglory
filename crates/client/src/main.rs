//! Vainglory 客户端入口

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use client::{
    networking::NetworkingPlugin,
    rendering::RenderingPlugin,
    input::InputPlugin,
    prediction::PredictionPlugin,
    ui::UiPlugin,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Vainglory - Client".to_string(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(NetworkingPlugin)
        .add_plugins(RenderingPlugin)
        .add_plugins(InputPlugin)
        .add_plugins(PredictionPlugin)
        .add_plugins(UiPlugin)
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.15)))
        .run();
}