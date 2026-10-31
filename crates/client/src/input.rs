//! 客户端输入处理
//!
//! 控制方案：
//! - 左键点击地面：英雄走向该点（点击移动，到达后停下）
//! - WASD / 方向键：直接控制移动方向（优先级高于点击移动）
//! - 鼠标移动：英雄朝鼠标指向的地面点（瞄准方向）
//! - 技能键 Q / E / R / F：释放 4 个技能槽（同时兼容数字键 1-4）
//! - 右键：普攻（bit0 预留）

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use game_core::components::{LocalPlayer, NetId, Selectable};
use crate::rendering::{HitFlash, TargetMarker};

/// 当前锁定的攻击目标（net id），右键敌人时设置，用于显示选中环。
#[derive(Resource, Default)]
pub struct SelectedTarget(pub Option<NetId>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInputState>()
            .init_resource::<LocalPlayerTransform>()
            .init_resource::<MoveTarget>()
            .init_resource::<SelectedTarget>()
            .add_systems(Startup, spawn_move_target_marker)
            .add_systems(
                Update,
                (
                    update_local_player_transform,
                    handle_input,
                    update_move_target_marker,
                )
                    .chain(),
            )
            // egui 调试面板必须在 EguiPrimaryContextPass 中绘制（bevy_egui 0.43 要求）
            .add_systems(EguiPrimaryContextPass, update_egui_input);
    }
}

/// 点击移动的目标点指示器（地面上的圆环）
#[derive(Component)]
pub struct MoveTargetMarker;

fn spawn_move_target_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 1.6,
            minor_radius: 0.25,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.2),
            ..default()
        })),
        // 平铺在地面上
        Transform::from_xyz(0.0, 0.2, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Visibility::Hidden,
        MoveTargetMarker,
    ));
}

fn update_move_target_marker(
    move_target: Res<MoveTarget>,
    mut marker: Query<
        (&mut Transform, &mut Visibility),
        (With<MoveTargetMarker>, Without<HitFlash>, Without<TargetMarker>, Without<Camera3d>),
    >,
) {
    if let Ok((mut transform, mut visibility)) = marker.single_mut() {
        if let Some(target) = move_target.0 {
            *visibility = Visibility::Visible;
            transform.translation = Vec3::new(target.x, 0.2, target.y);
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

/// 缓存本地玩家的 Transform，供输入系统计算移动方向
#[derive(Resource, Default)]
pub struct LocalPlayerTransform(Transform);

fn update_local_player_transform(
    transform_query: Query<&Transform, With<LocalPlayer>>,
    mut local_player_transform: ResMut<LocalPlayerTransform>,
) {
    if let Ok(transform) = transform_query.single() {
        local_player_transform.0 = *transform;
    }
}

/// 点击移动的目标点（世界坐标地面 x,z）。`None` 表示无目标（待命）。
#[derive(Resource, Default)]
pub struct MoveTarget(pub Option<Vec2>);

#[derive(Resource, Default)]
pub struct PlayerInputState {
    pub move_dir: Vec2,
    pub aim_dir: Vec2,
    pub ability_mask: u8,
    pub is_dragging_ability: bool,
    pub drag_start: Vec2,
    pub selected_ability: Option<u8>,
}

/// 把屏幕光标经摄像机投射到 y=0 地面，返回命中点（无交点返回 None）
fn ground_hit(camera: &Camera, cam_xform: &GlobalTransform, cursor: Vec2) -> Option<Vec3> {
    let ray = camera.viewport_to_world(cam_xform, cursor).ok()?;
    // 与 y=0 平面求交：origin.y + t * dir.y = 0
    if ray.direction.y.abs() < 1e-6 {
        return None;
    }
    let t = -ray.origin.y / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    Some(ray.origin + ray.direction * t)
}

fn handle_input(
    windows: Query<&Window>,
    camera: Query<&Camera>,
    camera_xform: Query<&GlobalTransform, With<Camera>>,
    local_player_transform: Res<LocalPlayerTransform>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    mut input_state: ResMut<PlayerInputState>,
    mut move_target: ResMut<MoveTarget>,
    mut selected: ResMut<SelectedTarget>,
    heroes: Query<(Entity, &Transform, &NetId, &Selectable)>,
    egui_ctx: Option<Res<EguiContexts>>,
) {
    let player_pos = local_player_transform.0.translation;

    // --- 1. 键盘方向（优先级最高，按下时取消点击移动）---
    // 相机固定俯视 -z 方向，故屏幕上方 = 世界 -z（前进/远离相机），屏幕下方 = +z（后退）。
    // W/↑ 对应世界 -z，S/↓ 对应 +z。
    let mut keyboard_dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        keyboard_dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        keyboard_dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        keyboard_dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        keyboard_dir.x += 1.0;
    }
    let keyboard_active = keyboard_dir.length_squared() > 0.0;
    if keyboard_active {
        move_target.0 = None;
    }

    // --- 2. 左键点击地面：设置移动目标（点 UI 时不触发）---
    let over_ui = egui_ctx
        .map_or(false, |ctx| ctx.ctx_mut().map_or(false, |c| c.is_pointer_over_egui()));
    if !over_ui && mouse_buttons.just_pressed(MouseButton::Left) {
        if let (Ok(window), Ok(camera), Ok(cam_xform)) =
            (windows.single(), camera.single(), camera_xform.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Some(hit) = ground_hit(camera, cam_xform, cursor) {
                    move_target.0 = Some(Vec2::new(hit.x, hit.z));
                }
            }
        }
    }

    // --- 3. 计算最终移动方向 ---
    let move_dir = if keyboard_active {
        keyboard_dir.normalize()
    } else if let Some(target) = move_target.0 {
        let to_target = Vec2::new(target.x - player_pos.x, target.y - player_pos.z);
        // 到达阈值内即停下，避免抖动
        if to_target.length() < 0.4 {
            move_target.0 = None;
            Vec2::ZERO
        } else {
            to_target.normalize()
        }
    } else {
        Vec2::ZERO
    };
    input_state.move_dir = move_dir;

    // --- 4. 技能按键 ---
    // bit0 预留给普攻（右键触发）。技能占用 bit1..=bit4，对应 4 个技能槽。
    // 为避免与方向键（WASD/方向键）冲突且不做重复绑定，仅用 Q/E/R/F。
    let mut ability_mask = 0u8;
    if keys.pressed(KeyCode::KeyQ) {
        ability_mask |= 0b0010; // 槽 0
    }
    if keys.pressed(KeyCode::KeyE) {
        ability_mask |= 0b0100; // 槽 1
    }
    if keys.pressed(KeyCode::KeyR) {
        ability_mask |= 0b1000; // 槽 2
    }
    if keys.pressed(KeyCode::KeyF) {
        ability_mask |= 0b0001_0000; // 槽 3
    }
    // 右键 = 普攻（bit0）
    if mouse_buttons.pressed(MouseButton::Right) {
        ability_mask |= 0b0001;
    }
    input_state.ability_mask = ability_mask;

    // --- 7. 右键敌人：锁定攻击目标（显示选中环，并让瞄准方向指向目标）---
    if !over_ui && mouse_buttons.just_pressed(MouseButton::Right) {
        if let (Ok(window), Ok(cam), Ok(cam_xform)) =
            (windows.single(), camera.single(), camera_xform.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Some(hit) = ground_hit(cam, cam_xform, cursor) {
                    let mut best: Option<NetId> = None;
                    let mut best_d = f32::MAX;
                    for (_, htr, net_id, sel) in heroes.iter() {
                        let d = (Vec2::new(hit.x, hit.z)
                            - Vec2::new(htr.translation.x, htr.translation.z))
                            .length();
                        // 命中目标本体（半径）或附近 3 单位内即可选中
                        if d < sel.radius + 3.0 && d < best_d {
                            best_d = d;
                            best = Some(*net_id);
                        }
                    }
                    selected.0 = best;
                    if let Some(t) = best {
                        info!("锁定攻击目标: {:?}", t);
                    }
                }
            }
        }
    }

    // --- 5. 瞄准方向：鼠标指向的地面点 ---
    if let (Ok(window), Ok(camera), Ok(cam_xform)) =
        (windows.single(), camera.single(), camera_xform.single())
    {
        if let Some(cursor) = window.cursor_position() {
            if let Some(hit) = ground_hit(camera, cam_xform, cursor) {
                let aim = Vec2::new(hit.x - player_pos.x, hit.z - player_pos.z);
                input_state.aim_dir = aim.normalize_or_zero();
            }
        }
    }

    // --- 6. 触摸（移动端虚拟摇杆，桌面端无触摸时不影响）---
    // 仅在有触摸时才覆盖移动方向，避免每次都把 move_dir 清零
    if touches.iter().count() > 0 {
        let screen_width = window_width(&windows);
        let screen_height = window_height(&windows);
        for touch in touches.iter() {
            let pos = touch.position();
            if pos.x < screen_width * 0.5 {
                let center = Vec2::new(screen_width * 0.25, screen_height * 0.75);
                let dir = (pos - center).normalize_or_zero();
                input_state.move_dir = dir.clamp_length_max(1.0);
                move_target.0 = None;
            } else {
                let aim_dir = (pos - Vec2::new(screen_width * 0.5, screen_height * 0.5))
                    .normalize_or_zero();
                input_state.aim_dir = aim_dir;
            }
        }
    }
}

fn window_width(windows: &Query<&Window>) -> f32 {
    windows.single().map(|w| w.resolution.width()).unwrap_or(1280.0)
}

fn window_height(windows: &Query<&Window>) -> f32 {
    windows.single().map(|w| w.resolution.height()).unwrap_or(720.0)
}

fn update_egui_input(mut contexts: EguiContexts, input_state: Res<PlayerInputState>) {
    // 无 egui 上下文（如 headless 测试环境未加载 EguiPlugin）时跳过绘制
    let Some(ctx) = contexts.ctx_mut().ok() else { return };

    // 调试面板显示当前输入状态
    egui::Window::new("Input Debug").show(ctx, |ui| {
        ui.label(format!(
            "Move: {:.2}, {:.2}",
            input_state.move_dir.x, input_state.move_dir.y
        ));
        ui.label(format!(
            "Aim: {:.2}, {:.2}",
            input_state.aim_dir.x, input_state.aim_dir.y
        ));
        ui.label(format!("Ability Mask: {:04b}", input_state.ability_mask));
        ui.label("左键点地=移动 · WASD=方向 · 鼠标=瞄准 · Q/E/R/F=技能 · 右键=普攻");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{Camera, PerspectiveProjection, Projection};
    use bevy::math::Vec3A;

    /// 构造一个 Projection 矩阵齐全的相机（仅设置 computed.clip_from_view）
    fn make_test_camera() -> Camera {
        let size = Vec2::new(1600.0, 900.0);
        let mut camera = Camera::default();
        let mut projection = Projection::Perspective(PerspectiveProjection::default());
        projection.update(size.x, size.y);
        camera.computed.clip_from_view = projection.get_clip_from_view();
        camera
    }

    /// 复刻 viewport_to_world + y=0 平面求交（避免依赖私有类型）
    fn test_ground_hit(
        camera: &Camera,
        cam_xform: &GlobalTransform,
        cursor: Vec2,
        w: f32,
        h: f32,
    ) -> Option<Vec3> {
        let mut ndc = (cursor / Vec2::new(w, h)) * 2.0 - Vec2::ONE;
        ndc.y = -ndc.y;
        let view_from_clip = camera.computed.clip_from_view.inverse();
        let world_from_view = cam_xform.affine();
        let vp_near = view_from_clip.project_point3a(Vec3A::new(ndc.x, ndc.y, 1.0));
        let vp_far = view_from_clip.project_point3a(Vec3A::new(ndc.x, ndc.y, 1e-6));
        let view_dir = vp_far - vp_near;
        let origin: Vec3 = world_from_view.transform_point3a(vp_near).into();
        let direction: Vec3 = world_from_view.transform_vector3a(view_dir).into();
        if direction.y.abs() < 1e-6 {
            return None;
        }
        let t = -origin.y / direction.y;
        if t <= 0.0 {
            return None;
        }
        Some(origin + direction * t)
    }

    #[test]
    fn ground_hit_varies_between_top_and_bottom() {
        let camera = make_test_camera();
        let cam_xform =
            GlobalTransform::from(Transform::from_xyz(0.0, 30.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y));
        let (w, h) = (1600.0, 900.0);

        let cases = [
            ("center", Vec2::new(w / 2.0, h / 2.0)),
            ("top", Vec2::new(w / 2.0, 50.0)),
            ("bottom", Vec2::new(w / 2.0, h - 50.0)),
            ("left", Vec2::new(50.0, h / 2.0)),
            ("right", Vec2::new(w - 50.0, h / 2.0)),
        ];

        for (name, cursor) in cases {
            let hit = test_ground_hit(&camera, &cam_xform, cursor, w, h);
            eprintln!(
                "{name:>6}: {:?}",
                hit.map(|v| (round(v.x), round(v.y), round(v.z)))
            );
        }
    }

    fn round(v: f32) -> f32 {
        (v * 100.0).round() / 100.0
    }

    #[test]
    fn handle_input_translates_keys_to_move_dir() {
        use bevy::prelude::*;
        use bevy::input::ButtonInput;

        let mut world = World::new();
        world.init_resource::<PlayerInputState>();
        world.init_resource::<MoveTarget>();
        world.init_resource::<SelectedTarget>();
        world.init_resource::<LocalPlayerTransform>();

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        world.insert_resource(keys);
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(Touches::default());

        world.run_system(handle_input).expect("handle_input 应可运行");

        let state = world.resource::<PlayerInputState>();
        assert_eq!(state.move_dir, Vec2::new(0.0, -1.0), "W 应映射为世界 -z 移动");
    }
}
