//! 服务端玩法系统（`GX-CBT-2026-001` §4~§15 的 ECS 接线）。
//!
//! 纯裁决逻辑位于 `game_core::gameplay`；本文件只负责：
//! 组件挂接、tick 驱动、输入快照、事件汇总与对既有 `Hero/Turret/JungleMonster` 的写回。
//! 系统顺序由 `SimulationPlugin` 的 `.chain()` 明确指定。

use crate::simulation::ServerTick;
use bevy::prelude::*;
use game_core::components::*;
use game_core::gameplay::{
    GameplayEvents, NetEvent, PendingDeaths, attributes, combat, jungle, objectives, registry,
    respawn, shop, structures,
};
use game_core::map::{BLUE_SOURCE_CORE, RED_SOURCE_CORE};
use game_core::net::JungleMonsterType;
use std::collections::HashMap;

/// 商品表资源：局内不可变，热更只影响新对局（`FR-61`）。
#[derive(Resource)]
pub struct ShopCatalogResource(pub shop::ShopCatalog);

impl Default for ShopCatalogResource {
    fn default() -> Self {
        Self(shop::ShopCatalog::placeholder())
    }
}

/// 购买意图队列。网络层收到 `ClientMessage::PurchaseItem` 后写入，同 tick 消费。
#[derive(Resource, Default, Debug)]
pub struct ShopIntents(pub Vec<(u64, shop::ShopRequest)>);

/// 售出意图队列（`slot` 由服务端重新核算返还，不信任客户端金额）。
#[derive(Resource, Default, Debug)]
pub struct SellIntents(pub Vec<(u64, u8)>);

/// 静态世界玩法组件只挂接一次，避免重复生成占领点。
#[derive(Resource, Default)]
pub struct GameplayWorldReady(pub bool);

/// 每 tick 由 `build_world_snapshot` 重建的只读世界视图，
/// 用于避免多个 `&mut Transform` / `&Transform` 查询产生 B0001 冲突。
#[derive(Resource, Default, Debug)]
pub struct WorldSnapshot {
    /// (平面位置, 阵营, 是否存活)
    pub heroes: Vec<(Vec2, Team, bool)>,
    pub minions: Vec<(Vec2, Team)>,
    pub turrets: Vec<(Vec2, Team, TurretType, f32)>,
}

pub fn build_world_snapshot(
    mut snapshot: ResMut<WorldSnapshot>,
    heroes: Query<(&Transform, Option<&Team>, &Hero)>,
    minions: Query<(&Transform, Option<&Team>), With<Minion>>,
    turrets: Query<(&Transform, Option<&Team>, &Turret)>,
) {
    snapshot.heroes = heroes
        .iter()
        .map(|(t, team, h)| {
            (
                Vec2::new(t.translation.x, t.translation.z),
                team.copied().unwrap_or(Team::Neutral),
                h.health > combat::EPS,
            )
        })
        .collect();
    snapshot.minions = minions
        .iter()
        .map(|(t, team)| {
            (
                Vec2::new(t.translation.x, t.translation.z),
                team.copied().unwrap_or(Team::Neutral),
            )
        })
        .collect();
    snapshot.turrets = turrets
        .iter()
        .map(|(t, team, tur)| {
            (
                Vec2::new(t.translation.x, t.translation.z),
                team.copied().unwrap_or(Team::Neutral),
                tur.turret_type,
                tur.health,
            )
        })
        .collect();
}

/// 为所有既有 `Hero` 实体幂等挂接玩法组件（不需要改动生成代码）。
pub fn attach_hero_gameplay(
    mut commands: Commands,
    heroes: Query<
        (
            Entity,
            &Hero,
            Option<&Team>,
            Option<&combat::CombatStats>,
            Option<&combat::ShieldPool>,
            Option<&combat::StatusEffects>,
            Option<&combat::RegenStats>,
            Option<&shop::Inventory>,
            Option<&respawn::RespawnState>,
            Option<&respawn::KillStreak>,
            Option<&attributes::AttributeSources>,
        ),
        With<Hero>,
    >,
) {
    for (e, hero, team, stats, shield, status, regen, inv, state, streak, sources) in heroes.iter()
    {
        let mut ec = commands.entity(e);
        if sources.is_none() {
            ec.insert(attributes::AttributeSources::new(hero));
        }
        if stats.is_none() {
            ec.insert(combat::CombatStats::default());
        }
        if shield.is_none() {
            ec.insert(combat::ShieldPool::default());
        }
        if status.is_none() {
            ec.insert(combat::StatusEffects::default());
        }
        if regen.is_none() {
            ec.insert(combat::RegenStats::default());
        }
        if inv.is_none() {
            ec.insert(shop::Inventory::default());
        }
        if state.is_none() {
            let core = match team.copied().unwrap_or(Team::Neutral) {
                Team::Red => RED_SOURCE_CORE,
                _ => BLUE_SOURCE_CORE,
            };
            ec.insert(respawn::RespawnState::new(1, Vec2::new(core.x, core.z)));
        }
        if streak.is_none() {
            ec.insert(respawn::KillStreak::default());
        }
    }
}

/// 为野怪 / 哨塔挂接 AI 与防御组件，并一次性生成 2 个占领型中立目标。
pub fn attach_world_gameplay(
    mut commands: Commands,
    mut ready: ResMut<GameplayWorldReady>,
    monsters: Query<(Entity, &JungleMonster, &Transform), Without<jungle::MonsterAi>>,
    turrets: Query<(Entity, &Turret), Without<structures::StructureDefense>>,
) {
    for (e, m, t) in monsters.iter() {
        let mut ai = jungle::MonsterAi::new(
            Vec2::new(t.translation.x, t.translation.z),
            m.max_health,
            m.aggro_range,
            m.leash_range,
            m.move_speed,
        );
        ai.attack_range = m.attack_range;
        commands
            .entity(e)
            .insert(ai)
            .insert(jungle::MonsterRespawn::new(m.spawn_tick, 5400));
    }
    for (e, tur) in turrets.iter() {
        commands.entity(e).insert(structures::StructureDefense::new(
            tur.max_health,
            tur.turret_type == TurretType::SourceCore,
        ));
    }
    if !ready.0 {
        ready.0 = true;
        // 征募井 / 辉矿（§13.2 / §13.3）：直接取自野区营地表中的两座野区商店，
        // 保证占点位置与地图（单条兵线 + 内侧野区）完全同源。
        let shops: Vec<Vec3> = game_core::map::JUNGLE_CAMPS
            .iter()
            .filter(|c| {
                matches!(
                    c.monster_type,
                    JungleMonsterType::MusterWell | JungleMonsterType::LodeShrine
                )
            })
            .map(|c| c.position)
            .collect();
        for (idx, pos) in shops.into_iter().enumerate() {
            commands.spawn((
                Transform::from_translation(pos),
                objectives::CapturePoint::new(idx as u32 + 1, 100, 5400),
            ));
        }
    }
}

/// 状态效果推进：到期移除、灼烧按 0.5 s 结算真实伤害（§8）。
pub fn status_tick_system(
    tick: Res<ServerTick>,
    mut query: Query<(&mut combat::StatusEffects, &mut Hero)>,
) {
    for (mut status, mut hero) in query.iter_mut() {
        for burn in status.tick(tick.0) {
            hero.health = (hero.health - burn.strength).max(0.0);
        }
    }
}

/// 生命 / 能量回复（§4.1）；阵亡期间不回复。
pub fn regen_system(
    mut query: Query<(
        &mut Hero,
        &combat::RegenStats,
        Option<&respawn::RespawnState>,
    )>,
) {
    for (mut hero, regen, state) in query.iter_mut() {
        // 阵亡或本 tick 刚被打至 0 生命的单位不得回复；死亡状态由链尾统一标记
        if state.is_some_and(|s| s.dead) || hero.health <= combat::EPS {
            continue;
        }
        combat::apply_regen(&mut hero, regen);
    }
}

/// 死亡与重生（§15）：消费击杀流水、结算连杀赏金、恢复生命并给出生保护。
#[allow(clippy::type_complexity)]
pub fn death_respawn_system(
    tick: Res<ServerTick>,
    mut pending: ResMut<PendingDeaths>,
    mut events: ResMut<GameplayEvents>,
    mut query: Query<(
        &NetId,
        &mut Hero,
        &mut respawn::RespawnState,
        &mut combat::StatusEffects,
        &mut combat::ShieldPool,
        &mut respawn::KillStreak,
        &mut Transform,
    )>,
    id_to_entity: Query<(Entity, &NetId)>,
) {
    let index: HashMap<u64, Entity> = id_to_entity.iter().map(|(e, n)| (n.0, e)).collect();

    for (t, killer_id, victim_id) in pending.0.drain(..) {
        let mut bounty = 0;
        if let Some(&ke) = index.get(&killer_id)
            && let Ok((_, mut hero, _, _, _, mut streak, _)) = query.get_mut(ke)
        {
            bounty = respawn::kill_bounty(&mut streak, t);
            respawn::register_kill(&mut streak, t);
            hero.gold = hero.gold.saturating_add(bounty);
        }
        if let Some(&ve) = index.get(&victim_id)
            && let Ok((_, _, mut state, mut status, mut shield, mut streak, _)) = query.get_mut(ve)
            && !state.dead
        {
            respawn::mark_dead(&mut state, t);
            respawn::reset_streak(&mut streak);
            status.clear();
            shield.value = 0.0;
        }
        events.deaths.push(respawn::DeathRecord {
            victim_net_id: victim_id,
            killer_net_id: killer_id,
            tick: t,
            bounty,
        });
    }

    for (_, mut hero, mut state, mut status, mut shield, _, mut transform) in query.iter_mut() {
        if respawn::step_respawn(&mut state, tick.0) {
            hero.health = hero.max_health;
            hero.energy = hero.max_energy;
            shield.value = 0.0;
            status.clear();
            // 出生保护：3 s 无敌 + 加速（主动攻击时由 consume_on_action 移除）
            status.apply(
                combat::EffectInstance {
                    kind: combat::StatusKind::Invulnerable,
                    strength: 1.0,
                    remaining_ticks: respawn::INVULNERABLE_TICKS as u32,
                    source_net_id: 0,
                },
                tick.0,
            );
            transform.translation.x = state.spawn_point.x;
            transform.translation.z = state.spawn_point.y;
        }
    }
}

/// 野怪 AI（§12.3）：从只读快照取最近敌方英雄，推进状态机并写回位移。
pub fn monster_ai_system(
    snapshot: Res<WorldSnapshot>,
    mut monsters: Query<(&mut jungle::MonsterAi, &mut Transform, Option<&Team>)>,
) {
    for (mut ai, mut transform, team) in monsters.iter_mut() {
        let pos = Vec2::new(transform.translation.x, transform.translation.z);
        let own_team = team.copied().unwrap_or(Team::Neutral);
        let nearest = snapshot
            .heroes
            .iter()
            .filter(|(_, t, alive)| *alive && *t != own_team)
            .map(|(p, _, _)| *p)
            .min_by(|a, b| {
                a.distance(pos)
                    .partial_cmp(&b.distance(pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let out = jungle::step_monster(
            &mut ai,
            &jungle::MonsterInput {
                position: pos,
                nearest_enemy: nearest,
            },
        );
        transform.translation.x += out.desired_move_dir.x / 60.0;
        transform.translation.z += out.desired_move_dir.y / 60.0;
    }
}

/// 中立目标占领（§13.1）：争夺冻结、进度推进、占领事件入汇。
pub fn capture_system(
    tick: Res<ServerTick>,
    snapshot: Res<WorldSnapshot>,
    mut events: ResMut<GameplayEvents>,
    mut points: Query<(&mut objectives::CapturePoint, &Transform)>,
) {
    for (mut cp, transform) in points.iter_mut() {
        let pos = Vec2::new(transform.translation.x, transform.translation.z);
        let radius = cp.radius;
        let within = |team: Team| {
            snapshot
                .heroes
                .iter()
                .filter(|(p, t, alive)| *alive && *t == team && p.distance(pos) <= radius)
                .count() as u32
        };
        let outcome = objectives::step_capture(
            &mut cp,
            objectives::CaptureInput {
                blue_count: within(Team::Blue),
                red_count: within(Team::Red),
                tick: tick.0,
            },
        );
        if outcome != objectives::CaptureOutcome::None
            && let Some(event) = objectives::capture_event(&cp, tick.0)
            && matches!(outcome, objectives::CaptureOutcome::Captured(_))
        {
            events.captured.push(event);
        }
    }
}

/// 哨塔防御状态更新（§14.2）：逐塔减伤、友方战偶计数、脱战回复。
pub fn structure_defense_system(
    snapshot: Res<WorldSnapshot>,
    mut query: Query<(
        &Transform,
        Option<&Team>,
        &Turret,
        &mut structures::StructureDefense,
    )>,
) {
    for (transform, team, turret, mut def) in query.iter_mut() {
        let own_team = team.copied().unwrap_or(Team::Neutral);
        let pos = Vec2::new(transform.translation.x, transform.translation.z);
        def.friendly_constructs_in_range = snapshot
            .minions
            .iter()
            .filter(|(p, t)| *t == own_team && p.distance(pos) <= turret.attack_range)
            .count() as u32;
        def.front_tower_alive = turret.turret_type != TurretType::Outer
            && snapshot.turrets.iter().any(|(_, t, kind, health)| {
                *t == own_team && *kind == TurretType::Outer && *health > combat::EPS
            });
        def.max_health = turret.max_health;
        def.health = structures::tick_out_of_combat(&mut def);
    }
}

/// 商店购买 / 售出事务（§10.3）。
///
/// 属性走 `AttributeSources` 聚合层重算（装备 + buff + 等级），不再直接 `+=`，
/// 从而保证售出、死亡、buff 过期后属性可还原。成功与拒绝都会写入 `NetEvent`
/// 供协议层广播，客户端据此刷新 UI 与置灰原因。
#[allow(clippy::type_complexity)]
pub fn shop_purchase_system(
    tick: Res<ServerTick>,
    catalog: Res<ShopCatalogResource>,
    mut intents: ResMut<ShopIntents>,
    mut sells: ResMut<SellIntents>,
    mut events: ResMut<GameplayEvents>,
    mut query: Query<(
        &NetId,
        &Transform,
        Option<&Team>,
        &mut Hero,
        &mut shop::Inventory,
        &mut combat::CombatStats,
        &mut attributes::AttributeSources,
        &respawn::RespawnState,
    )>,
) {
    for (net_id, mut request) in intents.0.drain(..) {
        let Some((_, transform, team, mut hero, mut inv, mut stats, mut sources, state)) = query
            .iter_mut()
            .find(|(n, _, _, _, _, _, _, _)| n.0 == net_id)
        else {
            continue;
        };
        let core = match team.copied().unwrap_or(Team::Neutral) {
            Team::Red => RED_SOURCE_CORE,
            _ => BLUE_SOURCE_CORE,
        };
        request.distance_to_source = Vec2::new(transform.translation.x, transform.translation.z)
            .distance(Vec2::new(core.x, core.z));
        request.is_dead = state.dead;
        request.death_tick = state.death_tick;
        request.current_tick = tick.0;

        let item_id = registry::item_id(request.item_id);
        let mut gold = hero.gold;
        match shop::try_buy(&catalog.0, &mut inv, &mut gold, &request) {
            Ok(receipt) => {
                hero.gold = gold;
                let delta = attributes::StatDelta::from_item(&receipt.delta);
                sources.equipment = sources.equipment.add(&delta);
                attributes::recompute(&sources, &mut hero, &mut stats);
                let paid = receipt.paid;
                events.purchases.push(receipt);
                events.emit(NetEvent::ItemPurchased {
                    net_id,
                    item_id,
                    paid,
                });
            }
            Err(reason) => {
                // 拒绝时 gold 局部变量被丢弃，英雄金币与栏位保持不变
                events.emit(NetEvent::ShopRejected {
                    net_id,
                    item_id,
                    reason: reject_code(&reason),
                });
            }
        }
    }

    for (net_id, slot) in sells.0.drain(..) {
        let Some((_, _, _, mut hero, mut inv, mut stats, mut sources, _)) = query
            .iter_mut()
            .find(|(n, _, _, _, _, _, _, _)| n.0 == net_id)
        else {
            continue;
        };
        let slot = slot as usize;
        let item_id = inv
            .slots
            .get(slot)
            .cloned()
            .flatten()
            .map(|s| registry::item_id(&s))
            .unwrap_or(0);
        let mut gold = hero.gold;
        match shop::try_sell(&catalog.0, &mut inv, &mut gold, slot) {
            Ok(refund) => {
                hero.gold = gold;
                // 售出的装备增量必须从来源中移除；用当前栏位重建，保证升级差价语义正确。
                sources.equipment = equipment_from_inventory(&catalog.0, &inv);
                attributes::recompute(&sources, &mut hero, &mut stats);
                events.emit(NetEvent::ItemSold {
                    net_id,
                    item_id,
                    refund,
                });
            }
            Err(reason) => events.emit(NetEvent::ShopRejected {
                net_id,
                item_id,
                reason: reject_code(&reason),
            }),
        }
    }
}

fn reject_code(reason: &shop::ShopReject) -> u8 {
    match reason {
        shop::ShopReject::UnknownItem => 1,
        shop::ShopReject::DeadTooLong => 2,
        shop::ShopReject::NotInShopZone => 3,
        shop::ShopReject::InsufficientGold => 4,
        shop::ShopReject::MissingPrerequisite => 5,
        shop::ShopReject::InventoryFull => 6,
    }
}

/// 按当前栏位重建装备增量（`try_sell` 后调用）。
fn equipment_from_inventory(
    catalog: &shop::ShopCatalog,
    inv: &shop::Inventory,
) -> attributes::StatDelta {
    inv.slots
        .iter()
        .flatten()
        .filter_map(|id| catalog.get(id))
        .fold(attributes::StatDelta::default(), |acc, def| {
            acc.add(&attributes::StatDelta::from_item(&def.stats))
        })
}

// 注：`AttributeSources` 由 `attach_hero_gameplay` 幂等挂载，是本模块唯一的
// 属性写入口；此前直写 `Hero` 的 legacy 分支已移除，避免装备加成在升级 /
// buff 过期 / 售出时被重算覆盖或重复叠加。
