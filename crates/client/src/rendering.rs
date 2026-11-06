//! 客户端渲染模块

use bevy::prelude::*;
use bevy::image::Image;
use bevy::render::camera::CameraRenderGraph;
use bevy::core_pipeline::Core3d;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::asset::RenderAssetUsages;
use bevy_egui::PrimaryEguiContext;
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::*;
use game_core::map::*;
use crate::input::{MoveTargetMarker, SelectedTarget, TargetingState};
use std::collections::HashMap;

/// 命中闪光：短暂放大脉冲，提供“被打中”的视觉反馈
#[derive(Component)]
pub struct HitFlash {
    pub t: f32,
}

/// 目标选中指示环（右键锁定目标时出现）
#[derive(Component)]
pub struct TargetMarker;

/// 技能瞄准范围指示环（按下 Q/E/R/F 后出现，半径=技能射程）
#[derive(Component)]
pub struct TargetingRing;

/// 悬停可攻击目标高亮环（瞄准模式下，射程内光标附近的可选实体）
#[derive(Component)]
pub struct HoverMarker;

/// 瞄准落点指示（光标在地面上的命中点）
#[derive(Component)]
pub struct AimMarker;

/// 技能释放的范围闪光（落地 AoE 提示），随时间放大并消失
#[derive(Component)]
pub struct CastEffect {
    pub t: f32,
    pub max: f32,
    pub radius: f32,
}

/// 受击闪光环（作为受击实体的子节点，自动跟随并上浮放大淡出）
#[derive(Component)]
pub struct DamageFlash {
    pub ttl: f32,
}

/// 世界空间血条：跟随某个英雄实体，显示其血量（本地玩家/敌方英雄/训练假人均可）
#[derive(Component)]
pub struct HealthBar {
    pub entity: Entity,
    pub fill: Entity,
}

/// 血条填充部分：scale.x 反映血量比例（由系统每帧更新）
#[derive(Component)]
pub struct HealthBarFill {
    pub hero: Entity,
}

/// 标记英雄实体已绑定血条，避免每帧重复生成（血条本身挂在独立实体上）
#[derive(Component)]
pub struct HealthBarAttached;

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
                    update_targeting_overlay,
                    tick_cast_effect,
                    tick_damage_flash,
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
        Transform::from_xyz(0.0, 0.15, 0.0),
        Visibility::Hidden,
        TargetMarker,
    ));

    // 技能瞄准范围环（按下技能键后出现，半径=技能射程，由系统动态缩放）
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 1.0,
            minor_radius: 0.12,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.9, 1.0),
            emissive: Color::srgb(0.1, 0.5, 0.8).into(),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.15, 0.0),
        Visibility::Hidden,
        TargetingRing,
    ));

    // 悬停可攻击目标高亮环
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 1.9,
            minor_radius: 0.18,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.2),
            emissive: Color::srgb(0.7, 0.5, 0.0).into(),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.15, 0.0),
        Visibility::Hidden,
        HoverMarker,
    ));

    // 瞄准落点指示
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 1.0,
            minor_radius: 0.15,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 1.0, 1.0),
            emissive: Color::srgb(0.8, 0.8, 0.8).into(),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.2, 0.0),
        Visibility::Hidden,
        AimMarker,
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
        PrimaryEguiContext,
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

/// 为每个有生命的英雄（本地玩家、敌方英雄、训练假人）生成世界空间血条；
/// 死亡或实体消失后自动移除。血条始终朝向相机，蓝队绿、红队红。
fn update_health_bars(
    mut commands: Commands,
    heroes: Query<(Entity, &Hero, Option<&LocalPlayer>, Option<&Team>), Without<HealthBarAttached>>,
    mut bars: Query<(Entity, &mut Transform, &HealthBar, &mut Visibility), Without<HealthBarFill>>,
    mut fills: Query<(Entity, &mut Transform, &HealthBarFill), Without<HealthBar>>,
    hero_query: Query<(&GlobalTransform, &Hero)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    const BAR_W: f32 = 3.0;
    const BAR_H: f32 = 0.4;
    const BAR_Y: f32 = 3.8;
    // 相机相对英雄的偏移固定为 (0,22,26)，故血条可用常量旋转（绕 X 轴）固定面向相机
    let billboard = Quat::from_rotation_x(-0.702);

    // 1) 为尚无血条的英雄创建血条（背景条 + 填充条，均为独立实体便于清理）
    for (entity, hero, _is_local, team) in heroes.iter() {
        if hero.health <= 0.0 {
            continue;
        }
        let fill_color = match team {
            Some(&Team::Red) => Color::srgb(0.9, 0.25, 0.2),
            _ => Color::srgb(0.25, 0.9, 0.35),
        };

        let fill = commands
            .spawn((
                HealthBarFill { hero: entity },
                Mesh3d(meshes.add(Rectangle::new(BAR_W, BAR_H))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: fill_color,
                    unlit: true,
                    ..default()
                })),
                Transform::from_xyz(0.0, BAR_Y, 0.02),
                Visibility::Visible,
            ))
            .id();
        commands.spawn((
            HealthBar { entity, fill },
            Mesh3d(meshes.add(Rectangle::new(BAR_W, BAR_H))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.08, 0.08, 0.08),
                unlit: true,
                ..default()
            })),
            Transform::from_xyz(0.0, BAR_Y, 0.0),
            Visibility::Visible,
        ));
        // 标记英雄已绑定血条，避免后续帧重复生成
        commands.entity(entity).insert(HealthBarAttached);
    }

    // 2) 同步存活血条：跟随目标 + 固定朝向相机
    for (e, mut transform, healthbar, mut visibility) in bars.iter_mut() {
        let Ok((hero_gt, hero)) = hero_query.get(healthbar.entity) else {
            // 英雄实体已消失（死亡被销毁），移除血条与填充条
            commands.entity(e).despawn();
            commands.entity(healthbar.fill).despawn();
            continue;
        };
        if hero.health <= 0.0 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;
        transform.translation = hero_gt.translation() + Vec3::new(0.0, BAR_Y, 0.0);
        transform.rotation = billboard;
    }

    // 3) 填充条：跟随目标 + 按血量比例缩放（中心收缩）
    for (_e, mut tr, fill) in fills.iter_mut() {
        if let Ok((hero_gt, hero)) = hero_query.get(fill.hero) {
            let ratio = (hero.health / hero.max_health).clamp(0.0, 1.0);
            if ratio < 0.999 {
                info!(
                    "[CLT-BAR] health={:.1}/{:.1} ratio={:.2}",
                    hero.health, hero.max_health, ratio
                );
            }
            tr.translation = hero_gt.translation() + Vec3::new(0.0, BAR_Y, 0.02);
            tr.rotation = billboard;
            tr.scale.x = ratio.max(0.02);
        }
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

/// 更新技能瞄准叠加层：范围环、悬停高亮、落点指示
fn update_targeting_overlay(
    targeting: Option<Res<TargetingState>>,
    local: Query<&GlobalTransform, (With<LocalPlayer>, With<Predicted>)>,
    heroes: Query<(Entity, &NetId, &GlobalTransform)>,
    mut ring_q: Query<
        (&mut Transform, &mut Visibility),
        (With<TargetingRing>, Without<HoverMarker>, Without<AimMarker>),
    >,
    mut hover_q: Query<
        (&mut Transform, &mut Visibility),
        (With<HoverMarker>, Without<TargetingRing>, Without<AimMarker>),
    >,
    mut aim_q: Query<
        (&mut Transform, &mut Visibility),
        (With<AimMarker>, Without<TargetingRing>,         Without<HoverMarker>),
    >,
) {
    let Some(targeting) = targeting else {
        return;
    };
    if let (Ok((mut tr, mut vis)), Ok(p)) = (ring_q.single_mut(), local.single()) {
        if let Some(slot) = targeting.slot {
            let range = get_ability_def(default_hero_abilities()[slot as usize]).range;
            *tr = Transform::from_translation(p.translation() + Vec3::Y * 0.15);
            tr.scale = Vec3::splat(range.max(0.5));
            *vis = Visibility::Visible;
        } else {
            tr.scale = Vec3::ONE;
            *vis = Visibility::Hidden;
        }
    }
    if let Ok((mut tr, mut vis)) = hover_q.single_mut() {
        if let Some(nid) = targeting.hovered {
            if let Some((_, _, t)) = heroes.iter().find(|(_, n, _)| **n == nid) {
                *tr = Transform::from_translation(t.translation() + Vec3::Y * 0.15);
                *vis = Visibility::Visible;
            } else {
                *vis = Visibility::Hidden;
            }
        } else {
            *vis = Visibility::Hidden;
        }
    }
    if let Ok((mut tr, mut vis)) = aim_q.single_mut() {
        if let Some(pt) = targeting.target_point {
            *tr = Transform::from_translation(pt + Vec3::Y * 0.2);
            *vis = Visibility::Visible;
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// 技能释放范围闪光：随时间放大并消失
fn tick_cast_effect(
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &mut CastEffect)>,
    time: Res<Time>,
) {
    for (e, mut tr, mut eff) in q.iter_mut() {
        eff.t -= time.delta_secs();
        let k = (1.0 - (eff.t / eff.max).clamp(0.0, 1.0)).clamp(0.0, 1.0); // 0→1
        let s = (0.3 + (eff.radius - 0.3) * k).max(0.1);
        tr.scale = Vec3::splat(s);
        if eff.t <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// 受击闪光环：随时间上浮并放大淡出
fn tick_damage_flash(
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &mut DamageFlash)>,
    time: Res<Time>,
) {
    for (e, mut tr, mut dmg) in q.iter_mut() {
        dmg.ttl -= time.delta_secs();
        tr.translation.y += time.delta_secs() * 1.5;
        let k = (dmg.ttl / 1.0).clamp(0.0, 1.0); // 1→0
        tr.scale = Vec3::splat(2.0 + (1.0 - k) * 2.0);
        if dmg.ttl <= 0.0 {
            commands.entity(e).despawn();
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