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

/// 单个玩家的**可见源**（VIS-3/4）：己方英雄恒为源，塔 / 小兵受策略开关控制。
///
/// 客户端战争迷雾直接复用本函数的结果（由服务端逐玩家下发），因此遮罩口径与
/// `compute_visibility` 的实体裁剪**严格一致**，不会出现「看得见敌人但地图是黑的」
/// 或「地图亮着却收不到敌人」。
pub fn vision_sources(
    viewer_team: Team,
    policy: &VisibilityPolicy,
    entities: &[EntitySnapshot],
) -> Vec<[f32; 3]> {
    entities
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
        .collect()
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
    let sources = vision_sources(viewer_team, policy, entities);
    let mut visible = Vec::with_capacity(entities.len());
    for e in entities {
        // 野怪是固定地图设施（营地点已知、不会移动出区域），必须始终下发：
        // 否则 18 单位视野外的营地既看不到模型、也无法锁定攻击，更没有血条。
        if e.entity_type == EntityType::JungleMonster {
            visible.push(e.net_id.0);
            continue;
        }
        // 阵亡英雄会被立即吸附到已方泉水（远离交战点）。若按常规视野裁剪，
        // 击杀者收不到 health=0 的快照、HeroDeath 事件也会被 event_visible 丢弃，
        // 于是客户端的敌方模型永远停在死亡位置 —— 正是「血打完人不消失」。
        // 尸体位置只有泉水这一公开信息，强制下发 health=0 不会泄露战术信息。
        if e.entity_type == EntityType::Hero && e.health <= 0.0 {
            visible.push(e.net_id.0);
            continue;
        }
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
            entity(2, Team::Blue, EntityType::Hero, 140.0, -40.0),
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
            entity(3, Team::Red, EntityType::Hero, 140.0, 0.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(set.contains(2));
        assert!(!set.contains(3));
    }

    /// 阵亡英雄被吸附到远处泉水后，仍必须下发 health=0，客户端才能隐藏其模型。
    #[test]
    fn dead_enemy_hero_is_always_visible() {
        // 阵亡英雄被服务端吸附到红方泉水 `(140,-40)`，远离击杀者视野
        let mut dead = entity(3, Team::Red, EntityType::Hero, 140.0, -40.0);
        dead.health = 0.0;
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
            dead,
            // 对照组：活着的远处敌人仍应被裁剪
            entity(4, Team::Red, EntityType::Hero, 140.0, -30.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(set.contains(3), "阵亡英雄必须下发，否则客户端看不到死亡");
        assert!(!set.contains(4), "存活的远处敌人仍应被裁剪");
    }

    /// 野怪是静态地图设施：即使远在视野外，也必须下发，否则客户端无法建模 / 锁定 / 显示血条。
    #[test]
    fn jungle_monsters_are_always_visible() {
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
            entity(2, Team::Neutral, EntityType::JungleMonster, 120.0, 60.0),
            // 对照组：同样距离的中立实体仍受视野裁剪
            entity(3, Team::Neutral, EntityType::Minion, 120.0, 60.0),
        ];
        let set = compute_visibility(Team::Blue, &policy(), &ents);
        assert!(set.contains(2), "远处野怪必须可见");
        assert!(!set.contains(3), "视野裁剪仍应对其他中立实体生效");
    }

    #[test]
    fn brush_requires_smaller_radius() {
        // 从真实草丛取坐标（地图等比放大后硬编码的 -24/-8 会失效）：
        // 敌人站在草丛中心，观察者在 8 单位外（> brush_reveal_radius=6，但 < 普通视野 18）。
        let b = BUSHES
            .iter()
            .find(|b| b.center.x < 0.0)
            .expect("地图必须有蓝方半区草丛");
        let (cx, cz) = (b.center.x, b.center.z);
        let ents = vec![
            entity(1, Team::Blue, EntityType::Hero, cx, cz - 8.0),
            entity(2, Team::Red, EntityType::Hero, cx, cz),
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
