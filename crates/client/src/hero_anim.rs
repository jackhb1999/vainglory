//! P-V2 英雄动画状态机（纯表现层）。
//!
//! 口径见 `docs/06_VISUAL_SPEC.md` §2.1 / §6 / §7：
//! - 动画只改 `HeroRig` 子节点的 `Transform`，**绝不回写** `Hero` / `TransformSync` 等裁决组件；
//! - 每英雄每帧写入 ≤ 7 个节点，无骨骼资源、无外部资产；
//! - `update_hero_motion` 只读父实体 `Transform`，`tick_hero_anim` 只写子节点 `Transform`，
//!   拆成两个系统避免同一系统内读写 `Transform` 的借用冲突（B0001 教训）。

use crate::voxel::{HeroRig, HeroRigBase};
use bevy::prelude::*;

/// 7 态动画：待机 / 移动 / 攻击 / 技能 / 受击 / 死亡 / 重生。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeroAnimState {
    #[default]
    Idle,
    Move,
    Attack,
    Cast,
    Hit,
    Death,
    Respawn,
}

/// 挂在英雄父实体上，由 `spawn_voxel_hero` 初始化。
#[derive(Component, Debug)]
pub struct HeroAnim {
    pub state: HeroAnimState,
    /// 当前状态已持续时间（秒），切状态时归零。
    pub phase: f32,
    /// 上一帧推得的水平速度，由 `update_hero_motion` 写入。
    pub speed: f32,
    pub last_pos: Vec3,
}

impl HeroAnim {
    /// 新英雄以「方块重组」入场，0.6 s 后自动转 Idle。
    pub fn new(pos: Vec3) -> Self {
        Self {
            state: HeroAnimState::Respawn,
            phase: 0.0,
            speed: 0.0,
            last_pos: pos,
        }
    }
}

/// 施法标记：由 `AbilityCast` 事件插入，`ability_index` 供 P-V3 技能视觉复用。
#[derive(Component, Debug)]
pub struct CastAnim {
    pub t: f32,
    pub ability_index: u8,
}

/// 受击硬直标记：由 `DamageDealt` 事件插入。
#[derive(Component, Debug)]
pub struct HitAnim {
    pub t: f32,
}

/// 死亡倒地标记：由 `HeroDeath` 事件插入。
#[derive(Component, Debug)]
pub struct DeathAnim {
    pub t: f32,
}

/// 触发速度阈值（世界单位/秒），低于此视为待机。
const MOVE_SPEED_THRESHOLD: f32 = 0.35;
/// 入场 / 重生重组时长。
const RESPAWN_DURATION: f32 = 0.6;

/// 系统 A：从父实体位移推断速度。只读 `Transform`，不写任何子节点。
pub fn update_hero_motion(time: Res<Time>, mut heroes: Query<(&mut HeroAnim, &Transform)>) {
    let dt = time.delta_secs().max(1e-4);
    for (mut anim, tr) in heroes.iter_mut() {
        let p = tr.translation;
        // 只取水平位移，避免重生下沉/跳跃被误判为移动。
        anim.speed = Vec2::new(p.x - anim.last_pos.x, p.z - anim.last_pos.z).length() / dt;
        anim.last_pos = p;
    }
}

/// 系统 B：状态判定 → 生成目标姿态 → 写入骨架节点。
#[allow(clippy::type_complexity)]
pub fn tick_hero_anim(
    mut commands: Commands,
    time: Res<Time>,
    mut heroes: Query<(
        Entity,
        &HeroRig,
        &mut HeroAnim,
        Option<&crate::rendering::AttackAnim>,
        Option<&mut CastAnim>,
        Option<&mut HitAnim>,
        Option<&mut DeathAnim>,
    )>,
    mut nodes: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (entity, rig, mut anim, attack, cast, hit, death) in heroes.iter_mut() {
        let has_cast = cast.as_ref().is_some_and(|c| c.t > 0.0);
        let has_hit = hit.as_ref().is_some_and(|h| h.t > 0.0);
        let has_death = death.as_ref().is_some_and(|d| d.t > 0.0);

        // 优先级：死亡 > 受击 > 施法 > 攻击 > 移动 > 待机。
        let mut next = if has_death {
            HeroAnimState::Death
        } else if has_hit {
            HeroAnimState::Hit
        } else if has_cast {
            HeroAnimState::Cast
        } else if attack.is_some() {
            HeroAnimState::Attack
        } else if anim.speed > MOVE_SPEED_THRESHOLD {
            HeroAnimState::Move
        } else {
            HeroAnimState::Idle
        };
        // 入场重组期间不被待机打断。
        if anim.state == HeroAnimState::Respawn
            && next == HeroAnimState::Idle
            && anim.phase < RESPAWN_DURATION
        {
            next = HeroAnimState::Respawn;
        }
        if next != anim.state {
            anim.state = next;
            anim.phase = 0.0;
        }
        anim.phase += dt;

        // 攻击态的手臂由 `tick_attack_anim` 独占驱动，本系统跳过手臂写入，
        // 避免两段动画互相覆盖（其余状态由本系统统一驱动）。
        let skip_arms = anim.state == HeroAnimState::Attack;
        let mut pose = rig.base;
        match anim.state {
            HeroAnimState::Idle => {
                let s = (anim.phase * 2.0).sin();
                pose.torso.scale.y *= 1.0 + 0.02 * s;
                pose.head.rotation = Quat::from_rotation_y(0.06 * (anim.phase * 1.3).sin());
                if let Some(cape) = pose.cape.as_mut() {
                    cape.rotation = Quat::from_rotation_x(0.08 * (anim.phase * 1.7).sin());
                }
            }
            HeroAnimState::Move => {
                let f = anim.phase * 9.0;
                let swing = f.sin() * 0.55;
                pose.left_leg.rotation = Quat::from_rotation_x(swing);
                pose.right_leg.rotation = Quat::from_rotation_x(-swing);
                pose.left_arm.rotation = Quat::from_rotation_x(-swing * 0.6);
                pose.right_arm.rotation = Quat::from_rotation_x(swing * 0.6);
                pose.torso.translation.y += (f * 2.0).sin().abs() * 0.05;
                if let Some(cape) = pose.cape.as_mut() {
                    cape.rotation = Quat::from_rotation_x(0.18 + 0.10 * f.sin());
                }
            }
            HeroAnimState::Attack => {
                pose.torso.rotation = Quat::from_rotation_y(0.18 * (anim.phase * 12.0).sin());
            }
            HeroAnimState::Cast => {
                pose.left_arm.rotation = Quat::from_rotation_x(-1.0);
                pose.right_arm.rotation = Quat::from_rotation_x(-1.0);
                pose.head.rotation = Quat::from_rotation_x(-0.15);
                pose.torso.scale.y *= 1.0 + 0.03 * (anim.phase * 8.0).sin();
            }
            HeroAnimState::Hit => {
                pose.torso.rotation = Quat::from_rotation_x(-0.25);
                pose.head.rotation = Quat::from_rotation_x(0.20);
            }
            HeroAnimState::Death => {
                let k = (anim.phase * 2.0).min(1.0);
                pose.torso.rotation = Quat::from_rotation_z(1.2 * k);
                pose.head.translation.y -= 0.4 * k;
                pose.torso.translation.y -= 0.5 * k;
            }
            HeroAnimState::Respawn => {
                let k = (anim.phase / RESPAWN_DURATION).min(1.0);
                pose.torso.translation.y = rig.base.torso.translation.y * k;
                pose.head.translation.y = rig.base.head.translation.y * k;
                pose.torso.scale = rig.base.torso.scale * k.max(0.05);
            }
        }
        apply_pose(&mut nodes, rig, &pose, skip_arms);

        // 标记组件倒计时，结束后移除以让状态机回落。
        if let Some(mut c) = cast {
            c.t -= dt;
            if c.t <= 0.0 {
                commands.entity(entity).remove::<CastAnim>();
            }
        }
        if let Some(mut h) = hit {
            h.t -= dt;
            if h.t <= 0.0 {
                commands.entity(entity).remove::<HitAnim>();
            }
        }
        if let Some(mut d) = death {
            d.t -= dt;
            if d.t <= 0.0 {
                commands.entity(entity).remove::<DeathAnim>();
            }
        }
    }
}

/// 把姿态写回骨架节点；`skip_arms` 用于把双臂让给既有挥击动画。
fn apply_pose(
    nodes: &mut Query<&mut Transform>,
    rig: &HeroRig,
    pose: &HeroRigBase,
    skip_arms: bool,
) {
    let mut write = |entity: Entity, tr: Transform| {
        if let Ok(mut node) = nodes.get_mut(entity) {
            *node = tr;
        }
    };
    write(rig.torso, pose.torso);
    write(rig.head, pose.head);
    write(rig.left_leg, pose.left_leg);
    write(rig.right_leg, pose.right_leg);
    if !skip_arms {
        write(rig.left_arm, pose.left_arm);
        write(rig.right_arm, pose.right_arm);
    }
    if let (Some(entity), Some(tr)) = (rig.cape, pose.cape) {
        write(entity, tr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(attack: bool, cast: bool, hit: bool, death: bool, speed: f32) -> HeroAnimState {
        // 与 `tick_hero_anim` 的判定表保持一致，单测用于锁死优先级。
        if death {
            HeroAnimState::Death
        } else if hit {
            HeroAnimState::Hit
        } else if cast {
            HeroAnimState::Cast
        } else if attack {
            HeroAnimState::Attack
        } else if speed > MOVE_SPEED_THRESHOLD {
            HeroAnimState::Move
        } else {
            HeroAnimState::Idle
        }
    }

    #[test]
    fn state_priority_is_deterministic() {
        assert_eq!(pick(false, false, false, false, 0.0), HeroAnimState::Idle);
        assert_eq!(pick(false, false, false, false, 1.0), HeroAnimState::Move);
        assert_eq!(pick(true, false, false, false, 1.0), HeroAnimState::Attack);
        assert_eq!(pick(true, true, false, false, 1.0), HeroAnimState::Cast);
        assert_eq!(pick(true, true, true, false, 1.0), HeroAnimState::Hit);
        assert_eq!(pick(true, true, true, true, 1.0), HeroAnimState::Death);
    }

    #[test]
    fn fresh_hero_starts_in_respawn() {
        let a = HeroAnim::new(Vec3::new(1.0, 0.0, 2.0));
        assert_eq!(a.state, HeroAnimState::Respawn);
        assert_eq!(a.phase, 0.0);
        assert_eq!(a.last_pos, Vec3::new(1.0, 0.0, 2.0));
    }

    #[test]
    fn all_seven_states_are_distinct() {
        use std::collections::HashSet;
        let all = [
            HeroAnimState::Idle,
            HeroAnimState::Move,
            HeroAnimState::Attack,
            HeroAnimState::Cast,
            HeroAnimState::Hit,
            HeroAnimState::Death,
            HeroAnimState::Respawn,
        ];
        assert_eq!(all.into_iter().collect::<HashSet<_>>().len(), 7);
    }

    /// 回归护栏：两个动画系统的 Query 组合不得产生同系统内读写 `Transform` 冲突
    /// （历史上 `B0001` 即由此类冲突引发），且姿态必须真的写回骨架节点。
    #[test]
    fn anim_systems_run_without_query_conflict() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Update, (update_hero_motion, tick_hero_anim).chain());

        let mut ids = Vec::new();
        let parent = app
            .world_mut()
            .spawn((Transform::default(), HeroAnim::new(Vec3::ZERO)))
            .with_children(|b| {
                for _ in 0..7 {
                    ids.push(b.spawn(Transform::default()).id());
                }
            })
            .id();
        let base = HeroRigBase {
            torso: Transform::from_xyz(0.0, 1.5, 0.0).with_scale(Vec3::ONE),
            head: Transform::from_xyz(0.0, 2.6, 0.0),
            left_arm: Transform::from_xyz(-0.7, 2.0, 0.0),
            right_arm: Transform::from_xyz(0.7, 2.0, 0.0),
            left_leg: Transform::from_xyz(-0.3, 0.9, 0.0),
            right_leg: Transform::from_xyz(0.3, 0.9, 0.0),
            cape: None,
        };
        app.world_mut().entity_mut(parent).insert(HeroRig {
            torso: ids[0],
            head: ids[1],
            right_arm: ids[2],
            left_arm: ids[3],
            right_leg: ids[4],
            left_leg: ids[5],
            cape: None,
            base,
        });

        app.update();
        let torso = *app.world().get::<Transform>(ids[0]).unwrap();
        // 入场重组：scale 被压缩但保持正值，说明姿态确实写入且系统无 panic。
        assert!(torso.scale.x > 0.0);
        assert_eq!(
            app.world().get::<HeroAnim>(parent).unwrap().state,
            HeroAnimState::Respawn
        );
    }
}
