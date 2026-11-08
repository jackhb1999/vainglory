//! P-B / P-C / PvE：世界实体驱动层。
//!
//! 之前 `Minion` / `Turret` / `JungleMonster` 只有数据组件，没有任何系统让它们
//! 移动、攻击、受伤或死亡；三条伤害管线又都只认 `Hero`。本模块补齐：
//! 波次生成 → 兵线行军 → 塔/野怪攻击 → 非英雄目标受伤 → 源核归零判胜。
//!
//! 权威时钟统一取 `MatchClock.tick`（由 `ServerTick` 同步），不使用墙钟。

use crate::networking::{ClientNetIds, DummyRegistry, NextNetId, RenetServerResource};
use crate::simulation::ServerTick;
use bevy::prelude::*;
use bitcode;
use game_core::components::*;
use game_core::gameplay::{
    GameplayEvents, NetEvent, attributes,
    clock::{MatchClock, MatchMode, MatchPhase},
    combat, jungle, registry, structures,
    waves::{self, MinionAi, WaveDef},
};
use game_core::net::{GameEvent, ServerMessage, Vec3f};
use game_core::systems::{AttackCommand, SimTime};
use renet2::DefaultChannel;

/// PvE AI 对手标记（服务端权威，客户端只看到普通 HeroSpawn）。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AiControlled;

/// PvE 对手只生成一次。
#[derive(Resource, Default)]
pub struct PveBotReady(pub bool);

/// 源核已被摧毁（防止重复播报胜负）。
#[derive(Resource, Default)]
pub struct CoreDestroyed(pub bool);

// ---------------------------------------------------------------------------
// PvE：单用户即可开局
// ---------------------------------------------------------------------------

/// 有任意真人连接且当前为 PvE 模式时，补一个红方 AI 英雄。
/// 复用 `DummyRegistry` 让**后加入**的客户端也能收到该 HeroSpawn 事件。
#[allow(clippy::too_many_arguments)]
pub fn ensure_pve_bot(
    mut commands: Commands,
    mut ready: ResMut<PveBotReady>,
    clock: Res<MatchClock>,
    mut next_net_id: ResMut<NextNetId>,
    mut dummy_registry: ResMut<DummyRegistry>,
    client_net_ids: Res<ClientNetIds>,
    mut server_res: Option<ResMut<RenetServerResource>>,
) {
    if ready.0 || clock.mode != MatchMode::Pve || client_net_ids.0.is_empty() {
        return;
    }
    ready.0 = true;
    let net_id = NetId(next_net_id.0);
    next_net_id.0 += 1;
    let pos = game_core::map::RED_BASE + Vec3::new(-10.0, 0.0, -10.0);

    commands.spawn((
        net_id,
        Team::Red,
        Transform::from_translation(pos),
        Hero {
            name: "PvE 训练对手".to_owned(),
            move_speed: HERO_BASE_MOVE_SPEED,
            ..default()
        },
        TransformSync {
            position: pos,
            ..default()
        },
        PlayerInput::default(),
        HeroAbilities {
            cooldowns: [0.0; 4],
            ability_ids: game_core::abilities::default_hero_abilities(),
        },
        Selectable {
            radius: 1.5,
            height: 3.0,
        },
        Collider { radius: 1.3 },
        ServerAuthority,
        LocalPlayer,
        AiControlled,
    ));

    let event = GameEvent::HeroSpawn {
        net_id,
        team: Team::Red,
        hero_name: "PvE 训练对手".to_owned(),
        position: Vec3f::from_vec3(pos),
        skin: 2,
    };
    dummy_registry.0.push(event.clone());
    if let Some(server) = server_res.as_mut() {
        for client_id in client_net_ids.0.keys() {
            server.0.send_message(
                *client_id,
                DefaultChannel::ReliableOrdered,
                bitcode::encode(&ServerMessage::Event(event.clone())),
            );
        }
    }
}

/// AI 对手：追击最近真人英雄，进入射程后持续普攻。
pub fn pve_bot_ai_system(
    mut bots: Query<(&Transform, &mut PlayerInput, &Team), With<AiControlled>>,
    players: Query<(&Transform, &Hero, &Team), (With<LocalPlayer>, Without<AiControlled>)>,
) {
    for (bot_t, mut input, bot_team) in bots.iter_mut() {
        let nearest = players
            .iter()
            .filter(|(_, h, t)| **t != *bot_team && h.health > combat::EPS)
            .map(|(t, _, _)| (t.translation - bot_t.translation, t.translation))
            .min_by(|a, b| a.0.length_squared().total_cmp(&b.0.length_squared()));

        if let Some((delta, target_pos)) = nearest {
            let dist = delta.length();
            input.aim_dir = Vec2::new(target_pos.x, target_pos.z);
            input.move_dir = if dist > 4.5 {
                Vec2::new(delta.x, delta.z).normalize_or_zero()
            } else {
                Vec2::ZERO
            };
            input.ability_mask = 0b0001;
            input.target_net_id = None;
        } else {
            input.move_dir = Vec2::ZERO;
            input.ability_mask = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// P-B：兵线波次与行军
// ---------------------------------------------------------------------------

/// 按 `WaveDef` 生成双方唯一兵线的战偶，并广播节奏事件。
pub fn wave_spawn_system(
    clock: Res<MatchClock>,
    mut commands: Commands,
    mut next_net_id: ResMut<NextNetId>,
    mut events: ResMut<GameplayEvents>,
) {
    if clock.is_over() {
        return;
    }
    let Some(index) = WaveDef::default().wave_at(clock.tick) else {
        return;
    };
    let scale = waves::wave_scale(index);
    for team in [Team::Blue, Team::Red] {
        for lane in 0..game_core::map::LANE_COUNT {
            for kind in waves::wave_composition(index) {
                let net_id = NetId(next_net_id.0);
                next_net_id.0 += 1;
                let spawn = waves::lane_waypoints(team, lane)[0];
                let pos = Vec3::new(spawn.x, 0.0, spawn.y);
                let (hp, ad, ms, range, gold) = match kind {
                    MinionType::Melee => (480.0, 24.0, 3.2, 1.8, 18),
                    MinionType::Ranged => (320.0, 30.0, 3.0, 5.5, 22),
                    MinionType::Siege => (900.0, 48.0, 2.6, 7.0, 40),
                    MinionType::Super => (1400.0, 70.0, 3.4, 2.0, 60),
                };
                commands.spawn((
                    net_id,
                    team,
                    Transform::from_translation(pos),
                    TransformSync {
                        position: pos,
                        ..default()
                    },
                    Minion {
                        minion_type: kind,
                        max_health: hp * scale,
                        health: hp * scale,
                        attack_damage: ad * scale,
                        armor: 10.0,
                        magic_resist: 10.0,
                        move_speed: ms,
                        attack_range: range,
                        gold_reward: gold,
                        exp_reward: gold / 2,
                    },
                    MinionAi::new(team, lane, ms, ad * scale),
                    Selectable {
                        radius: 0.8,
                        height: 2.0,
                    },
                    Collider { radius: 1.0 },
                    ServerAuthority,
                ));
            }
        }
    }
    events.emit(NetEvent::WaveSpawned(index));
}

/// 兵线沿路点推进；射程内有敌方单位则停下攻击。
/// 索敌读 `TargetCandidates` 快照资源，避免与 `&mut Transform` 触发 B0001。
#[allow(clippy::type_complexity)]
pub fn minion_ai_system(
    time: Res<SimTime>,
    mut commands: Commands,
    mut minions: Query<(
        Entity,
        &mut Transform,
        &mut TransformSync,
        &mut MinionAi,
        &Team,
    )>,
    world: Res<game_core::systems::TargetCandidates>,
) {
    let dt = time.delta_secs();

    for (entity, mut transform, mut sync, mut ai, team) in minions.iter_mut() {
        let pos = transform.translation;
        let enemy = world
            .0
            .iter()
            .filter(|c| {
                c.entity != entity
                    && c.health > 0.0
                    && c.team != *team
                    && c.team != Team::Neutral
                    && Vec2::new(c.position.x - pos.x, c.position.z - pos.z).length()
                        <= ai.attack_range
            })
            .min_by(|a, b| {
                (a.position - pos)
                    .length_squared()
                    .total_cmp(&(b.position - pos).length_squared())
            })
            .map(|c| c.entity);

        if let Some(target) = enemy {
            if ai.can_attack() {
                commands.entity(entity).insert(AttackCommand {
                    target,
                    attacker: entity,
                    damage: ai.attack_damage,
                    damage_type: DamageType::Physical,
                    tick: time.tick(),
                });
                ai.mark_attack(1.0);
            }
            sync.velocity = Vec3::ZERO;
            continue;
        }

        let dir = ai.step(Vec2::new(pos.x, pos.z), dt);
        let velocity = Vec3::new(dir.x, 0.0, dir.y) * ai.move_speed;
        transform.translation += velocity * dt;
        sync.position = transform.translation;
        sync.velocity = velocity;
    }
}

// ---------------------------------------------------------------------------
// P-B：非英雄目标的伤害管线（唯一裁决点，与 Hero 管线互补）
// ---------------------------------------------------------------------------

/// 处理 `AttackCommand` 中目标是塔 / 兵线 / 野怪的情况。
/// 必须在 `damage_system` **之前**运行；`damage_system` 会对这些命令 no-op 并负责移除组件。
#[allow(clippy::type_complexity)]
pub fn world_damage_system(
    mut commands: Commands,
    mut events: ResMut<GameplayEvents>,
    mut clock: ResMut<MatchClock>,
    mut core_destroyed: ResMut<CoreDestroyed>,
    attackers: Query<(Entity, &AttackCommand)>,
    mut turrets: Query<(
        &mut Turret,
        &NetId,
        &Team,
        Option<&mut structures::StructureDefense>,
    )>,
    mut minions: Query<(&mut Minion, &NetId)>,
    mut monsters: Query<(&mut JungleMonster, &NetId)>,
    mut heroes: Query<&mut Hero>,
) {
    for (_attacker_entity, cmd) in attackers.iter() {
        let raw = cmd.damage;

        if let Ok((mut turret, nid, team, def)) = turrets.get_mut(cmd.target) {
            if turret.health <= 0.0 {
                continue;
            }
            let armored = match cmd.damage_type {
                DamageType::Physical => raw * 100.0 / (100.0 + turret.armor.max(0.0)),
                DamageType::Magical => raw * 100.0 / (100.0 + turret.magic_resist.max(0.0)),
                DamageType::True => raw,
            };
            let dealt = match def {
                Some(mut d) => structures::apply_incoming(&mut d, armored),
                None => armored,
            };
            turret.health = (turret.health - dealt).max(0.0);
            if turret.health <= combat::EPS {
                events.emit(NetEvent::TurretDestroyed {
                    net_id: nid.0,
                    team: *team,
                });
                if turret.turret_type == TurretType::SourceCore && !core_destroyed.0 {
                    core_destroyed.0 = true;
                    let winner = match team {
                        Team::Blue => Team::Red,
                        _ => Team::Blue,
                    };
                    if !clock.is_over() {
                        let ticks = clock.tick;
                        clock.finish(winner);
                        events.emit(NetEvent::MatchFinished {
                            winner,
                            reason: 1, // 源核摧毁
                            duration_ticks: ticks,
                        });
                    }
                }
            }
            continue;
        }

        if let Ok((mut minion, _nid)) = minions.get_mut(cmd.target) {
            let dealt = raw * 100.0 / (100.0 + minion.armor.max(0.0));
            minion.health = (minion.health - dealt).max(0.0);
            if minion.health <= combat::EPS {
                if let Ok(mut attacker_hero) = heroes.get_mut(cmd.attacker) {
                    attacker_hero.gold = attacker_hero.gold.saturating_add(minion.gold_reward);
                    attacker_hero.experience =
                        attacker_hero.experience.saturating_add(minion.exp_reward);
                }
                commands.entity(cmd.target).despawn();
            }
            continue;
        }

        if let Ok((mut monster, _nid)) = monsters.get_mut(cmd.target) {
            let dealt = raw * 100.0 / (100.0 + monster.armor.max(0.0));
            monster.health = (monster.health - dealt).max(0.0);
            if monster.health <= combat::EPS {
                if let Ok(mut attacker_hero) = heroes.get_mut(cmd.attacker) {
                    attacker_hero.gold = attacker_hero.gold.saturating_add(monster.gold_reward);
                    attacker_hero.experience =
                        attacker_hero.experience.saturating_add(monster.exp_reward);
                }
                commands.entity(cmd.target).despawn();
            }
        }
    }
}

/// 野怪在 Aggro 状态下对射程内英雄发起攻击（旧 `jungle_ai_system` 是死代码）。
pub fn monster_attack_system(
    time: Res<SimTime>,
    mut commands: Commands,
    mut monsters: Query<(Entity, &Transform, &JungleMonster, &jungle::MonsterAi)>,
    heroes: Query<(Entity, &Transform, &Hero, Option<&Team>)>,
) {
    for (entity, transform, monster, ai) in monsters.iter_mut() {
        if ai.state != jungle::MonsterState::Aggro {
            continue;
        }
        let pos = transform.translation;
        let target = heroes
            .iter()
            .filter(|(_, _, h, _)| h.health > combat::EPS)
            .filter(|(_, t, _, _)| pos.distance(t.translation) <= monster.attack_range)
            .min_by(|a, b| {
                pos.distance(a.1.translation)
                    .total_cmp(&pos.distance(b.1.translation))
            })
            .map(|(e, _, _, _)| e);
        if let Some(target) = target {
            commands.entity(entity).insert(AttackCommand {
                target,
                attacker: entity,
                damage: monster.attack_damage,
                damage_type: DamageType::Physical,
                tick: time.tick(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// P-C：目标 buff、加时衰竭与胜负判定
// ---------------------------------------------------------------------------

/// 占领成功 → 给归属方全队发放可过期的 buff，并广播全局事件。
#[allow(clippy::type_complexity)]
pub fn objective_buff_system(
    mut events: ResMut<GameplayEvents>,
    mut heroes: Query<(
        &Team,
        &mut attributes::AttributeSources,
        &mut Hero,
        &mut combat::CombatStats,
    )>,
) {
    let captured_all = std::mem::take(&mut events.captured);
    for captured in captured_all {
        let Some(buff) = attributes::objective_buff(captured.objective_id, captured.owner) else {
            continue;
        };
        // 与 `attributes::objective_buff` 内部使用的 1/2/3 统一，避免两套 buff id
        // 导致客户端刷新与服务端去重不一致。
        let buff_id = registry::objective_id(buff.name).unwrap_or(captured.objective_id);
        for (team, mut sources, mut hero, mut stats) in heroes.iter_mut() {
            if *team != captured.owner {
                continue;
            }
            attributes::add_buff(&mut sources, buff.clone());
            attributes::recompute(&sources, &mut hero, &mut stats);
        }
        events.emit(NetEvent::ObjectiveCaptured {
            objective_id: captured.objective_id,
            owner: captured.owner,
        });
        events.emit(NetEvent::ObjectiveBuffGranted {
            objective_id: captured.objective_id,
            owner: captured.owner,
            buff_id,
        });
    }
}

/// 权威对局时钟：推进 tick、开启加时、硬上限按源核剩余生命比例判胜。
pub fn match_clock_system(
    tick: Res<ServerTick>,
    mut clock: ResMut<MatchClock>,
    mut events: ResMut<GameplayEvents>,
    cores: Query<(&Turret, &Team)>,
) {
    if clock.phase == MatchPhase::Warmup {
        // 开局帧记录权威 ServerTick 基准；不再 return，避免首帧丢 1 tick。
        clock.start_at(tick.0);
    }
    if clock.is_over() {
        return;
    }
    if clock.sync(tick.0) {
        events.emit(NetEvent::OvertimeStarted);
    }
    if clock.hard_cap_reached() {
        let (blue, red) = cores
            .iter()
            .filter(|(t, _)| t.turret_type == TurretType::SourceCore)
            .fold((0.0f32, 0.0f32), |(b, r), (t, team)| match team {
                Team::Blue => (b + t.health.max(0.0), r),
                _ => (b, r + t.health.max(0.0)),
            });
        let winner = if blue >= red { Team::Blue } else { Team::Red };
        let ticks = clock.tick;
        clock.finish(winner);
        events.emit(NetEvent::MatchFinished {
            winner,
            reason: 2, // 25 分钟硬上限
            duration_ticks: ticks,
        });
    }
}

/// buff 每 tick 推进；过期时重算属性，保证卖出/死亡后不会残留加成。
#[allow(clippy::type_complexity)]
pub fn buff_tick_system(
    mut heroes: Query<(
        &mut attributes::AttributeSources,
        &mut Hero,
        &mut combat::CombatStats,
    )>,
) {
    for (mut sources, mut hero, mut stats) in heroes.iter_mut() {
        if attributes::tick_buffs(&mut sources).is_empty() {
            continue;
        }
        attributes::recompute(&sources, &mut hero, &mut stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pve_bot_is_gated_on_connected_players() {
        let mut app = App::new();
        app.init_resource::<PveBotReady>()
            .init_resource::<NextNetId>()
            .init_resource::<DummyRegistry>()
            .init_resource::<ClientNetIds>()
            .insert_resource(MatchClock::new(MatchMode::Pve))
            .add_systems(Update, ensure_pve_bot);
        app.update();
        assert!(!app.world().resource::<PveBotReady>().0);
        assert_eq!(app.world().resource::<NextNetId>().0, 0);

        app.world_mut()
            .resource_mut::<ClientNetIds>()
            .0
            .insert(1u64, (NetId(99), Team::Blue));
        app.update();
        assert!(app.world().resource::<PveBotReady>().0);
        assert_eq!(app.world().resource::<NextNetId>().0, 1);
        assert_eq!(app.world().resource::<DummyRegistry>().0.len(), 1);
    }
}
