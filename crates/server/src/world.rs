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
    combat, jungle,
    objectives::ObjectiveCapturedEvent,
    objectives_economy::{
        ABYSS_WARDEN_CAPTURE_PER_PLAYER_GOLD, ABYSS_WARDEN_CAPTURE_TEAM_GOLD, AbyssWardenState,
        LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD, OBJECTIVE_ABYSS_WARDEN, OBJECTIVE_LODE_SHRINE,
        ObjectiveIncomeState, WARDEN_MINION_AD_MULT, WARDEN_MINION_HP_MULT,
        WARDEN_MINION_SPEED_BONUS, WardenBuff,
    },
    registry, structures,
    waves::{self, MinionAi, WaveDef},
};
use game_core::net::{GameEvent, JungleMonsterType, ServerMessage, Vec3f};
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
    warden_buff: Option<Res<WardenBuff>>,
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
                // §13.4：渊卫归属期间，归属方**新生成**的战偶获得推进强化。
                // 已存在的战偶不回溯增强；半径 20 的光环简化为整条兵线（见文档偏差登记）。
                let warden = warden_buff
                    .as_deref()
                    .is_some_and(|b| b.active(team, clock.tick));
                let (hp, ad, ms) = if warden {
                    (
                        hp * WARDEN_MINION_HP_MULT,
                        ad * WARDEN_MINION_AD_MULT,
                        ms + WARDEN_MINION_SPEED_BONUS,
                    )
                } else {
                    (hp, ad, ms)
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
        // 河道减速（视觉已由水面区分），与英雄共用同一地形系数。
        let speed = ai.move_speed * game_core::map::terrain_speed_mult(transform.translation);
        let velocity = Vec3::new(dir.x, 0.0, dir.y) * speed;
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
    mut heroes: Query<(&mut Hero, Option<&Team>)>,
    mut warden: Option<ResMut<AbyssWardenState>>,
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

        if let Ok((mut minion, nid)) = minions.get_mut(cmd.target) {
            let dealt = raw * 100.0 / (100.0 + minion.armor.max(0.0));
            minion.health = (minion.health - dealt).max(0.0);
            if minion.health <= combat::EPS {
                if let Ok((mut attacker_hero, _)) = heroes.get_mut(cmd.attacker) {
                    attacker_hero.gold = attacker_hero.gold.saturating_add(minion.gold_reward);
                    attacker_hero.experience =
                        attacker_hero.experience.saturating_add(minion.exp_reward);
                }
                // 广播死亡：客户端据此播放消失动画；服务端已 despawn（碰撞体随之移除）。
                events.emit(NetEvent::MinionDeath { net_id: nid.0 });
                commands.entity(cmd.target).despawn();
            }
            continue;
        }

        if let Ok((mut monster, nid)) = monsters.get_mut(cmd.target) {
            let dealt = raw * 100.0 / (100.0 + monster.armor.max(0.0));
            monster.health = (monster.health - dealt).max(0.0);
            let is_warden = monster.monster_type == JungleMonsterType::AbyssWarden;
            if is_warden {
                // §13.4：按队伍累计**英雄**造成的伤害判定归属，不用最后一击。
                // 只读查询拿队伍，避免与下面的可写借用同时存活。
                let team = heroes.get(cmd.attacker).ok().and_then(|(_, t)| t.copied());
                if let Some(state) = warden.as_deref_mut() {
                    state.record_damage(team, dealt);
                }
            }
            if monster.health <= combat::EPS {
                if let Ok((mut attacker_hero, _)) = heroes.get_mut(cmd.attacker) {
                    attacker_hero.gold = attacker_hero.gold.saturating_add(monster.gold_reward);
                    attacker_hero.experience =
                        attacker_hero.experience.saturating_add(monster.exp_reward);
                }
                events.emit(NetEvent::JungleMonsterDeath { net_id: nid.0 });
                if is_warden {
                    let owner = warden
                        .as_deref_mut()
                        .and_then(|state| state.on_defeated(clock.tick));
                    let killer_team = owner.unwrap_or(Team::Neutral);
                    events.emit(NetEvent::AbyssWardenDeath { killer_team });
                    if let Some(owner) = owner {
                        // 复用既有占领流水：objective_buff_system 会发放 120s 全队增益
                        // 并广播 ObjectiveCaptured / ObjectiveBuffGranted，无需第二套 buff 代码。
                        events.captured.push(ObjectiveCapturedEvent {
                            objective_id: OBJECTIVE_ABYSS_WARDEN,
                            owner,
                            event_id: (clock.tick << 8) | u64::from(OBJECTIVE_ABYSS_WARDEN),
                        });
                    }
                }
                commands.entity(cmd.target).despawn();
            }
        }
    }
}

/// 野怪对射程内英雄发起攻击（旧 `jungle_ai_system` 是死代码）。
///
/// P-U8 修两处旧缺口：
/// - **静止型营地不反击**：`step_monster` 让 `move_speed = 0` 的征募井 / 辉矿恒为
///   `Idle`，而旧版要求 `state == Aggro`，这 3 个营地因此永远打不到人。现在静止型
///   只要射程内有存活英雄就反击（与 `MonsterOutput.should_attack` 语义对齐）。
/// - **无攻击冷却**：旧版每 tick 插入一次 `AttackCommand`，被 `damage_system` 当 tick
///   消费，等于 60 次/秒。现在按 `attack_interval_ticks` 节流，时间点一律取权威
///   `SimTime::tick()`，0 值兜底为 60 tick（1 秒）。
pub fn monster_attack_system(
    time: Res<SimTime>,
    mut commands: Commands,
    mut monsters: Query<(Entity, &Transform, &mut JungleMonster, &jungle::MonsterAi)>,
    heroes: Query<(Entity, &Transform, &Hero, Option<&Team>)>,
) {
    let tick = time.tick();
    for (entity, transform, mut monster, ai) in monsters.iter_mut() {
        if tick < monster.next_attack_tick {
            continue;
        }
        if ai.state != jungle::MonsterState::Aggro && !ai.is_static() {
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
                tick,
            });
            let interval = if monster.attack_interval_ticks == 0 {
                60
            } else {
                monster.attack_interval_ticks
            };
            monster.next_attack_tick = tick + interval;
        }
    }
}

// ---------------------------------------------------------------------------
// P-C：目标 buff、加时衰竭与胜负判定
// ---------------------------------------------------------------------------

/// 占领成功 → 给归属方全队发放可过期的 buff，并广播全局事件。
#[allow(clippy::type_complexity)]
pub fn objective_buff_system(
    clock: Res<MatchClock>,
    mut income: ResMut<ObjectiveIncomeState>,
    mut warden_buff: ResMut<WardenBuff>,
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
        if captured.objective_id == OBJECTIVE_ABYSS_WARDEN {
            // §14.4 渊卫归属赏金 + §13.4 推进增益 120 s
            for (team, _, mut hero, _) in heroes.iter_mut() {
                if *team == captured.owner {
                    hero.gold = hero
                        .gold
                        .saturating_add(ABYSS_WARDEN_CAPTURE_PER_PLAYER_GOLD);
                }
            }
            income.credit_team(captured.owner, ABYSS_WARDEN_CAPTURE_TEAM_GOLD);
            warden_buff.grant(captured.owner, clock.tick);
        }
        // §13.2：兵矿占领瞬间全队每人 +40、团队池 +120，并重置该队的收益计时，
        // 避免「刚占领立刻又发一次 30 金」。渊卫（id 3）不享受这笔钱。
        if captured.objective_id == OBJECTIVE_LODE_SHRINE {
            for (team, _, mut hero, _) in heroes.iter_mut() {
                if *team == captured.owner {
                    hero.gold = hero
                        .gold
                        .saturating_add(LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD);
                }
            }
            income.on_captured(captured.objective_id, captured.owner, clock.tick);
            events.emit(NetEvent::LodeShrineCapture {
                team: captured.owner,
            });
        }
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

#[cfg(test)]
mod monster_attack_tests {
    use super::*;
    use game_core::net::JungleMonsterType;

    fn monster(move_speed: f32, attack_range: f32, interval: u64) -> JungleMonster {
        JungleMonster {
            monster_type: JungleMonsterType::MusterWell,
            max_health: 1000.0,
            health: 1000.0,
            attack_damage: 25.0,
            armor: 0.0,
            magic_resist: 0.0,
            move_speed,
            attack_range,
            gold_reward: 0,
            exp_reward: 0,
            aggro_range: 10.0,
            leash_range: 15.0,
            spawn_tick: 0,
            last_attacker: None,
            attack_interval_ticks: interval,
            next_attack_tick: 0,
        }
    }

    /// tick 0 且英雄在射程内，构造一只静止型 + 一只追击型（可选）。
    fn fixture(move_speed: f32, attack_range: f32, interval: u64, hero_x: f32) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(SimTime(0))
            .add_systems(Update, monster_attack_system);
        let ai = jungle::MonsterAi::new(Vec2::ZERO, 1000.0, 10.0, 15.0, move_speed);
        let e = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                monster(move_speed, attack_range, interval),
                ai,
                Team::Neutral,
            ))
            .id();
        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(hero_x, 0.0, 0.0)),
            Hero::default(),
            Team::Blue,
        ));
        (app, e)
    }

    fn attack_count(app: &mut App) -> usize {
        let mut q = app.world_mut().query::<&AttackCommand>();
        q.iter(app.world()).count()
    }

    fn clear_attack(app: &mut App, e: Entity) {
        app.world_mut().entity_mut(e).remove::<AttackCommand>();
    }

    /// 静止型（征募井 / 辉矿）状态机恒为 Idle，旧版要求 Aggro 导致永不反击。
    #[test]
    fn static_camps_retaliate_while_idle() {
        let (mut app, e) = fixture(0.0, 6.0, 60, 3.0);
        app.update();
        assert_eq!(attack_count(&mut app), 1, "静止型射程内必须反击");
        assert_eq!(
            app.world()
                .entity(e)
                .get::<JungleMonster>()
                .unwrap()
                .next_attack_tick,
            60
        );
    }

    /// 冷却按权威 tick 裁决：间隔内不重复插入，到期后再攻击。
    #[test]
    fn attack_is_rate_limited_by_authoritative_tick() {
        let (mut app, e) = fixture(0.0, 6.0, 60, 3.0);
        app.update();
        assert_eq!(attack_count(&mut app), 1);
        clear_attack(&mut app, e);

        for tick in [1, 30, 59] {
            app.insert_resource(SimTime(tick));
            app.update();
            assert_eq!(attack_count(&mut app), 0, "tick {tick} 仍在冷却中");
        }
        app.insert_resource(SimTime(60));
        app.update();
        assert_eq!(attack_count(&mut app), 1, "冷却到期应再次攻击");
    }

    /// 射程外不攻击；非静止型保持「必须进入 Aggro 才攻击」的原语义。
    #[test]
    fn out_of_range_and_idle_roamers_do_not_attack() {
        let (mut far, _) = fixture(0.0, 6.0, 60, 20.0);
        far.update();
        assert_eq!(attack_count(&mut far), 0);

        let (mut roamer, _) = fixture(3.5, 6.0, 60, 3.0);
        roamer.update();
        assert_eq!(attack_count(&mut roamer), 0, "Idle 追击型不应攻击");
    }

    /// 0 间隔兜底为 60 tick，避免配置遗漏导致 60 次/秒。
    #[test]
    fn zero_interval_falls_back_to_one_second() {
        let (mut app, e) = fixture(0.0, 6.0, 0, 3.0);
        app.update();
        assert_eq!(
            app.world()
                .entity(e)
                .get::<JungleMonster>()
                .unwrap()
                .next_attack_tick,
            60
        );
    }
}
