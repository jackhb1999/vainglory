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

/// 死亡后「缩小消失」动画时长（秒）。服务端已 despawn 实体（碰撞体随之移除），
/// 客户端仅据此延迟回收可视体并播放收缩动画，不回写任何裁决组件。
pub const DYING_TIME: f32 = 0.4;

/// 正在播放消失动画的实体（按 `NetId`），值为剩余秒数。纯表现层延迟回收。
#[derive(Resource, Default)]
pub struct DyingMinions(pub std::collections::HashMap<NetId, f32>);

pub struct InterpolationPlugin;

impl Plugin for InterpolationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityHistory>()
            .init_resource::<DyingMinions>()
            .add_systems(
                FixedUpdate,
                (
                    sync_projectiles,
                    sync_minions,
                    crate::entity_visual::sync_monsters,
                    store_snapshot_history,
                    interpolate_entities,
                    sync_hero_stats,
                    sync_world_health_bars,
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
/// 敌人死亡判定：实体从快照视野消失（`!seen`）且尚未进入消失动画时，
/// 应登记为缩小消失动画（而非立即移除）。抽成纯函数便于单测锁定不变量。
pub fn should_start_dying(seen: &HashSet<NetId>, nid: NetId, dying: &DyingMinions) -> bool {
    !seen.contains(&nid) && !dying.0.contains_key(&nid)
}

/// 小兵只在视野内出现在快照中，因此“消失”时直接回收即可。
#[allow(clippy::too_many_arguments)]
fn sync_minions(
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<(Entity, &NetId), With<MinionVisual>>,
    time: Res<Time>,
    mut transforms: Query<&mut Transform>,
    mut dying: ResMut<DyingMinions>,
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
            // 让小兵可被点击锁定（之前缺此组件，右键锁定查询只匹配到英雄）。
            Selectable {
                radius: 0.8,
                height: 2.0,
            },
            crate::rendering::WorldHealthBar {
                health: es.health,
                max_health: es.max_health,
                y_offset: 2.8,
            },
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
            // 实体从快照消失（服务端已 despawn，碰撞体随之移除）：触发缩小消失动画，
            // 而非立即移除，避免「只隐藏血条、实体仍占位」的观感。
            if should_start_dying(&seen, *nid, &dying) {
                dying.0.insert(*nid, DYING_TIME);
            }
        }
    }
}

/// 用权威快照刷新所有非英雄实体的 `WorldHealthBar`（野怪 / 小兵 / 哨塔）。
/// 英雄血量由 `sync_hero_stats` 写入 `Hero`；这里只处理带 NetId 的世界实体。
fn sync_world_health_bars(
    snapshot: Option<Res<ReceivedSnapshot>>,
    mut bars: Query<(&NetId, &mut crate::rendering::WorldHealthBar)>,
) {
    let Some(snapshot) = snapshot else { return };
    if snapshot.0.tick == 0 {
        return;
    }
    let hp: HashMap<u64, (f32, f32)> = snapshot
        .0
        .entities
        .iter()
        .map(|e| (e.net_id.0, (e.health, e.max_health)))
        .collect();
    for (nid, mut bar) in bars.iter_mut() {
        if let Some(&(health, max_health)) = hp.get(&nid.0) {
            bar.health = health;
            bar.max_health = max_health;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn minion_disappearance_triggers_dying_animation() {
        let mut seen = HashSet::new();
        seen.insert(NetId(1));
        let dying = DyingMinions::default();

        // 视野内仍可见的实体不应触发消失动画
        assert!(
            !should_start_dying(&seen, NetId(1), &dying),
            "可见实体不应进入消失动画"
        );
        // 从快照消失且尚未动画的实体应触发
        assert!(
            should_start_dying(&seen, NetId(2), &dying),
            "消失的实体应进入消失动画"
        );
        // 已处于消失动画的实体不重复登记
        let mut dying2 = DyingMinions::default();
        dying2.0.insert(NetId(2), 0.4);
        assert!(
            !should_start_dying(&seen, NetId(2), &dying2),
            "已动画实体不应重复登记"
        );
    }
}
