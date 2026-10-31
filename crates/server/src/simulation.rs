//! 服务端模拟系统

use bevy::prelude::*;
use game_core::components::*;
use game_core::systems::*;
use game_core::math::TickClock;
use crate::networking::PendingInputs;
use crate::systems::*;

pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerTick>()
            .init_resource::<crate::networking::ServerEvents>()
            .add_systems(FixedUpdate, (
                tick_clock_system,
                apply_pending_inputs,
                movement_system,
                attack_system,
                ability_cast_system,
                projectile_system,
                damage_system,
                resolve_pending_hits,
                ability_cooldown_system,
                level_up_system,
                passive_gold_system,
                health_regen_system,
                energy_regen_system,
            ).chain());
    }
}

#[derive(Resource, Default)]
pub struct ServerTick(pub u64);

fn tick_clock_system(mut tick: ResMut<ServerTick>) {
    tick.0 += 1;
}

/// 将各客户端最新输入写入对应英雄的 PlayerInput 组件，驱动移动/朝向
fn apply_pending_inputs(
    mut heroes: Query<(&NetId, &mut PlayerInput)>,
    pending: Res<PendingInputs>,
) {
    for (net_id, mut input) in heroes.iter_mut() {
        if let Some(cmd) = pending.inputs.get(&net_id.0) {
            input.move_dir = Vec2::new(cmd.move_dir.x, cmd.move_dir.y);
            input.aim_dir = Vec2::new(cmd.aim_dir.x, cmd.aim_dir.y);
            input.ability_mask = cmd.ability_mask;
            input.tick = cmd.tick;
        }
    }
}