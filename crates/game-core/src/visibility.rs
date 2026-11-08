//! 服务端视野判定（`GX-ARCH-2026-001` §8.5 VIS-1~9 / `FR-60` / `CR-05`）。
//!
//! 纯函数、无 ECS 依赖，便于 `TC-NET-03` 直接断言"视野外实体零下发"。

use crate::map::BUSHES;
use crate::net::{EntitySnapshot, EntityType, GameEvent, Team, VisibilityPolicy, VisibilitySet};

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn position_of(e: &EntitySnapshot) -> [f32; 3] {
    [e.position.x, e.position.y, e.position.z]
}

/// 是否处于遮蔽区（一期草丛；VIS-5）。
pub fn in_brush(position: [f32; 3]) -> bool {
    BUSHES.iter().any(|b| {
        let c = [b.center.x, b.center.y, b.center.z];
        distance(c, position) <= b.radius
    })
}

/// 为单个玩家计算可见实体集合。
///
/// - VIS-2：己方阵营实体始终可见；
/// - VIS-3/4：敌方与中立实体须落在己方可见源半径内；
/// - VIS-5：遮蔽区内使用更小的 `brush_reveal_radius`。
pub fn compute_visibility(
    viewer_team: Team,
    policy: &VisibilityPolicy,
    entities: &[EntitySnapshot],
) -> VisibilitySet {
    let sources: Vec<[f32; 3]> = entities
        .iter()
        .filter(|e| {
            if e.team != viewer_team {
                return false;
            }
            match e.entity_type {
                EntityType::Hero => true,
                EntityType::Turret => policy.sentries_provide_vision,
                EntityType::Minion => policy.constructs_provide_vision,
                _ => false,
            }
        })
        .map(position_of)
        .collect();

    let mut visible = Vec::with_capacity(entities.len());
    for e in entities {
        if e.team == viewer_team {
            visible.push(e.net_id.0);
            continue;
        }
        if sources.is_empty() {
            continue;
        }
        let pos = position_of(e);
        let nearest = sources
            .iter()
            .fold(f32::INFINITY, |acc, s| acc.min(distance(*s, pos)));
        let limit = if in_brush(pos) {
            policy.brush_reveal_radius
        } else {
            policy.vision_radius
        };
        if nearest <= limit {
            visible.push(e.net_id.0);
        }
    }
    VisibilitySet { visible }
}

/// 事件过滤（VIS-6 / VIS-7）：全局状态事件可广播，涉及不可见实体的事件一律丢弃。
pub fn event_visible(event: &GameEvent, set: &VisibilitySet) -> bool {
    if event.is_global() {
        return true;
    }
    event.involved_net_ids().iter().all(|id| set.contains(*id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::NetId;
    use crate::net::{Quatf, Vec3f};

    fn entity(net_id: u64, team: Team, kind: EntityType, x: f32, z: f32) -> EntitySnapshot {
        EntitySnapshot {
            net_id: NetId(net_id),
            entity_type: kind,
            team,
            position: Vec3f::new(x, 0.0, z),
            rotation: Quatf::identity(),
            velocity: Vec3f::zero(),
            health: 100.0,
            max_health: 100.0,
            energy: 0.0,
            max_energy: 0.0,
            level: 1,
            gold: 0,
            ability_cooldowns: [0.0; 4],
            status_mask: 0,
            shield: 0.0,
        }
    }

    fn policy() -> VisibilityPolicy {
        VisibilityPolicy {
            vision_radius: 18.0,
            brush_reveal_radius: 6.0,
            constructs_provide_vision: true,
            sentries_provide_vision: true,
        }
    }

    #[test]
    fn own_team_always_visible() {
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
            entity(2, Team::Blue, EntityType::Hero, 90.0, 90.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(set.contains(1));
        assert!(set.contains(2));
    }

    #[test]
    fn distant_enemy_is_culled_near_enemy_is_kept() {
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
            entity(2, Team::Red, EntityType::Hero, 5.0, 0.0),
            entity(3, Team::Red, EntityType::Hero, 100.0, 0.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(set.contains(2));
        assert!(!set.contains(3));
    }

    #[test]
    fn brush_requires_smaller_radius() {
        // BUSHES 含 (-24,0,-8) r=4；把敌方放在草丛边沿，8 单位外（>6 但 <18）
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, -24.0, -16.0),
            entity(2, Team::Red, EntityType::Hero, -24.0, -8.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(!set.contains(2), "brush-hidden enemy must not be visible");
    }

    #[test]
    fn global_events_pass_entity_events_are_filtered() {
        let set = VisibilitySet { visible: vec![1] };
        let hidden = GameEvent::DamageDealt {
            attacker: NetId(1),
            target: NetId(99),
            damage: 10.0,
            is_critical: false,
        };
        assert!(!event_visible(&hidden, &set));
        let global = GameEvent::ObjectiveCaptured {
            objective_id: "lode_shrine".into(),
            owner: Team::Blue,
            event_id: 1,
        };
        assert!(event_visible(&global, &set));
    }
}
