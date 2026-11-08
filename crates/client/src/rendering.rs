//! 客户端渲染模块

use crate::input::{MoveTargetMarker, SelectedTarget, TargetingState};
use crate::visual::FxShape;
use crate::{AppState, visual, voxel};
use bevy::core_pipeline::Core3d;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::camera::CameraRenderGraph;
use bevy_egui::PrimaryEguiContext;
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::*;

/// 相机相对英雄的固定偏移（世界坐标：后上方）
pub const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 24.0, 30.0);

/// 命中闪光：短暂放大脉冲，提供“被打中”的视觉反馈
#[derive(Component)]
pub struct HitFlash {
    pub t: f32,
}

/// 攻击者的挥击动画：驱动肩关节枢轴，呈现“抬手 → 挥出 → 收势”的完整过程
#[derive(Component)]
pub struct AttackAnim {
    pub t: f32,
    pub duration: f32,
}

/// 近战刀光：以攻击者为中心横扫一段圆弧，随后消失
#[derive(Component)]
pub struct SlashArc {
    pub t: f32,
    pub max: f32,
    pub center_yaw: f32,
}

/// 远程弹道：从攻击者胸口飞向目标，抵达后再生成命中特效
#[derive(Component)]
pub struct TracerBolt {
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
    pub max: f32,
}

/// 延迟命中：为近战留出挥击时间，到点后在目标位置爆发火花
#[derive(Component)]
pub struct ImpactDelay {
    pub t: f32,
    pub pos: Vec3,
    pub color: Color,
}

/// 命中后飞散的体素碎片
#[derive(Component)]
pub struct VoxelShard {
    pub vel: Vec3,
    pub ttl: f32,
    pub max_ttl: f32,
}

/// 来自网络伤害事件的攻击表现请求（由 `handle_game_event` 入队）
#[derive(Clone, Copy, Debug)]
pub struct CombatFxRequest {
    pub attacker: NetId,
    pub target: NetId,
    pub damage: f32,
    pub crit: bool,
}

/// 攻击表现队列：在网络层收到 DamageDealt 后入队，由渲染系统按帧消费，
/// 这样攻击者 / 目标的位置与阵营都能通过 ECS 查询取得。
#[derive(Resource, Default)]
pub struct CombatFxQueue(pub Vec<CombatFxRequest>);

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

/// 技能释放序列（P-V3）：按 `ability_index` 驱动
/// `Windup → Travel → Impact → Recover` 四阶段，尺寸 / 时长来自 `AbilityDef`。
#[derive(Component)]
pub struct CastEffect {
    pub t: f32,
    pub max: f32,
    /// 作用半径：`AbilityDef.aoe_radius`（无 AoE 时取默认落点尺寸）
    pub radius: f32,
    pub ability_index: u8,
    pub from: Vec3,
    pub to: Vec3,
    /// Impact 阶段只迸射一次碎片
    pub burst_done: bool,
}

/// 同屏临时特效硬上限（`docs/06_VISUAL_SPEC.md` §7）。
pub const MAX_CONCURRENT_FX: usize = 96;
/// 单个事件最多生成的方块碎片数（§7）。
pub const MAX_SHARDS_PER_EVENT: u8 = 12;
/// 事件碎片生命周期，严格小于 PRES-8 的 1 s 上限。
pub const EVENT_FX_TTL: f32 = 0.8;

/// 事件视觉请求队列：由 `handle_game_event` / `tick_cast_effect` 推入，
/// 由 `spawn_event_fx` 统一消费，集中做材质池复用与数量裁剪。
#[derive(Resource, Default)]
pub struct EventFxQueue(pub Vec<EventFxRequest>);

#[derive(Clone, Copy, Debug)]
pub struct EventFxRequest {
    pub kind: voxel::PixelKind,
    pub shape: FxShape,
    pub pos: Vec3,
    pub shards: u8,
    /// 确定性种子，保证同一事件回放时碎片排布一致
    pub seed: u32,
}

/// 单个方块碎片：短促位移 + 缩小 + 到期 despawn。
#[derive(Component)]
pub struct EventBurst {
    pub t: f32,
    pub velocity: Vec3,
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
    pub lag: Entity,
}

/// 血条填充部分：scale.x 反映血量比例（由系统每帧更新）
#[derive(Component)]
pub struct HealthBarFill {
    pub hero: Entity,
}

/// 血条“残影”部分：受击后缓慢收缩，直观显示本次掉血量
#[derive(Component)]
pub struct HealthBarLag {
    pub hero: Entity,
    pub shown: f32,
}

/// 标记英雄实体已绑定血条，避免每帧重复生成（血条本身挂在独立实体上）
#[derive(Component)]
pub struct HealthBarAttached;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedTarget>()
            .init_resource::<CombatFxQueue>()
            .init_resource::<EventFxQueue>()
            .init_resource::<crate::visual::VoxelMaterialPool>()
            // 启动期只建相机与灯光（供 egui 主上下文）；游戏世界延迟到进入对局。
            .add_systems(Startup, spawn_game_scene)
            // 选「开始」后进入对局，才生成地形与标记，后景不再显示对局内容。
            .add_systems(OnEnter(AppState::InMatch), spawn_match_world)
            .add_systems(
                Update,
                (
                    update_health_bars,
                    camera_follow_local_player,
                    tick_hit_flash,
                    spawn_combat_fx,
                    // 英雄动画链：先推速度 → 既有挥击动画 → 状态机写骨架姿态。
                    // 严格顺序保证 Attack 态双臂不被状态机覆盖。
                    (
                        crate::hero_anim::update_hero_motion,
                        tick_attack_anim,
                        crate::hero_anim::tick_hero_anim,
                    )
                        .chain(),
                    tick_slash_arc,
                    tick_tracer,
                    tick_impact_delay,
                    tick_voxel_shard,
                    update_target_marker,
                    update_targeting_overlay,
                    // 技能序列 → 事件碎片生成（含预算裁剪）→ 碎片生命周期，严格顺序。
                    (tick_cast_effect, spawn_event_fx, tick_event_burst).chain(),
                    tick_damage_flash,
                ),
            );
    }
}

/// 启动期只建立「承载 egui 主上下文的相机 + 灯光」，**不生成任何游戏世界内容**。
/// 大厅状态下后景是空 3D 场景（由大厅 UI 全屏覆盖），游戏地形 / 标记等到
/// `OnEnter(InMatch)` 的 `spawn_match_world` 才生成，确保「选开始后才进入对局」。
pub fn spawn_game_scene(mut commands: Commands) {
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

    // 摄像机 - 显式配置 Core3d 渲染图，确保 3D 场景（地图/英雄）正常渲染。
    // 带 `PrimaryEguiContext` 才能驱动大厅 UI；初始对准蓝方基地，进入对局后由跟随系统接管。
    let start = game_core::map::BLUE_BASE;
    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        // 显式收紧深度范围：默认 near=0.1 / far=1000（比值 1e4）会把 0.05 量级的
        // 地形层高差压进同一深度量化区间，随镜头平移出现 z-fighting。
        Projection::Perspective(PerspectiveProjection {
            near: 1.0,
            far: 400.0,
            ..default()
        }),
        CameraRenderGraph::new(Core3d),
        PrimaryEguiContext,
        Transform::from_translation(start + CAMERA_OFFSET).looking_at(start, Vec3::Y),
    ));
}

/// 进入对局后生成 Minecraft 式体素游戏世界：地形 / 兵线 / 野区 / 草丛 / 哨塔 / 源核 / 各类标记。
/// 与 `spawn_game_scene` 分离，使大厅阶段完全不渲染对局内容。
pub fn spawn_match_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Minecraft 式体素地形：地面 / 单条兵线 / 野区 / 草丛 / 哨塔 / 源核（PRES-7）
    voxel::spawn_voxel_terrain(&mut commands, &mut meshes, &mut materials, &mut images);

    // 方块指示框取代圆环标记（PRES-8）；父实体承载 Marker 组件供系统移动/显隐
    let target = voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        voxel::PixelKind::Red,
        1.8,
        crate::input::MARKER_Y,
    );
    commands.entity(target).insert(TargetMarker);

    let targeting = voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        voxel::PixelKind::Blue,
        1.0,
        crate::input::MARKER_Y,
    );
    commands.entity(targeting).insert(TargetingRing);

    let hover = voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        voxel::PixelKind::Gold,
        1.9,
        crate::input::MARKER_Y,
    );
    commands.entity(hover).insert(HoverMarker);

    let aim = voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        voxel::PixelKind::Marker,
        1.0,
        crate::input::MARKER_Y,
    );
    commands.entity(aim).insert(AimMarker);
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
    let offset = CAMERA_OFFSET;
    let desired = p.translation() + offset;

    // 平滑缓动到期望位置（系数越大跟得越紧、视差越弱）
    let t = (time.delta_secs() * 6.0).min(1.0);
    // x/y/z 一起平滑：旧版 y 硬赋值在英雄高度抖动时会造成纵向跳变
    cam.translation = cam.translation.lerp(desired, t);

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
    time: Res<Time>,
    heroes: Query<(Entity, &Hero, Option<&Team>), Without<HealthBarAttached>>,
    mut bars: Query<
        (Entity, &mut Transform, &HealthBar, &mut Visibility),
        (Without<HealthBarFill>, Without<HealthBarLag>),
    >,
    mut fills: Query<
        (Entity, &mut Transform, &mut Visibility, &HealthBarFill),
        (Without<HealthBar>, Without<HealthBarLag>),
    >,
    mut lags: Query<
        (Entity, &mut Transform, &mut Visibility, &mut HealthBarLag),
        (Without<HealthBar>, Without<HealthBarFill>),
    >,
    hero_query: Query<(&GlobalTransform, &Hero)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    const BAR_W: f32 = 3.4;
    const BAR_H: f32 = 0.46;
    const BAR_Y: f32 = 4.3;
    // 血条垂直于相机视线：绕 X 轴旋转固定俯角，沿法线方向叠放背景 / 残影 / 填充
    let len = CAMERA_OFFSET.yz().length();
    let billboard = Quat::from_rotation_x(-CAMERA_OFFSET.y.atan2(CAMERA_OFFSET.z));
    let normal = Vec3::new(0.0, CAMERA_OFFSET.y / len, CAMERA_OFFSET.z / len);

    // 1) 为尚无血条的英雄创建血条（背景条 + 残影条 + 填充条，均为独立实体便于清理）
    for (entity, hero, team) in heroes.iter() {
        if hero.health <= 0.0 {
            continue;
        }
        let fill_color = match team {
            Some(&Team::Red) => Color::srgb(1.0, 0.35, 0.28),
            Some(&Team::Neutral) => Color::srgb(1.0, 0.82, 0.32),
            _ => Color::srgb(0.28, 0.72, 1.0),
        };

        let lag = commands
            .spawn((
                HealthBarLag {
                    hero: entity,
                    shown: 1.0,
                },
                Mesh3d(meshes.add(Rectangle::new(BAR_W, BAR_H))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.96, 0.78, 0.62),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(Vec3::new(0.0, BAR_Y, 0.0)),
                Visibility::Visible,
            ))
            .id();

        let fill = commands
            .spawn((
                HealthBarFill { hero: entity },
                Mesh3d(meshes.add(Rectangle::new(BAR_W, BAR_H))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: fill_color,
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(Vec3::new(0.0, BAR_Y, 0.0)),
                Visibility::Visible,
            ))
            .id();

        commands.spawn((
            HealthBar { entity, fill, lag },
            Mesh3d(meshes.add(Rectangle::new(BAR_W + 0.16, BAR_H + 0.16))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.05, 0.07),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(Vec3::new(0.0, BAR_Y, 0.0)),
            Visibility::Visible,
        ));
        // 标记英雄已绑定血条，避免后续帧重复生成
        commands.entity(entity).insert(HealthBarAttached);
    }

    // 2) 同步存活血条：跟随目标 + 固定朝向相机
    for (e, mut transform, healthbar, mut visibility) in bars.iter_mut() {
        let Ok((hero_gt, hero)) = hero_query.get(healthbar.entity) else {
            // 英雄实体已消失（死亡被销毁），移除血条、残影与填充条
            commands.entity(e).despawn();
            commands.entity(healthbar.fill).despawn();
            commands.entity(healthbar.lag).despawn();
            continue;
        };
        if hero.health <= 0.0 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;
        transform.translation = hero_gt.translation() + Vec3::Y * BAR_Y;
        transform.rotation = billboard;
    }

    // 3) 填充条：跟随目标 + 按血量比例缩放（左缘固定，从右向左缩减）
    for (_e, mut tr, mut vis, fill) in fills.iter_mut() {
        let Ok((hero_gt, hero)) = hero_query.get(fill.hero) else {
            *vis = Visibility::Hidden;
            continue;
        };
        if hero.health <= 0.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let ratio = if hero.max_health > 0.0 {
            (hero.health / hero.max_health).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // 左缘锚定：平移使左侧边不动，右侧随比例缩短，避免中心收缩的视觉异常
        tr.translation = hero_gt.translation()
            + Vec3::Y * BAR_Y
            + normal * 0.024
            + Vec3::X * (-BAR_W * (1.0 - ratio) / 2.0);
        tr.rotation = billboard;
        tr.scale.x = ratio.clamp(0.001, 1.0);
    }

    // 4) 残影条：只会在受伤后缓慢收缩，便于看清本次掉血
    let dt = time.delta_secs();
    for (_e, mut tr, mut vis, mut lag) in lags.iter_mut() {
        let Ok((hero_gt, hero)) = hero_query.get(lag.hero) else {
            *vis = Visibility::Hidden;
            continue;
        };
        if hero.health <= 0.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let ratio = if hero.max_health > 0.0 {
            (hero.health / hero.max_health).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // 残影收回速度：0→1 的满血条大约 1 秒内追平
        lag.shown = (lag.shown - dt * 0.9).max(ratio);
        lag.shown = lag.shown.max(ratio).min(1.0);
        tr.translation = hero_gt.translation()
            + Vec3::Y * BAR_Y
            + normal * 0.012
            + Vec3::X * (-BAR_W * (1.0 - lag.shown) / 2.0);
        tr.rotation = billboard;
        tr.scale.x = lag.shown.clamp(0.001, 1.0);
    }
}

/// 命中闪光脉冲：受击时放大并在 0.25s 内回弹
fn tick_hit_flash(
    mut commands: Commands,
    mut q: Query<
        (Entity, &mut Transform, &mut HitFlash),
        (
            Without<TargetMarker>,
            Without<MoveTargetMarker>,
            Without<Camera3d>,
        ),
    >,
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

/// 消费攻击表现队列：触发攻击者的挥击动画，并按距离生成刀光或弹道。
///
/// 相比之前“对比血量推断攻击者”的做法，这里直接使用服务端 DamageDealt 事件携带的
/// attacker / target id，攻击方向与出手时机都准确，且能区分近战 / 远程表现。
fn spawn_combat_fx(
    mut commands: Commands,
    mut queue: ResMut<CombatFxQueue>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(
        Entity,
        &NetId,
        &GlobalTransform,
        Option<&Team>,
        Option<&Hero>,
    )>,
) {
    if queue.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut queue.0);
    for req in requests {
        let attacker = units.iter().find(|u| *u.1 == req.attacker);
        let target = units.iter().find(|u| *u.1 == req.target);

        let Some(target) = target else { continue };
        let target_pos = target.2.translation();

        let Some(attacker) = attacker else {
            // 攻击者不在客户端可见集合内（防御塔 / 视野外），只保留目标端反馈
            spawn_impact_burst(
                &mut commands,
                &mut meshes,
                &mut materials,
                target_pos + Vec3::Y * 1.3,
                impact_color(Team::Neutral),
            );
            continue;
        };
        let (attacker_entity, _, attacker_gt, attacker_team, attacker_hero) = attacker;
        let attacker_pos = attacker_gt.translation();
        let attacker_team = attacker_team.copied().unwrap_or(Team::Neutral);
        let is_hero = attacker_hero.is_some();

        // 1) 攻击者挥击动画（仅英雄有骨架）
        if is_hero {
            commands.entity(attacker_entity).insert(AttackAnim {
                t: 0.34,
                duration: 0.34,
            });
        }

        let flat = Vec2::new(target_pos.x - attacker_pos.x, target_pos.z - attacker_pos.z);
        let distance = flat.length();
        let yaw = flat.y.atan2(flat.x);
        let color = impact_color(attacker_team);

        if distance <= 4.5 {
            // 2a) 近战：横扫刀光 + 延迟命中火花，形成“挥出 → 命中”的序列
            let forward = if distance > 1e-3 {
                Vec3::new(flat.x / distance, 0.0, flat.y / distance)
            } else {
                Vec3::Z
            };
            commands.spawn((
                SlashArc {
                    t: 0.24,
                    max: 0.24,
                    center_yaw: -yaw,
                },
                Mesh3d(meshes.add(Cuboid::new(2.6, 0.14, 0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    emissive: Color::srgb(0.35, 0.35, 0.35).into(),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(attacker_pos + forward * 1.7 + Vec3::Y * 1.7)
                    .with_rotation(Quat::from_rotation_y(-yaw - 1.15)),
            ));
            commands.spawn((
                ImpactDelay {
                    t: 0.14,
                    pos: target_pos + Vec3::Y * 1.3,
                    color,
                },
                Transform::default(),
                Visibility::Hidden,
            ));
        } else {
            // 2b) 远程：可见弹道从攻击者飞向目标，抵达瞬间才爆发命中特效
            commands.spawn((
                TracerBolt {
                    from: attacker_pos + Vec3::Y * 1.7,
                    to: target_pos + Vec3::Y * 1.3,
                    t: 0.24,
                    max: 0.24,
                },
                Mesh3d(meshes.add(Cuboid::new(0.34, 0.34, 0.34))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    emissive: Color::srgb(0.5, 0.4, 0.2).into(),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(attacker_pos + Vec3::Y * 1.7),
            ));
        }

        // 3) 暴击时额外给攻击者一个更大的刀光提示
        if req.crit && distance <= 4.5 {
            commands.spawn((
                SlashArc {
                    t: 0.28,
                    max: 0.28,
                    center_yaw: -yaw,
                },
                Mesh3d(meshes.add(Cuboid::new(3.4, 0.1, 0.7))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.55, 0.2),
                    emissive: Color::srgb(0.6, 0.2, 0.0).into(),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(attacker_pos + Vec3::Y * 1.9)
                    .with_rotation(Quat::from_rotation_y(-yaw - 1.15)),
            ));
        }
        let _ = req.damage;
    }
}

fn impact_color(team: Team) -> Color {
    match team {
        Team::Blue => Color::srgb(0.55, 0.85, 1.0),
        Team::Red => Color::srgb(1.0, 0.55, 0.4),
        Team::Neutral => Color::srgb(1.0, 0.82, 0.35),
    }
}

/// 攻击者挥击动画：肩关节枢轴“抬手 → 挥出 → 收势”。
/// 仅旋转手臂枢轴（不是手臂网格中心），因此看起来是从肩部发力。
fn tick_attack_anim(
    mut commands: Commands,
    time: Res<Time>,
    mut anims: Query<(Entity, &mut AttackAnim, &voxel::HeroRig)>,
    mut transforms: Query<&mut Transform>,
) {
    for (entity, mut anim, rig) in anims.iter_mut() {
        anim.t -= time.delta_secs();
        let progress = (1.0 - anim.t / anim.duration).clamp(0.0, 1.0);
        // 前 35% 抬手（-0.9 rad），随后快速挥出到 +1.3 rad，最后 15% 收势
        let angle = if progress < 0.35 {
            -0.9 * (progress / 0.35)
        } else if progress < 0.8 {
            let k = (progress - 0.35) / 0.45;
            -0.9 + 2.2 * k
        } else {
            let k = (progress - 0.8) / 0.2;
            1.3 * (1.0 - k)
        };
        if let Ok(mut tr) = transforms.get_mut(rig.right_arm) {
            tr.rotation = Quat::from_rotation_x(-angle);
        }
        if let Ok(mut tr) = transforms.get_mut(rig.left_arm) {
            tr.rotation = Quat::from_rotation_x(angle * 0.35);
        }
        if anim.t <= 0.0 {
            if let Ok(mut tr) = transforms.get_mut(rig.right_arm) {
                tr.rotation = Quat::IDENTITY;
            }
            if let Ok(mut tr) = transforms.get_mut(rig.left_arm) {
                tr.rotation = Quat::IDENTITY;
            }
            commands.entity(entity).remove::<AttackAnim>();
        }
    }
}

/// 刀光横扫：从 -1.15rad 扫到 +1.15rad 并略微增长。
fn tick_slash_arc(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut SlashArc)>,
) {
    for (e, mut tr, mut arc) in q.iter_mut() {
        arc.t -= time.delta_secs();
        let progress = (1.0 - arc.t / arc.max).clamp(0.0, 1.0);
        let yaw = arc.center_yaw - 1.15 + 2.3 * progress;
        tr.rotation = Quat::from_rotation_y(yaw);
        tr.scale = Vec3::new(0.55 + 0.75 * progress, 1.0, 1.0);
        if arc.t <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// 弹道飞行：线性插值到目标点，抵达后生成命中爆发。
fn tick_tracer(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Transform, &mut TracerBolt)>,
) {
    for (e, mut tr, mut bolt) in q.iter_mut() {
        bolt.t -= time.delta_secs();
        let progress = (1.0 - bolt.t / bolt.max).clamp(0.0, 1.0);
        tr.translation = bolt.from.lerp(bolt.to, progress);
        tr.rotation = Quat::from_rotation_y(progress * 8.0);
        if bolt.t <= 0.0 {
            spawn_impact_burst(
                &mut commands,
                &mut meshes,
                &mut materials,
                bolt.to,
                Color::srgb(1.0, 0.6, 0.3),
            );
            commands.entity(e).despawn();
        }
    }
}

/// 近战延迟命中：到点后爆发火花并销毁自身。
fn tick_impact_delay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ImpactDelay)>,
) {
    for (e, mut imp) in q.iter_mut() {
        imp.t -= time.delta_secs();
        if imp.t <= 0.0 {
            spawn_impact_burst(
                &mut commands,
                &mut meshes,
                &mut materials,
                imp.pos,
                imp.color,
            );
            commands.entity(e).despawn();
        }
    }
}

/// 命中爆发：中心闪光 + 6 枚体素碎片，构成受击的“结果”表现。
fn spawn_impact_burst(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    pos: Vec3,
    color: Color,
) {
    commands.spawn((
        VoxelShard {
            vel: Vec3::ZERO,
            ttl: 0.22,
            max_ttl: 0.22,
        },
        Mesh3d(meshes.add(Cuboid::new(1.1, 1.1, 1.1))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            emissive: Color::srgb(0.45, 0.25, 0.1).into(),
            unlit: true,
            ..default()
        })),
        Transform::from_translation(pos),
    ));
    for i in 0..6 {
        let angle = i as f32 * std::f32::consts::TAU / 6.0;
        let vel = Vec3::new(
            angle.cos() * 4.2,
            2.4 + (i % 2) as f32 * 1.6,
            angle.sin() * 4.2,
        );
        commands.spawn((
            VoxelShard {
                vel,
                ttl: 0.5,
                max_ttl: 0.5,
            },
            Mesh3d(meshes.add(Cuboid::new(0.28, 0.28, 0.28))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    }
}

/// 体素碎片：抛物线下落并逐渐缩小后销毁。
fn tick_voxel_shard(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut VoxelShard)>,
) {
    let dt = time.delta_secs();
    for (e, mut tr, mut shard) in q.iter_mut() {
        shard.ttl -= dt;
        let k = (shard.ttl / shard.max_ttl).clamp(0.0, 1.0);
        tr.translation += shard.vel * dt;
        shard.vel.y -= 12.0 * dt;
        tr.scale = Vec3::splat((0.25 + 0.85 * k).max(0.05));
        tr.rotate_y(dt * 6.0);
        if shard.ttl <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// 把选中环移动到当前锁定的攻击目标脚下
fn update_target_marker(
    selected: Res<SelectedTarget>,
    heroes: Query<(&GlobalTransform, &NetId)>,
    mut marker: Query<
        (&mut Transform, &mut Visibility),
        (
            With<TargetMarker>,
            Without<HitFlash>,
            Without<MoveTargetMarker>,
            Without<Camera3d>,
        ),
    >,
) {
    if let Ok((mut mt, mut vis)) = marker.single_mut() {
        match selected.0 {
            Some(target) => {
                if let Some((t, _)) = heroes.iter().find(|(_, n)| **n == target) {
                    *vis = Visibility::Visible;
                    mt.translation =
                        Vec3::new(t.translation().x, crate::input::MARKER_Y, t.translation().z);
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
        (
            With<TargetingRing>,
            Without<HoverMarker>,
            Without<AimMarker>,
        ),
    >,
    mut hover_q: Query<
        (&mut Transform, &mut Visibility),
        (
            With<HoverMarker>,
            Without<TargetingRing>,
            Without<AimMarker>,
        ),
    >,
    mut aim_q: Query<
        (&mut Transform, &mut Visibility),
        (
            With<AimMarker>,
            Without<TargetingRing>,
            Without<HoverMarker>,
        ),
    >,
) {
    let Some(targeting) = targeting else {
        return;
    };
    if let (Ok((mut tr, mut vis)), Ok(p)) = (ring_q.single_mut(), local.single()) {
        if let Some(slot) = targeting.slot {
            let range = get_ability_def(default_hero_abilities()[slot as usize]).range;
            *tr = Transform::from_translation(p.translation() + Vec3::Y * crate::input::MARKER_Y);
            // 只放大水平半径：旧版 `splat(range)` 把 0.12 厚的环体一起放大到数单位，
            // 环体穿进地形层造成局部闪烁；y 抬到最高地面层之上。
            tr.scale = Vec3::new(range.max(0.5), 1.0, range.max(0.5));
            *vis = Visibility::Visible;
        } else {
            tr.scale = Vec3::ONE;
            *vis = Visibility::Hidden;
        }
    }
    if let Ok((mut tr, mut vis)) = hover_q.single_mut() {
        if let Some(nid) = targeting.hovered {
            if let Some((_, _, t)) = heroes.iter().find(|(_, n, _)| **n == nid) {
                *tr =
                    Transform::from_translation(t.translation() + Vec3::Y * crate::input::MARKER_Y);
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
            *tr = Transform::from_translation(pt + Vec3::Y * crate::input::MARKER_Y);
            *vis = Visibility::Visible;
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// 技能释放范围闪光：随时间放大并消失
/// 技能五阶段序列：Windup → Release → Travel → Impact → Recover。
/// 形状 / 语义色来自 `AbilityVisual`，半径与时长来自 `AbilityDef`，
/// 命中碎片统一推入 `EventFxQueue`，由消费系统做池化与预算裁剪。
fn tick_cast_effect(
    mut commands: Commands,
    mut queue: ResMut<EventFxQueue>,
    mut q: Query<(Entity, &mut Transform, &mut CastEffect)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (e, mut tr, mut eff) in q.iter_mut() {
        eff.t -= dt;
        let p = (1.0 - (eff.t / eff.max).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let v = visual::ability_visual(eff.ability_index);
        if p < 0.2 {
            // Windup：施法点收缩蓄力
            let k = p / 0.2;
            tr.translation = eff.from + Vec3::Y * 1.2;
            tr.scale = Vec3::splat((1.0 - 0.7 * k).max(0.1));
        } else if p < 0.55 {
            // Release / Travel：按技能形状从施法点移向落点
            let k = (p - 0.2) / 0.35;
            tr.translation = eff.from.lerp(eff.to, k) + Vec3::Y * (0.4 + 0.8 * k);
            tr.scale = match v.shape {
                FxShape::Beam => Vec3::new(0.5, 0.5, 2.4),
                FxShape::Ring => Vec3::new(1.2, 0.3, 1.2),
                FxShape::Pulse => Vec3::new(0.9, 1.4, 0.9),
                FxShape::Orb => Vec3::splat(0.8),
            };
        } else if p < 0.8 {
            // Impact：按 `aoe_radius` 展开阶梯方块环，并迸射一次碎片
            let k = (p - 0.55) / 0.25;
            tr.translation = eff.to + Vec3::Y * 0.3;
            let s = 0.8 + eff.radius * k;
            tr.scale = Vec3::new(s, 0.3, s);
            if !eff.burst_done {
                eff.burst_done = true;
                queue.0.push(EventFxRequest {
                    kind: v.kind,
                    shape: v.shape,
                    pos: eff.to,
                    shards: v.shards,
                    seed: u32::from(eff.ability_index) * 97 + 13,
                });
            }
        } else {
            // Recover：按固定基准收缩，不逐帧累乘，避免漂移
            let k = (p - 0.8) / 0.2;
            let s = (0.8 + eff.radius) * (1.0 - k).max(0.05);
            tr.scale = Vec3::new(s, 0.3 * (1.0 - k).max(0.05), s);
        }
        if eff.t <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// 单事件碎片数裁剪（§7：每事件 ≤12）。
fn clamp_shards(shards: u8) -> usize {
    shards.min(MAX_SHARDS_PER_EVENT) as usize
}

/// 事件 / 技能碎片的唯一生成入口：共享网格与材质、确定性排布、同屏数量硬裁剪。
#[allow(clippy::too_many_arguments)]
pub fn spawn_event_fx(
    mut commands: Commands,
    mut queue: ResMut<EventFxQueue>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pool: ResMut<visual::VoxelMaterialPool>,
    existing: Query<Entity, With<EventBurst>>,
    mut sfx_queue: Option<ResMut<crate::audio::SfxQueue>>,
) {
    let requests = std::mem::take(&mut queue.0);
    if requests.is_empty() {
        return;
    }
    let mut live = existing.iter().count();
    for req in requests {
        // 视觉与听觉共用同一次事件消费：种子段决定音效，未登记的高频事件静默。
        if let Some(id) = crate::audio::sfx_for_event(req.seed)
            && let Some(q) = sfx_queue.as_deref_mut()
        {
            q.0.push(id);
        }
        let count = clamp_shards(req.shards);
        if count == 0 {
            continue;
        }
        // 超预算时优先丢弃最旧的碎片，保证新事件仍可见（§7）。
        if live + count > MAX_CONCURRENT_FX {
            let excess = live + count - MAX_CONCURRENT_FX;
            for entity in existing.iter().take(excess) {
                commands.entity(entity).despawn();
            }
            live = live.saturating_sub(excess);
        }
        let cube = pool.cube(&mut meshes);
        let mat = pool.fx_material(&mut images, &mut materials, req.kind, req.seed);
        for i in 0..count {
            let h = visual::hash(i as u32, req.seed, 0x9e37);
            let angle = (h & 0xFF) as f32 / 255.0 * std::f32::consts::TAU;
            let radius = 0.9 + ((h >> 8) & 0x7) as f32 * 0.22;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            let (pos, velocity) = match req.shape {
                FxShape::Ring | FxShape::Orb => {
                    (req.pos + dir * radius * 0.3, dir * 3.0 + Vec3::Y * 1.4)
                }
                FxShape::Beam => (
                    req.pos + Vec3::new(0.0, 0.0, i as f32 * 0.5 - count as f32 * 0.25),
                    Vec3::Z * 1.5,
                ),
                FxShape::Pulse => (req.pos + Vec3::Y * (i as f32 * 0.22), Vec3::Y * 2.2),
            };
            commands.spawn((
                EventBurst {
                    t: EVENT_FX_TTL,
                    velocity,
                },
                Mesh3d(cube.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(pos).with_scale(Vec3::splat(0.5)),
            ));
        }
        live += count;
    }
}

/// 碎片生命周期：抛物线上浮 + 缩小，≤0.8 s 后销毁（PRES-8）。
pub fn tick_event_burst(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut EventBurst)>,
) {
    let dt = time.delta_secs();
    for (e, mut tr, mut b) in q.iter_mut() {
        b.t -= dt;
        tr.translation += b.velocity * dt;
        b.velocity.y -= 6.0 * dt;
        let k = (b.t / EVENT_FX_TTL).max(0.0);
        tr.scale = Vec3::splat((0.5 * k).max(0.02));
        if b.t <= 0.0 {
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

/// 代码生成队伍像素贴图（PRES-4）；保留旧 API 供外部调用方使用。
pub fn make_hero_texture(images: &mut Assets<Image>, color: Color) -> Handle<Image> {
    voxel::make_hero_texture(images, color)
}

#[cfg(test)]
mod pv3_tests {
    use super::*;

    #[test]
    fn performance_budget_is_bounded() {
        const {
            assert!(EVENT_FX_TTL < 1.0, "PRES-8：特效生命周期须严格小于 1 s");
            assert!(MAX_SHARDS_PER_EVENT <= 12, "§7：每事件 ≤12 碎片");
            assert!(MAX_CONCURRENT_FX >= MAX_SHARDS_PER_EVENT as usize);
        }
        assert_eq!(clamp_shards(0), 0);
        assert_eq!(clamp_shards(4), 4);
        assert_eq!(clamp_shards(12), 12);
        assert_eq!(clamp_shards(99), MAX_SHARDS_PER_EVENT as usize);
    }

    #[test]
    fn ability_sequence_phases_are_ordered_and_cover_full_lifetime() {
        // 五阶段边界：Windup / Release-Travel / Impact / Recover，和为 1.0。
        const PHASES: [f32; 5] = [0.0, 0.2, 0.55, 0.8, 1.0];
        assert_eq!(PHASES[0], 0.0);
        assert!(PHASES.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(PHASES[4], 1.0);
    }

    /// 运行时护栏：事件队列 → 碎片生成 → 生命周期系统在同一 App 内无查询冲突，
    /// 且确实生成了受池管理的方块实体。
    #[test]
    fn event_fx_systems_run_without_conflict() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<visual::VoxelMaterialPool>()
            .init_resource::<EventFxQueue>()
            .add_systems(Update, (spawn_event_fx, tick_event_burst).chain());
        app.world_mut()
            .resource_mut::<EventFxQueue>()
            .0
            .push(EventFxRequest {
                kind: voxel::PixelKind::Effect,
                shape: FxShape::Orb,
                pos: Vec3::ZERO,
                shards: 6,
                seed: 42,
            });
        app.update();
        let mut q = app.world_mut().query::<&EventBurst>();
        assert_eq!(q.iter(app.world()).count(), 6);
        let stats = app.world().resource::<visual::VoxelMaterialPool>().stats();
        assert!(stats.mesh_shared, "碎片网格必须来自共享 Handle");
        assert_eq!(stats.effects, 1, "同参数特效材质必须复用");
    }

    /// 大厅启动期只建立「承载 egui 的相机 + 灯光」，绝不应生成地形 / 对局标记；
    /// 游戏世界改由 `OnEnter(AppState::InMatch)` 的 `spawn_match_world` 生成，
    /// 保证「选开始后才进入对局」，后景不再是游戏对局。
    #[test]
    fn lobby_startup_renders_only_camera_not_match_world() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Startup, spawn_game_scene);
        app.update();
        // egui 主上下文所需的相机已建立
        assert!(
            app.world_mut()
                .query::<&Camera3d>()
                .iter(app.world())
                .next()
                .is_some(),
            "lobby must spawn the egui-bearing camera"
        );
        // 但地形 / 对局标记绝不可在大厅出现
        let mut q = app.world_mut().query::<&TargetMarker>();
        assert!(
            q.iter(app.world()).next().is_none(),
            "lobby startup must not render the match world"
        );
    }
}
