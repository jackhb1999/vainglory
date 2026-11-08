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
            // Bevy 的元组 chain 有元数上限，拆成多组再嵌套 chain 保持严格顺序。
            // 时钟组与事件下发组无条件执行：前者保证 ServerTick 连续性，后者保证
            // 判胜当 tick 产生的 MatchFinished 事件一定能被下发，不受冻结影响。
            .add_systems(
                FixedUpdate,
                (
                    (tick_clock_system, match_clock_system).chain(),
                    (
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
                        structure_defense_system,
                        shop_purchase_system,
                        buff_tick_system,
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

/// 将各客户端最新输入写入对应英雄的 PlayerInput 组件，驱动移动/朝向
fn apply_pending_inputs(
    mut heroes: Query<(&NetId, &mut PlayerInput)>,
    mut pending: ResMut<PendingInputs>,
) {
    for (net_id, mut input) in heroes.iter_mut() {
        if let Some(cmd) = pending.inputs.get(&net_id.0) {
            input.move_dir = Vec2::new(cmd.move_dir.x, cmd.move_dir.y);
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
