//! P-V1 视觉资源库：确定性纹理图案、伪 AO 明暗、材质 / 网格复用池。
//!
//! 设计口径见 `docs/06_VISUAL_SPEC.md` §3 / §5 / §7：
//! - 所有纹理由 `pattern_texture_data` 纯函数生成，随机性只来自确定性 `hash`，保证回放一致；
//! - `MaterialKey` 五元组是全局唯一资源键，禁止用字符串拼接做键；
//! - `VoxelMaterialPool` 按 key 缓存 `Handle<Image>` / `Handle<StandardMaterial>`，
//!   避免地形循环与逐帧特效反复新建材质；
//! - 零外部资产：全部几何、纹理、材质在运行时由代码生成。

use crate::voxel::{PixelKind, colors};
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

/// 纹理图案：决定像素级排布，不改变底色（`docs/06_VISUAL_SPEC.md` §4）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TexturePattern {
    /// 纯底色，用于大面积平坦表面
    Flat,
    /// 逐像素确定性噪点，布料 / 草皮 / 石面默认
    #[default]
    Speckle,
    /// 1px 描边 + 稀疏噪点，标记 / 头部部件
    Border,
    /// 横向条纹，路面 / 金属
    Stripe,
    /// 8px 网格，核心 / 机械
    Grid,
}

/// 伪 AO 明暗：按物体朝向选择材质变体，不修改网格 UV。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FaceShade {
    /// 朝上面，提亮
    Top,
    /// 默认立面
    #[default]
    Side,
    /// 底面 / 背光层，压暗
    Bottom,
}

impl FaceShade {
    pub const fn factor(self) -> f32 {
        match self {
            FaceShade::Top => 1.15,
            FaceShade::Side => 1.0,
            FaceShade::Bottom => 0.78,
        }
    }
}

/// 全局唯一资源键。五个维度全部参与哈希，任一不同即不同材质。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialKey {
    pub kind: PixelKind,
    pub seed: u32,
    pub pattern: TexturePattern,
    pub face: FaceShade,
    pub glow: bool,
}

impl MaterialKey {
    pub const fn new(kind: PixelKind, seed: u32) -> Self {
        Self {
            kind,
            seed,
            pattern: TexturePattern::Speckle,
            face: FaceShade::Side,
            glow: false,
        }
    }

    pub const fn with_pattern(mut self, pattern: TexturePattern) -> Self {
        self.pattern = pattern;
        self
    }

    pub const fn with_face(mut self, face: FaceShade) -> Self {
        self.face = face;
        self
    }

    pub const fn with_glow(mut self, glow: bool) -> Self {
        self.glow = glow;
        self
    }

    /// 资源调试名，遵循 `VIS-VOX-{KIND}-{SEED}-{PATTERN}-{FACE}`（§5）。
    pub fn debug_name(&self) -> String {
        format!(
            "VIS-VOX-{:?}-{}-{:?}-{:?}",
            self.kind, self.seed, self.pattern, self.face
        )
    }
}

/// 确定性整数哈希。相同输入必得相同结果，禁止使用系统时间或运行时随机源。
pub fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ seed.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h
}

/// 16×16 RGBA 像素数据的**纯函数**生成：逐像素噪点 + 图案 + 1px 描边 + 面明暗。
/// 抽成纯函数便于单测确定性与图案差异，也便于未来替换为 chunk 图集。
pub fn pattern_texture_data(
    kind: PixelKind,
    seed: u32,
    pattern: TexturePattern,
    face: FaceShade,
) -> Vec<u8> {
    const N: u32 = 16;
    let (base, accent) = colors(kind);
    let mut data = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let h = hash(x, y, seed);
            let noise = (h & 0xF) < 3;
            let use_accent = match pattern {
                TexturePattern::Flat => false,
                TexturePattern::Speckle => noise,
                TexturePattern::Border => x == 0 || y == 0 || x == N - 1 || y == N - 1 || noise,
                TexturePattern::Stripe => y % 4 == 0 || noise,
                TexturePattern::Grid => x % 8 == 0 || y % 8 == 0 || noise,
            };
            let mut c = if use_accent { accent } else { base };
            let edge = x == 0 || y == 0 || x == N - 1 || y == N - 1;
            let shade = face.factor()
                * if edge && pattern != TexturePattern::Border {
                    0.72
                } else {
                    1.0
                };
            for channel in &mut c {
                *channel = (f32::from(*channel) * shade).clamp(0.0, 255.0) as u8;
            }
            let i = ((y * N + x) * 4) as usize;
            data[i] = c[0];
            data[i + 1] = c[1];
            data[i + 2] = c[2];
            data[i + 3] = 255;
        }
    }
    data
}

/// 把纯函数数据包装成 `Image` 并注册（16×16、`Nearest` 采样，PRES-4）。
pub fn make_pattern_texture(
    images: &mut Assets<Image>,
    kind: PixelKind,
    seed: u32,
    pattern: TexturePattern,
    face: FaceShade,
) -> Handle<Image> {
    let data = pattern_texture_data(kind, seed, pattern, face);
    let mut image = Image::new(
        Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    images.add(image)
}

/// 池的规模快照，供验收与运行期自查（§7 性能预算）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolStats {
    pub textures: usize,
    pub materials: usize,
    pub effects: usize,
    pub mesh_shared: bool,
}

/// 视觉资源池（ECS 资源）：按 `MaterialKey` 去重并复用所有运行时 Handle。
#[derive(Resource, Default)]
pub struct VoxelMaterialPool {
    textures: HashMap<(PixelKind, u32, TexturePattern, FaceShade), Handle<Image>>,
    materials: HashMap<MaterialKey, Handle<StandardMaterial>>,
    effects: HashMap<MaterialKey, Handle<StandardMaterial>>,
    cube: Option<Handle<Mesh>>,
}

impl VoxelMaterialPool {
    /// 取纹理：同 `(kind, seed, pattern, face)` 必返回同一 Handle。
    pub fn texture(
        &mut self,
        images: &mut Assets<Image>,
        kind: PixelKind,
        seed: u32,
        pattern: TexturePattern,
        face: FaceShade,
    ) -> Handle<Image> {
        self.textures
            .entry((kind, seed, pattern, face))
            .or_insert_with(|| make_pattern_texture(images, kind, seed, pattern, face))
            .clone()
    }

    /// 取材质：同 `MaterialKey` 必返回同一 Handle，绝不重复 `Assets::add`。
    pub fn material(
        &mut self,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        key: MaterialKey,
    ) -> Handle<StandardMaterial> {
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let tex = self.texture(images, key.kind, key.seed, key.pattern, key.face);
        let handle = materials.add(StandardMaterial {
            base_color_texture: Some(tex),
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            emissive: if key.glow {
                Color::srgb(0.18, 0.18, 0.18).into()
            } else {
                Color::BLACK.into()
            },
            ..default()
        });
        self.materials.insert(key, handle.clone());
        handle
    }

    /// 共享单位立方体网格：所有 1×1×1 体素复用同一 Handle。
    pub fn cube(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        if let Some(handle) = &self.cube {
            return handle.clone();
        }
        let handle = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        self.cube = Some(handle.clone());
        handle
    }

    /// 特效材质：`unlit` + 语义色 emissive，按 `(kind, seed)` 复用。
    /// 弹体 / 技能 / 事件碎片统一走此入口，避免逐帧 `Assets::add`。
    pub fn fx_material(
        &mut self,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        kind: PixelKind,
        seed: u32,
    ) -> Handle<StandardMaterial> {
        let key = MaterialKey::new(kind, seed).with_pattern(TexturePattern::Border);
        if let Some(handle) = self.effects.get(&key) {
            return handle.clone();
        }
        let tex = self.texture(images, kind, seed, key.pattern, FaceShade::Top);
        let (base, _) = colors(kind);
        let handle = materials.add(StandardMaterial {
            base_color_texture: Some(tex),
            base_color: Color::WHITE,
            emissive: Color::srgb(
                f32::from(base[0]) / 510.0,
                f32::from(base[1]) / 510.0,
                f32::from(base[2]) / 510.0,
            )
            .into(),
            unlit: true,
            ..default()
        });
        self.effects.insert(key, handle.clone());
        handle
    }

    pub fn stats(&self) -> PoolStats {
        PoolStats {
            textures: self.textures.len(),
            materials: self.materials.len(),
            effects: self.effects.len(),
            mesh_shared: self.cube.is_some(),
        }
    }
}

/// 剪影原型：低分辨率下靠「体型 + 头部特征 + 武器轮廓」辨识角色
/// （`docs/06_VISUAL_SPEC.md` §2.1 / §4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Silhouette {
    /// 重甲护卫：宽厚偏矮、面甲、巨盾
    Bulwark,
    /// 剑士：匀称、王冠、长剑
    Blade,
    /// 游侠：修长窄肩、兜帽、长柄武器
    Ranger,
    /// 法师：修长、尖顶、法杖法球
    Mage,
    /// 治疗：中等、兜帽光环、短杖
    Healer,
    /// 攻城：极宽厚、双角盔、巨锤
    Siege,
}

/// 英雄造型数据表条目。`skin` 只作为本表索引，新增英雄 = 加一行数据，
/// 不改 `spawn_voxel_hero` 的几何代码。
#[derive(Clone, Copy, Debug)]
pub struct HeroVisual {
    pub silhouette: Silhouette,
    pub torso: PixelKind,
    pub limb: PixelKind,
    pub head: PixelKind,
    pub body_scale: Vec3,
    pub limb_scale: Vec3,
    pub cape: bool,
    pub pauldrons: bool,
    /// 该原型的技能主色（同调色板派生，不引入新色）
    pub cast: PixelKind,
}

/// 6 种剪影原型（§8.2 造型原型矩阵的代码化）。
pub const HERO_VISUALS: [HeroVisual; 6] = [
    HeroVisual {
        silhouette: Silhouette::Bulwark,
        torso: PixelKind::Blue,
        limb: PixelKind::Stone,
        head: PixelKind::Gold,
        body_scale: Vec3::new(1.12, 1.34, 0.68),
        limb_scale: Vec3::new(0.48, 1.12, 0.54),
        cape: false,
        pauldrons: true,
        cast: PixelKind::Blue,
    },
    HeroVisual {
        silhouette: Silhouette::Blade,
        torso: PixelKind::Red,
        limb: PixelKind::Dirt,
        head: PixelKind::Red,
        body_scale: Vec3::new(0.92, 1.14, 0.52),
        limb_scale: Vec3::new(0.36, 1.02, 0.44),
        cape: true,
        pauldrons: false,
        cast: PixelKind::Effect,
    },
    HeroVisual {
        silhouette: Silhouette::Ranger,
        torso: PixelKind::Core,
        limb: PixelKind::Stone,
        head: PixelKind::Core,
        body_scale: Vec3::new(1.02, 1.28, 0.58),
        limb_scale: Vec3::new(0.40, 1.14, 0.48),
        cape: true,
        pauldrons: true,
        cast: PixelKind::Core,
    },
    HeroVisual {
        silhouette: Silhouette::Mage,
        torso: PixelKind::Stone,
        limb: PixelKind::Blue,
        head: PixelKind::Marker,
        body_scale: Vec3::new(0.88, 1.10, 0.50),
        limb_scale: Vec3::new(0.34, 0.98, 0.42),
        cape: false,
        pauldrons: false,
        cast: PixelKind::Blue,
    },
    HeroVisual {
        silhouette: Silhouette::Healer,
        torso: PixelKind::Dirt,
        limb: PixelKind::Stone,
        head: PixelKind::Effect,
        body_scale: Vec3::new(1.00, 1.24, 0.58),
        limb_scale: Vec3::new(0.40, 1.10, 0.48),
        cape: true,
        pauldrons: true,
        cast: PixelKind::Brush,
    },
    HeroVisual {
        silhouette: Silhouette::Siege,
        torso: PixelKind::Gold,
        limb: PixelKind::Red,
        head: PixelKind::Marker,
        body_scale: Vec3::new(1.10, 1.30, 0.70),
        limb_scale: Vec3::new(0.50, 1.08, 0.56),
        cape: false,
        pauldrons: false,
        cast: PixelKind::Gold,
    },
];

/// 训练假人（`skin = 9`）单独成项，不占用 6 原型索引。
pub const TRAINING_DUMMY_VISUAL: HeroVisual = HeroVisual {
    silhouette: Silhouette::Bulwark,
    torso: PixelKind::Stone,
    limb: PixelKind::Dirt,
    head: PixelKind::Marker,
    body_scale: Vec3::new(1.25, 1.50, 0.75),
    limb_scale: Vec3::new(0.55, 1.25, 0.60),
    cape: false,
    pauldrons: false,
    cast: PixelKind::Gold,
};

pub fn hero_visual(skin: u8) -> HeroVisual {
    if skin == 9 {
        TRAINING_DUMMY_VISUAL
    } else {
        HERO_VISUALS[(skin as usize) % HERO_VISUALS.len()]
    }
}

/// 技能 / 事件特效形状：一律用轴对齐方块或阶梯方块环近似（PRES-4）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FxShape {
    /// 单方块 + 迸射碎片
    #[default]
    Orb,
    /// 阶梯方块环，用于 AoE / 占领 / 加时 / 胜负
    Ring,
    /// 沿线段连续方块束
    Beam,
    /// 自下而上的上升脉冲
    Pulse,
}

/// 技能视觉描述。由 `AbilityCast.ability_index` 索引；尺寸 / 时长仍从
/// `game_core::abilities::AbilityDef` 的 `range` / `aoe_radius` / `projectile_speed` 读取，
/// 本表只负责「风格」，不重复数值，避免第二套平衡表。
#[derive(Clone, Copy, Debug)]
pub struct AbilityVisual {
    pub index: u8,
    pub name: &'static str,
    pub kind: PixelKind,
    pub shape: FxShape,
    /// 命中阶段迸射的方块碎片数（受 §7 每事件 ≤12 约束）
    pub shards: u8,
    pub aoe: bool,
}

/// 四技能差异化：四种语义色 × 四种形状，保证任意两个技能视觉可区分。
pub const ABILITY_VISUALS: [AbilityVisual; 4] = [
    AbilityVisual {
        index: 0,
        name: "火焰弹",
        kind: PixelKind::Effect,
        shape: FxShape::Orb,
        shards: 8,
        aoe: false,
    },
    AbilityVisual {
        index: 1,
        name: "寒冰新星",
        kind: PixelKind::Marker,
        shape: FxShape::Ring,
        shards: 6,
        aoe: true,
    },
    AbilityVisual {
        index: 2,
        name: "能量光束",
        kind: PixelKind::Blue,
        shape: FxShape::Beam,
        shards: 4,
        aoe: false,
    },
    AbilityVisual {
        index: 3,
        name: "自我治疗",
        kind: PixelKind::Brush,
        shape: FxShape::Pulse,
        shards: 6,
        aoe: false,
    },
];

pub fn ability_visual(index: u8) -> AbilityVisual {
    ABILITY_VISUALS
        .get(index as usize)
        .copied()
        .unwrap_or(ABILITY_VISUALS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_data_is_deterministic() {
        let a = pattern_texture_data(
            PixelKind::Grass,
            41,
            TexturePattern::Speckle,
            FaceShade::Top,
        );
        let b = pattern_texture_data(
            PixelKind::Grass,
            41,
            TexturePattern::Speckle,
            FaceShade::Top,
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 16 * 16 * 4);
    }

    #[test]
    fn different_keys_produce_different_data() {
        let base = pattern_texture_data(
            PixelKind::Grass,
            41,
            TexturePattern::Speckle,
            FaceShade::Side,
        );
        assert_ne!(
            base,
            pattern_texture_data(
                PixelKind::Grass,
                42,
                TexturePattern::Speckle,
                FaceShade::Side
            ),
            "seed must change texture"
        );
        assert_ne!(
            base,
            pattern_texture_data(PixelKind::Grass, 41, TexturePattern::Grid, FaceShade::Side),
            "pattern must change texture"
        );
        assert_ne!(
            base,
            pattern_texture_data(
                PixelKind::Dirt,
                41,
                TexturePattern::Speckle,
                FaceShade::Side
            ),
            "kind must change texture"
        );
    }

    #[test]
    fn face_shade_changes_brightness_in_order() {
        let sample = |face: FaceShade| {
            pattern_texture_data(PixelKind::Stone, 7, TexturePattern::Flat, face)[0]
        };
        let bottom = sample(FaceShade::Bottom);
        let side = sample(FaceShade::Side);
        let top = sample(FaceShade::Top);
        assert!(
            bottom < side && side < top,
            "bottom={bottom} side={side} top={top}"
        );
    }

    #[test]
    fn material_key_is_unique_per_dimension() {
        let base = MaterialKey::new(PixelKind::Grass, 1);
        assert_ne!(base, base.with_pattern(TexturePattern::Grid));
        assert_ne!(base, base.with_face(FaceShade::Top));
        assert_ne!(base, base.with_glow(true));
        assert_ne!(base, MaterialKey::new(PixelKind::Dirt, 1));
        assert_ne!(base, MaterialKey::new(PixelKind::Grass, 2));
        assert_eq!(base, MaterialKey::new(PixelKind::Grass, 1));
    }

    #[test]
    fn debug_name_follows_naming_rule() {
        let name = MaterialKey::new(PixelKind::Gold, 52)
            .with_pattern(TexturePattern::Border)
            .with_face(FaceShade::Top)
            .debug_name();
        assert!(name.starts_with("VIS-VOX-Gold-52-Border-Top"), "got {name}");
    }

    #[test]
    fn hash_is_stable_and_seed_sensitive() {
        assert_eq!(hash(3, 7, 41), hash(3, 7, 41));
        assert_ne!(hash(3, 7, 41), hash(3, 7, 42));
    }

    #[test]
    fn six_silhouettes_have_distinct_palettes() {
        let mut seen = std::collections::HashSet::new();
        for v in HERO_VISUALS {
            assert!(
                seen.insert((v.torso, v.limb, v.head)),
                "每种剪影原型的配色组合必须唯一，否则无法远距离辨识"
            );
            assert!(v.body_scale.x > 0.0 && v.body_scale.y > 0.0);
            assert!(v.limb_scale.x > 0.0 && v.limb_scale.y > 0.0);
        }
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn skin_index_wraps_and_dummy_is_separate() {
        for skin in 0..=5 {
            assert_eq!(
                hero_visual(skin).silhouette,
                HERO_VISUALS[skin as usize].silhouette
            );
        }
        assert_eq!(hero_visual(6).silhouette, hero_visual(0).silhouette);
        assert_eq!(hero_visual(9).silhouette, TRAINING_DUMMY_VISUAL.silhouette);
        assert_ne!(hero_visual(9).torso, hero_visual(0).torso);
    }

    #[test]
    fn four_ability_visuals_are_mutually_distinct() {
        use std::collections::HashSet;
        let kinds: HashSet<_> = ABILITY_VISUALS.iter().map(|v| v.kind).collect();
        let shapes: HashSet<_> = ABILITY_VISUALS.iter().map(|v| v.shape).collect();
        let indices: HashSet<_> = ABILITY_VISUALS.iter().map(|v| v.index).collect();
        assert_eq!(kinds.len(), 4, "四技能语义色必须互异");
        assert_eq!(shapes.len(), 4, "四技能形状必须互异");
        assert_eq!(indices.len(), 4);
        for v in ABILITY_VISUALS {
            assert!(v.shards <= 12, "碎片数不得超过 §7 每事件上限");
        }
    }

    #[test]
    fn ability_visual_falls_back_for_unknown_index() {
        assert_eq!(ability_visual(0).kind, PixelKind::Effect);
        assert_eq!(ability_visual(3).kind, PixelKind::Brush);
        assert_eq!(ability_visual(99).index, 0);
    }

    #[test]
    fn builds_are_visibly_different() {
        let widest = HERO_VISUALS
            .iter()
            .map(|v| v.body_scale.x)
            .fold(0.0_f32, f32::max);
        let narrowest = HERO_VISUALS
            .iter()
            .map(|v| v.body_scale.x)
            .fold(f32::INFINITY, f32::min);
        assert!(widest - narrowest >= 0.2, "重甲与修长体型须有明显剪影差");
    }
}
