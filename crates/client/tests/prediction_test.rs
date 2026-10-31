//! 验证客户端本地英雄预测：给定移动输入，本地英雄应被预测系统移动。
//! 若预测不生效（比如本地英雄没拿到 LocalPlayer/Predicted），则“前后左右”全部失灵。

use bevy::asset::AssetPlugin;
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use game_core::components::*;
use client::input::PlayerInputState;
use client::networking::ClientLobbyInfo;
use client::prediction::PredictionPlugin;

fn build_client() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(PredictionPlugin)
        .init_resource::<PlayerInputState>()
        .init_resource::<ClientLobbyInfo>();
    app
}

#[test]
fn local_hero_predicts_movement() {
    let mut app = build_client();

    let net_id = NetId(1);
    let hero = app
        .world_mut()
        .spawn((
            net_id,
            Team::Blue,
            Transform::from_xyz(0.0, 0.0, 0.0),
            TransformSync::default(),
            Hero::default(),
            LocalPlayer,
            Predicted,
        ))
        .id();

    app.world_mut().resource_mut::<ClientLobbyInfo>().my_net_id = Some(net_id);
    // W / 上 = 世界 -z
    app.world_mut().resource_mut::<PlayerInputState>().move_dir = Vec2::new(0.0, -1.0);

    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.update();
    }

    let t = app.world().get::<Transform>(hero).unwrap().translation;
    eprintln!("local hero pos after input: {:?}", t);
    assert!(
        t.z < -0.5,
        "本地英雄在移动输入下没有移动（预测失效？）：{:?}",
        t
    );
}
