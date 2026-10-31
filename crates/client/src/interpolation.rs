//! 实体插值系统（远端实体平滑渲染）

use bevy::prelude::*;
use game_core::components::*;
use game_core::net::{WorldSnapshot, EntitySnapshot, EntityType, Vec3f, Quatf};
use game_core::math::{lerp_vec3, lerp_quat, TickClock};
use crate::networking::ReceivedSnapshot;
use std::collections::{HashMap, HashSet, VecDeque};

/// 客户端用于渲染弹道的可视化标记
#[derive(Component)]
struct ProjectileVisual;

pub struct InterpolationPlugin;

impl Plugin for InterpolationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityHistory>()
            .add_systems(FixedUpdate, (
                sync_projectiles,
                store_snapshot_history,
                interpolate_entities,
            ).chain());
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
    
    pub fn get_interpolated(&self, net_id: NetId, target_tick: u64, alpha: f32) -> Option<EntitySnapshot> {
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
    mut materials: ResMut<Assets<StandardMaterial>>,
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

        let color = match es.team {
            Team::Blue => Color::srgb(0.3, 0.6, 1.0),
            Team::Red => Color::srgb(1.0, 0.5, 0.2),
            Team::Neutral => Color::srgb(0.7, 0.7, 0.7),
        };
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
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