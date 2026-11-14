//! 服务端实体生成系统

use crate::networking::{ClientNetIds, DummyRegistry, NextNetId, RenetServerResource};
use bevy::prelude::*;
use bitcode;
use game_core::components::*;
use game_core::map::*;
use game_core::net::{GameEvent, JungleMonsterType, ServerMessage, Vec3f};
use renet2::DefaultChannel;

/// 一次性生成标记：静态世界（炮塔 / 水晶 / 野怪 / 假人）只在第一次进入 Running 时生成，
/// 之后即使状态在 Running↔WaitingForPlayers 间抖动也不重复生成，否则 NetId 会无限增长、
/// 客户端实体爆炸（GPU OOM）。
#[derive(Resource, Default)]
struct SpawnFlags {
    turrets: bool,
    crystals: bool,
    jungle: bool,
    dummies: bool,
    lane_walls: bool,
}

pub struct SpawnPlugin;

impl Plugin for SpawnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnFlags>().add_systems(
            OnEnter(crate::ServerState::Running),
            (
                spawn_turrets,
                spawn_source_cores,
                spawn_jungle_camps,
                spawn_training_dummies,
                spawn_lane_walls,
            ),
        );
    }
}

/// 兵线 / 野区分隔墙的**权威静态碰撞体**。
///
/// `collision_system` 只支持圆形 `Collider`，所以每段长墙由 `map::wall_colliders()`
/// 采样成连续的圆。客户端 `prediction::static_obstacles()` 读取同一份数据，
/// 保证「服务端挡住的地方，客户端也挡住」—— 否则贴墙走会持续回滚抖动。
/// 墙不挂 `NetId`，因此只参与碰撞，不会被下发成世界实体。
fn spawn_lane_walls(mut commands: Commands, mut flags: ResMut<SpawnFlags>) {
    if flags.lane_walls {
        return;
    }
    flags.lane_walls = true;
    for (pos, radius) in game_core::map::wall_colliders() {
        commands.spawn((
            Transform::from_translation(pos),
            TransformSync {
                position: pos,
                ..default()
            },
            Collider { radius },
            StaticCollider,
        ));
    }
}

/// 只放一个训练假人（中立阵营），放在蓝方野区，供单人测试技能命中。
fn spawn_training_dummies(
    mut commands: Commands,
    mut next_net_id: ResMut<NextNetId>,
    mut dummy_registry: ResMut<DummyRegistry>,
    mut server_res: Option<ResMut<RenetServerResource>>,
    client_net_ids: Res<ClientNetIds>,
    mut flags: ResMut<SpawnFlags>,
) {
    if flags.dummies {
        return;
    }
    flags.dummies = true;
    let positions = [
        game_core::map::TRAINING_DUMMY_POS, // 蓝方野区的单个假人，避开野怪与金矿坑
    ];

    for pos in positions {
        let net_id = NetId(next_net_id.0);
        next_net_id.0 += 1;

        commands.spawn((
            net_id,
            Team::Neutral,
            Transform::from_translation(pos + Vec3::Y * 1.5),
            TransformSync {
                position: pos,
                ..default()
            },
            Selectable {
                radius: 1.5,
                height: 3.0,
            },
            Hero {
                name: "训练假人".to_string(),
                health: 600.0,
                max_health: 600.0,
                move_speed: 0.0,
                ..default()
            },
            HeroAbilities {
                cooldowns: [0.0; 4],
                ability_ids: [0; 4],
            },
            ServerAuthority,
            Collider { radius: 1.5 },
            StaticCollider,
            TrainingDummy,
        ));

        let event = GameEvent::HeroSpawn {
            net_id,
            team: Team::Neutral,
            hero_name: "训练假人".to_string(),
            position: Vec3f::from_vec3(pos),
            skin: 9,
        };
        dummy_registry.0.push(event.clone());

        // 通知当前已连接的客户端渲染假人
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
}

fn spawn_turrets(
    mut commands: Commands,
    _meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
    mut flags: ResMut<SpawnFlags>,
) {
    if flags.turrets {
        return;
    }
    flags.turrets = true;
    for turret_pos in TURRET_POSITIONS {
        let (max_health, attack_damage, armor, magic_resist, attack_range, attack_speed) =
            match turret_pos.turret_type {
                TurretType::Outer => (3000.0, 150.0, 100.0, 100.0, 12.0, 0.8),
                TurretType::Inner => (3500.0, 200.0, 150.0, 150.0, 12.0, 0.8),
                TurretType::Base => (4000.0, 250.0, 200.0, 200.0, 14.0, 0.7),
                TurretType::SourceCore => (5000.0, 300.0, 300.0, 300.0, 16.0, 0.6),
            };

        commands.spawn((
            Transform::from_translation(turret_pos.position + Vec3::Y * 4.0),
            TransformSync {
                position: turret_pos.position,
                ..default()
            },
            Turret {
                turret_type: turret_pos.turret_type,
                max_health,
                health: max_health,
                attack_damage,
                armor,
                magic_resist,
                attack_range,
                attack_speed,
                last_attack_tick: 0,
            },
            turret_pos.team,
            NetId(rand::random()),
            ServerAuthority,
            Collider { radius: 2.5 },
            StaticCollider,
            Selectable {
                radius: 2.5,
                height: 8.0,
            },
        ));
    }
}

fn spawn_source_cores(mut commands: Commands, mut flags: ResMut<SpawnFlags>) {
    if flags.crystals {
        return;
    }
    flags.crystals = true;
    for (pos, team) in [(BLUE_SOURCE_CORE, Team::Blue), (RED_SOURCE_CORE, Team::Red)] {
        commands.spawn((
            Transform::from_translation(pos + Vec3::Y * 6.0),
            TransformSync {
                position: pos,
                ..default()
            },
            Turret {
                turret_type: TurretType::SourceCore,
                max_health: 5000.0,
                health: 5000.0,
                attack_damage: 300.0,
                armor: 300.0,
                magic_resist: 300.0,
                attack_range: 16.0,
                attack_speed: 0.6,
                last_attack_tick: 0,
            },
            team,
            NetId(rand::random()),
            ServerAuthority,
            Collider { radius: 3.5 },
            StaticCollider,
            Selectable {
                radius: 3.5,
                height: 12.0,
            },
        ));
    }
}

/// 按营地定义生成一只野怪并返回其 Entity（数值表唯一真相源）。
///
/// 开局 `spawn_jungle_camps` 与 `objectives::abyss_warden_spawn_system`（T=54000 首刷 /
/// 7200 tick 重生）共用，避免两处各写一份 11 元组导致渊卫数值漂移。
pub fn spawn_monster(commands: &mut Commands, camp: &JungleCamp) -> Entity {
    {
        // 攻击射程必须覆盖双方 `Collider` 的推出距离：野怪半径 2.0 + 英雄半径，
        // 旧版静止型 2.0 / 近战型 1.5 在碰撞解算后永远不成立，等于打不到人。
        let (
            max_health,
            attack_damage,
            armor,
            magic_resist,
            move_speed,
            attack_range,
            gold_reward,
            exp_reward,
            aggro_range,
            leash_range,
            attack_interval_ticks,
        ) = match camp.monster_type {
            JungleMonsterType::MusterWell => {
                (5000.0, 120.0, 50.0, 50.0, 0.0, 6.0, 300, 500, 0.0, 0.0, 90)
            }
            JungleMonsterType::LodeShrine => {
                (4000.0, 100.0, 40.0, 40.0, 0.0, 6.0, 200, 300, 0.0, 0.0, 90)
            }
            JungleMonsterType::AbyssWarden => (
                20000.0, 250.0, 100.0, 100.0, 3.0, 5.0, 500, 1000, 20.0, 30.0, 75,
            ),
            JungleMonsterType::Treant => {
                (2500.0, 80.0, 30.0, 30.0, 3.5, 4.5, 60, 100, 10.0, 15.0, 60)
            }
            JungleMonsterType::CoreWarden => {
                (1800.0, 60.0, 20.0, 20.0, 3.5, 6.0, 40, 60, 12.0, 15.0, 60)
            }
            JungleMonsterType::GildedToad => {
                (1200.0, 40.0, 10.0, 10.0, 4.0, 4.5, 80, 50, 8.0, 12.0, 75)
            }
        };

        commands
            .spawn((
                Transform::from_translation(camp.position),
                TransformSync {
                    position: camp.position,
                    ..default()
                },
                Team::Neutral,
                JungleMonster {
                    monster_type: camp.monster_type,
                    max_health,
                    health: max_health,
                    attack_damage,
                    armor,
                    magic_resist,
                    move_speed,
                    attack_range,
                    gold_reward,
                    exp_reward,
                    aggro_range,
                    leash_range,
                    spawn_tick: 0,
                    last_attacker: None,
                    attack_interval_ticks,
                    next_attack_tick: 0,
                },
                NetId(rand::random()),
                ServerAuthority,
                Aggro::default(),
                Collider { radius: 2.0 },
                Selectable {
                    radius: 2.0,
                    height: 3.0,
                },
            ))
            .id()
    }
}

fn spawn_jungle_camps(mut commands: Commands, mut flags: ResMut<SpawnFlags>) {
    if flags.jungle {
        return;
    }
    flags.jungle = true;
    for camp in JUNGLE_CAMPS {
        // 渊卫是 `T=54000` 才首刷的全局目标（§13.4），开局不生成；
        // 由 `objectives::abyss_warden_spawn_system` 用同一个 `spawn_monster` 生成。
        if matches!(camp.monster_type, JungleMonsterType::AbyssWarden) {
            continue;
        }
        spawn_monster(&mut commands, camp);
    }
}
