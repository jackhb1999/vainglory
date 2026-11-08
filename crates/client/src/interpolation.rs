//! 实体插值系统（远端实体平滑渲染）

use crate::networking::{HeroRegistry, ReceivedSnapshot};
use crate::visual::VoxelMaterialPool;
use crate::voxel::{MinionVisual, PixelKind, spawn_voxel_minion};
use bevy::image::Image;
use bevy::prelude::*;
use game_core::components::*;
use game_core::math::{TickClock, lerp_quat, lerp_vec3};
use game_core::net::{EntitySnapshot, EntityType, Quatf, Vec3f};
use std::collections::{HashMap, HashSet, VecDeque};

/// 客户端用于渲染弹道的可视化标记
#[derive(Component)]
struct ProjectileVisual;

pub struct InterpolationPlugin;

impl Plugin for InterpolationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityHistory>().add_systems(
            FixedUpdate,
            (
                sync_projectiles,
                sync_minions,
                crate::entity_visual::sync_monsters,
                store_snapshot_history,
                interpolate_entities,
                sync_hero_stats,
                crate::entity_visual::tick_non_hero_anim,
            )
                .chain(),
        );
    }
}

#[derive(Resource, Default)]
pub struct EntityHistory {
    /// 每个实体的历史快照（最多保留 60 帧 = 1秒）
    pub histories: std::collections::HashMap<NetId, VecDeque<EntitySnapshot>>,
}

impl EntityHistory {
    pub fn add_snapshot(&mut self, snapshot: &EntitySnapshot) {
        let history = self.histories.entry(snapshot.net_id).or_default();
        history.push_back(snapshot.clone());
        if history.len() > 60 {
            history.pop_front();
        }
    }

    pub fn get_interpolated(
        &self,
        net_id: NetId,
        _target_tick: u64,
        alpha: f32,
    ) -> Option<EntitySnapshot> {
        let history = self.histories.get(&net_id)?;

        // EntitySnapshot 本身不携带 tick，这里在最近两帧之间做插值
        let len = history.len();
        if len == 0 {
            return None;
        }
        if len == 1 {
            return Some(history[0].clone());
        }

        let prev = &history[len - 2];
        let next = &history[len - 1];

        let p_pos: Vec3 = prev.position.into();
        let n_pos: Vec3 = next.position.into();
        let p_rot: Quat = prev.rotation.into();
        let n_rot: Quat = next.rotation.into();
        let p_vel: Vec3 = prev.velocity.into();
        let n_vel: Vec3 = next.velocity.into();

        let t = alpha.clamp(0.0, 1.0);
        let mut result = prev.clone();
        result.position = Vec3f::from_vec3(lerp_vec3(p_pos, n_pos, t));
        result.rotation = Quatf::from_quat(lerp_quat(p_rot, n_rot, t));
        result.velocity = Vec3f::from_vec3(lerp_vec3(p_vel, n_vel, t));
        Some(result)
    }
}

/// 根据服务端快照创建/回收弹道实体的可视化网格。
/// 英雄由 HeroSpawn 事件创建，而弹道只出现在快照实体列表中，故在此按需同步。
fn sync_projectiles(
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pool: ResMut<VoxelMaterialPool>,
    existing: Query<(Entity, &NetId), With<ProjectileVisual>>,
) {
    let Some(snapshot) = snapshot else { return };

    let mut seen: HashSet<NetId> = HashSet::new();
    let mut existing_map: HashMap<NetId, Entity> = HashMap::new();
    for (e, nid) in existing.iter() {
        existing_map.insert(*nid, e);
    }

    for es in &snapshot.0.entities {
        if es.entity_type != EntityType::Projectile {
            continue;
        }
        seen.insert(es.net_id);
        if existing_map.contains_key(&es.net_id) {
            continue;
        }

        // PRES-4：3D 弹体必须是轴对齐方块；语义色来自统一调色板，
        // 网格与特效材质都经 `VoxelMaterialPool` 复用，不再每次新建。
        let kind = match es.team {
            Team::Blue => PixelKind::Blue,
            Team::Red => PixelKind::Red,
            Team::Neutral => PixelKind::Marker,
        };
        let cube = pool.cube(&mut meshes);
        let fx = pool.fx_material(&mut images, &mut materials, kind, 61);
        commands.spawn((
            Mesh3d(cube),
            MeshMaterial3d(fx),
            Transform::from_translation(es.position.into()),
            es.net_id,
            es.team,
            ProjectileVisual,
            Interpolated,
            TransformSync::default(),
        ));
    }

    // 回收已不在快照中的弹道
    for (nid, e) in existing_map {
        if !seen.contains(&nid) {
            commands.entity(e).despawn();
        }
    }
}

/// 根据快照创建 / 回收小兵可视化实体。
/// 小兵只在视野内出现在快照中，因此“消失”时直接回收即可。
fn sync_minions(
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<(Entity, &NetId), With<MinionVisual>>,
) {
    let Some(snapshot) = snapshot else { return };
    if snapshot.0.tick == 0 {
        return; // 尚未收到有效快照
    }

    let mut seen: HashSet<NetId> = HashSet::new();
    let mut existing_map: HashMap<NetId, Entity> = HashMap::new();
    for (e, nid) in existing.iter() {
        existing_map.insert(*nid, e);
    }

    for es in &snapshot.0.entities {
        if es.entity_type != EntityType::Minion {
            continue;
        }
        seen.insert(es.net_id);
        if existing_map.contains_key(&es.net_id) {
            continue;
        }
        let entity = spawn_voxel_minion(
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
            MinionVisual,
        ));
    }

    for (nid, e) in existing_map {
        if !seen.contains(&nid) {
            commands.entity(e).despawn();
        }
    }
}

fn store_snapshot_history(
    mut history: ResMut<EntityHistory>,
    snapshot: Option<Res<ReceivedSnapshot>>,
) {
    let Some(snapshot) = snapshot else { return };

    for entity_snap in &snapshot.0.entities {
        history.add_snapshot(entity_snap);
    }
}

fn interpolate_entities(
    tick_clock: Res<TickClock>,
    history: Res<EntityHistory>,
    mut query: Query<(&mut Transform, &mut TransformSync, &NetId), With<Interpolated>>,
) {
    let target_tick = tick_clock.current_tick;
    let alpha = tick_clock.interpolation_alpha();

    for (mut transform, mut sync, net_id) in query.iter_mut() {
        if let Some(interpolated) = history.get_interpolated(*net_id, target_tick, alpha) {
            transform.translation = interpolated.position.into();
            transform.rotation = interpolated.rotation.into();
            sync.position = interpolated.position.into();
            sync.rotation = interpolated.rotation.into();
            sync.velocity = interpolated.velocity.into();
        }
    }
}

/// 用服务端快照同步所有英雄的数值面板（含本地预测英雄）。
///
/// 之前只同步 `Interpolated` 敌人，导致本地玩家的血量 / 能量 / 等级永远停留在初始值，
/// 本地血条与 HUD 实际“显示异常”。这里按 NetId 统一写回，位置仍由预测/插值负责。
fn sync_hero_stats(
    snapshot: Option<Res<ReceivedSnapshot>>,
    registry: Res<HeroRegistry>,
    mut heroes: Query<(&NetId, &mut Hero, Option<&mut HeroAbilities>)>,
) {
    let Some(snapshot) = snapshot else { return };
    if snapshot.0.tick == 0 {
        return;
    }
    for es in &snapshot.0.entities {
        if es.entity_type != EntityType::Hero {
            continue;
        }
        let Some(&entity) = registry.0.get(&es.net_id) else {
            continue;
        };
        let Ok((_, mut hero, abilities)) = heroes.get_mut(entity) else {
            continue;
        };
        hero.health = es.health;
        hero.max_health = es.max_health;
        hero.energy = es.energy;
        hero.max_energy = es.max_energy;
        hero.level = es.level;
        hero.gold = es.gold;
        if let Some(mut abilities) = abilities {
            abilities.cooldowns = es.ability_cooldowns;
        }
    }
}
