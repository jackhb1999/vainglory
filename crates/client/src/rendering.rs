//! 客户端渲染模块

use bevy::prelude::*;
use bevy::image::Image;
use bevy::render::camera::CameraRenderGraph;
use bevy::core_pipeline::Core3d;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::asset::RenderAssetUsages;
use game_core::components::*;
use game_core::map::*;
use crate::input::{MoveTargetMarker, SelectedTarget};
use std::collections::HashMap;

/// 命中闪光：短暂放大脉冲，提供“被打中”的视觉反馈
#[derive(Component)]
pub struct HitFlash {
    t: f32,
}

/// 目标选中指示环
#[derive(Component)]
pub struct TargetMarker;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedTarget>()
            .add_systems(Startup, spawn_game_scene)
            .add_systems(
                Update,
                (
                    update_health_bars,
                    camera_follow_local_player,
                    tick_hit_flash,
                    update_target_marker,
                ),
            );
    }
}

pub fn spawn_game_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // 地面
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAP_WIDTH, MAP_HEIGHT))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.35, 0.2),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));

    // 河道
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(RIVER_WIDTH, 0.2, MAP_WIDTH))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.7),
            perceptual_roughness: 0.3,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // 中路
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(LANE_WIDTH, 0.15, MAP_WIDTH))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.3, 0.2),
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // 上路
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(LANE_WIDTH, 0.15, MAP_WIDTH))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.3, 0.2),
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(-40.0, 0.0, 0.0),
    ));

    // 下路
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(LANE_WIDTH, 0.15, MAP_WIDTH))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.3, 0.2),
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(40.0, 0.0, 0.0),
    ));

    // 目标选中环（右键敌人时出现）
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 1.8,
            minor_radius: 0.22,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.2, 0.2),
            emissive: Color::srgb(0.6, 0.0, 0.0).into(),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.15, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Visibility::Hidden,
        TargetMarker,
    ));

    // 光照
    commands.spawn((
        DirectionalLight {
            illuminance: 50000.0,
            ..default()
        },
        Transform::from_xyz(0.0, 50.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // 环境光
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
        ..default()
    });

    // 摄像机 - 显式配置 Core3d 渲染图，确保 3D 场景（地图/英雄）正常渲染
    // 俯角约 40°：比 56° 的陡视更能体现纵深方向的地面透视变化
    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        CameraRenderGraph::new(Core3d),
        Transform::from_xyz(0.0, 22.0, 26.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// 摄像机平滑跟随本地英雄。
///
/// 采用缓动跟随（相机平滑移动到「英雄 + 固定偏移」的期望位置），
/// 英雄始终留在视野内、不会撞上"死区空气墙"；同时因相机略有滞后，
/// 前后移动时英雄会在画面上轻微上下偏移，保留纵深方向的视差反馈。
fn camera_follow_local_player(
    mut camera: Query<
        &mut Transform,
        (
            With<Camera3d>,
            Without<LocalPlayer>,
            Without<HitFlash>,
            Without<TargetMarker>,
            Without<MoveTargetMarker>,
        ),
    >,
    player: Query<&GlobalTransform, (With<LocalPlayer>, With<Predicted>)>,
    time: Res<Time>,
) {
    let (Ok(mut cam), Ok(p)) = (camera.single_mut(), player.single()) else {
        return;
    };

    // 固定观察偏移：相机在英雄后上方
    let offset = Vec3::new(0.0, 22.0, 26.0);
    let desired = p.translation() + offset;

    // 平滑缓动到期望位置（系数越大跟得越紧、视差越弱）
    let t = (time.delta_secs() * 6.0).min(1.0);
    cam.translation = cam.translation.lerp(desired, t);
    cam.translation.y = desired.y;

    // 固定俯角朝向地面。Bevy 相机沿局部 -Z 观察，直接用 look_at 保证约定正确；
    // （此前手动用 Mat3::from_cols(right, up, forward) 把前向放进了 +Z 列，
    //   等价于让相机朝 -forward 即朝天空看，导致整个 3D 世界不可见。）
    let pitch = offset.xz().length().atan2(offset.y); // 视线与竖直方向夹角
    let forward = Vec3::new(0.0, -pitch.cos(), -pitch.sin());
    let target = cam.translation + forward;
    cam.look_at(target, Vec3::Y);
}

/// 更新血条显示
fn update_health_bars(
    mut commands: Commands,
    query: Query<(Entity, &Transform, &Hero), Changed<Hero>>,
    mut last_health: Local<HashMap<Entity, f32>>,
) {
    // TODO: 血条渲染
    for (entity, transform, hero) in &query {
        let prev = last_health.get(&entity).copied().unwrap_or(hero.health);
        if hero.health < prev - 0.01 {
            let dmg = prev - hero.health;
            info!(
                "英雄 {:?} 受到 {:.0} 伤害 (HP {:.0}/{:.0})",
                hero.name, dmg, hero.health, hero.max_health
            );
            // 命中闪光：插入短暂放大脉冲，提供“被打中”的视觉反馈
            commands.entity(entity).try_insert(HitFlash { t: 0.25 });
        }
        last_health.insert(entity, hero.health);
    }
}

/// 命中闪光脉冲：受击时放大并在 0.25s 内回弹
fn tick_hit_flash(
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &mut HitFlash), (Without<TargetMarker>, Without<MoveTargetMarker>, Without<Camera3d>)>,
    time: Res<Time>,
) {
    for (e, mut tr, mut flash) in q.iter_mut() {
        flash.t -= time.delta_secs();
        let k = (flash.t / 0.25).clamp(0.0, 1.0);
        let s = 1.0 + 0.7 * k;
        tr.scale = Vec3::new(s, s, s);
        if flash.t <= 0.0 {
            tr.scale = Vec3::ONE;
            commands.entity(e).remove::<HitFlash>();
        }
    }
}

/// 把选中环移动到当前锁定的攻击目标脚下
fn update_target_marker(
    selected: Res<SelectedTarget>,
    heroes: Query<(&GlobalTransform, &NetId)>,
    mut marker: Query<
        (&mut Transform, &mut Visibility),
        (With<TargetMarker>, Without<HitFlash>, Without<MoveTargetMarker>, Without<Camera3d>),
    >,
) {
    if let Ok((mut mt, mut vis)) = marker.single_mut() {
        match selected.0 {
            Some(target) => {
                if let Some((t, _)) = heroes.iter().find(|(_, n)| **n == target) {
                    *vis = Visibility::Visible;
                    mt.translation = Vec3::new(t.translation().x, 0.15, t.translation().z);
                } else {
                    *vis = Visibility::Hidden;
                }
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

/// 程序化生成英雄贴图（队伍色圆盘，带高亮描边），用于在 3D 网格上显示 UI 贴图
pub fn make_hero_texture(images: &mut Assets<Image>, color: Color) -> Handle<Image> {
    let size = 64u32;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let r = size as f32 / 2.0 - 4.0;
    let inner = r * 0.6;
    let srgba = color.to_srgba();
    let (rc, gc, bc) = (srgba.red, srgba.green, srgba.blue);
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let (r_c, g_c, b_c, a) = if dist <= inner {
                // 实心圆盘
                (rc, gc, bc, 1.0)
            } else if dist <= r {
                // 浅色描边环
                (1.0, 1.0, 1.0, 1.0)
            } else {
                // 透明
                (0.0, 0.0, 0.0, 0.0)
            };
            data.push((r_c * 255.0) as u8);
            data.push((g_c * 255.0) as u8);
            data.push((b_c * 255.0) as u8);
            data.push((a * 255.0) as u8);
        }
    }
    let image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    images.add(image)
}