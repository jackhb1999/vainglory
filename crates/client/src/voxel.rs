//! Minecraft 式体素表现层（`GX-CBT-2026-001` §16 / PRES-1~9）。
//!
//! **全部几何、纹理、材质由代码在运行时生成，不读取任何外部资源文件。**
//! - 几何：轴对齐立方体（`Cuboid`）拼装，英雄为多关节方块人，地形/建筑为体素堆叠；
//! - 纹理：16×16 程序化像素图，逐像素确定性噪点 + 图案 + 1px 描边 + 伪 AO 面明暗，
//!   由 `visual::pattern_texture_data` 统一生成，`Nearest` 采样（P-V1）；
//! - 调色板：12 色有限方块化调色板，无渐变/抗锯齿；资源键见 `visual::MaterialKey`。

use crate::visual::{FaceShade as F, TexturePattern as P};
use bevy::image::Image;
use bevy::prelude::*;
use game_core::components::Team;
use game_core::map::{
    BLUE_SOURCE_CORE, BUSHES, GOLD_MINE_POS, JUNGLE_CAMPS, JUNGLE_SHOP_POS, LANE_COUNT, LANE_WALLS,
    LANE_WIDTH, MAP_H_HALF, MAP_HEIGHT, MAP_W_HALF, MAP_WIDTH, RED_SOURCE_CORE, RIVER_HALF_WIDTH,
    TRAINING_DUMMY_POS, TURRET_POSITIONS, WALL_HALF_THICKNESS, fold_line_endpoints, is_bridge,
    is_river, lane_waypoints,
};

/// 有限调色板（PRES-5）。每类只有底色与点缀色两种。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelKind {
    Grass,
    Dirt,
    Stone,
    Water,
    Lane,
    Brush,
    Blue,
    Red,
    Gold,
    Core,
    Marker,
    Effect,
}

const PALETTE: [(PixelKind, [u8; 3], [u8; 3]); 12] = [
    (PixelKind::Grass, [86, 156, 70], [104, 174, 82]),
    (PixelKind::Dirt, [134, 96, 67], [155, 116, 82]),
    (PixelKind::Stone, [128, 128, 132], [155, 155, 158]),
    (PixelKind::Water, [52, 104, 190], [72, 134, 214]),
    (PixelKind::Lane, [168, 148, 112], [190, 170, 132]),
    (PixelKind::Brush, [46, 108, 52], [62, 132, 66]),
    (PixelKind::Blue, [46, 96, 214], [78, 132, 240]),
    (PixelKind::Red, [200, 56, 52], [232, 84, 76]),
    (PixelKind::Gold, [226, 184, 62], [246, 210, 92]),
    (PixelKind::Core, [148, 96, 214], [182, 130, 236]),
    (PixelKind::Marker, [240, 240, 240], [255, 255, 255]),
    (PixelKind::Effect, [250, 148, 52], [255, 194, 92]),
];

/// 有限调色板的底色 / 点缀色，供 `visual` 资源库生成纹理使用。
pub fn colors(kind: PixelKind) -> ([u8; 3], [u8; 3]) {
    PALETTE
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, base, accent)| (*base, *accent))
        .unwrap_or(([255, 255, 255], [255, 255, 255]))
}

/// 地形高度层（P-V4）。每层给出中心 `y` 与厚度，顶面 = `y + thickness / 2`。
/// 全部是**纯装饰层**，服务端裁决平面恒为 y=0（英雄可涉水、不被高地阻挡）。
/// 相邻层顶面间隔 ≥ 0.05，配合收紧的相机 near/far，消除随镜头平移的 z-fighting。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainLayer {
    pub y: f32,
    pub thickness: f32,
}

impl TerrainLayer {
    pub const fn top(self) -> f32 {
        self.y + self.thickness * 0.5
    }
}

pub const LAYER_RIVER_BED: TerrainLayer = TerrainLayer {
    y: -0.55,
    thickness: 0.30,
}; // 顶 -0.40
pub const LAYER_WATER: TerrainLayer = TerrainLayer {
    y: -0.30,
    thickness: 0.10,
}; // 顶 -0.25
pub const LAYER_GROUND: TerrainLayer = TerrainLayer {
    y: -0.25,
    thickness: 0.40,
}; // 顶 -0.05
pub const LAYER_FOLD: TerrainLayer = TerrainLayer {
    y: -0.02,
    thickness: 0.07,
}; // 顶  0.015
pub const LAYER_BANK: TerrainLayer = TerrainLayer {
    y: 0.03,
    thickness: 0.14,
}; // 顶  0.10
pub const LAYER_LANE: TerrainLayer = TerrainLayer {
    y: 0.12,
    thickness: 0.07,
}; // 顶  0.155
pub const LAYER_WAYPOINT: TerrainLayer = TerrainLayer {
    y: 0.20,
    thickness: 0.05,
}; // 顶  0.225

/// 有序层表，供生成函数与单测共用（**规范即实现**）。
/// 地面拼贴按 **2:1 长方形**分轴取块数，每格 10 单位：
/// 32 列 × 16 行正好铺满 320×160，且「块宽 == 中心距」不产生水平重叠共面。
pub const GROUND_TILES_X: usize = 32;
pub const GROUND_TILES_Z: usize = 16;

pub const TERRAIN_LAYERS: [TerrainLayer; 7] = [
    LAYER_RIVER_BED,
    LAYER_WATER,
    LAYER_GROUND,
    LAYER_FOLD,
    LAYER_BANK,
    LAYER_LANE,
    LAYER_WAYPOINT,
];

/// 生成 16×16 像素纹理（P-V1）：委托视觉资源库的增强工厂，
/// 逐像素确定性噪点 + 1px 描边 + 伪 AO 面明暗，`Nearest` 采样（PRES-4）。
/// 保留旧签名，使所有既有调用点自动升级到统一风格。
pub fn make_pixel_texture(images: &mut Assets<Image>, kind: PixelKind, seed: u32) -> Handle<Image> {
    crate::visual::make_pattern_texture(
        images,
        kind,
        seed,
        crate::visual::TexturePattern::Speckle,
        crate::visual::FaceShade::Side,
    )
}

/// 生成体素材质（纹理全部来自代码）；默认 `Speckle` + `Side`。
pub fn voxel_material(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    kind: PixelKind,
    seed: u32,
    glow: bool,
) -> Handle<StandardMaterial> {
    voxel_material_keyed(
        materials,
        images,
        crate::visual::MaterialKey::new(kind, seed).with_glow(glow),
    )
}

/// 按 `MaterialKey` 生成材质：供场景 / 道具 / 角色按语义选择图案与明暗。
pub fn voxel_material_keyed(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    key: crate::visual::MaterialKey,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color_texture: Some(crate::visual::make_pattern_texture(
            images,
            key.kind,
            key.seed,
            key.pattern,
            key.face,
        )),
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        metallic: 0.0,
        emissive: if key.glow {
            Color::srgb(0.18, 0.18, 0.18).into()
        } else {
            Color::BLACK.into()
        },
        ..default()
    })
}

/// 语义化快捷入口：按「对象类别」选择图案与明暗，保证风格一致且调用点不散落常量。
fn voxel_mat(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    kind: PixelKind,
    seed: u32,
    glow: bool,
    pattern: P,
    face: F,
) -> Handle<StandardMaterial> {
    voxel_material_keyed(
        materials,
        images,
        crate::visual::MaterialKey::new(kind, seed)
            .with_glow(glow)
            .with_pattern(pattern)
            .with_face(face),
    )
}

/// 骨架静止姿态：动画系统每帧在此基础上叠加，避免累积漂移。
#[derive(Clone, Copy, Debug)]
pub struct HeroRigBase {
    pub torso: Transform,
    pub head: Transform,
    pub left_arm: Transform,
    pub right_arm: Transform,
    pub left_leg: Transform,
    pub right_leg: Transform,
    pub cape: Option<Transform>,
}

/// 英雄分层骨架：可动画节点句柄 + 静止姿态（P-V2）。
#[derive(Component, Clone, Copy, Debug)]
pub struct HeroRig {
    pub torso: Entity,
    pub head: Entity,
    pub right_arm: Entity,
    pub left_arm: Entity,
    pub right_leg: Entity,
    pub left_leg: Entity,
    pub cape: Option<Entity>,
    pub base: HeroRigBase,
}

/// 不同 skin 的身体配色：数据来自 `visual::HERO_VISUALS`，队伍色只作点缀。
fn hero_palette(team: Team, skin: u8) -> (PixelKind, PixelKind, PixelKind, PixelKind) {
    let v = crate::visual::hero_visual(skin);
    let accent = match team {
        Team::Blue => PixelKind::Blue,
        Team::Red => PixelKind::Red,
        Team::Neutral => PixelKind::Gold,
    };
    (v.torso, v.limb, v.head, accent)
}

/// 方块英雄：头 / 躯干 / 双臂 / 双腿 + 头饰 / 武器 / 披风 / 脚环。
///
/// 手臂拆成“肩关节枢轴 + 手臂网格”两层，攻击动画只需旋转枢轴即可挥动手臂与武器。
/// 返回父实体，调用方负责挂载 `NetId` / `Hero` / `LocalPlayer` 等逻辑组件。
#[allow(clippy::too_many_arguments)]
pub fn spawn_voxel_hero(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    team: Team,
    skin: u8,
    position: Vec3,
) -> Entity {
    let (torso_kind, limb_kind, head_kind, accent_kind) = hero_palette(team, skin);
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // 角色：躯干/四肢布纹、头部描边提亮、队伍点缀走发光网格（`docs/06_VISUAL_SPEC.md` §3）。
    let torso_mat = voxel_mat(
        materials,
        images,
        torso_kind,
        11,
        false,
        P::Speckle,
        F::Side,
    );
    let limb_mat = voxel_mat(materials, images, limb_kind, 12, false, P::Speckle, F::Side);
    let head_mat = voxel_mat(
        materials,
        images,
        head_kind,
        13 + skin as u32,
        false,
        P::Border,
        F::Top,
    );
    let accent_mat = voxel_mat(materials, images, accent_kind, 17, true, P::Grid, F::Top);

    // 体型 / 披风 / 肩甲全部来自造型数据表，6 种剪影原型有可辨识的轮廓差。
    let visual = crate::visual::hero_visual(skin);
    let body_scale = visual.body_scale;
    let limb_scale = visual.limb_scale;

    let parent = commands
        .spawn((Transform::from_translation(position), Visibility::default()))
        .id();

    let mut torso_entity = Entity::PLACEHOLDER;
    let mut head_entity = Entity::PLACEHOLDER;
    let mut left_arm = Entity::PLACEHOLDER;
    let mut right_arm = Entity::PLACEHOLDER;
    let mut left_leg = Entity::PLACEHOLDER;
    let mut right_leg = Entity::PLACEHOLDER;
    let mut cape_entity: Option<Entity> = None;

    commands.entity(parent).with_children(|b| {
        // 双腿：髋部枢轴 + 腿方块，动画只旋转枢轴即可摆腿
        left_leg = b
            .spawn((Transform::from_xyz(-0.3, 0.9, 0.0), Visibility::default()))
            .with_children(|leg| {
                leg.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(limb_mat.clone()),
                    Transform::from_xyz(0.0, -0.45, 0.0).with_scale(Vec3::new(0.44, 0.9, 0.5)),
                ));
            })
            .id();
        right_leg = b
            .spawn((Transform::from_xyz(0.3, 0.9, 0.0), Visibility::default()))
            .with_children(|leg| {
                leg.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(limb_mat.clone()),
                    Transform::from_xyz(0.0, -0.45, 0.0).with_scale(Vec3::new(0.44, 0.9, 0.5)),
                ));
            })
            .id();

        // 躯干
        torso_entity = b
            .spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(torso_mat),
                Transform::from_xyz(0.0, 1.45, 0.0).with_scale(body_scale),
            ))
            .id();

        // 肩关节枢轴（空实体），旋转它即可带动整条手臂与武器
        left_arm = b
            .spawn((Transform::from_xyz(-0.7, 1.95, 0.0), Visibility::default()))
            .with_children(|arm| {
                arm.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(limb_mat.clone()),
                    Transform::from_xyz(0.0, -0.5, 0.0).with_scale(limb_scale),
                ));
            })
            .id();
        right_arm = b
            .spawn((Transform::from_xyz(0.7, 1.95, 0.0), Visibility::default()))
            .with_children(|arm| {
                arm.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(limb_mat.clone()),
                    Transform::from_xyz(0.0, -0.5, 0.0).with_scale(limb_scale),
                ));
                spawn_weapon(arm, &cube, &head_mat, &accent_mat, skin);
            })
            .id();

        // 头部：眼睛作为 head 子节点，转身 / 抬头时「脸」始终跟着头（P-V2 朝向修正）。
        head_entity = b
            .spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(head_mat.clone()),
                Transform::from_xyz(0.0, 2.65, 0.0).with_scale(Vec3::splat(0.9)),
            ))
            .with_children(|head| {
                for x in [-0.2_f32, 0.2] {
                    head.spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(accent_mat.clone()),
                        Transform::from_xyz(x, 0.06, 0.52).with_scale(Vec3::splat(0.135)),
                    ));
                }
            })
            .id();

        // 头饰 / 肩甲 / 披风：轮廓由 `HeroVisual` 数据表决定，披风另记句柄供摆动。
        spawn_headgear(b, &cube, &head_mat, &accent_mat, skin);
        if visual.cape {
            cape_entity = Some(
                b.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(accent_mat.clone()),
                    Transform::from_xyz(0.0, 1.35, -0.5).with_scale(Vec3::new(1.0, 1.6, 0.16)),
                ))
                .id(),
            );
        }
        if visual.pauldrons {
            for x in [-0.72_f32, 0.72] {
                b.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(accent_mat.clone()),
                    Transform::from_xyz(x, 2.1, 0.0).with_scale(Vec3::new(0.42, 0.3, 0.6)),
                ));
            }
        }

        // 队伍脚环：4 根扁平方块条，方便在混战中快速分辨敌我
        let ring_kind = match team {
            Team::Blue => PixelKind::Blue,
            Team::Red => PixelKind::Red,
            Team::Neutral => PixelKind::Gold,
        };
        let ring_mat = voxel_material(materials, images, ring_kind, 19, true);
        let side = 1.15_f32;
        let t = 0.16_f32;
        for (x, z, sx, sz) in [
            (0.0, -side, side * 2.0 + t, t),
            (0.0, side, side * 2.0 + t, t),
            (-side, 0.0, t, side * 2.0 + t),
            (side, 0.0, t, side * 2.0 + t),
        ] {
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(ring_mat.clone()),
                Transform::from_xyz(x, 0.08, z).with_scale(Vec3::new(sx, 0.1, sz)),
            ));
        }
    });

    // 静止姿态与生成时各节点的 Transform 严格一致，动画在此基础上叠加而不累积。
    let base = HeroRigBase {
        torso: Transform::from_xyz(0.0, 1.45, 0.0).with_scale(body_scale),
        head: Transform::from_xyz(0.0, 2.65, 0.0).with_scale(Vec3::splat(0.9)),
        left_arm: Transform::from_xyz(-0.7, 1.95, 0.0),
        right_arm: Transform::from_xyz(0.7, 1.95, 0.0),
        left_leg: Transform::from_xyz(-0.3, 0.9, 0.0),
        right_leg: Transform::from_xyz(0.3, 0.9, 0.0),
        cape: cape_entity
            .map(|_| Transform::from_xyz(0.0, 1.35, -0.5).with_scale(Vec3::new(1.0, 1.6, 0.16))),
    };
    commands.entity(parent).insert((
        HeroRig {
            torso: torso_entity,
            head: head_entity,
            right_arm,
            left_arm,
            right_leg,
            left_leg,
            cape: cape_entity,
            base,
        },
        crate::hero_anim::HeroAnim::new(position),
    ));
    parent
}

/// 根据 skin 生成头饰轮廓。
fn spawn_headgear(
    b: &mut ChildSpawnerCommands<'_>,
    cube: &Handle<Mesh>,
    head_mat: &Handle<StandardMaterial>,
    accent_mat: &Handle<StandardMaterial>,
    skin: u8,
) {
    let variant = skin % 6;
    match variant {
        0 => {
            // 王冠：顶部一圈金色方块
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, 3.18, 0.0).with_scale(Vec3::new(0.95, 0.24, 0.95)),
            ));
        }
        1 => {
            // 双角头盔
            for x in [-0.42_f32, 0.42] {
                b.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(head_mat.clone()),
                    Transform::from_xyz(x, 3.2, -0.05).with_scale(Vec3::new(0.2, 0.7, 0.2)),
                ));
            }
        }
        2 => {
            // 紫色大帽 + 顶部宝石
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(head_mat.clone()),
                Transform::from_xyz(0.0, 3.2, 0.0).with_scale(Vec3::new(1.1, 0.5, 1.1)),
            ));
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, 3.55, 0.0).with_scale(Vec3::splat(0.24)),
            ));
        }
        3 => {
            // 竖向头冠
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(head_mat.clone()),
                Transform::from_xyz(0.0, 3.35, -0.1).with_scale(Vec3::new(0.16, 0.9, 0.45)),
            ));
        }
        4 => {
            // 兜帽 + 尖顶
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(head_mat.clone()),
                Transform::from_xyz(0.0, 3.05, -0.05).with_scale(Vec3::new(1.15, 0.55, 1.15)),
            ));
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, 3.45, -0.1).with_scale(Vec3::new(0.2, 0.6, 0.3)),
            ));
        }
        _ => {
            // 光环
            b.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, 3.45, 0.0).with_scale(Vec3::new(1.4, 0.1, 1.4)),
            ));
        }
    }
}

/// 根据 skin 生成右手武器（作为肩关节枢轴的子实体）。
fn spawn_weapon(
    arm: &mut ChildSpawnerCommands<'_>,
    cube: &Handle<Mesh>,
    _head_mat: &Handle<StandardMaterial>,
    accent_mat: &Handle<StandardMaterial>,
    skin: u8,
) {
    // 假人无武器
    if skin == 9 {
        return;
    }
    match skin % 6 {
        0 => {
            // 长剑
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.05, 0.45).with_scale(Vec3::new(0.14, 1.7, 0.14)),
            ));
        }
        1 => {
            // 战斧
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.05, 0.5).with_scale(Vec3::new(0.55, 0.75, 0.18)),
            ));
        }
        2 => {
            // 巨棒
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.1, 0.45).with_scale(Vec3::new(0.4, 1.1, 0.4)),
            ));
        }
        3 => {
            // 长枪
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.35, 0.35).with_scale(Vec3::new(0.12, 2.5, 0.12)),
            ));
        }
        4 => {
            // 法杖 + 法球
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.2, 0.4).with_scale(Vec3::new(0.14, 2.0, 0.14)),
            ));
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -2.15, 0.4).with_scale(Vec3::splat(0.32)),
            ));
        }
        _ => {
            // 巨锤
            arm.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(0.0, -1.05, 0.45).with_scale(Vec3::new(0.7, 0.7, 0.35)),
            ));
        }
    }
}

/// 小兵可视标记（快照驱动，非英雄）。
#[derive(Component)]
pub struct MinionVisual;

/// P-V4 通用非英雄体素模型：躯干 / 头 / 特征块三节点 + 动态骨架。
/// 造型全部读 `entity_visual::NON_HERO_VISUALS`，新增类型只加数据行。
pub fn spawn_voxel_non_hero(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    kind: crate::entity_visual::NonHeroKind,
    team: Team,
    position: Vec3,
) -> Entity {
    let v = crate::entity_visual::non_hero_visual(kind);
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // 队伍色优先于数据表默认体色；中立单位沿用数据表
    let body_kind = match team {
        Team::Blue => PixelKind::Blue,
        Team::Red => PixelKind::Red,
        Team::Neutral => v.body,
    };
    let body_mat = voxel_material(materials, images, body_kind, v.seed, false);
    let head_mat = voxel_material(materials, images, v.head, v.seed + 1, false);
    let accent_mat = voxel_material(materials, images, v.accent, v.seed + 2, true);

    let body_base = Transform::from_xyz(0.0, v.body_scale.y * 0.5, 0.0).with_scale(v.body_scale);
    let head_base = Transform::from_xyz(0.0, v.body_scale.y + v.head_scale.y * 0.5, 0.0)
        .with_scale(v.head_scale);
    let accent_base = Transform::from_xyz(v.body_scale.x * 0.6, v.body_scale.y * 0.7, 0.15)
        .with_scale(Vec3::new(0.14, v.body_scale.y * 0.8, 0.14));

    let parent = commands
        .spawn((
            Transform::from_translation(position),
            Visibility::default(),
            crate::entity_visual::NonHeroAnim::new(kind, position),
        ))
        .id();
    let mut ids = Vec::with_capacity(3);
    commands.entity(parent).with_children(|b| {
        ids.push(
            b.spawn((Mesh3d(cube.clone()), MeshMaterial3d(body_mat), body_base))
                .id(),
        );
        ids.push(
            b.spawn((Mesh3d(cube.clone()), MeshMaterial3d(head_mat), head_base))
                .id(),
        );
        ids.push(
            b.spawn((Mesh3d(cube), MeshMaterial3d(accent_mat), accent_base))
                .id(),
        );
    });
    commands
        .entity(parent)
        .insert(crate::entity_visual::NonHeroRig {
            body: ids[0],
            head: ids[1],
            accent: ids[2],
            body_base,
            head_base,
            accent_base,
        });
    parent
}

/// 生成方块小兵：队伍色躯干 + 头部 + 小武器，体型明显小于英雄。
pub fn spawn_voxel_minion(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    team: Team,
    position: Vec3,
) -> Entity {
    spawn_voxel_non_hero(
        commands,
        meshes,
        materials,
        images,
        crate::entity_visual::NonHeroKind::Minion,
        team,
        position,
    )
}

/// 生成野怪体素模型（P-V4 新增；此前野怪只有服务端逻辑、客户端无实体）。
pub fn spawn_voxel_monster(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    team: Team,
    position: Vec3,
) -> Entity {
    spawn_voxel_non_hero(
        commands,
        meshes,
        materials,
        images,
        crate::entity_visual::NonHeroKind::Monster,
        team,
        position,
    )
}

/// 方块指示框：4 个扁平 `Cuboid` 组成方形标记（取代圆环，PRES-8）。
/// 返回父实体，调用方插入对应的 Marker 组件。
pub fn spawn_voxel_marker(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    kind: PixelKind,
    side: f32,
    y: f32,
) -> Entity {
    // 道具/标记：1px 描边图案，四边框体在不同目标尺寸下保持清晰轮廓。
    let mat = voxel_mat(materials, images, kind, 21, true, P::Border, F::Top);
    let bar = meshes.add(Cuboid::new(1.0, 0.12, 1.0));
    let parent = commands
        .spawn((Transform::from_xyz(0.0, y, 0.0), Visibility::Hidden))
        .id();
    let t = 0.18;
    commands.entity(parent).with_children(|b| {
        for (x, z, sx, sz) in [
            (0.0, -side, side * 2.0 + t, t),
            (0.0, side, side * 2.0 + t, t),
            (-side, 0.0, t, side * 2.0 + t),
            (side, 0.0, t, side * 2.0 + t),
        ] {
            b.spawn((
                Mesh3d(bar.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(x, 0.0, z).with_scale(Vec3::new(sx, 1.0, sz)),
            ));
        }
    });
    parent
}

/// 体素堆叠建筑（哨塔 / 源核）。P-V4 改为父子结构并挂动态骨架，
/// 顶层核心块由 `tick_non_hero_anim` 驱动脉冲与血量损伤，不再是静态装饰。
#[allow(clippy::too_many_arguments)]
fn spawn_voxel_stack(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    cube: Handle<Mesh>,
    kind: PixelKind,
    position: Vec3,
    levels: usize,
    visual_kind: crate::entity_visual::NonHeroKind,
) -> Entity {
    let mat = voxel_material(materials, images, kind, 31, false);
    let cap = voxel_material(materials, images, PixelKind::Gold, 32, true);
    let parent = commands
        .spawn((
            Transform::from_translation(position),
            Visibility::default(),
            crate::entity_visual::NonHeroAnim::new(visual_kind, position),
        ))
        .id();
    let mut ids = Vec::with_capacity(levels);
    let mut bases = Vec::with_capacity(levels);
    commands.entity(parent).with_children(|b| {
        for level in 0..levels {
            let w = 2.4 - level as f32 * 0.35;
            let tr = Transform::from_xyz(0.0, 0.6 + level as f32 * 1.2, 0.0)
                .with_scale(Vec3::new(w, 1.2, w));
            let m = if level == levels - 1 {
                cap.clone()
            } else {
                mat.clone()
            };
            ids.push(b.spawn((Mesh3d(cube.clone()), MeshMaterial3d(m), tr)).id());
            bases.push(tr);
        }
    });
    let last = levels - 1;
    let mid = levels / 2;
    commands
        .entity(parent)
        .insert(crate::entity_visual::NonHeroRig {
            body: ids[0],
            head: ids[mid],
            accent: ids[last],
            body_base: bases[0],
            head_base: bases[mid],
            accent_base: bases[last],
        });
    parent
}

/// 在地面铺一条带旋转的路面（用于水平兵线与纵向河岸石径）。
fn spawn_ground_strip(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    kind: PixelKind,
    from: Vec3,
    to: Vec3,
    width: f32,
    y: f32,
) {
    let delta = to - from;
    let len = Vec2::new(delta.x, delta.z).length();
    if len < 0.01 {
        return;
    }
    // Cuboid 的局部 X 轴沿线段方向：绕 Y 轴旋转 -atan2(dz, dx)
    let angle = -delta.z.atan2(delta.x);
    let mat = voxel_material(materials, images, kind, 43, false);
    let mid = (from + to) * 0.5;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(len, 0.07, width))),
        MeshMaterial3d(mat),
        Transform::from_translation(Vec3::new(mid.x, y, mid.z))
            .with_rotation(Quat::from_rotation_y(angle)),
    ));
}

/// 体素地形（2:1 横向长方形）：棋盘式地面 / 纵向河道与兵线跨河桥面 / 单条水平兵线 / 南侧野区 / 草丛 / 基地 / 哨塔 / 源核 / 中央矿坑 / 共用野区商店。
pub fn spawn_voxel_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    // 场景：朝上的地面/路面提亮，土层压暗，石墙走条纹，源核走发光网格。
    let grass = voxel_mat(
        materials,
        images,
        PixelKind::Grass,
        41,
        false,
        P::Speckle,
        F::Top,
    );
    let grass_b = voxel_mat(
        materials,
        images,
        PixelKind::Grass,
        45,
        false,
        P::Speckle,
        F::Side,
    );
    let dirt = voxel_mat(
        materials,
        images,
        PixelKind::Dirt,
        46,
        false,
        P::Flat,
        F::Bottom,
    );
    let brush = voxel_mat(
        materials,
        images,
        PixelKind::Brush,
        44,
        false,
        P::Speckle,
        F::Top,
    );
    let stone = voxel_mat(
        materials,
        images,
        PixelKind::Stone,
        48,
        false,
        P::Stripe,
        F::Side,
    );
    let lane_mat = voxel_mat(
        materials,
        images,
        PixelKind::Lane,
        43,
        false,
        P::Stripe,
        F::Top,
    );
    let core = voxel_mat(
        materials,
        images,
        PixelKind::Core,
        47,
        true,
        P::Grid,
        F::Top,
    );
    // P-V4：启用此前定义了却从未使用的 `PixelKind::Water`。
    let water = voxel_mat(
        materials,
        images,
        PixelKind::Water,
        49,
        true,
        P::Stripe,
        F::Top,
    );
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    // 地面（P-V4）：32×16 长方形拼贴，块宽严格等于中心距（旧版 `tile + 0.2` 造成
    // 相邻块水平重叠 0.2 且完全共面，是「随镜头移动闪烁」的主因）。
    // 河道是沿中轴 `x = 0` 的**纵向带**（与北侧水平兵线正交），用整块归类铺成体素带：
    // 河块 = 下沉河床 + 水面，桥块 = 石质桥面，陆块 = 草地/泥土。
    // 每块只生成其中一种，水平不重叠，因此不会 z-fighting。
    let tile = MAP_WIDTH / GROUND_TILES_X as f32;
    for iz in 0..GROUND_TILES_Z {
        let z = -MAP_H_HALF + tile * (iz as f32 + 0.5);
        for ix in 0..GROUND_TILES_X {
            let x = -MAP_W_HALF + tile * (ix as f32 + 0.5);
            let center = Vec3::new(x, 0.0, z);
            let pick = (ix * 5 + iz * 3) % 13;
            let mat = if pick == 0 {
                dirt.clone()
            } else if pick % 2 == 0 {
                grass_b.clone()
            } else {
                grass.clone()
            };
            if is_bridge(center) {
                // 中央桥面：与兵线同高的石质平台，标识全速通行区
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(stone.clone()),
                    Transform::from_xyz(x, LAYER_FOLD.y, z).with_scale(Vec3::new(
                        tile,
                        LAYER_FOLD.thickness,
                        tile,
                    )),
                ));
            } else if is_river(center) {
                // 河道：河床 + 水面，均低于地面顶面，形成可涉水下沉带
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(dirt.clone()),
                    Transform::from_xyz(x, LAYER_RIVER_BED.y, z).with_scale(Vec3::new(
                        tile,
                        LAYER_RIVER_BED.thickness,
                        tile,
                    )),
                ));
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(water.clone()),
                    Transform::from_xyz(x, LAYER_WATER.y, z).with_scale(Vec3::new(
                        tile,
                        LAYER_WATER.thickness,
                        tile,
                    )),
                ));
            } else {
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(mat),
                    Transform::from_xyz(x, LAYER_GROUND.y, z).with_scale(Vec3::new(
                        tile,
                        LAYER_GROUND.thickness,
                        tile,
                    )),
                ));
            }
        }
    }

    // 河岸：沿纵向河道两侧各一条窄石带（x = ±(半宽+0.6)），
    // 位于独立高度层（与地面顶面差 ≥0.05，不共面）。
    let (fold_a, fold_b) = fold_line_endpoints();
    for side in [-1.0_f32, 1.0] {
        let off = Vec3::new(side * (RIVER_HALF_WIDTH + 0.6), 0.0, 0.0);
        spawn_ground_strip(
            commands,
            meshes,
            materials,
            images,
            PixelKind::Stone,
            fold_a + off,
            fold_b + off,
            1.2,
            LAYER_BANK.y,
        );
    }

    // 地图边界：低矮石墙，明确可活动范围（长方形分轴取半径）
    for (x, z, sx, sz) in [
        (0.0, -MAP_H_HALF, MAP_WIDTH + 1.0, 1.0),
        (0.0, MAP_H_HALF, MAP_WIDTH + 1.0, 1.0),
        (-MAP_W_HALF, 0.0, 1.0, MAP_HEIGHT + 1.0),
        (MAP_W_HALF, 0.0, 1.0, MAP_HEIGHT + 1.0),
    ] {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(x, 0.45, z).with_scale(Vec3::new(sx, 1.1, sz)),
        ));
    }

    // 兵线 / 野区分隔墙：**只在兵线南侧（靠野区一侧）**有一道堤墙，
    // 在对称坡道开口处断开；兵线北侧靠地图边界，不设墙。
    // 与 `game_core::map::LANE_WALLS` / `wall_colliders()` 同源 —— 服务端静态碰撞、
    // 客户端预测、A* 障碍格、小地图都读这一份，这里只负责把它画出来。
    // 段端立柱同时标出坡道入口，玩家能一眼看出从哪里可以转进野区。
    const WALL_HEIGHT: f32 = 2.6;
    for w in LANE_WALLS {
        let len = w.x1 - w.x0;
        let mid = (w.x0 + w.x1) * 0.5;
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(mid, WALL_HEIGHT * 0.5, w.z).with_scale(Vec3::new(
                len,
                WALL_HEIGHT,
                WALL_HALF_THICKNESS * 2.0,
            )),
        ));
        // 压顶石：位于主体顶面之上，独立高度层，不与主体共面
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(lane_mat.clone()),
            Transform::from_xyz(mid, WALL_HEIGHT + 0.16, w.z).with_scale(Vec3::new(
                len,
                0.32,
                WALL_HALF_THICKNESS * 2.0 + 0.5,
            )),
        ));
        for x in [w.x0, w.x1] {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(stone.clone()),
                Transform::from_xyz(x, WALL_HEIGHT * 0.75, w.z).with_scale(Vec3::new(
                    WALL_HALF_THICKNESS * 2.4,
                    WALL_HEIGHT * 1.5,
                    WALL_HALF_THICKNESS * 2.4,
                )),
            ));
        }
    }

    // 中央矿坑 / 海兽坑：位于**兵线中点正下方**的河道之上（石台盖住水面），后期由海兽占据
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(14.0, 0.3, 14.0))),
        MeshMaterial3d(stone.clone()),
        Transform::from_xyz(GOLD_MINE_POS.x, 0.02, GOLD_MINE_POS.z),
    ));
    for (dx, dz) in [(-5.0_f32, -5.0), (5.0, -5.0), (-5.0, 5.0), (5.0, 5.0)] {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(voxel_material(materials, images, PixelKind::Gold, 52, true)),
            Transform::from_xyz(GOLD_MINE_POS.x + dx, 1.2, GOLD_MINE_POS.z + dz)
                .with_scale(Vec3::new(1.8, 2.4, 1.8)),
        ));
    }

    // 野区商店：地图正下方中央、双方共用。石质广场跨在河道南端之上，
    // 四角立柱 + 中央青色柜台，对应营地点 `MusterWell`。
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(20.0, 0.3, 16.0))),
        MeshMaterial3d(stone.clone()),
        Transform::from_xyz(JUNGLE_SHOP_POS.x, 0.02, JUNGLE_SHOP_POS.z),
    ));
    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(voxel_material(materials, images, PixelKind::Core, 53, true)),
        Transform::from_xyz(JUNGLE_SHOP_POS.x, 1.0, JUNGLE_SHOP_POS.z)
            .with_scale(Vec3::new(3.0, 2.0, 3.0)),
    ));
    for dx in [-8.0_f32, 8.0] {
        for dz in [-6.0_f32, 6.0] {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(stone.clone()),
                Transform::from_xyz(JUNGLE_SHOP_POS.x + dx, 1.4, JUNGLE_SHOP_POS.z + dz)
                    .with_scale(Vec3::new(1.2, 2.8, 1.2)),
            ));
        }
    }

    // 唯一兵线（3v3 竞技场）：与 `game_core::map` 的路点完全同源，分段铺成连续路面
    for lane in 0..LANE_COUNT {
        let points = lane_waypoints(Team::Blue, lane);
        for pair in points.windows(2) {
            spawn_ground_strip(
                commands,
                meshes,
                materials,
                images,
                PixelKind::Lane,
                pair[0],
                pair[1],
                LANE_WIDTH,
                LAYER_LANE.y,
            );
        }
        // 路点补缝：抬到独立高度层（旧版与条带同 y 同厚、完全共面，是次生闪烁源）
        for p in &points {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(lane_mat.clone()),
                Transform::from_xyz(p.x, LAYER_WAYPOINT.y, p.z).with_scale(Vec3::new(
                    LANE_WIDTH,
                    LAYER_WAYPOINT.thickness,
                    LANE_WIDTH,
                )),
            ));
        }
    }

    // 基地平台：石质底座 + 队伍色地砖 + 四角立柱
    for (base, kind) in [
        (BLUE_SOURCE_CORE, PixelKind::Blue),
        (RED_SOURCE_CORE, PixelKind::Red),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(30.0, 0.5, 30.0))),
            MeshMaterial3d(stone.clone()),
            // 石座顶面 0.27 与地砖顶面 0.38 拉开 0.11，消除近共面抖动
            Transform::from_xyz(base.x, 0.02, base.z),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(22.0, 0.52, 22.0))),
            MeshMaterial3d(voxel_material(materials, images, kind, 51, true)),
            Transform::from_xyz(base.x, 0.12, base.z),
        ));
        for (dx, dz) in [
            (-13.0_f32, -13.0),
            (13.0, -13.0),
            (-13.0, 13.0),
            (13.0, 13.0),
        ] {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(stone.clone()),
                Transform::from_xyz(base.x + dx, 1.6, base.z + dz)
                    .with_scale(Vec3::new(1.5, 3.2, 1.5)),
            ));
        }
    }

    // 野区营地：石质巢穴 + 树干与树冠，营造两片野区的层次
    for camp in JUNGLE_CAMPS {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(7.0, 0.35, 7.0))),
            MeshMaterial3d(dirt.clone()),
            Transform::from_xyz(camp.position.x, 0.05, camp.position.z),
        ));
        for (dx, dz) in [(-3.0_f32, -2.5), (3.2, -1.8), (-2.0, 3.0), (2.6, 3.2)] {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(brush.clone()),
                Transform::from_translation(camp.position + Vec3::new(dx, 2.2, dz))
                    .with_scale(Vec3::new(2.2, 3.6, 2.2)),
            ));
        }
    }
    // 训练假人处的石台
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(5.0, 0.3, 5.0))),
        MeshMaterial3d(stone),
        Transform::from_xyz(TRAINING_DUMMY_POS.x, 0.05, TRAINING_DUMMY_POS.z),
    ));

    // 草丛：每丛 5 个错落体素
    for bush in BUSHES {
        for (dx, dz, h) in [
            (-1.2, -0.8, 1.0),
            (1.0, -1.0, 1.4),
            (0.2, 0.6, 1.8),
            (-1.4, 1.1, 1.1),
            (1.3, 1.2, 1.3),
        ] {
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(brush.clone()),
                Transform::from_translation(bush.center + Vec3::new(dx, h / 2.0, dz))
                    .with_scale(Vec3::new(1.4, h, 1.4)),
            ));
        }
    }

    // 哨塔：体素堆叠，队伍色
    for turret in TURRET_POSITIONS {
        let kind = match turret.team {
            Team::Blue => PixelKind::Blue,
            Team::Red => PixelKind::Red,
            Team::Neutral => PixelKind::Stone,
        };
        spawn_voxel_stack(
            commands,
            materials,
            images,
            cube.clone(),
            kind,
            turret.position,
            3,
            crate::entity_visual::NonHeroKind::Turret,
        );
    }

    // 源核：更高的体素塔
    for (pos, kind) in [
        (BLUE_SOURCE_CORE, PixelKind::Blue),
        (RED_SOURCE_CORE, PixelKind::Red),
    ] {
        spawn_voxel_stack(
            commands,
            materials,
            images,
            cube.clone(),
            kind,
            pos,
            5,
            crate::entity_visual::NonHeroKind::Core,
        );
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(core.clone()),
            Transform::from_translation(pos + Vec3::Y * 6.6).with_scale(Vec3::splat(1.6)),
        ));
    }
}

/// 代码生成的英雄/队伍像素贴图（保留旧函数名以兼容调用方）。
pub fn make_hero_texture(images: &mut Assets<Image>, color: Color) -> Handle<Image> {
    let s = color.to_srgba();
    let kind = if s.blue > s.red {
        PixelKind::Blue
    } else if s.red > s.blue {
        PixelKind::Red
    } else {
        PixelKind::Stone
    };
    make_pixel_texture(images, kind, 7)
}

#[cfg(test)]
mod terrain_tests {
    use super::*;
    use game_core::map::{BRIDGE_Z, LANE_Z};

    /// 回归护栏：地形分层必须严格递增且相邻顶面间隔 ≥ 0.05，
    /// 否则随相机平移会重新出现共面 z-fighting（本次闪烁 bug 的结构性根因）。
    #[test]
    fn terrain_layers_are_ordered_and_separated() {
        assert_eq!(TERRAIN_LAYERS.len(), 7);
        for pair in TERRAIN_LAYERS.windows(2) {
            let gap = pair[1].top() - pair[0].top();
            assert!(
                gap >= 0.05,
                "相邻层顶面间隔 {gap:.3} 过小：{:?} → {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(LAYER_RIVER_BED.top() < LAYER_WATER.top());
        assert!(LAYER_WATER.top() < LAYER_GROUND.top(), "水面必须低于地面");
    }

    /// 地面拼贴块宽必须严格等于中心距（旧版 `tile + 0.2` 造成水平重叠共面），
    /// 且长方形下分轴块数分别铺满 MAP_WIDTH / MAP_HEIGHT。
    #[test]
    fn ground_tiles_do_not_overlap() {
        let tile = MAP_WIDTH / GROUND_TILES_X as f32;
        // 地图整体等比放大后，拼贴块数不变、块边长随 MAP_SCALE 一起变。
        assert_eq!(tile, 10.0 * game_core::map::MAP_SCALE);
        assert_eq!(tile * GROUND_TILES_Z as f32, MAP_HEIGHT);
        assert_eq!(tile * GROUND_TILES_X as f32, MAP_WIDTH);
        assert!(RIVER_HALF_WIDTH > 0.0 && RIVER_HALF_WIDTH < tile);
        assert!((LAYER_GROUND.top() + 0.05).abs() < 1e-5);
    }

    /// 渲染用的墙段与服务端碰撞 / A* 障碍必须是同一份数据，且只存在于兵线南侧。
    #[test]
    fn lane_walls_are_rendered_from_the_authoritative_segments() {
        assert_eq!(LANE_WALLS.len(), 4);
        let south = game_core::map::WALL_SOUTH_Z;
        assert!(south > LANE_Z);
        assert_eq!(LANE_WALLS.iter().filter(|w| w.z == south).count(), 4);
        assert!(
            LANE_WALLS.iter().all(|w| w.z > LANE_Z),
            "兵线北侧（靠地图边界）不得渲染墙体"
        );
        // 渲染用的墙厚必须与服务端 / 客户端预测的碰撞圆半径完全同源
        assert!(
            game_core::map::wall_colliders()
                .iter()
                .all(|(_, r)| *r == WALL_HALF_THICKNESS)
        );
        // 坡道开口处不能渲染出墙段
        for w in LANE_WALLS {
            assert!(!game_core::map::is_lane_wall_gap((w.x0 + w.x1) * 0.5));
        }
    }

    /// 经典 3v3 回归护栏：河道是沿 `x = 0` 的**纵向**带，与北侧水平兵线正交，
    /// 且渲染层与裁决层用同一套几何（不出现「看到的河」和「减速的河」不一致）。
    #[test]
    fn river_is_longitudinal_and_orthogonal_to_lane() {
        assert!(is_river(Vec3::new(0.0, 0.0, 40.0)));
        assert!(is_river(Vec3::new(4.0, 0.0, -70.0)));
        assert!(is_bridge(Vec3::new(0.0, 0.0, LANE_Z)));
        // 左右野区（河道两侧）都是陆地
        assert!(!is_river(Vec3::new(40.0, 0.0, 0.0)));
        assert!(!is_river(Vec3::new(-40.0, 0.0, 0.0)));
        // 折叠线是沿 z 的中轴，两端 x 恒为 0
        let (a, b) = fold_line_endpoints();
        assert_eq!(a.x, 0.0);
        assert_eq!(b.x, 0.0);
        assert!(a.z < LANE_Z && b.z > LANE_Z);
        assert_eq!(BRIDGE_Z, LANE_Z, "桥面必须在兵线跨河处");
    }

    /// P-V4：`PixelKind::Water` 已从「定义未启用」变为真实可用。
    #[test]
    fn water_palette_is_enabled() {
        let (base, accent) = colors(PixelKind::Water);
        assert_ne!(base, accent);
        assert_ne!(base, [255, 255, 255], "不得回退到默认白色");
        assert!(base[2] > base[0], "水体主色应偏蓝");
    }
}
