//! 战争迷雾（`GX-ARCH-2026-001` §8.5 VIS-1~9 / `FR-60` / `FR-81`）。
//!
//! **可见源由服务端逐玩家下发**（`ServerMessage::VisionUpdate`），客户端不自行
//! 从渲染实体推断：后者已被视野裁剪过，用它反推视野会把「恰好没收到敌人」误当
//! 「此处不可见」。服务端 `vision_sources` 与实体裁剪共用同一函数，遮罩口径因此
//! 与可见集严格一致。
//!
//! 三档记忆（VIS-9 一期简化）：
//! - `Visible`：当前在视野内 → 不遮挡；
//! - `Explored`：曾经看见、当前不可见 → 半暗（保留地形记忆，不显示动态单位）；
//! - `Unexplored`：从未看见 → 全暗。
//!
//! 地图骨架不动：网格尺寸与 `game_core::map` 的 2:1 长方形同源，草丛遮蔽半径
//! 直接复用 `game_core::visibility::in_brush`。

use bevy::pbr::StandardMaterial;
// `AlphaMode` 由 `bevy::prelude` 从 `bevy_material::prelude` 重导出；
// `bevy::pbr::AlphaMode` 在 0.20 是私有 use，不能显式引入。
use bevy::prelude::*;
use game_core::map::{MAP_H_HALF, MAP_W_HALF};
use game_core::visibility::in_brush;

/// 迷雾网格与 A* 网格同为 64×32，格边长 `MAP_WIDTH / 64 = 7.5`。
pub const FOG_NX: usize = 64;
pub const FOG_NZ: usize = 32;
/// 3D 遮罩拼贴沿用体素地形的 32×16，每块覆盖 2×2 个迷雾格。
pub const FOG_TILES_X: usize = 32;
pub const FOG_TILES_Z: usize = 16;
pub const FOG_CELL_X: f32 = (MAP_W_HALF * 2.0) / FOG_NX as f32;
pub const FOG_CELL_Z: f32 = (MAP_H_HALF * 2.0) / FOG_NZ as f32;
/// 遮罩层贴在地形最高层（waypoint 顶 = 0.225）之上、英雄脚底之下。
pub const FOG_PLANE_Y: f32 = 0.28;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogLevel {
    Unexplored,
    Explored,
    Visible,
}

/// 服务端下发的逐玩家视野快照。
#[derive(Clone, Debug)]
pub struct VisionSnapshot {
    pub sources: Vec<Vec2>,
    pub vision_radius: f32,
    pub brush_reveal_radius: f32,
    pub server_tick: u64,
}

#[derive(Resource, Default)]
pub struct ClientVisionSources(pub Option<VisionSnapshot>);

/// 迷雾状态；`refresh` 是纯逻辑，便于 `TC-FOG` 直接断言。
#[derive(Resource)]
pub struct FogOfWar {
    pub visible: Vec<bool>,
    pub explored: Vec<bool>,
    pub vision_radius: f32,
    pub brush_reveal_radius: f32,
    pub last_server_tick: u64,
    /// 是否开启；便于调试与 headless 夹具关闭遮罩。
    pub enabled: bool,
}

impl Default for FogOfWar {
    fn default() -> Self {
        Self {
            visible: vec![false; FOG_NX * FOG_NZ],
            explored: vec![false; FOG_NX * FOG_NZ],
            vision_radius: 18.0,
            brush_reveal_radius: 6.0,
            last_server_tick: 0,
            enabled: true,
        }
    }
}

impl FogOfWar {
    pub fn index(ix: usize, iz: usize) -> usize {
        iz * FOG_NX + ix
    }

    /// 格中心的世界坐标（x, z）。
    pub fn cell_center(ix: usize, iz: usize) -> Vec2 {
        Vec2::new(
            -MAP_W_HALF + (ix as f32 + 0.5) * FOG_CELL_X,
            -MAP_H_HALF + (iz as f32 + 0.5) * FOG_CELL_Z,
        )
    }

    pub fn level(&self, ix: usize, iz: usize) -> FogLevel {
        let i = Self::index(ix, iz);
        if self.visible[i] {
            FogLevel::Visible
        } else if self.explored[i] {
            FogLevel::Explored
        } else {
            FogLevel::Unexplored
        }
    }

    /// 依据可见源重算当前可见集，并累积「已探明」记忆。
    ///
    /// VIS-5：草丛格用更小的 `brush_reveal_radius` 判定，与服务端对草丛内
    /// 敌方实体的裁剪半径一致 —— 否则会出现「地图亮着但草丛里的敌人收不到」。
    pub fn refresh(&mut self, sources: &[Vec2], vision_radius: f32, brush_reveal_radius: f32) {
        self.vision_radius = vision_radius.max(0.0);
        self.brush_reveal_radius = brush_reveal_radius.max(0.0);
        self.visible.fill(false);
        for iz in 0..FOG_NZ {
            for ix in 0..FOG_NX {
                let c = Self::cell_center(ix, iz);
                let radius = if in_brush([c.x, 0.0, c.y]) {
                    self.brush_reveal_radius
                } else {
                    self.vision_radius
                };
                let r2 = radius * radius;
                let lit = sources
                    .iter()
                    .any(|s| (s.x - c.x).powi(2) + (s.y - c.y).powi(2) <= r2);
                if lit {
                    let i = Self::index(ix, iz);
                    self.visible[i] = true;
                    self.explored[i] = true;
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.visible.fill(false);
        self.explored.fill(false);
        self.last_server_tick = 0;
    }

    /// 3D 遮罩拼贴的档位：块内 2×2 格取**最亮**的一档。
    pub fn tile_level(&self, tx: usize, tz: usize) -> FogLevel {
        let mut best = FogLevel::Unexplored;
        for dz in 0..2 {
            for dx in 0..2 {
                let lv = self.level(tx * 2 + dx, tz * 2 + dz);
                if (lv as u8) > (best as u8) {
                    best = lv;
                }
            }
        }
        best
    }
}

/// 消费服务端视野报文；迟到的旧 tick 直接丢弃，避免遮罩回退闪烁。
pub fn update_fog_of_war(sources: Res<ClientVisionSources>, mut fog: ResMut<FogOfWar>) {
    let Some(snap) = &sources.0 else { return };
    if snap.server_tick <= fog.last_server_tick {
        return;
    }
    fog.refresh(&snap.sources, snap.vision_radius, snap.brush_reveal_radius);
    fog.last_server_tick = snap.server_tick;
}

/// 三档遮罩材质；`Visible` 用 `Visibility::Hidden`，不需要材质。
#[derive(Resource, Default)]
pub struct FogTileMaterials {
    pub unexplored: Option<Handle<StandardMaterial>>,
    pub explored: Option<Handle<StandardMaterial>>,
    pub mesh: Option<Handle<Mesh>>,
}

#[derive(Component)]
pub struct FogTile {
    pub tx: usize,
    pub tz: usize,
}

#[derive(Component)]
pub struct FogPlaneRoot;

/// 进入对局时生成遮罩拼贴。共享 1 个网格 + 2 个材质，避免 512 份资源。
pub fn spawn_fog_tiles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut res: ResMut<FogTileMaterials>,
) {
    let unexplored = materials.add(StandardMaterial {
        base_color: Color::srgba(0.015, 0.02, 0.035, 0.94),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let explored = materials.add(StandardMaterial {
        base_color: Color::srgba(0.02, 0.025, 0.04, 0.62),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let tile_x = (MAP_W_HALF * 2.0) / FOG_TILES_X as f32;
    let tile_z = (MAP_H_HALF * 2.0) / FOG_TILES_Z as f32;
    let mesh = meshes.add(Cuboid::new(tile_x, 0.02, tile_z));
    res.unexplored = Some(unexplored.clone());
    res.explored = Some(explored);
    res.mesh = Some(mesh.clone());

    commands
        .spawn((FogPlaneRoot, Transform::default(), Visibility::default()))
        .with_children(|root| {
            for tz in 0..FOG_TILES_Z {
                for tx in 0..FOG_TILES_X {
                    let x = -MAP_W_HALF + (tx as f32 + 0.5) * tile_x;
                    let z = -MAP_H_HALF + (tz as f32 + 0.5) * tile_z;
                    root.spawn((
                        FogTile { tx, tz },
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(unexplored.clone()),
                        Transform::from_xyz(x, FOG_PLANE_Y, z),
                    ));
                }
            }
        });
}

/// 退出对局清理遮罩，避免回到大厅后仍有黑块。
pub fn despawn_fog_tiles(
    mut commands: Commands,
    tiles: Query<Entity, Or<(With<FogPlaneRoot>, With<FogTile>)>>,
) {
    // 显式列出根节点与拼贴，避免不同 Bevy 版本 `despawn()` 的递归语义差异导致残留。
    for e in tiles.iter() {
        commands.entity(e).despawn();
    }
}

/// 按迷雾档位切换拼贴材质 / 可见性；只在档位变化时写 ECS。
#[allow(clippy::type_complexity)]
pub fn sync_fog_tiles(
    fog: Res<FogOfWar>,
    res: Res<FogTileMaterials>,
    mut tiles: Query<(
        &FogTile,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
) {
    let (Some(unexplored), Some(explored)) = (&res.unexplored, &res.explored) else {
        return;
    };
    let hidden = !fog.enabled;
    for (tile, mut mat, mut vis) in tiles.iter_mut() {
        if hidden {
            *vis = Visibility::Hidden;
            continue;
        }
        match fog.tile_level(tile.tx, tile.tz) {
            FogLevel::Visible => *vis = Visibility::Hidden,
            FogLevel::Explored => {
                *vis = Visibility::Inherited;
                if mat.0 != *explored {
                    mat.0 = explored.clone();
                }
            }
            FogLevel::Unexplored => {
                *vis = Visibility::Inherited;
                if mat.0 != *unexplored {
                    mat.0 = unexplored.clone();
                }
            }
        }
    }
}

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogOfWar>()
            .init_resource::<ClientVisionSources>()
            .init_resource::<FogTileMaterials>()
            .add_systems(OnEnter(AppState::InMatch), spawn_fog_tiles)
            .add_systems(OnExit(AppState::InMatch), despawn_fog_tiles)
            .add_systems(Update, (update_fog_of_war, sync_fog_tiles).chain());
    }
}

use crate::AppState;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_grid_matches_map_extents() {
        assert_eq!(
            FogOfWar::cell_center(0, 0).x,
            -MAP_W_HALF + FOG_CELL_X * 0.5
        );
        assert_eq!(
            FogOfWar::cell_center(FOG_NX - 1, FOG_NZ - 1).x,
            MAP_W_HALF - FOG_CELL_X * 0.5
        );
        // 迷雾格是正方形（地图 2:1、格数也 2:1），否则小地图矩形拼接会出现缝隙
        let dx = FogOfWar::cell_center(1, 0).x - FogOfWar::cell_center(0, 0).x;
        let dz = FogOfWar::cell_center(0, 1).y - FogOfWar::cell_center(0, 0).y;
        assert!((dx - dz).abs() < 1e-3, "迷雾格必须是正方形: {dx} vs {dz}");
    }

    #[test]
    fn visible_circles_are_recorded_as_explored_then_fade_to_explored() {
        let mut fog = FogOfWar::default();
        let src = FogOfWar::cell_center(32, 16);
        fog.refresh(&[src], 10.0, 3.0);
        assert_eq!(fog.level(32, 16), FogLevel::Visible);
        assert_eq!(fog.level(0, 0), FogLevel::Unexplored);

        fog.refresh(&[], 10.0, 3.0);
        assert_eq!(fog.level(32, 16), FogLevel::Explored, "离开后保留地形记忆");
        assert_eq!(fog.level(0, 0), FogLevel::Unexplored);
    }

    #[test]
    fn brush_cells_need_closer_source_than_open_ground() {
        let b = game_core::map::BUSHES
            .iter()
            .find(|b| b.center.x < 0.0)
            .expect("地图须有蓝方草丛");
        let p = Vec2::new(b.center.x, b.center.z);
        // 取离草丛中心最近的格中心（格边长 7.5，不能假设一定落在 2 单位内）
        let (mut hit_ix, mut hit_iz, mut best) = (0usize, 0usize, f32::INFINITY);
        for iz in 0..FOG_NZ {
            for ix in 0..FOG_NX {
                let c = FogOfWar::cell_center(ix, iz);
                let d = (c.x - p.x).hypot(c.y - p.y);
                if d < best {
                    (hit_ix, hit_iz, best) = (ix, iz, d);
                }
            }
        }
        let c = FogOfWar::cell_center(hit_ix, hit_iz);
        assert!(best <= b.radius, "草丛必须覆盖至少一个格中心");
        assert!(in_brush([c.x, 0.0, c.y]), "测试格必须落在草丛内");
        // 8 单位外：普通视野 18 能看见，草丛视野 6 看不见
        let far = c + Vec2::new(8.0, 0.0);
        let mut fog = FogOfWar::default();
        fog.refresh(&[far], 18.0, 6.0);
        assert_eq!(
            fog.level(hit_ix, hit_iz),
            FogLevel::Unexplored,
            "草丛内需要更近的可见源"
        );
        fog.refresh(&[c + Vec2::new(2.0, 0.0)], 18.0, 6.0);
        assert_eq!(fog.level(hit_ix, hit_iz), FogLevel::Visible);
    }

    #[test]
    fn stale_vision_packets_are_ignored() {
        let mut app = App::new();
        app.init_resource::<ClientVisionSources>()
            .init_resource::<FogOfWar>()
            .add_systems(Update, update_fog_of_war);
        let src = FogOfWar::cell_center(32, 16);
        app.world_mut().resource_mut::<ClientVisionSources>().0 = Some(VisionSnapshot {
            sources: vec![src],
            vision_radius: 10.0,
            brush_reveal_radius: 4.0,
            server_tick: 9,
        });
        app.update();
        assert_eq!(
            app.world().resource::<FogOfWar>().level(32, 16),
            FogLevel::Visible
        );
        // 旧 tick 的空视野不得把遮罩打回全黑
        app.world_mut().resource_mut::<ClientVisionSources>().0 = Some(VisionSnapshot {
            sources: Vec::new(),
            vision_radius: 10.0,
            brush_reveal_radius: 4.0,
            server_tick: 8,
        });
        app.update();
        assert_eq!(
            app.world().resource::<FogOfWar>().level(32, 16),
            FogLevel::Visible
        );
        assert_eq!(app.world().resource::<FogOfWar>().last_server_tick, 9);
    }

    #[test]
    fn tile_level_uses_brightest_of_four_cells() {
        let mut fog = FogOfWar::default();
        fog.visible[FogOfWar::index(2, 2)] = true;
        assert_eq!(fog.tile_level(1, 1), FogLevel::Visible);
        assert_eq!(fog.tile_level(0, 0), FogLevel::Unexplored);
    }
}
