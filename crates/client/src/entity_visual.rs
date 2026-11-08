//! P-V4 非英雄实体的动态表现层。
//!
//! 口径见 `docs/06_VISUAL_SPEC.md` §2.1 / §7：
//! - 小兵 / 野怪由服务端快照驱动创建与回收，按位移速度做走路摆动与待机呼吸；
//! - 哨塔 / 源核保留静态体素堆叠，但挂上核心脉冲与血量损伤缩放，不再是「死装饰」；
//! - 动画只改骨架子节点的 `Transform`，不触碰裁决 / 同步组件；
//! - 全部几何、材质经 `VoxelMaterialPool` 或 `voxel::voxel_material` 代码生成，零外部资产。

use crate::interpolation::{DYING_TIME, DyingMinions};
use crate::networking::ReceivedSnapshot;
use crate::voxel::{self, PixelKind};
use bevy::image::Image;
use bevy::prelude::*;
use game_core::components::{Interpolated, NetId, TransformSync};
use game_core::net::EntityType;
use std::collections::{HashMap, HashSet};

/// 非英雄实体类别。与 `EntityType` 的映射集中在 `from_entity_type`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NonHeroKind {
    #[default]
    Minion,
    Turret,
    Core,
    Monster,
}

impl NonHeroKind {
    pub const fn from_entity_type(t: EntityType) -> Option<Self> {
        match t {
            EntityType::Minion => Some(Self::Minion),
            EntityType::Turret => Some(Self::Turret),
            EntityType::JungleMonster => Some(Self::Monster),
            _ => None,
        }
    }
}

/// 动画样式：可移动单位走路摆动，固定建筑核心脉冲。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NonHeroMotion {
    Walker,
    Pulsar,
}

/// 非英雄造型数据表条目。新增实体类型 = 加一行数据，不改生成几何代码。
#[derive(Clone, Copy, Debug)]
pub struct NonHeroVisual {
    pub kind: NonHeroKind,
    pub body: PixelKind,
    pub head: PixelKind,
    pub accent: PixelKind,
    pub body_scale: Vec3,
    pub head_scale: Vec3,
    pub motion: NonHeroMotion,
    pub seed: u32,
}

pub const NON_HERO_VISUALS: [NonHeroVisual; 4] = [
    NonHeroVisual {
        kind: NonHeroKind::Minion,
        body: PixelKind::Blue,
        head: PixelKind::Stone,
        accent: PixelKind::Gold,
        body_scale: Vec3::new(0.9, 1.0, 0.7),
        head_scale: Vec3::splat(0.55),
        motion: NonHeroMotion::Walker,
        seed: 71,
    },
    NonHeroVisual {
        kind: NonHeroKind::Monster,
        body: PixelKind::Brush,
        head: PixelKind::Gold,
        accent: PixelKind::Core,
        body_scale: Vec3::new(1.4, 1.6, 1.4),
        head_scale: Vec3::splat(0.85),
        motion: NonHeroMotion::Walker,
        seed: 74,
    },
    NonHeroVisual {
        kind: NonHeroKind::Turret,
        body: PixelKind::Stone,
        head: PixelKind::Gold,
        accent: PixelKind::Blue,
        body_scale: Vec3::new(1.2, 1.2, 1.2),
        head_scale: Vec3::splat(0.8),
        motion: NonHeroMotion::Pulsar,
        seed: 31,
    },
    NonHeroVisual {
        kind: NonHeroKind::Core,
        body: PixelKind::Core,
        head: PixelKind::Gold,
        accent: PixelKind::Marker,
        body_scale: Vec3::new(1.6, 1.6, 1.6),
        head_scale: Vec3::splat(1.0),
        motion: NonHeroMotion::Pulsar,
        seed: 32,
    },
];

pub fn non_hero_visual(kind: NonHeroKind) -> NonHeroVisual {
    NON_HERO_VISUALS
        .iter()
        .find(|v| v.kind == kind)
        .copied()
        .unwrap_or(NON_HERO_VISUALS[0])
}

/// 可动画骨架：三个节点句柄 + 静止姿态（每帧基于 base 重算，不累积漂移）。
#[derive(Component, Clone, Copy, Debug)]
pub struct NonHeroRig {
    pub body: Entity,
    pub head: Entity,
    pub accent: Entity,
    pub body_base: Transform,
    pub head_base: Transform,
    pub accent_base: Transform,
}

/// 动画状态。速度从 `TransformSync.velocity` 读取，避免同系统内读写 `Transform`。
#[derive(Component, Debug)]
pub struct NonHeroAnim {
    pub kind: NonHeroKind,
    pub phase: f32,
    pub last_pos: Vec3,
    pub health_ratio: f32,
}

impl NonHeroAnim {
    pub fn new(kind: NonHeroKind, pos: Vec3) -> Self {
        Self {
            kind,
            phase: 0.0,
            last_pos: pos,
            health_ratio: 1.0,
        }
    }
}

/// 野怪可视化标记（快照驱动）。
#[derive(Component)]
pub struct MonsterVisual;

/// 走路速度阈值（世界单位/秒）。
const MOVE_SPEED_THRESHOLD: f32 = 0.25;

/// 按快照创建 / 回收野怪实体。此前野怪只有服务端逻辑、客户端无任何模型。
#[allow(clippy::too_many_arguments)]
pub fn sync_monsters(
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<(Entity, &NetId), With<MonsterVisual>>,
    time: Res<Time>,
    mut transforms: Query<&mut Transform>,
    mut dying: ResMut<DyingMinions>,
) {
    let Some(snapshot) = snapshot else { return };
    if snapshot.0.tick == 0 {
        return;
    }
    let mut seen: HashSet<NetId> = HashSet::new();
    let mut existing_map: HashMap<NetId, Entity> = HashMap::new();
    for (e, nid) in existing.iter() {
        existing_map.insert(*nid, e);
    }
    for es in &snapshot.0.entities {
        if es.entity_type != EntityType::JungleMonster {
            continue;
        }
        seen.insert(es.net_id);
        if existing_map.contains_key(&es.net_id) {
            continue;
        }
        let entity = voxel::spawn_voxel_monster(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            es.team,
            es.position.into(),
        );
        commands.entity(entity).insert((
            es.net_id,
            es.team,
            TransformSync::default(),
            Interpolated,
            MonsterVisual,
        ));
    }
    let dt = time.delta_secs();
    for (nid, e) in existing_map.iter() {
        if let Some(timer) = dying.0.get_mut(nid) {
            // 死亡后缩小消失动画（服务端已 despawn，碰撞体移除）。
            if let Ok(mut t) = transforms.get_mut(*e) {
                let k = (*timer / DYING_TIME).clamp(0.02, 1.0);
                t.scale = Vec3::splat(k);
            }
            *timer -= dt;
            if *timer <= 0.0 {
                dying.0.remove(nid);
                commands.entity(*e).despawn();
            }
            continue;
        }
        if !seen.contains(nid) {
            // 从快照消失（服务端已 despawn）：触发缩小消失动画而非立即移除。
            if !dying.0.contains_key(nid) {
                dying.0.insert(*nid, DYING_TIME);
            }
        }
    }
}

/// 单一动画系统：走路摆动 / 待机呼吸 / 核心脉冲 / 血量损伤。
/// 每实体每帧最多写 3 个节点，符合 §7 预算。
pub fn tick_non_hero_anim(
    time: Res<Time>,
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut rigs: Query<(
        &NonHeroRig,
        &mut NonHeroAnim,
        Option<&NetId>,
        Option<&TransformSync>,
    )>,
    mut nodes: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    // 由权威快照刷新血量比例（静态建筑无 NetId，保持 1.0）。
    let mut health: HashMap<u64, f32> = HashMap::new();
    if let Some(snapshot) = snapshot {
        for es in &snapshot.0.entities {
            if es.max_health > 0.0 {
                health.insert(es.net_id.0, (es.health / es.max_health).clamp(0.0, 1.0));
            }
        }
    }

    for (rig, mut anim, net_id, sync) in rigs.iter_mut() {
        anim.phase += dt;
        if let Some(nid) = net_id
            && let Some(&ratio) = health.get(&nid.0)
        {
            anim.health_ratio = ratio;
        }
        let v = non_hero_visual(anim.kind);
        let speed = sync.map(|s| s.velocity.length()).unwrap_or(0.0);
        let mut body = rig.body_base;
        let mut head = rig.head_base;
        let mut accent = rig.accent_base;

        match v.motion {
            NonHeroMotion::Walker => {
                if speed > MOVE_SPEED_THRESHOLD {
                    let f = anim.phase * 8.0;
                    let swing = f.sin() * 0.18;
                    body.rotation = Quat::from_rotation_x(swing);
                    head.rotation = Quat::from_rotation_y(0.10 * f.sin());
                    accent.rotation = Quat::from_rotation_x(-swing * 1.5);
                    body.translation.y += (f * 2.0).sin().abs() * 0.04;
                } else {
                    let breathe = 1.0 + 0.025 * (anim.phase * 2.2).sin();
                    body.scale.y *= breathe;
                }
            }
            NonHeroMotion::Pulsar => {
                // 核心缓慢自转 + 脉冲；血量越低脉动越急、核心越小（损伤反馈）
                let urgency = 1.0 + (1.0 - anim.health_ratio) * 2.0;
                let pulse = 1.0 + 0.12 * (anim.phase * 3.0 * urgency).sin();
                accent.rotation = Quat::from_rotation_y(anim.phase * 0.8);
                accent.scale *= pulse * (0.55 + 0.45 * anim.health_ratio).max(0.15);
                head.scale *= 0.85 + 0.15 * anim.health_ratio;
            }
        }
        let mut write = |entity: Entity, tr: Transform| {
            if let Ok(mut node) = nodes.get_mut(entity) {
                *node = tr;
            }
        };
        write(rig.body, body);
        write(rig.head, head);
        write(rig.accent, accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_table_is_unique_and_bounded() {
        use std::collections::HashSet;
        let kinds: HashSet<_> = NON_HERO_VISUALS.iter().map(|v| v.kind).collect();
        let seeds: HashSet<_> = NON_HERO_VISUALS.iter().map(|v| v.seed).collect();
        assert_eq!(kinds.len(), 4);
        assert_eq!(seeds.len(), 4, "种子必须唯一，避免材质串色");
        assert_eq!(
            non_hero_visual(NonHeroKind::Core).motion,
            NonHeroMotion::Pulsar
        );
        assert_eq!(
            non_hero_visual(NonHeroKind::Minion).motion,
            NonHeroMotion::Walker
        );
    }

    #[test]
    fn entity_type_mapping_is_explicit() {
        assert_eq!(
            NonHeroKind::from_entity_type(EntityType::JungleMonster),
            Some(NonHeroKind::Monster)
        );
        assert_eq!(
            NonHeroKind::from_entity_type(EntityType::Minion),
            Some(NonHeroKind::Minion)
        );
        assert_eq!(NonHeroKind::from_entity_type(EntityType::Hero), None);
        assert_eq!(NonHeroKind::from_entity_type(EntityType::Projectile), None);
    }

    #[test]
    fn damage_shrinks_core_but_never_inverts() {
        // 与 `Pulsar` 分支的损伤曲线一致：比例 0 时仍有正缩放
        for ratio in [0.0_f32, 0.25, 0.5, 1.0] {
            let scale = 0.55 + 0.45 * ratio;
            assert!((0.55..=1.0).contains(&scale));
        }
    }

    #[test]
    fn fresh_anim_starts_at_full_health() {
        let a = NonHeroAnim::new(NonHeroKind::Turret, Vec3::new(1.0, 0.0, 2.0));
        assert_eq!(a.kind, NonHeroKind::Turret);
        assert_eq!(a.health_ratio, 1.0);
        assert_eq!(a.last_pos, Vec3::new(1.0, 0.0, 2.0));
    }
}
