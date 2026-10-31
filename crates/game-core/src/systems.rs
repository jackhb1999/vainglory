//! 游戏核心系统：移动、技能、伤害、经济

use serde::{Deserialize, Serialize};
use bevy::prelude::*;
use crate::components::*;
use crate::map::{MAP_WIDTH, MAP_HEIGHT};

/// 对英雄施加伤害（含护甲/魔抗减免），返回实际造成的伤害
pub fn apply_mitigated_damage(hero: &mut Hero, raw: f32, dtype: DamageType) -> f32 {
    let mult = match dtype {
        DamageType::Physical => 100.0 / (100.0 + hero.armor),
        DamageType::Magical => 100.0 / (100.0 + hero.magic_resist),
        DamageType::True => 1.0,
    };
    let dealt = raw * mult;
    hero.health = (hero.health - dealt).max(0.0);
    dealt
}

/// 固定时间步长（60Hz = 1/60 秒 ≈ 16.67ms）
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;
pub const TICK_RATE: u64 = 60;

/// 移动系统
pub fn movement_system(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut TransformSync, &Hero, &PlayerInput), Without<Interpolated>>,
) {
    let dt = time.delta_secs().min(FIXED_TIMESTEP * 3.0);
    
    for (mut transform, mut sync, hero, input) in query.iter_mut() {
        if input.move_dir != Vec2::ZERO {
            let direction = Vec3::new(input.move_dir.x, 0.0, input.move_dir.y).normalize();
            let velocity = direction * hero.move_speed;
            
            transform.translation += velocity * dt;
            // 限制英雄在地图边界内，避免跑出地图
            let half_w = MAP_WIDTH / 2.0 - 1.0;
            let half_h = MAP_HEIGHT / 2.0 - 1.0;
            transform.translation.x = transform.translation.x.clamp(-half_w, half_w);
            transform.translation.z = transform.translation.z.clamp(-half_h, half_h);
            sync.position = transform.translation;
            sync.velocity = velocity;
        } else {
            sync.velocity = Vec3::ZERO;
        }
    }
}

/// 简单的朝向系统
pub fn facing_system(
    mut query: Query<(&mut Transform, &PlayerInput), (With<Hero>, Without<Interpolated>)>,
) {
    for (mut transform, input) in query.iter_mut() {
        if input.aim_dir != Vec2::ZERO {
            let angle = input.aim_dir.y.atan2(input.aim_dir.x);
            transform.rotation = Quat::from_rotation_y(-angle);
        }
    }
}

/// 攻击系统
pub fn attack_system(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Hero, &Transform, &PlayerInput), With<LocalPlayer>>,
    targets: Query<(Entity, &Transform, &Hero), (Without<LocalPlayer>, With<Selectable>)>,
) {
    let current_tick = (time.elapsed_secs() * TICK_RATE as f32) as u64;
    
    for (entity, mut hero, transform, input) in query.iter_mut() {
        if input.ability_mask & 0b0001 != 0 { // 基础攻击
            let attack_range = hero.attack_range;
            
            // 寻找攻击范围内最近的敌人
            let mut closest_target = None;
            let mut closest_dist = f32::MAX;
            
            for (target_entity, target_transform, _) in targets.iter() {
                let dist = transform.translation.distance(target_transform.translation);
                if dist <= attack_range && dist < closest_dist {
                    closest_dist = dist;
                    closest_target = Some(target_entity);
                }
            }
            
            if let Some(target) = closest_target {
                commands.entity(entity).insert(AttackCommand {
                    target,
                    attacker: entity,
                    damage: hero.attack_damage,
                    damage_type: DamageType::Physical,
                    tick: current_tick,
                });
            }
        }
    }
}

/// 攻击命令
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct AttackCommand {
    pub target: Entity,
    pub attacker: Entity,
    pub damage: f32,
    pub damage_type: DamageType,
    pub tick: u64,
}

/// 伤害结算系统
pub fn damage_system(
    mut commands: Commands,
    mut attackers: Query<(Entity, &AttackCommand)>,
    mut targets: Query<(&mut Hero, Option<&TrainingDummy>)>,
) {
    for (attacker, cmd) in attackers.iter_mut() {
        if let Ok((mut target_hero, dummy)) = targets.get_mut(cmd.target) {
            // 含护甲/魔抗减免的伤害结算
            apply_mitigated_damage(&mut target_hero, cmd.damage, cmd.damage_type);

            if target_hero.health <= 0.0 && dummy.is_none() {
                // 击杀奖励
                if let Ok((mut attacker_hero, _)) = targets.get_mut(cmd.attacker) {
                    attacker_hero.gold += 50;
                    attacker_hero.experience += 100;
                }
                commands.entity(cmd.target).despawn();
            }
        }
        commands.entity(attacker).remove::<AttackCommand>();
    }
}

/// 技能冷却系统（递减英雄各槽位冷却）
pub fn ability_cooldown_system(
    time: Res<Time>,
    mut query: Query<&mut HeroAbilities>,
) {
    let dt = time.delta_secs();
    for mut abilities in query.iter_mut() {
        for cd in &mut abilities.cooldowns {
            if *cd > 0.0 {
                *cd = (*cd - dt).max(0.0);
            }
        }
    }
}

/// 经验升级系统
pub fn level_up_system(
    mut query: Query<&mut Hero>,
) {
    for mut hero in query.iter_mut() {
        let exp_needed = (hero.level as u32 * 100) + 50; // 简单公式
        if hero.experience >= exp_needed && hero.level < 20 {
            hero.level += 1;
            hero.experience -= exp_needed;
            
            // 升级属性成长
            hero.max_health += 80.0;
            hero.health = hero.max_health;
            hero.max_energy += 40.0;
            hero.energy = hero.max_energy;
            hero.attack_damage += 3.5;
            hero.armor += 1.5;
            hero.magic_resist += 1.0;
        }
    }
}

/// 经济系统：被动金币
pub fn passive_gold_system(
    time: Res<Time>,
    mut query: Query<&mut Hero>,
) {
    let gold_per_second = 1.3; // 基础被动金币
    let dt = time.delta_secs();
    
    for mut hero in query.iter_mut() {
        hero.gold += (gold_per_second * dt) as u32;
    }
}

/// 小兵生成系统
pub fn minion_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    team_query: Query<&Team, With<Minion>>,
) {
    let current_tick = (time.elapsed_secs() * TICK_RATE as f32) as u64;
    let spawn_interval = 30 * TICK_RATE; // 30秒一波
    
    if current_tick % spawn_interval == 0 && current_tick > 0 {
        // TODO: 在三条路径生成小兵
    }
}

/// 野怪生成系统
pub fn jungle_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
) {
    let current_tick = (time.elapsed_secs() * TICK_RATE as f32) as u64;
    let spawn_interval = 120 * TICK_RATE; // 2分钟刷新
    
    if current_tick % spawn_interval == 0 && current_tick > 0 {
        // TODO: 生成野怪
    }
}

/// 仇恨系统
pub fn aggro_system(
    mut query: Query<(&mut Aggro, &Transform, &JungleMonster)>,
    heroes: Query<(Entity, &Transform, &Hero, &NetId)>,
) {
    for (mut aggro, transform, monster) in query.iter_mut() {
        let mut highest_threat = 0.0;
        let mut new_target = None;

        for (_, hero_transform, _, hero_net_id) in heroes.iter() {
            let dist = transform.translation.distance(hero_transform.translation);
            if dist <= monster.aggro_range {
                // 简化：最近的英雄仇恨最高
                let threat = 1.0 / (dist + 1.0);
                if threat > highest_threat {
                    highest_threat = threat;
                    new_target = Some(*hero_net_id);
                }
            }
        }

        aggro.target = new_target;
    }
}

/// 野怪 AI 系统
pub fn jungle_ai_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform, &mut TransformSync, &JungleMonster, &mut Aggro, &NetId)>,
    heroes: Query<(Entity, &Transform, &Hero, &NetId), With<Hero>>,
) {
    let dt = time.delta_secs().min(FIXED_TIMESTEP * 3.0);

    // Build NetId -> Entity mapping
    let netid_to_entity: std::collections::HashMap<_, _> = heroes
        .iter()
        .map(|(entity, _, _, net_id)| (*net_id, entity))
        .collect();

    for (entity, mut transform, mut sync, monster, mut aggro, _) in query.iter_mut() {
        if let Some(target_net_id) = aggro.target {
            if let Some(&target_entity) = netid_to_entity.get(&target_net_id) {
                if let Ok(target_transform) = heroes.get(target_entity).map(|t| t.1) {
                    let direction = (target_transform.translation - transform.translation).normalize();
                    let velocity = direction * monster.move_speed;

                    transform.translation += velocity * dt;
                    sync.position = transform.translation;
                    sync.velocity = velocity;

                    // 攻击逻辑
                    let dist = transform.translation.distance(target_transform.translation);
                    if dist <= monster.attack_range {
                        commands.entity(entity).insert(AttackCommand {
                            target: target_entity,
                            attacker: entity,
                            damage: monster.attack_damage,
                            damage_type: DamageType::Physical,
                            tick: (time.elapsed_secs() * TICK_RATE as f32) as u64,
                        });
                    }
                } else {
                    aggro.target = None;
                }
            } else {
                aggro.target = None;
            }
        }
    }
}

/// 防御塔 AI 系统
pub fn turret_ai_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &Transform, &Turret)>,
    targets: Query<(Entity, &Transform, &Hero, &Team)>,
) {
    let current_tick = (time.elapsed_secs() * TICK_RATE as f32) as u64;
    
    for (entity, transform, turret) in query.iter_mut() {
        if current_tick.saturating_sub(turret.last_attack_tick) < (1.0 / turret.attack_speed * TICK_RATE as f32) as u64 {
            continue;
        }
        
        // 寻找攻击范围内的敌方英雄/小兵
        for (target_entity, target_transform, _, target_team) in targets.iter() {
            let dist = transform.translation.distance(target_transform.translation);
            if dist <= turret.attack_range {
                // 简化：攻击第一个发现的目标
                commands.entity(entity).insert(AttackCommand {
                    target: target_entity,
                    attacker: entity,
                    damage: turret.attack_damage,
                    damage_type: DamageType::Physical,
                    tick: current_tick,
                });
                break;
            }
        }
    }
}

/// 血量回复系统
pub fn health_regen_system(
    time: Res<Time>,
    mut query: Query<&mut Hero>,
) {
    let dt = time.delta_secs();
    for mut hero in query.iter_mut() {
        if hero.health > 0.0 && hero.health < hero.max_health {
            hero.health = (hero.health + hero.max_health * 0.01 * dt).min(hero.max_health);
        }
    }
}

/// 能量回复系统
pub fn energy_regen_system(
    time: Res<Time>,
    mut query: Query<&mut Hero>,
) {
    let dt = time.delta_secs();
    for mut hero in query.iter_mut() {
        if hero.energy < hero.max_energy {
            hero.energy = (hero.energy + hero.max_energy * 0.02 * dt).min(hero.max_energy);
        }
    }
}