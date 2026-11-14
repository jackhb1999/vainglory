//! 服务端模拟系统：60Hz 权威 `FixedUpdate`，严格 `.chain()` 顺序。

use crate::gameplay::*;
use crate::net_bridge::drain_gameplay_events;
use crate::networking::PendingInputs;
use crate::systems::*;
use crate::world::*;
use bevy::prelude::*;
use game_core::components::*;
use game_core::gameplay::clock::MatchClock;
use game_core::gameplay::{GameplayEvents, PendingDeaths};
use game_core::systems::*;

pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerTick>()
            .init_resource::<SimTime>()
            // NAV-M2-01/03：双端同源动态障碍网格 + 连续违规审计
            .init_resource::<game_core::navgrid::NavGrid>()
            .init_resource::<game_core::navigation::MovementViolations>()
            .init_resource::<MatchClock>()
            .init_resource::<crate::networking::ServerEvents>()
            .init_resource::<DamageSink>()
            // 玩法资源：商品表局内不可变，事件汇与击杀流水由玩法系统消费
            .init_resource::<ShopCatalogResource>()
            .init_resource::<ShopIntents>()
            .init_resource::<SellIntents>()
            .init_resource::<GameplayWorldReady>()
            .init_resource::<WorldSnapshot>()
            .init_resource::<GameplayEvents>()
            .init_resource::<PendingDeaths>()
            .init_resource::<PveBotReady>()
            .init_resource::<CoreDestroyed>()
            .init_resource::<TargetCandidates>()
            // §13.2 / §13.4：渊卫刷新状态与兵矿收益账本
            .init_resource::<game_core::gameplay::objectives_economy::AbyssWardenState>()
            .init_resource::<game_core::gameplay::objectives_economy::ObjectiveIncomeState>()
            .init_resource::<game_core::gameplay::objectives_economy::WardenBuff>()
            // Bevy 的元组 chain 有元数上限，拆成多组再嵌套 chain 保持严格顺序。
            // 时钟组与事件下发组无条件执行：前者保证 ServerTick 连续性，后者保证
            // 判胜当 tick 产生的 MatchFinished 事件一定能被下发，不受冻结影响。
            .add_systems(
                FixedUpdate,
                (
                    (tick_clock_system, match_clock_system).chain(),
                    (
                        // 渊卫刷新必须先于网格重建：同一 tick 生成的首领要立刻
                        // 进入 NavGrid 动态障碍，否则客户端会规划出穿渊卫的路径。
                        crate::objectives::abyss_warden_spawn_system,
                        // 网格重建必须早于 movement_system：校验要用当 tick 的
                        // 动态障碍，塔被摧毁的同一 tick 就要解除阻挡（C3）。
                        rebuild_server_nav_grid,
                        ensure_pve_bot,
                        build_world_snapshot,
                        attach_hero_gameplay,
                        attach_world_gameplay,
                        apply_pending_inputs,
                        pve_bot_ai_system,
                        movement_system,
                        facing_system,
                        wave_spawn_system,
                        collect_target_candidates_system,
                    )
                        .chain()
                        .run_if(match_running),
                    (
                        minion_ai_system,
                        collision_system,
                        turret_ai_system,
                        monster_attack_system,
                        attack_system,
                        ability_cast_system,
                        projectile_system,
                        world_damage_system,
                        damage_system,
                        resolve_pending_hits,
                        flush_damage_events,
                        ability_cooldown_system,
                        level_up_system,
                        passive_gold_system,
                    )
                        .chain()
                        .run_if(match_running),
                    (
                        health_regen_system,
                        energy_regen_system,
                        status_tick_system,
                        regen_system,
                        monster_ai_system,
                        capture_system,
                        objective_buff_system,
                        // 归属结算后按队发放兵矿持续收益（§13.2）
                        crate::objectives::objective_income_system,
                        structure_defense_system,
                        shop_purchase_system,
                        buff_tick_system,
                        // 涉水伤害在回复之后、死亡登记之前结算，淹死也能走统一复活流程。
                        river_hazard_system,
                        death_respawn_system,
                    )
                        .chain()
                        .run_if(match_running),
                    drain_gameplay_events,
                )
                    .chain(),
            );
    }
}

#[derive(Resource, Default)]
pub struct ServerTick(pub u64);

/// `NAV-M2-01`：每 tick 从权威 ECS 生成**全局**障碍视图并重建服务端网格。
///
/// - 哨塔死亡后实体仍以 `health = 0` 常驻并继续下发（`world_damage_system` 不 despawn），
///   因此必须显式给出存活位；客户端禁止靠 `health` 或渲染实体推断（C1）。
/// - 渊卫是 `JungleMonster`（死亡即 despawn），存活时才有 `abyss_warden` 坐标。
/// - 障碍集合变化由 `NavGrid::rebuild` 当 tick 检测并递增 `version`（C3）；
///   无变化时由网格内部按 30 tick 节流（C4）。草丛 Brush 可通行、不进网格（C7）。
pub fn rebuild_server_nav_grid(
    tick: Res<ServerTick>,
    mut grid: ResMut<game_core::navgrid::NavGrid>,
    turrets: Query<(&Transform, &Turret)>,
    monsters: Query<(&Transform, &JungleMonster)>,
) {
    const EPS: f32 = game_core::gameplay::combat::EPS;
    let mut turret_list = Vec::new();
    let mut cores = Vec::new();
    for (t, tur) in turrets.iter() {
        let p = [t.translation.x, t.translation.z];
        let alive = tur.health > EPS;
        if tur.turret_type == TurretType::SourceCore {
            if alive {
                cores.push(p);
            }
        } else {
            turret_list.push((p, alive));
        }
    }
    let warden = monsters
        .iter()
        .find(|(_, m)| {
            m.monster_type == game_core::net::JungleMonsterType::AbyssWarden && m.health > EPS
        })
        .map(|(t, _)| [t.translation.x, t.translation.z]);
    grid.rebuild(&game_core::navgrid::NavObstacleSnapshot {
        turrets: turret_list,
        source_cores: cores,
        abyss_warden: warden,
        server_tick: tick.0,
    });
}

/// 每个 FixedUpdate 同步权威 tick 到玩法层使用的 `SimTime`。
fn tick_clock_system(mut tick: ResMut<ServerTick>, mut sim: ResMut<SimTime>) {
    tick.0 += 1;
    sim.0 = tick.0;
}

/// 冻结门禁（P-A）：`MatchClock.finish()` 置 `frozen` 后停止一切玩法裁决系统，
/// 避免胜负已定后仍继续移动 / 攻击 / 交易。用 `Option<Res<_>>` 兼容不含
/// MatchClock 的测试夹具；回放 / 观战模式可置 `frozen=false` 保持模拟。
fn match_running(clock: Option<Res<MatchClock>>) -> bool {
    !clock.is_some_and(|c| c.frozen)
}

/// 将各客户端最新输入写入对应英雄的 PlayerInput 组件，驱动移动/朝向。
///
/// 死亡门禁：阵亡英雄（health<=0）的输入被强制清零，客户端无法再控制尸体移动/攻击/施法。
/// 这是「血空仍能控制」的权威修复点（其余系统另有二次守卫，避免其他路径绕过）。
fn apply_pending_inputs(
    mut heroes: Query<(&NetId, &mut PlayerInput, &Hero)>,
    mut pending: ResMut<PendingInputs>,
) {
    for (net_id, mut input, hero) in heroes.iter_mut() {
        if hero.health <= game_core::gameplay::combat::EPS {
            input.move_dir = Vec2::ZERO;
            input.ability_mask = 0;
            input.target_net_id = None;
            continue;
        }
        if let Some(cmd) = pending.inputs.get(&net_id.0) {
            // §24.3 反作弊底线：上行方向必须先清洗。NaN / Inf 一旦进入位置积分，
            // 会永久污染权威坐标与之后所有快照，且无法通过回滚自愈。
            input.move_dir =
                game_core::navigation::sanitize_move_dir(Vec2::new(cmd.move_dir.x, cmd.move_dir.y));
            input.aim_dir = Vec2::new(cmd.aim_dir.x, cmd.aim_dir.y);
            input.ability_mask = cmd.ability_mask;
            input.target_net_id = cmd.target_net_id.map(NetId);
            input.tick = cmd.tick;
        }
    }
    // 技能位是**一次性事件**：`PendingInputs` 从不清空，旧版同一个 InputCmd
    // 会每个 FixedUpdate 反复触发施法（冷却一结束就自动再放一次），且丢包时
    // 用滞后的 aim_dir。消费后只清技能位，保留 bit0 普攻持续位。
    for cmd in pending.inputs.values_mut() {
        cmd.ability_mask &= 1;
    }
}
