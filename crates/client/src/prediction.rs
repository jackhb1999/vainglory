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
    /// 上一次已对账的服务端快照 tick。快照以 20Hz 到达、FixedUpdate 以 60Hz 运行，
    /// 不去重会让同一份「过期」快照被消费 3 次，把本地英雄反复往回拉 → 走路抖动。
    pub last_reconciled_tick: u64,
}

impl PredictionState {
    pub fn add_input(&mut self, input: InputCmd) {
        self.pending_inputs.push_back(input);
        // 保留最近 120 帧（2秒@60Hz）
        if self.pending_inputs.len() > 120 {
            self.pending_inputs.pop_front();
        }
    }

    /// 记录服务端确认进度。
    ///
    /// 不能用 `pending_inputs.retain(|cmd| cmd.tick > tick)`：`cmd.tick` 是**本地预测 tick**，
    /// `tick` 是**服务端 tick**，两者是不同时钟（客户端上报的 tick 甚至恒为 0）。
    /// 用服务端 tick 去裁本地历史会把所有待确认输入一次性清空。历史上限已由 120 帧兜底。
    pub fn confirm_up_to(&mut self, tick: u64) {
        self.server_confirmed_tick = tick;
    }
}

/// 一份服务端快照只允许对账一次：20Hz 快照在 60Hz 的 FixedUpdate 里会连到 3 帧，
/// 重复消费等于反复把本地位置往同一个（已经过期的）权威位置拉。
fn should_reconcile(last_reconciled_tick: u64, snapshot_tick: u64) -> bool {
    snapshot_tick != 0 && snapshot_tick != last_reconciled_tick
}

/// 本地预测使用的静态障碍：塔、源核、训练假人与**兵线分隔墙**。
/// 与 `game_core::map` / 服务端 `StaticCollider` 同源，保证客户端不会“穿过”服务端认为存在的墙。
/// 野怪属于**动态**碰撞体（会追击、死亡即消失），不在此列。
///
/// `NAV-M2-01-C7`：塔 / 源核改由服务端全局障碍视图驱动，只保留**存活**的。
/// 旧版把 `map::TURRET_POSITIONS` 全部写死为静态圆，塔被摧毁后客户端仍把英雄推开，
/// 与权威位置持续偏离 → 原地上下来回晃。尚未收到视图时退回常量，避免开局穿塔。
fn static_obstacles(nav: Option<&crate::input::ClientNavObstacles>) -> Vec<(Vec3, f32)> {
    let mut out: Vec<(Vec3, f32)> = Vec::new();
    if let Some(snap) = nav.and_then(|n| n.0.as_ref()) {
        for &(p, alive) in &snap.turrets {
            if alive {
                out.push((Vec3::new(p[0], 0.0, p[1]), 2.5));
            }
        }
        for &p in &snap.source_cores {
            out.push((Vec3::new(p[0], 0.0, p[1]), 3.5));
        }
    } else {
        for t in game_core::map::TURRET_POSITIONS {
            out.push((t.position, 2.5));
        }
        out.push((game_core::map::BLUE_SOURCE_CORE, 3.5));
        out.push((game_core::map::RED_SOURCE_CORE, 3.5));
    }
    // 野怪营地**不是**静态障碍：服务端野怪是可移动的 `Collider`（无 `StaticCollider`），
    // 双方各退 overlap*0.5，且击杀后碰撞体消失。客户端若把固定营地点当作静态墙、
    // 每次都全额推出，会与权威位置持续偏离 → 靠近野区时反复回滚抖动。
    out.push((game_core::map::TRAINING_DUMMY_POS, 1.5));
    // 兵线 / 野区堤墙：与服务端 spawn_lane_walls 逐圆同源。
    // 少了这份数据，本地预测会直接穿墙、每个权威快照再被顶回来，正是「贴墙乱晃」的成因。
    out.extend(game_core::map::wall_colliders());
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
    nav_obstacles: Option<Res<crate::input::ClientNavObstacles>>,
) {
    let ticks_advanced = tick_clock.advance(time.delta_secs());
    // 阵亡期间冻结本地预测：服务端已把尸体吸附到泉水，继续预测会与权威位置打架。
    if input_state.dead {
        return;
    }
    let obstacles = static_obstacles(nav_obstacles.as_deref());

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
                // 河道减速必须与服务端 movement_system 同源，否则涉水时客户端走得更快、
                // 每个快照都被拉回，表现为持续左右晃动。
                let speed =
                    hero.move_speed * game_core::map::terrain_speed_mult(transform.translation);
                let velocity = direction * speed;

                transform.translation += velocity * tick_clock.tick_duration();

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

                // 本地预测也限制在地图边界内，避免越界后被服务端快照拉回造成抖动。
                // 地图是 2:1 长方形，x / z 必须分别取半径，不能共用一个 MAP_HALF。
                let hx = game_core::map::MAP_W_HALF - 1.0;
                let hz = game_core::map::MAP_H_HALF - 1.0;
                transform.translation.x = transform.translation.x.clamp(-hx, hx);
                transform.translation.z = transform.translation.z.clamp(-hz, hz);
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

/// 与服务端快照对账。
///
/// 三条不变量，缺一条就会出现「走路随机乱晃」：
/// 1. **同一份快照只对账一次**：快照 20Hz、FixedUpdate 60Hz，不去重会把过期位置
///    反复 lerp 0.5，形成锯齿式回拉；
/// 2. **移动中不做小修正**：服务端位置天然滞后 RTT，向它收敛等于持续把人往回拽；
///    只在玩家停下后缓慢收敛，或误差极大（传送/重生）时硬修正；
/// 3. **朝向由本地 aim 独占**：快照朝向同样滞后，slerp 会让模型边走边左右扭。
fn reconcile_with_server(
    mut pred_state: ResMut<PredictionState>,
    snapshot: Option<Res<crate::networking::ReceivedSnapshot>>,
    lobby_info: Res<crate::networking::ClientLobbyInfo>,
    input_state: Res<crate::input::PlayerInputState>,
    mut local_hero: Query<
        (&mut Transform, &mut TransformSync),
        (With<LocalPlayer>, With<Predicted>, Without<Interpolated>),
    >,
) {
    let Some(snapshot) = snapshot else { return };
    let snapshot = &snapshot.0;
    if !should_reconcile(pred_state.last_reconciled_tick, snapshot.tick) {
        return;
    }
    pred_state.last_reconciled_tick = snapshot.tick;
    pred_state.confirm_up_to(snapshot.tick);

    let Some(my_net_id) = lobby_info.my_net_id else {
        return;
    };
    let Some(me) = snapshot.entities.iter().find(|e| e.net_id == my_net_id) else {
        return;
    };
    // 阵亡/重生瞬间由权威位置接管（泉水），其余时刻照常对账。
    let moving = !input_state.dead && input_state.move_dir != bevy::math::Vec2::ZERO;
    let server_pos = me.position.to_vec3();

    for (mut transform, mut sync) in local_hero.iter_mut() {
        let diff = transform.translation - server_pos;
        let dist = Vec3::new(diff.x, 0.0, diff.z).length();
        if dist > 8.0 {
            // 传送 / 复活 / 严重不同步：硬修正位置，但仍保留本地朝向。
            transform.translation = server_pos;
            sync.position = server_pos;
            sync.velocity = me.velocity.to_vec3();
            info!("Rollback correction (teleport): diff = {:.2}", dist);
        } else if !moving && dist > 0.12 {
            // 静止时才缓慢收敛，每个新快照只补 20%，避免高频回拉。
            transform.translation -= diff * 0.2;
            sync.position = transform.translation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_snapshot_reconciles_once() {
        assert!(!should_reconcile(0, 0), "未收到有效快照");
        assert!(should_reconcile(0, 100));
        assert!(!should_reconcile(100, 100), "同一快照不得重复对账");
        assert!(should_reconcile(100, 101));
    }

    /// 跨时钟裁剪会把本地待确认输入一次性清空（服务端 tick ≠ 本地 tick）。
    #[test]
    fn confirm_up_to_keeps_local_input_history() {
        let mut state = PredictionState {
            local_tick: 5,
            ..Default::default()
        };
        for t in 0..3 {
            state.add_input(InputCmd {
                tick: t,
                seq: t as u32,
                client_time_us: 0,
                move_dir: Vec2f::zero(),
                aim_dir: Vec2f::zero(),
                ability_mask: 0,
                target_net_id: None,
                ping: 0,
                config_hash: Default::default(),
                checksum: 0,
            });
        }
        state.confirm_up_to(900_000);
        assert_eq!(state.server_confirmed_tick, 900_000);
        assert_eq!(
            state.pending_inputs.len(),
            3,
            "本地输入历史不应被服务端 tick 清空"
        );
    }
}
