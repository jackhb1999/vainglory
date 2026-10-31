//! 服务端系统（复用 game-core 的系统 + 技能/弹道等服务器专属系统）

use bevy::prelude::*;
use game_core::abilities::*;
use game_core::components::*;
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
            // 瞄准方向（世界 x,z 平面，归一化）
            let aim = if input.aim_dir.length_squared() > 0.0 {
                Vec3::new(input.aim_dir.x, 0.0, input.aim_dir.y).normalize()
            } else {
                Vec3::new(0.0, 0.0, -1.0) // 兜底：朝前方
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
                        // 指向/瞬发 AoE：在落点对范围内敌人施加待结算伤害
                        let center = caster_pos + aim * def.range;
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
                target_position: Vec3f::from_vec3(caster_pos + aim * def.range),
                target_entity: None,
            });
        }
    }
}

/// 弹道系统：推进弹体、检测命中、命中后通过 AttackCommand 走统一伤害结算。
pub fn projectile_system(
    mut commands: Commands,
    time: Res<Time>,
    heroes: Query<(Entity, &Transform, &Team)>,
    mut projectiles: Query<(Entity, &mut TransformSync, &mut Projectile)>,
) {
    let dt = time.delta_secs().min(FIXED_TIMESTEP * 3.0);

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
                tick: 0,
            });
            proj.dead = true;
        } else if proj.traveled >= proj.max_range {
            proj.dead = true;
        }
    }
}

/// 结算指向/瞬发 AoE 的待结算伤害（PendingHit）。死亡则销毁实体（训练假人除外）。
pub fn resolve_pending_hits(
    mut commands: Commands,
    mut hits: Query<(Entity, &mut Hero, &PendingHit, Option<&TrainingDummy>)>,
) {
    for (entity, mut hero, hit, dummy) in hits.iter_mut() {
        apply_mitigated_damage(&mut hero, hit.damage, hit.damage_type);
        if hero.health <= 0.0 && dummy.is_none() {
            commands.entity(entity).despawn();
        }
        commands.entity(entity).remove::<PendingHit>();
    }
}