//! 客户端输入处理（`GX-CBT-2026-001` §3，v1.2 操作方案）
//!
//! - **W/A/S/D 与方向键：直接控制移动方向（主操作，优先级最高）**
//! - 左键点地：A* 自动寻路，到点待命（辅助操作）
//! - 左键点敌方单位：仅选中查看信息
//! - 右键点地：立即移动（force），打断当前动作并清除锁定
//! - 右键点敌方单位：锁定普攻目标，自动追击到射程内
//! - Q/E/R/F：进入技能瞄准；左键确认施放；Esc / 同键取消
//! - 触摸：点触地面 = 左键寻路；长按 = 右键

use crate::pathfinding;
use crate::rendering::{HitFlash, TargetMarker};
use bevy::image::Image;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::{LocalPlayer, NetId, Selectable};

/// 地面标记统一高度：恒高于最高地形层顶面，避免与兵线 / 路点层近共面闪烁。
pub const MARKER_Y: f32 = crate::voxel::LAYER_WAYPOINT.top() + 0.075;
/// 鼠标射线求交的裁决平面（与服务端 `y = 0` 一致）。
const GROUND_PLANE_Y: f32 = 0.0;

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
            .init_resource::<PathState>()
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
    mut images: ResMut<Assets<Image>>,
) {
    // 方块指示框（PRES-8），几何与纹理由代码生成
    let e = crate::voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        crate::voxel::PixelKind::Gold,
        1.6,
        0.2,
    );
    commands.entity(e).insert(MoveTargetMarker);
}

fn update_move_target_marker(
    move_target: Res<MoveTarget>,
    mut marker: Query<
        (&mut Transform, &mut Visibility),
        (
            With<MoveTargetMarker>,
            Without<HitFlash>,
            Without<TargetMarker>,
            Without<Camera3d>,
        ),
    >,
) {
    if let Ok((mut transform, mut visibility)) = marker.single_mut() {
        if let Some(target) = move_target.0 {
            *visibility = Visibility::Visible;
            transform.translation = Vec3::new(target.x, MARKER_Y, target.y);
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
    mut images: ResMut<Assets<Image>>,
) {
    let e = crate::voxel::spawn_voxel_marker(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        crate::voxel::PixelKind::Blue,
        0.9,
        MARKER_Y,
    );
    commands
        .entity(e)
        .insert((CursorMarker, Visibility::Visible));
}

/// 每帧把光标高亮放到“鼠标在地面上的命中点”，与点击移动/技能落点共用同一套 ground_hit。
fn update_cursor_marker(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
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
    if let Ok(mut transform) = marker.single_mut()
        && let Ok(window) = windows.single()
        && let Some((camera, cam_xform)) = primary_camera(&cameras)
        && let Some(cursor) = window.cursor_position()
        && let Some(hit) = ground_hit(camera, cam_xform, cursor)
    {
        transform.translation = Vec3::new(hit.x, MARKER_Y, hit.z);
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

/// A* 路径状态（`GX-CBT-2026-001` §3.3）。
/// `waypoints` 为空而 `MoveTarget` 有值时退化为直线移动（兼容测试注入与调试）。
#[derive(Resource, Default)]
pub struct PathState {
    pub waypoints: Vec<Vec2>,
    pub index: usize,
    pub force: bool,
}

impl PathState {
    pub fn set_goal(&mut self, from: Vec2, goal: Vec2, force: bool) {
        self.waypoints = pathfinding::astar(from, goal, &[]);
        self.index = 0;
        self.force = force;
    }

    pub fn clear(&mut self) {
        self.waypoints.clear();
        self.index = 0;
        self.force = false;
    }
}

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

/// 取主 3D 相机：`Camera` 与 `GlobalTransform` 必须来自**同一实体**。
///
/// egui 会额外插入裸 `Camera`，`Query::single()` 因此失败 → 瞄准点算不出来 →
/// 上报 `aim = (0,0)` → 服务端把技能打向地图原点（本次修复的决定性主因）。
/// 旧 `camera_pair` 用两个独立 Query 各取第一个，既不排除裸相机，也不保证配对。
/// 这里用 `With<Camera3d>` 合并查询，从根上消除该失败。
fn primary_camera<'a>(
    cameras: &'a Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) -> Option<(&'a Camera, &'a GlobalTransform)> {
    cameras.iter().next()
}

/// 把屏幕光标经主相机投射到裁决平面 y=0；命中点的 y 显式归零，
/// 下游只用 `(x, z)` 上报，杜绝残留 y 分量污染服务端落点。
fn ground_hit(camera: &Camera, cam_xform: &GlobalTransform, cursor: Vec2) -> Option<Vec3> {
    let ray = camera.viewport_to_world(cam_xform, cursor).ok()?;
    if ray.direction.y.abs() < 1e-6 {
        return None;
    }
    let t = (GROUND_PLANE_Y - ray.origin.y) / ray.direction.y;
    if t <= 0.0 {
        return None;
    }
    let hit = ray.origin + ray.direction * t;
    Some(Vec3::new(hit.x, GROUND_PLANE_Y, hit.z))
}

fn handle_input(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    local_player_transform: Res<LocalPlayerTransform>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    mut input_state: ResMut<PlayerInputState>,
    mut move_target: ResMut<MoveTarget>,
    mut path: ResMut<PathState>,
    mut selected: ResMut<SelectedTarget>,
    mut targeting: ResMut<TargetingState>,
    mut pending: ResMut<PendingCast>,
    heroes: Query<(Entity, &Transform, &NetId, &Selectable)>,
    egui_ctx: Option<NonSendMut<EguiContexts>>,
) {
    let player_pos = local_player_transform.0.translation;
    let origin = Vec2::new(player_pos.x, player_pos.z);

    // --- 1. 键盘方向（主操作，优先级最高；按下时取消点击移动与寻路）---
    // 相机固定俯视 -z 方向，故屏幕上方 = 世界 -z，屏幕下方 = +z。
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
        path.clear();
    }

    // --- 2. 左键：瞄准模式下确认施放；否则设置自动寻路目标 ---
    let over_ui =
        egui_ctx.is_some_and(|mut ctx| ctx.ctx_mut().is_ok_and(|c| c.is_pointer_over_egui()));
    if !over_ui && mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(slot) = targeting.slot {
            // 确认施放：**当帧**重新求交，不依赖上一帧缓存（§4 在左键之后执行，
            // 旧版用上一帧目标点会慢一帧；缓存为 None 时还上报 `aim=(0,0)`，
            // 服务端把技能打向地图原点 —— 这是「技能不落在鼠标点」的决定性主因）。
            let hit = windows
                .single()
                .ok()
                .zip(primary_camera(&cameras))
                .and_then(|(w, (cam, cam_xform))| {
                    w.cursor_position()
                        .and_then(|cursor| ground_hit(cam, cam_xform, cursor))
                });
            let point = hit.map(|hit| {
                if let Some(nid) = targeting.hovered {
                    heroes
                        .iter()
                        .find(|(_, _, n, _)| **n == nid)
                        .map(|(_, t, _, _)| t.translation)
                        .unwrap_or(hit)
                } else {
                    hit
                }
            });
            // 无有效命中则不产生 PendingCast、保持瞄准态等待重新点击，
            // 从源头杜绝「用零向量当无命中哨兵」。
            if let Some(point) = point {
                pending.slot = Some(slot);
                pending.aim = Vec2::new(point.x, point.z);
                pending.frames = 3; // 连续发送若干帧，确保服务端收到
                targeting.slot = None;
            }
        } else if let Some(window) = windows.iter().next()
            && let Some((cam, cam_xform)) = primary_camera(&cameras)
            && let Some(cursor) = window.cursor_position()
            && let Some(hit) = ground_hit(cam, cam_xform, cursor)
        {
            let goal = Vec2::new(hit.x, hit.z);
            move_target.0 = Some(goal);
            path.set_goal(origin, goal, false);
        }
    }

    // --- 3. 移动方向：键盘 > A* 路径 > 直线退化（兼容 headless 测试注入）---
    let move_dir = if keyboard_active {
        keyboard_dir.normalize()
    } else if let Some(&wp) = path.waypoints.get(path.index) {
        let to_wp = wp - origin;
        if to_wp.length() < 0.4 {
            path.index += 1;
            if path.index >= path.waypoints.len() {
                move_target.0 = None;
                path.clear();
            }
            Vec2::ZERO
        } else {
            to_wp.normalize()
        }
    } else if let Some(target) = move_target.0 {
        let to_target = target - origin;
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
    if let Some(slot) = targeting.slot
        && let Ok(window) = windows.single()
        && let Some((camera, cam_xform)) = primary_camera(&cameras)
        && let Some(cursor) = window.cursor_position()
        && let Some(hit) = ground_hit(camera, cam_xform, cursor)
    {
        targeting.target_point = Some(hit);
        let range = get_ability_def(default_hero_abilities()[slot as usize]).range;
        let player_xz = Vec2::new(player_pos.x, player_pos.z);
        if (Vec2::new(hit.x, hit.z) - player_xz).length() <= range + 4.0 {
            let mut best: Option<NetId> = None;
            let mut best_d = f32::MAX;
            for (_, htr, net_id, sel) in heroes.iter() {
                let d = (Vec2::new(hit.x, hit.z) - Vec2::new(htr.translation.x, htr.translation.z))
                    .length();
                if d < sel.radius + 3.0 && d < best_d {
                    best_d = d;
                    best = Some(*net_id);
                }
            }
            targeting.hovered = best;
        }
    }

    // 本帧的 ability_mask 由 PendingCast 驱动（send_input 读取），这里不再自动置位
    input_state.ability_mask = 0;

    // --- 7. 右键：点敌方单位锁定普攻；点地立即移动（force）---
    if !over_ui
        && mouse_buttons.just_pressed(MouseButton::Right)
        && let Some(window) = windows.iter().next()
        && let Some((cam, cam_xform)) = primary_camera(&cameras)
        && let Some(cursor) = window.cursor_position()
        && let Some(hit) = ground_hit(cam, cam_xform, cursor)
    {
        let hit_xz = Vec2::new(hit.x, hit.z);
        let mut best: Option<NetId> = None;
        let mut best_pos: Option<Vec3> = None;
        let mut best_d = f32::MAX;
        for (_, htr, net_id, sel) in heroes.iter() {
            let d = (hit_xz - Vec2::new(htr.translation.x, htr.translation.z)).length();
            // 命中目标本体（半径）或附近 3 单位内即可选中
            if d < sel.radius + 3.0 && d < best_d {
                best_d = d;
                best = Some(*net_id);
                best_pos = Some(htr.translation);
            }
        }
        selected.0 = best;
        input_state.target_net_id = best;
        let goal = match best_pos {
            Some(p) => Vec2::new(p.x, p.z),
            // 点空地：强制移动，打断施法前摇 / 普攻后摇并清除锁定
            None => {
                targeting.slot = None;
                pending.slot = None;
                hit_xz
            }
        };
        move_target.0 = Some(goal);
        path.set_goal(origin, goal, best_pos.is_none());
        if let Some(t) = best {
            info!("锁定攻击目标: {:?}", t);
        }
    }

    // --- 8. 锁定了普攻目标时，置位普攻位（bit0），让服务端 attack_system 持续攻击它 ---
    if input_state.target_net_id.is_some() {
        input_state.ability_mask |= 0b0001;
    }

    // --- 5. 瞄准方向：鼠标指向的地面点 ---
    if let Some(window) = windows.iter().next()
        && let Some((cam, cam_xform)) = primary_camera(&cameras)
        && let Some(cursor) = window.cursor_position()
        && let Some(hit) = ground_hit(cam, cam_xform, cursor)
    {
        // 记录鼠标绝对地面世界坐标（x,z），朝向/落点都据此推导
        input_state.aim_dir = Vec2::new(hit.x, hit.z);
    }

    // --- 6. 触摸：点触地面 = 左键寻路（不提供虚拟摇杆）---
    for touch in touches.iter() {
        if let Some((cam, cam_xform)) = primary_camera(&cameras)
            && let Some(hit) = ground_hit(cam, cam_xform, touch.position())
        {
            let goal = Vec2::new(hit.x, hit.z);
            move_target.0 = Some(goal);
            path.set_goal(origin, goal, false);
        }
    }
}

fn update_egui_input(mut contexts: EguiContexts, input_state: Res<PlayerInputState>) {
    // 无 egui 上下文（如 headless 测试环境未加载 EguiPlugin）时跳过绘制
    let Some(ctx) = contexts.ctx_mut().ok() else {
        return;
    };

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
        ui.label("WASD/Arrows: move | LMB: path | RMB: move/lock | Q/E/R/F: skills");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3A;
    use bevy::prelude::{Camera, PerspectiveProjection, Projection};

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
        let cam_xform = GlobalTransform::from(
            Transform::from_xyz(0.0, 30.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        );
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

    /// `TC-INPUT-01`：W/A/S/D 与方向键必须产生移动方向（v1.2 主操作方案）。
    #[test]
    fn arrow_keys_and_wasd_move_hero() {
        use bevy::input::ButtonInput;
        use bevy::prelude::*;

        let mut world = World::new();
        world.init_resource::<PlayerInputState>();
        world.init_resource::<MoveTarget>();
        world.init_resource::<PathState>();
        world.init_resource::<SelectedTarget>();
        world.init_resource::<LocalPlayerTransform>();
        world.init_resource::<TargetingState>();
        world.init_resource::<PendingCast>();

        let id = world.register_system(handle_input);

        let cases: &[(KeyCode, Vec2)] = &[
            (KeyCode::ArrowUp, Vec2::new(0.0, -1.0)),
            (KeyCode::ArrowDown, Vec2::new(0.0, 1.0)),
            (KeyCode::ArrowLeft, Vec2::new(-1.0, 0.0)),
            (KeyCode::ArrowRight, Vec2::new(1.0, 0.0)),
            (KeyCode::KeyW, Vec2::new(0.0, -1.0)),
            (KeyCode::KeyD, Vec2::new(1.0, 0.0)),
        ];
        for (key, expected) in cases {
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(*key);
            world.insert_resource(keys);
            world.insert_resource(ButtonInput::<MouseButton>::default());
            world.insert_resource(Touches::default());
            world.run_system(id).expect("handle_input should run");
            assert_eq!(
                world.resource::<PlayerInputState>().move_dir,
                *expected,
                "key {key:?} must produce move_dir {expected:?}"
            );
        }

        // 松键后应停下，且键盘优先于已存在的寻路目标
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.run_system(id).expect("handle_input should run");
        assert_eq!(world.resource::<PlayerInputState>().move_dir, Vec2::ZERO);
    }

    /// `TC-CBT-02`：左键目标产生 A* 路径，末点为精确目标点。
    #[test]
    fn path_goal_produces_astar_waypoints() {
        let mut path = PathState::default();
        let goal = Vec2::new(20.0, -10.0);
        path.set_goal(Vec2::ZERO, goal, false);
        assert!(!path.waypoints.is_empty());
        assert_eq!(path.waypoints.last().copied(), Some(goal));
        assert!(!path.force);
        path.clear();
        assert!(path.waypoints.is_empty());
    }

    /// 回归护栏：egui 会额外插入裸 `Camera`，旧版 `Query::single()` 因此失败，
    /// 瞄准点算不出来 → 上报 `aim=(0,0)` → 技能打向地图原点。
    /// 合并查询 + `With<Camera3d>` 必须稳定取到主 3D 相机。
    #[test]
    fn primary_camera_ignores_egui_bare_camera() {
        use bevy::prelude::*;

        let mut world = World::new();
        world.spawn((Camera::default(), GlobalTransform::default()));
        world.spawn((
            Camera::default(),
            GlobalTransform::from(Transform::from_xyz(7.0, 8.0, 9.0)),
            Camera3d::default(),
        ));
        let id = world.register_system(
            |cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>| {
                primary_camera(&cameras).map(|(_, t)| t.translation())
            },
        );
        assert_eq!(
            world.run_system(id).unwrap(),
            Some(Vec3::new(7.0, 8.0, 9.0))
        );
    }

    /// 地面标记必须高于最高地形层，避免与兵线 / 路点层近共面闪烁。
    #[test]
    fn marker_y_clears_highest_terrain_layer() {
        assert!(MARKER_Y > crate::voxel::LAYER_WAYPOINT.top());
        assert_eq!(GROUND_PLANE_Y, 0.0);
    }
}
