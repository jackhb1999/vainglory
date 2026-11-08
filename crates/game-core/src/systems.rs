//! 游戏核心系统：移动、技能、伤害、经济

use crate::components::*;
use crate::gameplay::{PendingDeaths, attributes, combat, deterministic_roll, respawn};
use crate::map::{MAP_HEIGHT, MAP_WIDTH};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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

/// 碰撞解算系统（P-COLLISION）：在 XZ 平面按圆形 `Collider` 分离实体，
/// 避免英雄 / 小兵互相穿模，以及它们与塔 / 源核 / 假人等静态障碍穿模。
///
/// 动态实体（英雄 / 小兵 / 野怪）之间互相推开；动态实体被静态实体（塔 / 源核 / 假人）
/// 整体推出。每 tick 在移动系统之后执行一次。
///
/// 关键点：解算结果必须同时写回 `Transform` 与 `TransformSync`。只写同步组件会让
/// 下一次移动系统用未修正的 `Transform` 覆盖位置，碰撞在视觉上等于失效。
// 内层需按索引写回其它动态体位置（互推各退一半），范围索引可避免同时可变借用
// `bodies` 与读取 `bodies[i]` 的冲突，属 `needless_range_loop` 的合理例外。
#[allow(clippy::type_complexity, clippy::needless_range_loop)]
pub fn collision_system(
    mut query: Query<(
        Entity,
        &mut Transform,
        &mut TransformSync,
        &Collider,
        Option<&StaticCollider>,
    )>,
) {
    // 读取阶段：收集 (实体, 位置, 半径, 是否静态)
    let mut bodies: Vec<(Entity, Vec3, f32, bool)> = query
        .iter()
        .map(|(e, transform, _sync, collider, static_collider)| {
            (
                e,
                Vec3::new(transform.translation.x, 0.0, transform.translation.z),
                collider.radius,
                static_collider.is_some(),
            )
        })
        .collect();

    let half_w = MAP_WIDTH / 2.0;
    let half_h = MAP_HEIGHT / 2.0;

    for i in 0..bodies.len() {
        if bodies[i].3 {
            continue; // 静态体不参与被推开
        }
        let mut pos = bodies[i].1;
        let r_i = bodies[i].2;
        for j in 0..bodies.len() {
            if i == j {
                continue;
            }
            let (r_j, static_j, pos_j) = (bodies[j].2, bodies[j].3, bodies[j].1);
            let delta = pos - pos_j;
            let dist = delta.length();
            let min_dist = r_i + r_j;
            // 完全重叠时用固定角度兜底，避免两具身体永远粘在一起
            let dir = if dist > 1e-4 {
                delta / dist
            } else {
                let angle = ((i * 7 + j * 13) % 16) as f32 * std::f32::consts::TAU / 16.0;
                Vec3::new(angle.cos(), 0.0, angle.sin())
            };
            if dist < min_dist {
                let overlap = min_dist - dist.max(1e-4);
                if static_j {
                    pos += dir * overlap; // 被静态体推出
                } else {
                    pos += dir * overlap * 0.5; // 双方各退一半
                    bodies[j].1 -= dir * overlap * 0.5;
                }
            }
        }
        // 限制在地图范围内
        pos.x = pos.x.clamp(-half_w + r_i, half_w - r_i);
        pos.z = pos.z.clamp(-half_h + r_i, half_h - r_i);
        bodies[i].1 = pos;
    }

    // 写回阶段：同时更新权威 Transform 与同步快照组件
    for (e, mut transform, mut sync, _, _) in query.iter_mut() {
        if let Some(b) = bodies.iter().find(|(ent, _, _, _)| *ent == e) {
            transform.translation.x = b.1.x;
            transform.translation.z = b.1.z;
            sync.position.x = b.1.x;
            sync.position.z = b.1.z;
        }
    }
}

/// 固定时间步长（60Hz = 1/60 秒 ≈ 16.67ms）
pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;
pub const TICK_RATE: u64 = 60;

/// 权威模拟时间（P-A）：由服务端 `ServerTick` 在每个 `FixedUpdate` 同步。
///
/// 所有玩法系统只能经它取时间，**禁止读取 `bevy::time::Time` 墙钟**——墙钟的
/// 可变 `delta` 会让波次、冷却、加时、速度校验与回放结果不可复现。
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimTime(pub u64);

impl SimTime {
    pub const fn delta_secs(self) -> f32 {
        FIXED_TIMESTEP
    }
    pub fn elapsed_secs(self) -> f32 {
        self.0 as f32 / TICK_RATE as f32
    }
    pub const fn tick(self) -> u64 {
        self.0
    }
}

/// 移动系统
pub fn movement_system(
    time: Res<SimTime>,
    mut query: Query<
        (&mut Transform, &mut TransformSync, &Hero, &PlayerInput),
        Without<Interpolated>,
    >,
) {
    let dt = time.delta_secs().min(FIXED_TIMESTEP * 3.0);

    for (mut transform, mut sync, hero, input) in query.iter_mut() {
        if input.move_dir != Vec2::ZERO {
            let direction = Vec3::new(input.move_dir.x, 0.0, input.move_dir.y).normalize();
            // 河道（|x| ≤ RIVER_HALF_WIDTH）减速为一半；仅影响速度，不改裁决平面。
            let speed = hero.move_speed * crate::map::terrain_speed_mult(transform.translation);
            let velocity = direction * speed;

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
#[allow(clippy::type_complexity)]
pub fn facing_system(
    mut query: Query<(&mut Transform, &PlayerInput), (With<Hero>, Without<Interpolated>)>,
) {
    for (mut transform, input) in query.iter_mut() {
        if input.aim_dir != Vec2::ZERO {
            // aim_dir 是地面世界坐标点：朝向 = 该点 - 自身位置
            let dir = input.aim_dir - Vec2::new(transform.translation.x, transform.translation.z);
            if dir.length_squared() > 1e-6 {
                let angle = dir.y.atan2(dir.x);
                transform.rotation = Quat::from_rotation_y(-angle);
            }
        }
    }
}

/// 可攻击目标的一帧快照。
///
/// 英雄/兵线/塔/野怪的 `Transform` 既被攻击者以 `&mut` 修改，又被索敌逻辑以 `&` 读取；
/// 放在同一个系统里会触发 Bevy `B0001` 冲突。这里用「只读收集系统 → 资源 → 消费系统」
/// 把两段访问拆到不同系统，同时让英雄、兵线、野怪、塔共用同一份候选集。
#[derive(Clone, Copy, Debug)]
pub struct TargetCandidate {
    pub entity: Entity,
    pub position: Vec3,
    pub team: Team,
    pub net_id: Option<NetId>,
    pub health: f32,
    pub is_hero: bool,
}

#[derive(Resource, Default, Debug)]
pub struct TargetCandidates(pub Vec<TargetCandidate>);

#[allow(clippy::type_complexity)]
pub fn collect_target_candidates_system(
    mut out: ResMut<TargetCandidates>,
    heroes: Query<(Entity, &Transform, &Hero, Option<&Team>, Option<&NetId>)>,
    constructs: Query<
        (
            Entity,
            &Transform,
            Option<&Team>,
            Option<&NetId>,
            Option<&Minion>,
            Option<&Turret>,
            Option<&JungleMonster>,
        ),
        Or<(With<Minion>, With<Turret>, With<JungleMonster>)>,
    >,
) {
    out.0.clear();
    for (e, t, h, team, nid) in heroes.iter() {
        out.0.push(TargetCandidate {
            entity: e,
            position: t.translation,
            team: team.copied().unwrap_or(Team::Neutral),
            net_id: nid.copied(),
            health: h.health,
            is_hero: true,
        });
    }
    for (e, t, team, nid, minion, turret, monster) in constructs.iter() {
        let health = minion
            .map(|m| m.health)
            .or_else(|| turret.map(|x| x.health))
            .or_else(|| monster.map(|x| x.health))
            .unwrap_or(0.0);
        out.0.push(TargetCandidate {
            entity: e,
            position: t.translation,
            team: team.copied().unwrap_or(Team::Neutral),
            net_id: nid.copied(),
            health,
            is_hero: false,
        });
    }
}

/// 攻击系统（P-B）：英雄可以攻击任意敌方实体（英雄 / 兵线 / 野怪 / 哨塔）。
/// `Team::Neutral`（训练假人、野怪）可被任意阵营攻击；中立阵营自身不发起攻击。
pub fn attack_system(
    time: Res<SimTime>,
    mut commands: Commands,
    mut query: Query<
        (Entity, &mut Hero, &Transform, &PlayerInput, Option<&Team>),
        With<LocalPlayer>,
    >,
    world: Res<TargetCandidates>,
) {
    let current_tick = time.tick();

    for (entity, mut hero, transform, input, own_team) in query.iter_mut() {
        if hero.attack_cooldown > 0.0 {
            hero.attack_cooldown = (hero.attack_cooldown - FIXED_TIMESTEP).max(0.0);
        }
        if input.ability_mask & 0b0001 == 0 || hero.attack_cooldown > 0.0 {
            continue;
        }

        let own_team = own_team.copied().unwrap_or(Team::Neutral);
        if own_team == Team::Neutral {
            continue;
        }
        let range = hero.attack_range;
        let pos = transform.translation;

        // 候选集：(entity, 距离, NetId)，只保留射程内的敌方单位
        let candidates: Vec<(Entity, f32, Option<NetId>)> = world
            .0
            .iter()
            .filter(|c| c.entity != entity && c.health > 0.0 && c.team != own_team)
            .filter_map(|c| {
                let d = pos.distance(c.position);
                (d <= range).then_some((c.entity, d, c.net_id))
            })
            .collect();

        // 锁定目标优先；否则取射程内最近的敌人
        let chosen = input
            .target_net_id
            .and_then(|tid| candidates.iter().find(|(_, _, n)| *n == Some(tid)).copied())
            .or_else(|| {
                candidates
                    .iter()
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .copied()
            })
            .map(|(e, _, _)| e);

        if let Some(t) = chosen {
            commands.entity(entity).insert(AttackCommand {
                target: t,
                attacker: entity,
                damage: hero.attack_damage,
                damage_type: DamageType::Physical,
                tick: current_tick,
            });
            hero.attack_cooldown = 1.0 / hero.attack_speed.max(0.1);
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

/// 伤害结算系统（服务端唯一裁决点）。
///
/// 目标带 `CombatStats/ShieldPool/StatusEffects` 时走 `GX-CBT-2026-001` §5 的
/// 完整公式（减免/暴击/穿透/护盾/放大/吸血/无敌）；所有二级属性取默认值时
/// 结果与旧 `apply_mitigated_damage` 等价，保证既有行为不回归。
pub fn damage_system(
    mut commands: Commands,
    mut attackers: Query<(Entity, &AttackCommand)>,
    mut targets: Query<(
        &mut Hero,
        Option<&TrainingDummy>,
        Option<&NetId>,
        Option<&combat::CombatStats>,
        Option<&mut combat::ShieldPool>,
        Option<&combat::StatusEffects>,
        Option<&respawn::RespawnState>,
    )>,
    attacker_lookup: Query<(&combat::CombatStats, Option<&NetId>)>,
    match_clock: Option<Res<crate::gameplay::clock::MatchClock>>,
    mut damage_sink: Option<ResMut<DamageSink>>,
    mut pending_deaths: Option<ResMut<PendingDeaths>>,
) {
    for (attacker, cmd) in attackers.iter_mut() {
        let (atk_stats, killer_id) = attacker_lookup
            .get(cmd.attacker)
            .map(|(s, n)| (*s, n.map(|x| x.0).unwrap_or(0)))
            .unwrap_or((combat::CombatStats::default(), 0));

        if let Ok((
            mut target_hero,
            dummy,
            target_net_id,
            target_stats,
            mut shield,
            status,
            respawn_state,
        )) = targets.get_mut(cmd.target)
        {
            let dealt = match shield.as_deref_mut() {
                Some(pool) => {
                    let stats = target_stats.copied().unwrap_or_default();
                    let status_snapshot = status.cloned().unwrap_or_default();
                    let req = combat::DamageRequest {
                        can_crit: true,
                        crit_roll: deterministic_roll(
                            cmd.tick,
                            cmd.attacker.to_bits(),
                            cmd.target.to_bits(),
                        ),
                        exhaustion: match_clock
                            .as_deref()
                            .is_some_and(|c| c.exhaustion_active()),
                        ..combat::DamageRequest::new(cmd.damage, cmd.damage_type)
                    };
                    combat::apply_hero_damage(
                        &mut target_hero,
                        &atk_stats,
                        &stats,
                        pool,
                        &status_snapshot,
                        req,
                    )
                    .dealt
                }
                None => apply_mitigated_damage(&mut target_hero, cmd.damage, cmd.damage_type),
            };
            if let Some(sink) = damage_sink.as_deref_mut() {
                sink.0
                    .push((cmd.attacker, cmd.target, dealt, cmd.damage_type));
            }

            let lethal = target_hero.health <= combat::EPS && dummy.is_none();
            let has_respawn = respawn_state.is_some();
            let victim_id = target_net_id.map(|n| n.0).unwrap_or(0);
            if lethal {
                // 击杀奖励由服务端 `death_respawn_system` 统一经 `kill_bounty`
                // （基础 180 + 连杀加成）结算给击杀者，这里不再额外发奖，避免双重发放。
                if let Some(pending) = pending_deaths.as_deref_mut() {
                    pending.0.push((cmd.tick, killer_id, victim_id));
                }
                // 具备重生状态的实体不再被 despawn，交由重生系统恢复
                if !has_respawn {
                    commands.entity(cmd.target).despawn();
                }
            }
        }
        commands.entity(attacker).remove::<AttackCommand>();
    }
}

/// 技能冷却系统（递减英雄各槽位冷却）
pub fn ability_cooldown_system(time: Res<SimTime>, mut query: Query<&mut HeroAbilities>) {
    let dt = time.delta_secs();
    for mut abilities in query.iter_mut() {
        for cd in &mut abilities.cooldowns {
            if *cd > 0.0 {
                *cd = (*cd - dt).max(0.0);
            }
        }
    }
}

/// 经验升级系统（P-A）：等级只写入 `AttributeSources.level`，最终属性由
/// `recompute` 统一重算，避免 buff 过期 / 售出装备时抹掉等级成长。
pub fn level_up_system(
    mut query: Query<(
        &mut Hero,
        &mut combat::CombatStats,
        &mut attributes::AttributeSources,
    )>,
) {
    for (mut hero, mut stats, mut sources) in query.iter_mut() {
        let exp_needed = (hero.level as u32 * 100) + 50; // 简单公式
        if hero.experience >= exp_needed && hero.level < 20 {
            hero.level += 1;
            hero.experience -= exp_needed;
            sources.level = hero.level;
            attributes::recompute(&sources, &mut hero, &mut stats);
        }
    }
}

/// 经济系统：被动金币
pub fn passive_gold_system(time: Res<SimTime>, mut query: Query<&mut Hero>) {
    let gold_per_second = 1.3; // 基础被动金币
    let dt = time.delta_secs();

    for mut hero in query.iter_mut() {
        hero.gold += (gold_per_second * dt) as u32;
    }
}

/// 防御塔 AI 系统（P-B）：按队伍选敌，攻击冷却由 `last_attack_tick` 驱动。
///
/// 目标为任意带 `Team` 的实体（英雄 / 兵线 / 野怪），由
/// `server::gameplay::world_damage_system` 统一处理非英雄目标的伤害。
pub fn turret_ai_system(
    mut commands: Commands,
    time: Res<SimTime>,
    mut turrets: Query<(Entity, &Transform, &Team, &mut Turret)>,
    targets: Query<(Entity, &Transform, &Team, &Hero)>,
    minions: Query<(Entity, &Transform, &Team), With<Minion>>,
) {
    let current_tick = time.tick();

    for (entity, transform, team, mut turret) in turrets.iter_mut() {
        let interval = (1.0 / turret.attack_speed.max(0.1) * TICK_RATE as f32) as u64;
        if current_tick.saturating_sub(turret.last_attack_tick) < interval {
            continue;
        }

        let pos = transform.translation;
        let mut best: Option<Entity> = None;

        // 兵线优先于英雄（防御塔仇恨规则）
        for (te, tt, tteam) in minions.iter() {
            if tteam == team {
                continue;
            }
            let d = pos.distance(tt.translation);
            if d <= turret.attack_range {
                best = Some(te);
                break;
            }
        }
        if best.is_none() {
            for (te, tt, tteam, hero) in targets.iter() {
                if tteam == team || hero.health <= 0.0 {
                    continue;
                }
                let d = pos.distance(tt.translation);
                if d <= turret.attack_range {
                    best = Some(te);
                    break;
                }
            }
        }

        if let Some(target) = best {
            turret.last_attack_tick = current_tick;
            commands.entity(entity).insert(AttackCommand {
                target,
                attacker: entity,
                damage: turret.attack_damage,
                damage_type: DamageType::Physical,
                tick: current_tick,
            });
        }
    }
}

/// 血量回复系统（训练假人不回血，方便测试攻击是否生效）
pub fn health_regen_system(
    time: Res<SimTime>,
    mut query: Query<(&mut Hero, Option<&TrainingDummy>)>,
) {
    let dt = time.delta_secs();
    for (mut hero, dummy) in query.iter_mut() {
        if dummy.is_some() {
            continue;
        }
        if hero.health > 0.0 && hero.health < hero.max_health {
            hero.health = (hero.health + hero.max_health * 0.01 * dt).min(hero.max_health);
        }
    }
}

/// 能量回复系统
pub fn energy_regen_system(time: Res<SimTime>, mut query: Query<&mut Hero>) {
    let dt = time.delta_secs();
    for mut hero in query.iter_mut() {
        if hero.energy < hero.max_energy {
            hero.energy = (hero.energy + hero.max_energy * 0.02 * dt).min(hero.max_energy);
        }
    }
}
