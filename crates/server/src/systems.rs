//! 服务端系统（复用 game-core 的系统 + 技能/弹道等服务器专属系统）

use bevy::prelude::*;
use game_core::abilities::*;
use game_core::components::*;
use game_core::map::{MAP_H_HALF, MAP_W_HALF};
use game_core::net::{GameEvent, Vec3f};
use game_core::systems::*;

use crate::networking::{NextNetId, ServerEvents};

// 重新导出 game-core 的系统供服务端使用
pub use game_core::systems::*;

/// 技能施放系统：读取 `PlayerInput.ability_mask`（bit0 保留给普攻，bit1-4 = 技能槽 0-3），
/// 校验冷却/蓝量后发起施法。命中裁决全部在服务端完成。
pub fn ability_cast_system(
    mut commands: Commands,
    mut server_events: ResMut<ServerEvents>,
    mut next_net_id: ResMut<NextNetId>,
    enemies: Query<(Entity, &Transform, &Team)>,
    mut heroes: Query<(
        Entity,
        &NetId,
        &Team,
        &Transform,
        &PlayerInput,
        &mut Hero,
        &mut HeroAbilities,
    )>,
) {
    // 先快照敌方英雄位置，避免与下方对英雄的 &mut 查询产生借用冲突
    let enemy_snapshot: Vec<(Entity, Vec3, Team)> = enemies
        .iter()
        .map(|(e, t, team)| (e, t.translation, *team))
        .collect();

    for (entity, net_id, team, transform, input, mut hero, mut abilities) in heroes.iter_mut() {
        let mask = input.ability_mask;
        for slot in 0..4u8 {
            // bit0 留给普攻，技能占用 bit1..=bit4
            let bit = 1u8 << (slot + 1);
            if mask & bit == 0 {
                continue;
            }
            if abilities.cooldowns[slot as usize] > 0.0 {
                continue; // 冷却中
            }
            let ability_id = abilities.ability_ids[slot as usize];
            let def = get_ability_def(ability_id);
            if hero.energy < def.mana_cost {
                continue; // 蓝量不足
            }

            // 扣蓝 + 进入冷却
            hero.energy -= def.mana_cost;
            abilities.cooldowns[slot as usize] = def.cooldown;

            let caster_pos = transform.translation;
            // `aim_dir` 是鼠标在地面上的**绝对世界坐标** (x,z)，不是方向向量。
            // 拒绝零值 / NaN / 越界：旧版把客户端相机查询失败时上报的 (0,0)
            // 当成合法目标，技能因此固定打向地图原点。
            let aim_xz = Vec2::new(input.aim_dir.x, input.aim_dir.y);
            let valid = aim_xz.is_finite()
                && aim_xz.length_squared() > 1e-4
                && aim_xz.x.abs() <= MAP_W_HALF
                && aim_xz.y.abs() <= MAP_H_HALF;
            // 距离取水平距离：旧版用 3D `length()` 会被施法者的 y 分量污染。
            let flat = Vec2::new(aim_xz.x - caster_pos.x, aim_xz.y - caster_pos.z);
            let dist = if valid { flat.length() } else { 0.0 };
            let aim = if dist > 1e-4 {
                Vec3::new(flat.x / dist, 0.0, flat.y / dist)
            } else {
                Vec3::new(0.0, 0.0, -1.0)
            };
            // 射程内直接采用上报点（= 鼠标点击处），超出射程压到射程边缘；
            // 无效目标不再发射到地图原点，落点留在施法者脚下。
            let center = if valid {
                let reach = dist.min(def.range);
                Vec3::new(
                    caster_pos.x + aim.x * reach,
                    0.0,
                    caster_pos.z + aim.z * reach,
                )
            } else {
                caster_pos
            };
            let damage = def.damage(hero.attack_damage, hero.ability_power);

            match def.cast_type {
                CastType::Projectile => {
                    let proj_net_id = NetId(next_net_id.0);
                    next_net_id.0 += 1;
                    let start = caster_pos + aim * 1.5; // 略微前移，避免卡在施法者身上
                    commands.spawn((
                        proj_net_id,
                        *team,
                        Transform::from_translation(start),
                        TransformSync {
                            position: start,
                            ..default()
                        },
                        Projectile {
                            owner: *net_id,
                            team: *team,
                            damage,
                            damage_type: def.damage_type,
                            direction: aim,
                            speed: def.projectile_speed,
                            traveled: 0.0,
                            max_range: def.range,
                            radius: def.aoe_radius.max(1.2),
                            dead: false,
                        },
                        ServerAuthority,
                    ));
                }
                CastType::Instant | CastType::PointTarget => {
                    if def.heal_amount > 0.0 {
                        // 瞬发治疗
                        hero.health = (hero.health + def.heal_amount).min(hero.max_health);
                    } else {
                        // 指向/瞬发 AoE：在落点（已按射程夹紧）对范围内敌人施加待结算伤害
                        for (e_ent, e_pos, e_team) in &enemy_snapshot {
                            if *e_team == *team {
                                continue;
                            }
                            if e_pos.distance(center) <= def.aoe_radius + 1.0 {
                                commands.entity(*e_ent).insert(PendingHit {
                                    attacker: entity,
                                    damage,
                                    damage_type: def.damage_type,
                                });
                            }
                        }
                    }
                }
            }

            server_events.0.push(GameEvent::AbilityCast {
                caster: *net_id,
                ability_index: slot,
                target_position: Vec3f::from_vec3(center),
                target_entity: None,
            });
        }
    }
}

/// 弹道系统：推进弹体、检测命中、命中后通过 AttackCommand 走统一伤害结算。
pub fn projectile_system(
    mut commands: Commands,
    time: Res<SimTime>,
    heroes: Query<(Entity, &Transform, &Team)>,
    mut projectiles: Query<(Entity, &mut TransformSync, &mut Projectile)>,
) {
    // 权威固定步长：不用墙钟 delta，保证命中裁决与回放可复现。
    let dt = time.delta_secs();
    let current_tick = time.tick();

    // 先快照英雄位置（只读 Transform），再单独遍历弹体（只写 TransformSync，避免读写冲突）
    let hero_snapshot: Vec<(Entity, Vec3, Team)> = heroes
        .iter()
        .map(|(e, t, team)| (e, t.translation, *team))
        .collect();

    for (proj_entity, mut sync, mut proj) in projectiles.iter_mut() {
        if proj.dead {
            commands.entity(proj_entity).despawn();
            continue;
        }

        let step = proj.speed * dt;
        let mut pos = sync.position;
        pos += proj.direction * step;
        sync.position = pos;
        proj.traveled += step;

        // 命中检测：敌方英雄
        let mut hit_target: Option<Entity> = None;
        for (e_ent, e_pos, e_team) in &hero_snapshot {
            if *e_team == proj.team {
                continue;
            }
            if e_pos.distance(pos) <= proj.radius {
                hit_target = Some(*e_ent);
                break;
            }
        }

        if let Some(target) = hit_target {
            // 复用 AttackCommand 统一结算（attacker = 弹体自身）
            commands.entity(proj_entity).insert(AttackCommand {
                target,
                attacker: proj_entity,
                damage: proj.damage,
                damage_type: proj.damage_type,
                tick: current_tick,
            });
            proj.dead = true;
        } else if proj.traveled >= proj.max_range {
            proj.dead = true;
        }
    }
}

/// 结算指向/瞬发 AoE 的待结算伤害（PendingHit）。
///
/// 具备 `RespawnState` 的英雄绝不在此 despawn：技能致死也必须走 `death_respawn_system`
/// 的统一死亡/重生流程，否则「被技能打死」的英雄会永久消失、永不复活。
pub fn resolve_pending_hits(
    mut commands: Commands,
    mut hits: Query<(
        Entity,
        &mut Hero,
        &PendingHit,
        Option<&TrainingDummy>,
        Option<&game_core::gameplay::respawn::RespawnState>,
    )>,
    mut damage_sink: Option<ResMut<game_core::components::DamageSink>>,
) {
    for (entity, mut hero, hit, dummy, respawn_state) in hits.iter_mut() {
        let dealt = apply_mitigated_damage(&mut hero, hit.damage, hit.damage_type);
        if let Some(sink) = damage_sink.as_deref_mut() {
            sink.0.push((hit.attacker, entity, dealt, hit.damage_type));
        }
        if hero.health <= 0.0 && dummy.is_none() && respawn_state.is_none() {
            commands.entity(entity).despawn();
        }
        commands.entity(entity).remove::<PendingHit>();
    }
}

/// 河道浅滩的**权威持续伤害**（每 0.5 s 结算一次）。
///
/// - 只有 `is_hazardous_river` 为真（在河里且不在桥面）的**英雄**受伤：
///   小兵沿兵线走桥、野怪是固定设施，都不该被环境伤害磨死。
/// - 阵亡 / 复活无敌期的英雄免伤；真实伤害不吃护甲减免。
/// - 扣血只写 `Hero.health`，死亡判定统一交给 `death_respawn_system`
///   （它以权威血量登记死亡，所以淹死也能正常进复活流程、广播 HeroDeath）。
/// - 广播 `EnvironmentDamage` 而不是 `DamageDealt`：环境伤害没有攻击者，
///   走 DamageDealt 会让客户端误播一次攻击者的挥击动画。
pub fn river_hazard_system(
    tick: Res<crate::simulation::ServerTick>,
    mut query: Query<(
        &NetId,
        &mut Hero,
        &Transform,
        Option<&game_core::gameplay::respawn::RespawnState>,
    )>,
    mut server_events: ResMut<ServerEvents>,
) {
    if !tick
        .0
        .is_multiple_of(game_core::map::RIVER_HAZARD_PERIOD_TICKS)
    {
        return;
    }
    for (net_id, mut hero, transform, respawn) in query.iter_mut() {
        if hero.health <= 0.0 {
            continue;
        }
        if respawn.is_some_and(|s| s.dead || tick.0 < s.invulnerable_until_tick) {
            continue;
        }
        if !game_core::map::is_hazardous_river(transform.translation) {
            continue;
        }
        let damage = game_core::map::RIVER_HAZARD_DAMAGE.min(hero.health);
        hero.health = (hero.health - damage).max(0.0);
        server_events.0.push(GameEvent::EnvironmentDamage {
            target: *net_id,
            damage,
            kind: 0,
        });
    }
}

/// 把本帧累积的伤害事件（DamageSink）转换为 `GameEvent::DamageDealt` 广播给客户端，
/// 让攻击方也能看到伤害数字与受击反馈。
pub fn flush_damage_events(
    damage_sink: Option<ResMut<game_core::components::DamageSink>>,
    net_ids: Query<(Entity, &NetId)>,
    mut server_events: ResMut<ServerEvents>,
) {
    let Some(mut sink) = damage_sink else {
        return;
    };
    if sink.0.is_empty() {
        return;
    }
    let id_map: std::collections::HashMap<Entity, NetId> =
        net_ids.iter().map(|(e, n)| (e, *n)).collect();
    for (attacker, target, dealt, _dtype) in sink.0.drain(..) {
        if let (Some(a), Some(t)) = (id_map.get(&attacker), id_map.get(&target)) {
            server_events.0.push(GameEvent::DamageDealt {
                attacker: *a,
                target: *t,
                damage: dealt,
                is_critical: false,
            });
        }
    }
}

#[cfg(test)]
mod river_hazard_tests {
    use super::*;
    use game_core::gameplay::respawn::RespawnState;
    use game_core::map::{LANE_Z, MAP_SCALE, RIVER_HAZARD_DAMAGE, RIVER_HAZARD_PERIOD_TICKS};

    const PERIOD: u64 = RIVER_HAZARD_PERIOD_TICKS;

    struct Fixture {
        app: App,
        hero: Entity,
    }

    fn fixture(tick: u64, pos: Vec3, health: f32, respawn: Option<RespawnState>) -> Fixture {
        let mut app = App::new();
        app.insert_resource(crate::simulation::ServerTick(tick))
            .insert_resource(ServerEvents::default())
            .add_systems(Update, river_hazard_system);
        let mut entity = app.world_mut().spawn((
            NetId(7),
            Hero {
                health,
                ..default()
            },
            Transform::from_translation(pos),
        ));
        if let Some(state) = respawn {
            entity.insert(state);
        }
        let hero = entity.id();
        Fixture { app, hero }
    }

    fn health(f: &mut Fixture) -> f32 {
        f.app.world().entity(f.hero).get::<Hero>().unwrap().health
    }

    fn events(f: &mut Fixture) -> usize {
        f.app.world().resource::<ServerEvents>().0.len()
    }

    #[test]
    fn standing_in_river_burns_hero_and_broadcasts_once() {
        let mut f = fixture(PERIOD, Vec3::new(0.0, 0.0, 40.0 * MAP_SCALE), 1000.0, None);
        f.app.update();
        assert_eq!(health(&mut f), 1000.0 - RIVER_HAZARD_DAMAGE);
        assert_eq!(events(&mut f), 1);
    }

    #[test]
    fn bridge_land_and_non_period_ticks_are_safe() {
        // 桥面（兵线跨河处）安全
        let mut bridge = fixture(PERIOD, Vec3::new(0.0, 0.0, LANE_Z), 1000.0, None);
        bridge.app.update();
        assert_eq!(health(&mut bridge), 1000.0);
        assert_eq!(events(&mut bridge), 0);
        // 陆地安全
        let mut land = fixture(PERIOD, Vec3::new(40.0 * MAP_SCALE, 0.0, 0.0), 1000.0, None);
        land.app.update();
        assert_eq!(health(&mut land), 1000.0);
        // 非结算 tick 不掉血
        let mut off = fixture(
            PERIOD + 1,
            Vec3::new(0.0, 0.0, 40.0 * MAP_SCALE),
            1000.0,
            None,
        );
        off.app.update();
        assert_eq!(health(&mut off), 1000.0);
        assert_eq!(events(&mut off), 0);
    }

    #[test]
    fn dead_and_invulnerable_heroes_are_exempt() {
        let mut dead = fixture(PERIOD, Vec3::new(0.0, 0.0, 40.0 * MAP_SCALE), 0.0, None);
        dead.app.update();
        assert_eq!(health(&mut dead), 0.0);
        assert_eq!(events(&mut dead), 0);

        let invuln = RespawnState {
            invulnerable_until_tick: PERIOD + 100,
            ..default()
        };
        let mut f = fixture(
            PERIOD,
            Vec3::new(0.0, 0.0, 40.0 * MAP_SCALE),
            1000.0,
            Some(invuln),
        );
        f.app.update();
        assert_eq!(health(&mut f), 1000.0);
    }

    #[test]
    fn damage_never_drives_health_below_zero() {
        let mut f = fixture(PERIOD, Vec3::new(0.0, 0.0, 40.0 * MAP_SCALE), 10.0, None);
        f.app.update();
        assert_eq!(health(&mut f), 0.0);
        assert_eq!(events(&mut f), 1);
    }
}
