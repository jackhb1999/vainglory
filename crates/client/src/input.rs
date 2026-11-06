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
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::{LocalPlayer, NetId, Selectable};
use crate::rendering::{HitFlash, TargetMarker};

/// 当前锁定的攻击目标（net id），右键敌人时设置，用于显示选中环。
#[derive(Resource, Default)]
pub struct SelectedTarget(pub Option<NetId>);

/// 技能瞄准 / 目标选取状态：按下 Q/E/R/F 进入对应技能槽的瞄准模式。
/// `target_point` 为光标在地面上的命中点（用于范围指示），
/// `hovered` 为射程内、光标附近的可选实体（用于高亮），点击即对该目标施放。
#[derive(Resource, Default)]
pub struct TargetingState {
    pub slot: Option<u8>,
    pub target_point: Option<Vec3>,
    pub hovered: Option<NetId>,
}

/// 一次已确认的施放请求。点击确认后产生，`send_input` 连续发送若干帧，
/// 保证服务端 `ability_cast_system` 至少在一个 tick 收到该次施放。
#[derive(Resource, Default)]
pub struct PendingCast {
    pub slot: Option<u8>,
    pub aim: Vec2,
    pub frames: u8,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInputState>()
            .init_resource::<LocalPlayerTransform>()
            .init_resource::<MoveTarget>()
            .init_resource::<SelectedTarget>()
            .init_resource::<TargetingState>()
            .init_resource::<PendingCast>()
            .add_systems(Startup, (spawn_move_target_marker, spawn_cursor_marker))
            .add_systems(
                Update,
                (
                    update_local_player_transform,
                    handle_input,
                    update_move_target_marker,
                    update_cursor_marker,
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
        // 平铺在地面上（Torus 默认即躺在 XZ 平面，无需额外旋转）
        Transform::from_xyz(0.0, 0.2, 0.0),
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

/// 跟随鼠标在地面上的光标高亮（青色细环）。
/// 用于诊断“攻击没落在鼠标点击处”到底是【鼠标位置没感知对】还是【释放逻辑算错】：
/// - 高亮正确跟随鼠标 → 鼠标感知 OK，问题在释放逻辑；
/// - 高亮不动 / 明显错位 → 鼠标坐标→世界坐标的射线投射(ground_hit)有问题。
#[derive(Component)]
pub struct CursorMarker;

fn spawn_cursor_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Torus {
            major_radius: 0.9,
            minor_radius: 0.12,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.9, 1.0),
            emissive: Color::srgb(0.1, 0.6, 0.8).into(),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.06, 0.0),
        CursorMarker,
    ));
}

/// 每帧把光标高亮放到“鼠标在地面上的命中点”，与点击移动/技能落点共用同一套 ground_hit。
fn update_cursor_marker(
    windows: Query<&Window>,
    camera: Query<&Camera>,
    camera_xform: Query<&GlobalTransform, With<Camera>>,
    mut marker: Query<
        &mut Transform,
        (
            With<CursorMarker>,
            Without<MoveTargetMarker>,
            Without<HitFlash>,
            Without<TargetMarker>,
            Without<Camera3d>,
        ),
    >,
) {
    if let Ok(mut transform) = marker.single_mut() {
        if let (Ok(window), Ok(camera), Ok(cam_xform)) =
            (windows.single(), camera.single(), camera_xform.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Some(hit) = ground_hit(camera, cam_xform, cursor) {
                    transform.translation = Vec3::new(hit.x, 0.06, hit.z);
                }
            }
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
    /// 锁定的普攻目标（NetId）。右键敌人时设置，普攻/自动攻击优先打它。
    pub target_net_id: Option<NetId>,
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
    mut targeting: ResMut<TargetingState>,
    mut pending: ResMut<PendingCast>,
    heroes: Query<(Entity, &Transform, &NetId, &Selectable)>,
    egui_ctx: Option<NonSendMut<EguiContexts>>,
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

    // --- 2. 左键：瞄准模式下确认施放；否则点击移动（点 UI 时不触发）---
    let over_ui = egui_ctx
        .map_or(false, |mut ctx| ctx.ctx_mut().map_or(false, |c| c.is_pointer_over_egui()));
    if !over_ui && mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(slot) = targeting.slot {
            // 确认施放：朝悬停敌人（优先）或光标地面点
            let mut aim = Vec2::ZERO;
            if let Some(hit) = targeting.target_point {
                let point = if let Some(nid) = targeting.hovered {
                    heroes
                        .iter()
                        .find(|(_, _, n, _)| **n == nid)
                        .map(|(_, t, _, _)| t.translation)
                        .unwrap_or(hit)
                } else {
                    hit
                };
                // 记录鼠标的绝对地面世界坐标（x,z），由服务端按射程夹紧后作为落点
                aim = Vec2::new(point.x, point.z);
            }
            pending.slot = Some(slot);
            pending.aim = aim;
            pending.frames = 3; // 连续发送若干帧，确保服务端收到
            targeting.slot = None;
        } else if let (Ok(window), Ok(camera), Ok(cam_xform)) =
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

    // --- 4. 技能按键：进入“目标选取”瞄准模式 ---
    // 按 Q/E/R/F 进入对应技能槽的瞄准模式（再按一次或 Esc 取消）。
    // 瞄准模式会出现范围指示环与可攻击目标高亮；左键确认后由 `PendingCast` 触发施放。
    // （不再用按住即放的方式，避免无目标时把技能打空且没有任何反馈。）
    let pressed_q = keys.just_pressed(KeyCode::KeyQ);
    let pressed_e = keys.just_pressed(KeyCode::KeyE);
    let pressed_r = keys.just_pressed(KeyCode::KeyR);
    let pressed_f = keys.just_pressed(KeyCode::KeyF);
    let pressed_esc = keys.just_pressed(KeyCode::Escape);

    if pressed_esc {
        targeting.slot = None;
    }
    let entered = if pressed_q {
        Some(0)
    } else if pressed_e {
        Some(1)
    } else if pressed_r {
        Some(2)
    } else if pressed_f {
        Some(3)
    } else {
        None
    };
    if let Some(slot) = entered {
        // 同一技能键再次按下 = 取消瞄准
        targeting.slot = if targeting.slot == Some(slot) {
            None
        } else {
            Some(slot)
        };
    }

    // 计算光标地面命中点与射程内悬停目标（供范围指示与确认施放使用）
    targeting.target_point = None;
    targeting.hovered = None;
    if let Some(slot) = targeting.slot {
        if let (Ok(window), Ok(camera), Ok(cam_xform)) =
            (windows.single(), camera.single(), camera_xform.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Some(hit) = ground_hit(camera, cam_xform, cursor) {
                    targeting.target_point = Some(hit);
                    let range = get_ability_def(default_hero_abilities()[slot as usize]).range;
                    let player_xz = Vec2::new(player_pos.x, player_pos.z);
                    if (Vec2::new(hit.x, hit.z) - player_xz).length() <= range + 4.0 {
                        let mut best: Option<NetId> = None;
                        let mut best_d = f32::MAX;
                        for (_, htr, net_id, sel) in heroes.iter() {
                            let d = (Vec2::new(hit.x, hit.z)
                                - Vec2::new(htr.translation.x, htr.translation.z))
                                .length();
                            if d < sel.radius + 3.0 && d < best_d {
                                best_d = d;
                                best = Some(*net_id);
                            }
                        }
                        targeting.hovered = best;
                    }
                }
            }
        }
    }

    // 本帧的 ability_mask 由 PendingCast 驱动（send_input 读取），这里不再自动置位
    input_state.ability_mask = 0;

    // --- 7. 右键敌人：锁定普攻目标（显示选中环 + 朝它移动 + 持续普攻）---
    if !over_ui && mouse_buttons.just_pressed(MouseButton::Right) {
        if let (Ok(window), Ok(cam), Ok(cam_xform)) =
            (windows.single(), camera.single(), camera_xform.single())
        {
            if let Some(cursor) = window.cursor_position() {
                if let Some(hit) = ground_hit(cam, cam_xform, cursor) {
                    let mut best: Option<NetId> = None;
                    let mut best_pos: Option<Vec3> = None;
                    let mut best_d = f32::MAX;
                    for (_, htr, net_id, sel) in heroes.iter() {
                        let d = (Vec2::new(hit.x, hit.z)
                            - Vec2::new(htr.translation.x, htr.translation.z))
                            .length();
                        // 命中目标本体（半径）或附近 3 单位内即可选中
                        if d < sel.radius + 3.0 && d < best_d {
                            best_d = d;
                            best = Some(*net_id);
                            best_pos = Some(htr.translation);
                        }
                    }
                    selected.0 = best;
                    // 锁定目标 → 持续普攻它；点空地 → 取消锁定、停止普攻
                    input_state.target_net_id = best;
                    match best_pos {
                        Some(p) => move_target.0 = Some(Vec2::new(p.x, p.z)),
                        None => move_target.0 = None,
                    }
                    if let Some(t) = best {
                        info!("锁定攻击目标: {:?}", t);
                    }
                }
            }
        }
    }

    // --- 8. 锁定了普攻目标时，置位普攻位（bit0），让服务端 attack_system 持续攻击它 ---
    if input_state.target_net_id.is_some() {
        input_state.ability_mask |= 0b0001;
    }

    // --- 5. 瞄准方向：鼠标指向的地面点 ---
    if let (Ok(window), Ok(camera), Ok(cam_xform)) =
        (windows.single(), camera.single(), camera_xform.single())
    {
        if let Some(cursor) = window.cursor_position() {
            if let Some(hit) = ground_hit(camera, cam_xform, cursor) {
                // 记录鼠标绝对地面世界坐标（x,z），朝向/落点都据此推导
                input_state.aim_dir = Vec2::new(hit.x, hit.z);
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
                // aim_dir 现在表示世界坐标点：以玩家为中心、朝触摸方向延伸 10 单位
                input_state.aim_dir =
                    Vec2::new(player_pos.x, player_pos.z) + aim_dir * 10.0;
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
        world.init_resource::<TargetingState>();
        world.init_resource::<PendingCast>();

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        world.insert_resource(keys);
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(Touches::default());

        let id = world.register_system(handle_input);
        world.run_system(id).expect("handle_input 应可运行");

        let state = world.resource::<PlayerInputState>();
        assert_eq!(state.move_dir, Vec2::new(0.0, -1.0), "W 应映射为世界 -z 移动");
    }
}
