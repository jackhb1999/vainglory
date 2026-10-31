//! 客户端预测与回滚系统

use bevy::prelude::*;
use game_core::components::*;
use game_core::net::{InputCmd, WorldSnapshot, EntitySnapshot, Vec2f};
use game_core::math::{TickClock, lerp_vec3, lerp_quat};
use std::collections::VecDeque;

pub struct PredictionPlugin;

impl Plugin for PredictionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TickClock>()
            .init_resource::<PredictionState>()
            .add_systems(FixedUpdate, (
                apply_prediction,
                reconcile_with_server,
            ).chain());
    }
}

#[derive(Resource, Default)]
pub struct PredictionState {
    /// 待确认的预测输入历史
    pub pending_inputs: VecDeque<InputCmd>,
    /// 服务端确认的最新 tick
    pub server_confirmed_tick: u64,
    /// 本地预测的 tick
    pub local_tick: u64,
}

impl PredictionState {
    pub fn add_input(&mut self, input: InputCmd) {
        self.pending_inputs.push_back(input);
        // 保留最近 120 帧（2秒@60Hz）
        if self.pending_inputs.len() > 120 {
            self.pending_inputs.pop_front();
        }
    }
    
    pub fn confirm_up_to(&mut self, tick: u64) {
        self.server_confirmed_tick = tick;
        // 移除已确认的输入
        self.pending_inputs.retain(|cmd| cmd.tick > tick);
    }
}

fn apply_prediction(
    time: Res<Time>,
    mut tick_clock: ResMut<TickClock>,
    mut pred_state: ResMut<PredictionState>,
    input_state: Res<crate::input::PlayerInputState>,
    mut local_hero: Query<(&mut Transform, &mut TransformSync, &Hero), (With<LocalPlayer>, With<Predicted>, Without<Interpolated>)>,
) {
    let ticks_advanced = tick_clock.advance(time.delta_secs());
    
    for _ in 0..ticks_advanced {
        pred_state.local_tick += 1;
        
        // 创建本地输入命令
        let cmd = InputCmd {
            tick: pred_state.local_tick,
            move_dir: input_state.move_dir.into(),
            aim_dir: input_state.aim_dir.into(),
            ability_mask: input_state.ability_mask,
            ping: 0,
        };
        
        pred_state.add_input(cmd.clone());
        
        // 应用预测移动
        for (mut transform, mut sync, hero) in local_hero.iter_mut() {
            if cmd.move_dir != Vec2f::zero() {
                let direction = Vec3::new(cmd.move_dir.x, 0.0, cmd.move_dir.y).normalize();
                let velocity = direction * hero.move_speed * tick_clock.tick_duration();
                
                transform.translation += velocity;
                // 本地预测也限制在地图边界内，避免越界后被服务端快照拉回造成抖动
                let half_w = game_core::map::MAP_WIDTH / 2.0 - 1.0;
                let half_h = game_core::map::MAP_HEIGHT / 2.0 - 1.0;
                transform.translation.x = transform.translation.x.clamp(-half_w, half_w);
                transform.translation.z = transform.translation.z.clamp(-half_h, half_h);
                sync.position = transform.translation;
                sync.velocity = velocity;
                sync.last_update_tick = pred_state.local_tick;
            }
            
            // 朝向更新
            if cmd.aim_dir != Vec2f::zero() {
                let angle = cmd.aim_dir.y.atan2(cmd.aim_dir.x);
                transform.rotation = Quat::from_rotation_y(-angle);
                sync.rotation = transform.rotation;
            }
        }
    }
}

fn reconcile_with_server(
    mut pred_state: ResMut<PredictionState>,
    snapshot: Option<Res<crate::networking::ReceivedSnapshot>>,
    lobby_info: Res<crate::networking::ClientLobbyInfo>,
    mut local_hero: Query<(&mut Transform, &mut TransformSync), (With<LocalPlayer>, With<Predicted>, Without<Interpolated>)>,
) {
    let Some(snapshot) = snapshot else { return };
    let snapshot = &snapshot.0;
    
    // 确认服务端处理到的 tick
    pred_state.confirm_up_to(snapshot.tick);
    
    // 找到本地英雄在快照中的数据
    let my_net_id = lobby_info.my_net_id;
    
    for entity_snap in &snapshot.entities {
        if Some(entity_snap.net_id) == my_net_id {
            // 回滚修正
            for (mut transform, mut sync) in local_hero.iter_mut() {
                let pos_diff = (transform.translation - entity_snap.position.to_vec3()).length();
                
                // 如果差异超过阈值，硬修正
                if pos_diff > 0.5 {
                    transform.translation = entity_snap.position.to_vec3();
                    transform.rotation = entity_snap.rotation.to_quat();
                    sync.position = transform.translation;
                    sync.rotation = transform.rotation;
                    sync.velocity = entity_snap.velocity.to_vec3();
                    info!("Rollback correction: diff = {:.2}", pos_diff);
                }
                // 小差异平滑修正
                else if pos_diff > 0.05 {
                    transform.translation = transform.translation.lerp(entity_snap.position.to_vec3(), 0.5);
                    transform.rotation = transform.rotation.slerp(entity_snap.rotation.to_quat(), 0.5);
                    sync.position = transform.translation;
                    sync.rotation = transform.rotation;
                }
            }
            break;
        }
    }
}