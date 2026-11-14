//! 客户端输入处理（`GX-CBT-2026-001` §3，v1.3 操作方案）
//!
//! - **纯点击控制**：左键点地 A* 自动寻路、到点待命；右键点地立即移动（force）
//! - 左键点敌方单位：仅选中查看信息；右键点敌方单位：锁定普攻，自动追击到射程内
//! - 触摸：点触地面 = 左键寻路（不提供虚拟摇杆）
//! - **键盘不再产生移动方向**（v1.2 的 WASD / 方向键直控已移除）：两套移动源
//!   同帧互相抢 `MoveTarget` / A* 路径，是「走路随机乱晃」的主因，现统一收敛到点击寻路
//! - Q/E/R/F：进入技能瞄准；左键确认施放；Esc / 同键取消；P：商店

use crate::AppState;
use crate::lobby::CodexOpen;
use crate::networking::{ClientLobbyInfo, LocalHeroEntity, ReceivedSnapshot};
use crate::pathfinding;
use crate::rendering::{HitFlash, TargetMarker};
use bevy::image::Image;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::{LocalPlayer, NetId, Selectable};
use game_core::navgrid::{NavGrid, NavObstacleSnapshot};

/// 地面标记统一高度：恒高于最高地形层顶面，避免与兵线 / 路点层近共面闪烁。
pub const MARKER_Y: f32 = crate::voxel::LAYER_WAYPOINT.top() + 0.075;
/// 鼠标射线求交的裁决平面（与服务端 `y = 0` 一致）。
const GROUND_PLANE_Y: f32 = 0.0;

/// 商店面板开关（P 键）。旧版是 `lobby_ui::draw_shop` 的 `Local<bool>`，
/// **跨系统不可见** —— `handle_input` 无从得知商店已打开，点购买按钮就穿到地面。
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct ShopOpen(pub bool);

/// 世界输入门禁快照：由 `update_input_gate` 在 `Update`（`handle_input` 之前）算出。
///
/// 不依赖 `EguiPrimaryContextPass` 的 `is_pointer_over_egui()`：那个值来自
/// **上一帧**的 UI 命中布局，而 `handle_input` 跑在 `Update`，商店窗口当帧才建立，
/// 必然慢一拍；egui 主上下文尚未就绪时更是直接返回 false。
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct InputGate {
    pub frozen: bool,
}

/// 世界输入是否应被冻结（单点收敛，便于单测）。
///
/// - `in_match`：大厅 / 尚未进入对局时不接受任何世界输入；
/// - `hero_ready`：`OnEnter(InMatch)` 到首个 `HeroSpawn` 之间本地英雄还没生成，
///   此时 `LocalPlayerTransform` 仍是原点，点击会算出一条从 `(0,0)` 出发的寻路；
/// - `modal_open`：商店 / 图鉴是模态面板，打开时点击绝不能穿透到地面。
pub const fn input_frozen(in_match: bool, hero_ready: bool, modal_open: bool) -> bool {
    !in_match || !hero_ready || modal_open
}

/// 每帧在 `handle_input` 之前计算门禁。P 键切换也从 UI pass 移到这里，
/// 保证「按下 P 的当帧」世界输入就被冻结，而不是等下一次 egui 布局。
pub fn update_input_gate(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut shop: Option<ResMut<ShopOpen>>,
    codex: Option<Res<CodexOpen>>,
    app_state: Option<Res<State<AppState>>>,
    local_hero: Option<Res<LocalHeroEntity>>,
    mut gate: Option<ResMut<InputGate>>,
) {
    if let Some(k) = keys.as_deref()
        && k.just_pressed(KeyCode::KeyP)
        && let Some(s) = shop.as_deref_mut()
    {
        s.0 = !s.0;
    }
    // 资源不存在（headless 最小夹具）时视为「无门禁」，不影响既有输入单测。
    let in_match = app_state
        .as_deref()
        .is_none_or(|s| *s.get() == AppState::InMatch);
    let hero_ready = local_hero.as_deref().is_none_or(|h| h.0.is_some());
    let modal_open = shop.as_deref().is_some_and(|s| s.0) || codex.as_deref().is_some_and(|c| c.0);
    if let Some(g) = gate.as_deref_mut() {
        g.frozen = input_frozen(in_match, hero_ready, modal_open);
    }
}

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
            .init_resource::<ShopOpen>()
            .init_resource::<InputGate>()
            .init_resource::<MoveTarget>()
            .init_resource::<PathState>()
            .init_resource::<ClientNavObstacles>()
            .init_resource::<ClientTick>()
            .init_resource::<SelectedTarget>()
            .init_resource::<TargetingState>()
            .init_resource::<PendingCast>()
            .add_systems(Startup, (spawn_move_target_marker, spawn_cursor_marker))
            // 60Hz 逻辑 tick 供 Periodic / Stuck 计时；不能用渲染帧数或 20Hz 快照 tick。
            .add_systems(FixedUpdate, tick_client_clock)
            .add_systems(
                Update,
                (
                    update_local_player_transform,
                    sync_local_death_state,
                    // 门禁必须先于 handle_input 且与它同帧，不能等 egui pass 的上一帧结果
                    update_input_gate,
                    // 服务端障碍视图 → 客户端 NavGrid；必须早于 handle_input，
                    // 否则 GridChanged 会晚一帧才触发重算。
                    sync_nav_grid,
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

/// waypoint / 目标到达半径：由寻路网格尺寸派生（0.2 格）。
///
/// 旧版硬编码 `0.4`，相对 `CELL_SIZE = 7.5` 单位/格过小，且「到达」的那一帧直接
/// 返回零方向、下一个路径点要等下一帧才生效 —— 每过一个 waypoint 就顿一帧，
/// 与「键盘清路径 → 点击写路径」的交替一起构成走路随机乱晃。随格尺寸派生后，
/// 地图整体等比放大也不会失配。
pub const ARRIVE_RADIUS: f32 = pathfinding::CELL_SIZE * 0.2;

/// 目标变化阈值：小于此值视为同一目标，不产生 `NewGoal`（§24.2 `NAV-M2-02-C6`）。
pub const GOAL_EPS: f32 = 0.5;
/// `Periodic` 重算间隔（tick，C2）。
pub const RECOMPUTE_INTERVAL_TICKS: u64 = 30;
/// `Stuck` 判定：连续 60 tick 位移 < 0.1（C3）。
pub const STUCK_TICKS: u32 = 60;
pub const STUCK_DISPLACEMENT: f32 = 0.1;
/// `Stuck` 重算仍失败后的静默期（C3）。
pub const STUCK_COOLDOWN_TICKS: u64 = 30;

/// 服务端下发的**全局**动态障碍视图（C1：禁止从渲染实体推断）。
#[derive(Resource, Default)]
pub struct ClientNavObstacles(pub Option<NavObstacleSnapshot>);

/// 客户端 60Hz 逻辑 tick；`Periodic` / `Stuck` 以它为时间基准，
/// 不能用渲染帧数或 20Hz 快照 tick（那会把 60 tick 拉成 3 秒）。
#[derive(Resource, Default)]
pub struct ClientTick(pub u64);

pub fn tick_client_clock(mut tick: ResMut<ClientTick>) {
    tick.0 = tick.0.saturating_add(1);
}

/// 用服务端权威障碍视图重建客户端 `NavGrid`。
///
/// 只重建、不更新 `PathState.grid_version`：让下一帧 `should_recompute` 看到
/// `grid.version != state.grid_version` 从而立即触发 `GridChanged`（C2）。
pub fn sync_nav_grid(
    obstacles: Res<ClientNavObstacles>,
    tick: Res<ClientTick>,
    mut path: ResMut<PathState>,
) {
    path.nav_tick = tick.0;
    if let Some(snap) = &obstacles.0 {
        path.grid.rebuild(snap);
    }
}

/// 重算触发原因，声明顺序即优先级（`None < ... < NewGoal`，§24.2 C1）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecomputeReason {
    #[default]
    None,
    Periodic,
    Stuck,
    GridChanged,
    NewGoal,
}

/// 单帧点击产生的路径请求。`version` 仅用于诊断，**不参与裁决**：
/// 网格版本必须由服务端下发，禁止客户端上报（§24.3 C8）。
#[derive(Clone, Copy, Debug)]
pub struct PathRequest {
    pub goal: Vec2,
    pub force: bool,
    pub version: u32,
}

/// §24.2 C1/C2/C3/C6：纯函数决定本 tick 是否需要重算，便于 `TC-NAV-04` 直接断言。
///
/// 优先级 `NewGoal`（含 `force`）> `GridChanged` > `Stuck` > `Periodic` > `None`。
/// 待命（`goal == None`）时不触发 `Periodic`，避免静止英雄每 30 tick 白跑一次 A*。
pub fn should_recompute(
    state: &PathState,
    goal: Option<Vec2>,
    force: bool,
    stuck: bool,
    tick: u64,
) -> RecomputeReason {
    if tick < state.stalled_until_tick {
        return RecomputeReason::None;
    }
    let new_goal =
        force || goal.is_some_and(|g| state.goal.is_none_or(|old| old.distance(g) >= GOAL_EPS));
    if new_goal {
        return RecomputeReason::NewGoal;
    }
    if state.grid.version != state.grid_version {
        return RecomputeReason::GridChanged;
    }
    if stuck {
        return RecomputeReason::Stuck;
    }
    if state.goal.is_some()
        && tick.saturating_sub(state.last_recompute_tick) >= RECOMPUTE_INTERVAL_TICKS
    {
        return RecomputeReason::Periodic;
    }
    RecomputeReason::None
}

/// A* 路径状态（`GX-CBT-2026-001` §3.3 / §24.2）。
/// `waypoints` 为空而 `MoveTarget` 有值时退化为直线移动（兼容测试注入与调试）。
#[derive(Resource, Default)]
pub struct PathState {
    pub waypoints: Vec<Vec2>,
    pub index: usize,
    pub force: bool,
    /// 最近一次规划的目标点。
    pub goal: Option<Vec2>,
    /// 与服务端同源的动态障碍网格（C1/C8）。
    pub grid: NavGrid,
    /// 上次重算时看到的网格版本；与 `grid.version` 不同即 `GridChanged`。
    pub grid_version: u32,
    pub last_recompute_tick: u64,
    pub stuck_ticks: u32,
    pub stalled_until_tick: u64,
    pub last_origin: Vec2,
    /// 本进程累计 A* 调用次数（`TC-NAV-05` 计数断言）。
    pub recompute_calls: u32,
    /// C5：超预算时沿用的上一次有效路径。
    pub last_valid_path: Vec<Vec2>,
    pub nav_tick: u64,
}

impl PathState {
    /// 全量规划到 `goal`（不走节流），网格含静态堤墙 + 服务端动态障碍。
    /// C5 超预算沿用旧路径；C7 按新路径最近点重定位 `index`。
    pub fn set_goal(&mut self, from: Vec2, goal: Vec2, force: bool) {
        let tick = self.nav_tick;
        self.recompute(goal, from, force, tick);
    }

    /// 单请求节流入口（保留给既有调用方 / 单测）。
    pub fn request_goal(&mut self, from: Vec2, goal: Vec2, force: bool) {
        let tick = self.nav_tick;
        let req = PathRequest {
            goal,
            force,
            version: self.grid.version,
        };
        self.process_requests(&[req], from, tick);
    }

    /// §24.2 C1/C5：`handle_input` 帧末统一入口，**单 tick 最多执行一次 A\***。
    /// 同帧注入多个请求时也只规划一次，`force` 任一为真即当帧生效（C4）。
    pub fn process_requests(
        &mut self,
        requests: &[PathRequest],
        from: Vec2,
        tick: u64,
    ) -> RecomputeReason {
        self.nav_tick = tick;
        // C3：连续 60 tick 位移 < 0.1 判卡住；没在移动时不累计。
        let moved = from.distance(self.last_origin);
        self.last_origin = from;
        let wants_move = self.goal.is_some() || !requests.is_empty();
        if wants_move && moved < STUCK_DISPLACEMENT {
            self.stuck_ticks = self.stuck_ticks.saturating_add(1);
        } else {
            self.stuck_ticks = 0;
        }
        let stuck = self.stuck_ticks >= STUCK_TICKS;

        let goal = requests.last().map(|r| r.goal);
        let force = requests.iter().any(|r| r.force);
        let reason = should_recompute(self, goal, force, stuck, tick);
        if reason == RecomputeReason::None {
            return reason;
        }

        if let Some(g) = goal.or(self.goal) {
            if self.recompute(g, from, force, tick) {
                self.stuck_ticks = 0;
            } else if reason == RecomputeReason::Stuck {
                // C3：重算仍失败 → 清路径、move_dir=0、30 tick 内不再触发。
                self.waypoints.clear();
                self.index = 0;
                self.goal = None;
                self.stalled_until_tick = tick + STUCK_COOLDOWN_TICKS;
            }
        } else {
            // 待命时网格变了也没有可重算的路径；消费掉版本差，避免每帧空报
            // `GridChanged` 却什么也不做。下次真正点击时仍会走 `NewGoal`。
            self.grid_version = self.grid.version;
            self.last_recompute_tick = tick;
        }
        reason
    }

    /// 一次 A*；返回是否产出了可用路径。
    fn recompute(&mut self, goal: Vec2, from: Vec2, force: bool, tick: u64) -> bool {
        self.recompute_calls = self.recompute_calls.saturating_add(1);
        let result = pathfinding::find_path(from, goal, &self.grid, &self.last_valid_path);
        self.last_recompute_tick = tick;
        self.grid_version = self.grid.version;
        self.force = force;
        self.goal = Some(goal);
        if result.over_budget {
            // C5：预算耗尽沿用上一次有效路径，不急停
            return !self.waypoints.is_empty();
        }
        if result.path.is_empty() {
            return false;
        }
        self.waypoints = result.path.clone();
        self.last_valid_path = result.path;
        // C7：index 按新路径最近点重新定位，不能无脑归零导致回头走。
        self.index = self
            .waypoints
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.distance(from).total_cmp(&b.distance(from)))
            .map(|(i, _)| i)
            .unwrap_or(0);
        true
    }

    pub fn stalled(&self, tick: u64) -> bool {
        tick < self.stalled_until_tick
    }

    /// 本帧移动方向；静默期内强制归零并清掉目标（C3 / `TC-NAV-06`）。
    pub fn frame_move_dir(&mut self, origin: Vec2, target: &mut MoveTarget, tick: u64) -> Vec2 {
        if self.stalled(tick) {
            target.0 = None;
            return Vec2::ZERO;
        }
        path_move_dir(origin, self, target)
    }

    pub fn clear(&mut self) {
        self.waypoints.clear();
        self.index = 0;
        self.force = false;
        self.goal = None;
        self.stuck_ticks = 0;
        self.stalled_until_tick = 0;
    }
}

/// 计算本帧移动方向并推进路径点；方向始终连续，不会出现「到达帧零向量」。
///
/// 旧实现在 waypoint 进入阈值时 `index += 1` 后直接 `return Vec2::ZERO`，下一个点
/// 要等下一帧才被读取。这里在同一帧内循环跳过所有已进入到达半径的 waypoint，
/// 因此每过一个路径点都不会顿一下。全部走完后清空目标，退化为待命。
fn path_move_dir(origin: Vec2, path: &mut PathState, move_target: &mut MoveTarget) -> Vec2 {
    while let Some(&wp) = path.waypoints.get(path.index) {
        let to_wp = wp - origin;
        if to_wp.length() < ARRIVE_RADIUS {
            path.index += 1;
            continue;
        }
        return to_wp.normalize();
    }
    if !path.waypoints.is_empty() {
        move_target.0 = None;
        path.clear();
        return Vec2::ZERO;
    }
    // 无路径时的直线退化（兼容测试注入 / 调试）：同样用派生阈值，避免终点抖动。
    if let Some(target) = move_target.0 {
        let to_target = target - origin;
        if to_target.length() < ARRIVE_RADIUS {
            move_target.0 = None;
            Vec2::ZERO
        } else {
            to_target.normalize()
        }
    } else {
        Vec2::ZERO
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
    /// 本地英雄是否已阵亡（由权威快照驱动）。为 true 时所有输入被冻结。
    pub dead: bool,
    /// 复活倒计时（秒），仅 HUD 展示用；0 表示未阵亡或即将复活。
    pub respawn_remaining: f32,
}

/// 从权威快照读取本地英雄血量，冻结阵亡期间的一切输入。
///
/// 服务端已对死亡英雄做输入门禁，但客户端若继续上报旧输入，本地预测仍会推着尸体走，
/// 视觉上就是「血掉完人还在、而且还能控制攻击」。这里在输入链最前面统一清零。
pub fn sync_local_death_state(
    snapshot: Option<Res<ReceivedSnapshot>>,
    lobby: Option<Res<ClientLobbyInfo>>,
    time: Res<Time>,
    mut input: ResMut<PlayerInputState>,
    mut move_target: ResMut<MoveTarget>,
    mut path: ResMut<PathState>,
    mut selected: ResMut<SelectedTarget>,
    mut targeting: ResMut<TargetingState>,
    mut pending: ResMut<PendingCast>,
) {
    let Some(snapshot) = snapshot else { return };
    if snapshot.0.tick == 0 {
        return;
    }
    let Some(my_id) = lobby.and_then(|l| l.my_net_id) else {
        return;
    };
    let me = snapshot
        .0
        .entities
        .iter()
        .find(|e| e.entity_type == game_core::net::EntityType::Hero && e.net_id == my_id);
    let dead = me.is_some_and(|e| e.health <= 0.0);

    if dead {
        if !input.dead {
            // 首次阵亡：按等级推算本地倒计时（与服务端 respawn_ticks 同源）。
            let level = me.map(|e| e.level).unwrap_or(1);
            input.respawn_remaining =
                game_core::gameplay::respawn::respawn_ticks(level) as f32 / 60.0;
        } else {
            input.respawn_remaining = (input.respawn_remaining - time.delta_secs()).max(0.0);
        }
        input.dead = true;
        input.move_dir = Vec2::ZERO;
        input.ability_mask = 0;
        input.target_net_id = None;
        move_target.0 = None;
        path.clear();
        selected.0 = None;
        targeting.slot = None;
        pending.slot = None;
    } else {
        input.dead = false;
        input.respawn_remaining = 0.0;
    }
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
    gate: Option<Res<InputGate>>,
    tick: Option<Res<ClientTick>>,
) {
    let player_pos = local_player_transform.0.translation;
    let origin = Vec2::new(player_pos.x, player_pos.z);

    // 世界输入门禁：大厅 / 连接中、本地英雄尚未生成、商店或图鉴打开时冻结全部输入。
    // 阵亡也走这里（与服务端 apply_pending_inputs 的门禁成对）。
    // **必须同时清空残留的 MoveTarget / 路径 / 待发施法**：只 return 不清的话，
    // 打开商店后英雄会沿旧目标继续走，看起来依旧是「点击穿透控制了移动」。
    if gate.as_deref().is_some_and(|g| g.frozen) || input_state.dead {
        input_state.move_dir = Vec2::ZERO;
        input_state.ability_mask = 0;
        input_state.target_net_id = None;
        move_target.0 = None;
        path.clear();
        selected.0 = None;
        targeting.slot = None;
        pending.slot = None;
        return;
    }

    // --- 1. 本帧的点击移动请求（v1.3 纯点击控制）---
    // 键盘不再产生移动方向：旧版「键盘清路径 → 同帧点击写路径 → 下一帧键盘再清」
    // 的交替，加上到达帧零向量，正是走路随机乱晃的两大成因。
    // 三条点击分支只登记请求，帧末统一规划，保证同帧最多执行一次 A*。
    let mut move_request: Option<(Vec2, bool)> = None;

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
            move_request = Some((Vec2::new(hit.x, hit.z), false));
        }
    }

    // 移动方向不在中段计算：所有点击分支登记完请求后，在帧末统一规划并推进路径点
    // （`path_move_dir`），避免「先规划后又被另一条分支覆盖」。

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
        move_request = Some((goal, best_pos.is_none()));
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
    // 旧版完全不判 over_ui，点商店按钮在触屏设备上必然穿透。
    if !over_ui {
        for touch in touches.iter() {
            if let Some((cam, cam_xform)) = primary_camera(&cameras)
                && let Some(hit) = ground_hit(cam, cam_xform, touch.position())
            {
                move_request = Some((Vec2::new(hit.x, hit.z), false));
            }
        }
    }

    // --- 9. 统一规划 + 连续方向（帧末）---
    // 单帧最多执行一次 A*（`NAV-M2-02-C1/C5`）；`process_requests` 按
    // `NewGoal > GridChanged > Stuck > Periodic > None` 裁决一次，右键 `force`
    // 当帧生效。随后循环推进已进入到达半径的 waypoint，方向不会突然归零。
    let tick = tick.map(|t| t.0).unwrap_or(path.nav_tick);
    let requests: Vec<PathRequest> = move_request
        .into_iter()
        .map(|(goal, force)| PathRequest {
            goal,
            force,
            version: path.grid.version,
        })
        .collect();
    for r in &requests {
        move_target.0 = Some(r.goal);
    }
    path.process_requests(&requests, origin, tick);
    // C3：卡住后重算仍失败时，frame_move_dir 内部把 move_dir 归 0 并静默 30 tick。
    input_state.move_dir = path.frame_move_dir(origin, &mut move_target, tick);
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
        ui.label("LMB click: move(path) | RMB: force move / lock | Q/E/R/F: skills | P: shop");
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

    /// `TC-INPUT-01`（v1.3 修订）：**键盘不再产生移动方向**，只有点击目标能驱动移动。
    /// 回归护栏：v1.2 的 WASD / 方向键直控与点击寻路同帧互抢路径，是随机乱晃主因。
    #[test]
    fn keyboard_does_not_move_hero_only_click_target_does() {
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

        let keys = [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
        ];
        for key in keys {
            let mut kb = ButtonInput::<KeyCode>::default();
            kb.press(key);
            world.insert_resource(kb);
            world.insert_resource(ButtonInput::<MouseButton>::default());
            world.insert_resource(Touches::default());
            world.run_system(id).expect("handle_input should run");
            assert_eq!(
                world.resource::<PlayerInputState>().move_dir,
                Vec2::ZERO,
                "key {key:?} must NOT move the hero (v1.3 click-only)"
            );
        }

        // `MoveTarget` 是点击的最终产物：必须产生指向它的连续方向
        world.resource_mut::<MoveTarget>().0 = Some(Vec2::new(20.0, 0.0));
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.run_system(id).expect("handle_input should run");
        assert_eq!(world.resource::<PlayerInputState>().move_dir, Vec2::X);
    }

    /// 到达半径必须由网格尺寸派生；过 waypoint 时方向连续，不再出现零向量帧。
    #[test]
    fn path_move_dir_skips_arrived_waypoints_without_zero_frame() {
        // 派生关系锁定：地图等比放大后到达半径自动跟随，不再硬编码 0.4
        assert!((ARRIVE_RADIUS - crate::pathfinding::CELL_SIZE * 0.2).abs() < 1e-5);

        let mut path = PathState {
            waypoints: vec![Vec2::new(0.5, 0.0), Vec2::new(10.0, 0.0)],
            goal: Some(Vec2::new(10.0, 0.0)),
            ..default()
        };
        let mut target = MoveTarget(Some(Vec2::new(10.0, 0.0)));
        // 第一个点已在到达半径内，必须同帧跳到第二个点并给方向，而不是返回 ZERO
        assert_eq!(path_move_dir(Vec2::ZERO, &mut path, &mut target), Vec2::X);
        assert_eq!(path.index, 1);

        // 全部走完后清空目标与路径，退化为待命
        assert_eq!(
            path_move_dir(Vec2::new(10.0, 0.0), &mut path, &mut target),
            Vec2::ZERO
        );
        assert!(path.waypoints.is_empty());
        assert_eq!(target.0, None);
    }

    /// 重算节流：同目标连点沿用路径；目标明显变化 / `force` 必须立即重算。
    #[test]
    fn request_goal_deduplicates_but_force_always_recomputes() {
        const SENTINEL: Vec2 = Vec2::new(999.0, 999.0);
        let mut path = PathState::default();
        path.request_goal(Vec2::ZERO, Vec2::new(20.0, 0.0), false);
        assert!(!path.waypoints.is_empty(), "首次请求必须规划路径");

        // 同目标（< GOAL_EPS）连点：不重算，哨兵仍在
        path.waypoints.push(SENTINEL);
        path.request_goal(Vec2::ZERO, Vec2::new(20.0, 0.1), false);
        assert_eq!(path.waypoints.last().copied(), Some(SENTINEL));

        // 目标明显变化：立即重算
        path.request_goal(Vec2::ZERO, Vec2::new(-20.0, 5.0), false);
        assert!(!path.waypoints.contains(&SENTINEL));

        // 右键 force：即使目标相同也当帧重算，打断语义不被节流削弱
        path.waypoints.push(SENTINEL);
        path.request_goal(Vec2::ZERO, Vec2::new(-20.0, 5.0), true);
        assert!(!path.waypoints.contains(&SENTINEL));
        assert!(path.force);
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

    /// `input_frozen` 是点击穿透归一的判定核心：
    /// 商店打开 / 大厅 / 英雄未生成三种情况都必须冻结世界输入。
    #[test]
    fn input_frozen_rules() {
        // 对局中 + 英雄就绪 + 无模态 → 可操作
        assert!(!input_frozen(true, true, false));
        // 商店打开（即使对局中、英雄就绪）→ 冻结，点击不再穿透到地面
        assert!(input_frozen(true, true, true));
        // 大厅 / 未进入对局 → 冻结
        assert!(input_frozen(false, true, false));
        // 开局首帧：已进入对局但本地英雄尚未生成 → 冻结
        assert!(input_frozen(true, false, false));
    }

    /// `update_input_gate` 必须把 `ShopOpen` 资源映射到 `InputGate.frozen`，
    /// 否则商店打开时 `handle_input` 仍会处理点击，复现「点商店按钮却移动英雄」。
    #[test]
    fn update_input_gate_maps_shop_open_to_frozen() {
        // 只插最小资源：商店开 → 冻结；关 → 解冻。
        // 其余资源（AppState / LocalHeroEntity）缺省时 `Option<Res<_>>` 为 None，
        // 被 `is_none_or` 视作「在对局中且英雄就绪」，正好构造出「仅看商店开关」的用例。
        let mut world = World::new();
        world.insert_resource(ShopOpen(true));
        world.insert_resource(InputGate::default());
        let id = world.register_system(update_input_gate);
        world.run_system(id).unwrap();
        assert!(world.resource::<InputGate>().frozen);

        let mut world = World::new();
        world.insert_resource(ShopOpen(false));
        world.insert_resource(InputGate::default());
        let id = world.register_system(update_input_gate);
        world.run_system(id).unwrap();
        assert!(!world.resource::<InputGate>().frozen);
    }

    // ------------------------------------------------------------------
    // TC-NAV-04 ~ 06（§24.2 重算节流验收）
    // ------------------------------------------------------------------

    /// `TC-NAV-04`：静止 60 tick 内仅在 grid / 目标变化时返回非 `None`；
    /// `Periodic` 间隔恰为 30 tick。
    #[test]
    fn tc_nav_04_idle_recompute_triggers_and_periodic_interval() {
        let mut state = PathState::default();
        for tick in 0..60 {
            assert_eq!(
                should_recompute(&state, None, false, false, tick),
                RecomputeReason::None,
                "待命 tick {tick} 不应触发重算"
            );
        }

        // 网格版本变化 → 立即 GridChanged（优先于 Periodic）
        state.grid.rebuild(&NavObstacleSnapshot {
            turrets: vec![([0.0, 0.0], true)],
            source_cores: Vec::new(),
            abyss_warden: None,
            server_tick: 1,
        });
        assert_eq!(
            should_recompute(&state, None, false, false, 60),
            RecomputeReason::GridChanged
        );
        state.grid_version = state.grid.version;

        // 新目标 → NewGoal（优先级最高）
        assert_eq!(
            should_recompute(&state, Some(Vec2::new(20.0, 0.0)), false, false, 61),
            RecomputeReason::NewGoal
        );
        state.recompute(Vec2::new(20.0, 0.0), Vec2::ZERO, false, 61);

        assert_eq!(
            should_recompute(&state, None, false, false, 90),
            RecomputeReason::None,
            "90 - 61 = 29 < 30"
        );
        assert_eq!(
            should_recompute(&state, None, false, false, 91),
            RecomputeReason::Periodic,
            "恰好 30 tick 触发"
        );
        // force 右键点地当帧必重算（C4）
        assert_eq!(
            should_recompute(&state, Some(Vec2::new(20.0, 0.0)), true, false, 62),
            RecomputeReason::NewGoal
        );
    }

    /// `TC-NAV-05`：同一 tick 连续注入 5 个请求，A* 实际调用次数为 1；
    /// `force` 请求当帧生效。
    #[test]
    fn tc_nav_05_five_requests_in_one_tick_call_astar_once() {
        let mut state = PathState::default();
        let mut reqs: Vec<PathRequest> = (0..4)
            .map(|i| PathRequest {
                goal: Vec2::new(20.0 + i as f32 * 0.1, 0.0),
                force: false,
                version: 0,
            })
            .collect();
        reqs.push(PathRequest {
            goal: Vec2::new(25.0, 0.0),
            force: true,
            version: 0,
        });
        let before = state.recompute_calls;
        let reason = state.process_requests(&reqs, Vec2::ZERO, 1);
        assert_eq!(reason, RecomputeReason::NewGoal);
        assert_eq!(state.recompute_calls - before, 1, "单 tick 最多一次 A*");
        assert_eq!(state.goal, Some(Vec2::new(25.0, 0.0)));
        assert!(state.force, "force 当帧生效");
        assert!(!state.waypoints.is_empty());
    }

    /// `TC-NAV-06`：模拟卡住 60 tick → `Stuck`；重算仍失败后 `move_dir == 0`
    /// 且 30 tick 内不再触发。
    #[test]
    fn tc_nav_06_stuck_then_silent_for_thirty_ticks() {
        // 全格阻挡 ⇒ A* 起/终点都钳不到可通行格 ⇒ 重算必然失败
        let all: Vec<(i32, i32)> = (0..pathfinding::GRID_NZ)
            .flat_map(|z| (0..pathfinding::GRID_NX).map(move |x| (x, z)))
            .collect();
        let mut state = PathState {
            goal: Some(Vec2::new(20.0, 0.0)),
            grid: NavGrid::with_extra_cells(&all),
            ..default()
        };

        let mut last_reason = RecomputeReason::None;
        for tick in 1..=60 {
            if tick == 60 {
                // 第 60 tick、静默期开启之前：Stuck 必须优先于 Periodic 被选中
                assert_eq!(
                    should_recompute(&state, None, false, true, 60),
                    RecomputeReason::Stuck
                );
            }
            last_reason = state.process_requests(&[], Vec2::ZERO, tick);
        }
        assert_eq!(last_reason, RecomputeReason::Stuck);
        assert!(state.stalled(60), "重算仍失败必须进入静默期");
        assert!(state.waypoints.is_empty());

        let mut target = MoveTarget(Some(Vec2::new(20.0, 0.0)));
        assert_eq!(
            state.frame_move_dir(Vec2::ZERO, &mut target, 60),
            Vec2::ZERO,
            "静默期 move_dir 必须为 0"
        );
        assert_eq!(target.0, None);
        for tick in 61..90 {
            assert_eq!(
                should_recompute(&state, None, false, true, tick),
                RecomputeReason::None,
                "静默 tick {tick} 不应再触发"
            );
        }
    }
}
