//! 客户端预测与回滚系统

use bevy::prelude::*;
use game_core::components::*;
use game_core::math::TickClock;
use game_core::net::{InputCmd, Vec2f};
use std::collections::VecDeque;

pub struct PredictionPlugin;

impl Plugin for PredictionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TickClock>()
            .init_resource::<PredictionState>()
            .add_systems(
                FixedUpdate,
                (apply_prediction, reconcile_with_server).chain(),
            );
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

/// 本地预测使用的静态障碍：塔、源核、野怪营地与训练假人。
/// 与 `game_core::map` 同源，保证客户端不会“穿过”服务端认为存在的障碍。
fn static_obstacles() -> Vec<(Vec3, f32)> {
    let mut out: Vec<(Vec3, f32)> = Vec::new();
    for t in game_core::map::TURRET_POSITIONS {
        out.push((t.position, 2.5));
    }
    out.push((game_core::map::BLUE_SOURCE_CORE, 3.5));
    out.push((game_core::map::RED_SOURCE_CORE, 3.5));
    for camp in game_core::map::JUNGLE_CAMPS {
        // 与服务端野怪的 `Collider` 半径保持一致（2.0），避免客户端预测穿模
        out.push((camp.position, 2.0));
    }
    out.push((game_core::map::TRAINING_DUMMY_POS, 1.5));
    out
}

/// 把一个圆形身体从静态障碍中推出（完全由本方承担位移）。
fn push_out_of_static(pos: &mut Vec3, radius: f32, obstacles: &[(Vec3, f32)]) {
    for (center, other_radius) in obstacles {
        let delta = Vec2::new(pos.x - center.x, pos.z - center.z);
        let dist = delta.length();
        let min_dist = radius + *other_radius;
        if dist < min_dist {
            let dir = if dist > 1e-4 {
                delta / dist
            } else {
                Vec2::new(1.0, 0.0)
            };
            let overlap = min_dist - dist;
            pos.x += dir.x * overlap;
            pos.z += dir.y * overlap;
        }
    }
}

fn apply_prediction(
    time: Res<Time>,
    mut tick_clock: ResMut<TickClock>,
    mut pred_state: ResMut<PredictionState>,
    input_state: Res<crate::input::PlayerInputState>,
    mut local_hero: Query<
        (Entity, &mut Transform, &mut TransformSync, &Hero),
        (With<LocalPlayer>, With<Predicted>, Without<Interpolated>),
    >,
    other_heroes: Query<(Entity, &Transform, &Collider), (With<Hero>, Without<LocalPlayer>)>,
) {
    let ticks_advanced = tick_clock.advance(time.delta_secs());
    let obstacles = static_obstacles();

    for _ in 0..ticks_advanced {
        pred_state.local_tick += 1;

        // 创建本地输入命令
        let cmd = InputCmd {
            tick: pred_state.local_tick,
            seq: pred_state.local_tick as u32,
            client_time_us: 0,
            move_dir: input_state.move_dir.into(),
            aim_dir: input_state.aim_dir.into(),
            ability_mask: input_state.ability_mask,
            target_net_id: input_state.target_net_id.map(|n| n.0),
            ping: 0,
            config_hash: [0; 32],
            checksum: 0,
        };

        pred_state.add_input(cmd.clone());

        // 应用预测移动
        for (local_entity, mut transform, mut sync, hero) in local_hero.iter_mut() {
            if cmd.move_dir != Vec2f::zero() {
                let direction = Vec3::new(cmd.move_dir.x, 0.0, cmd.move_dir.y).normalize();
                let velocity = direction * hero.move_speed * tick_clock.tick_duration();

                transform.translation += velocity;

                // 本地预测碰撞：先与静态障碍分离，再与其他英雄互相推开，
                // 与服务端 collision_system 保持一致的“不穿模”体验
                push_out_of_static(&mut transform.translation, 1.3, &obstacles);
                for (other_entity, other_transform, other_collider) in other_heroes.iter() {
                    if other_entity == local_entity {
                        continue;
                    }
                    let delta = Vec2::new(
                        transform.translation.x - other_transform.translation.x,
                        transform.translation.z - other_transform.translation.z,
                    );
                    let dist = delta.length();
                    let min_dist = 1.3 + other_collider.radius;
                    if dist < min_dist {
                        let dir = if dist > 1e-4 {
                            delta / dist
                        } else {
                            Vec2::new(1.0, 0.0)
                        };
                        let overlap = min_dist - dist;
                        transform.translation.x += dir.x * overlap;
                        transform.translation.z += dir.y * overlap;
                    }
                }

                // 本地预测也限制在地图边界内，避免越界后被服务端快照拉回造成抖动
                let half = game_core::map::MAP_HALF - 1.0;
                transform.translation.x = transform.translation.x.clamp(-half, half);
                transform.translation.z = transform.translation.z.clamp(-half, half);
                sync.position = transform.translation;
                sync.velocity = velocity;
                sync.last_update_tick = pred_state.local_tick;
            }

            // 朝向更新：cmd.aim_dir 是地面世界坐标点，朝向 = 该点 - 自身位置
            if cmd.aim_dir != Vec2f::zero() {
                let dir = Vec2::new(
                    cmd.aim_dir.x - transform.translation.x,
                    cmd.aim_dir.y - transform.translation.z,
                );
                if dir.length_squared() > 1e-6 {
                    let angle = dir.y.atan2(dir.x);
                    transform.rotation = Quat::from_rotation_y(-angle);
                    sync.rotation = transform.rotation;
                }
            }
        }
    }
}

fn reconcile_with_server(
    mut pred_state: ResMut<PredictionState>,
    snapshot: Option<Res<crate::networking::ReceivedSnapshot>>,
    lobby_info: Res<crate::networking::ClientLobbyInfo>,
    mut local_hero: Query<
        (&mut Transform, &mut TransformSync),
        (With<LocalPlayer>, With<Predicted>, Without<Interpolated>),
    >,
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
                    transform.translation = transform
                        .translation
                        .lerp(entity_snap.position.to_vec3(), 0.5);
                    transform.rotation = transform
                        .rotation
                        .slerp(entity_snap.rotation.to_quat(), 0.5);
                    sync.position = transform.translation;
                    sync.rotation = transform.rotation;
                }
            }
            break;
        }
    }
}
