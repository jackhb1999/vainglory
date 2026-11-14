//! GX-01 客户端入口

use bevy::audio::AddAudioSource;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;
use client::{
    AppState,
    audio::{ChipAudioPlugin, ChipSource},
    fog::FogPlugin,
    input::InputPlugin,
    interpolation::InterpolationPlugin,
    networking::NetworkingPlugin,
    prediction::PredictionPlugin,
    rendering::RenderingPlugin,
    ui::UiPlugin,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "GX-01 Client".to_string(),
                resolution: WindowResolution::new(1280, 720).with_scale_factor_override(1.0),
                // 固定 1.0 缩放因子：高 DPI 屏幕下 bevy_egui 会把「物理像素」当作
                // egui 视口尺寸，同时又按系统缩放因子(如 2.0)做 tessellation，
                // 导致整套 UI 被放大 2 倍并越出窗口（大厅/HUD 被裁切）。
                // 锁定 1.0 后 1 point == 1 物理像素，UI 布局尺寸与窗口完全一致。
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        // 注册内存 PCM 音源；库级 headless 测试不加载此行，自动静默降级。
        .add_audio_source::<ChipSource>()
        .add_plugins(ChipAudioPlugin)
        .add_plugins(NetworkingPlugin)
        .add_plugins(RenderingPlugin)
        .add_plugins(InputPlugin)
        .add_plugins(PredictionPlugin)
        .add_plugins(InterpolationPlugin)
        .add_plugins(FogPlugin)
        .add_plugins(UiPlugin)
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.15)))
        .run();
}
