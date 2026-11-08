//! 玩法闭环集成测试：`game-core::gameplay` 纯逻辑经 `server::gameplay` 系统链
//! 真正驱动 Bevy ECS 实体（属性/护盾/商店/占领/野怪/塔/死亡重生）。

use bevy::prelude::*;
use game_core::components::*;
use game_core::gameplay::{
    GameplayEvents, PendingDeaths, combat, jungle, objectives, respawn, shop, structures,
};
use game_core::map::BLUE_SOURCE_CORE;
use game_core::net::JungleMonsterType;
use game_core::systems::{AttackCommand, damage_system};
use server::gameplay::*;
use server::simulation::ServerTick;

fn advance_tick(mut tick: ResMut<ServerTick>) {
    tick.0 += 1;
}

fn hero(name: &str, net_id: u64, team: Team, at: Vec3, health: f32) -> impl Bundle {
    (
        NetId(net_id),
        team,
        Transform::from_translation(at),
        Hero {
            name: name.to_string(),
            health,
            max_health: 1000.0,
            gold: 1000,
            ..Default::default()
        },
    )
}

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<ServerTick>()
        .init_resource::<ShopCatalogResource>()
        .init_resource::<ShopIntents>()
        .init_resource::<SellIntents>()
        .init_resource::<GameplayWorldReady>()
        .init_resource::<WorldSnapshot>()
        .init_resource::<GameplayEvents>()
        .init_resource::<PendingDeaths>()
        .init_resource::<DamageSink>()
        .add_systems(
            Update,
            (
                advance_tick,
                build_world_snapshot,
                attach_hero_gameplay,
                attach_world_gameplay,
                damage_system,
                status_tick_system,
                regen_system,
                monster_ai_system,
                capture_system,
                structure_defense_system,
                shop_purchase_system,
                death_respawn_system,
            )
                .chain(),
        );
    app
}

#[test]
fn gameplay_systems_drive_a_full_ecs_match_slice() {
    let mut app = build_app();

    let attacker = app
        .world_mut()
        .spawn(hero("blue", 1, Team::Blue, BLUE_SOURCE_CORE, 1000.0))
        .id();
    let victim = app
        .world_mut()
        .spawn(hero("red", 2, Team::Red, Vec3::new(10.0, 0.0, 10.0), 100.0))
        .id();
    // 站在本方野区商店（征募井）上的第三名英雄，用于触发占领
    // 位置直接从地图营地数据取，保证与“单条兵线 + 内侧野区”布局同源
    let shop_pos = game_core::map::JUNGLE_CAMPS
        .iter()
        .find(|c| matches!(c.monster_type, JungleMonsterType::MusterWell))
        .map(|c| c.position)
        .expect("地图必须包含征募井");
    let _caps = app
        .world_mut()
        .spawn(hero("blue-cap", 3, Team::Blue, shop_pos, 1000.0))
        .id();

    app.world_mut().spawn((
        Transform::from_translation(Vec3::new(30.0, 0.0, 30.0)),
        Team::Neutral,
        JungleMonster {
            monster_type: JungleMonsterType::Treant,
            max_health: 2500.0,
            health: 2500.0,
            attack_damage: 80.0,
            armor: 30.0,
            magic_resist: 30.0,
            move_speed: 3.5,
            attack_range: 1.5,
            gold_reward: 60,
            exp_reward: 100,
            aggro_range: 10.0,
            leash_range: 15.0,
            spawn_tick: 900,
            last_attacker: None,
        },
    ));
    app.world_mut().spawn((
        Transform::from_translation(Vec3::new(-40.0, 0.0, -40.0)),
        Team::Blue,
        Turret {
            turret_type: TurretType::Outer,
            max_health: 5000.0,
            health: 5000.0,
            attack_damage: 150.0,
            armor: 40.0,
            magic_resist: 40.0,
            attack_range: 12.0,
            attack_speed: 1.0,
            last_attack_tick: 0,
        },
    ));

    // 第 1 帧：挂接玩法组件、生成占领点、读世界快照
    app.update();

    {
        let world = app.world_mut();
        assert!(world.entity(victim).contains::<combat::ShieldPool>());
        assert!(world.entity(victim).contains::<shop::Inventory>());
        assert!(world.entity(attacker).contains::<respawn::KillStreak>());
        assert!(world.entity(victim).contains::<respawn::RespawnState>());
        assert_eq!(world.resource::<WorldSnapshot>().heroes.len(), 3);

        // 让占领点在下一帧即可判定（阈值 100 → 1）
        let mut points = world.query::<&mut objectives::CapturePoint>();
        for mut cp in points.iter_mut(world) {
            cp.threshold = 1;
        }
        // 给受害者 50 点护盾，验证伤害走完整公式而非旧简化式
        let mut shield = world.get_mut::<combat::ShieldPool>(victim).unwrap();
        shield.value = 50.0;
    }

    // 购买意图：英雄位于己方源核，商店区判定应通过
    app.world_mut().resource_mut::<ShopIntents>().0.push((
        1,
        shop::ShopRequest {
            item_id: "item_edge_shard",
            distance_to_source: 0.0,
            is_dead: false,
            death_tick: 0,
            current_tick: 2,
        },
    ));

    // 一次致命攻击
    app.world_mut().spawn(AttackCommand {
        target: victim,
        attacker,
        damage: 1000.0,
        damage_type: DamageType::Physical,
        tick: 1,
    });

    // 第 2 帧：伤害 → 占领 → 商店 → 死亡
    app.update();

    {
        let world = app.world_mut();
        // 护盾先被消耗，生命归零
        assert_eq!(
            world.get::<combat::ShieldPool>(victim).unwrap().value,
            0.0,
            "shield must absorb before health"
        );
        assert_eq!(world.get::<Hero>(victim).unwrap().health, 0.0);
        // 实体未被 despawn，进入阵亡状态等待重生
        assert!(world.get_entity(victim).is_ok());
        assert!(world.get::<respawn::RespawnState>(victim).unwrap().dead);

        // 商店事务：扣 300 金币、+12 AD、占用一个栏位。
        // 起始 1000 + 击杀奖励 50 + 连杀赏金 180 - 装备 300 = 930
        let hero = world.get::<Hero>(attacker).unwrap();
        assert_eq!(hero.gold, 930);
        assert_eq!(hero.attack_damage, 60.0 + 12.0);
        let inv = world.get::<shop::Inventory>(attacker).unwrap();
        assert_eq!(inv.slots[0].as_deref(), Some("item_edge_shard"));
        assert_eq!(world.resource::<GameplayEvents>().purchases.len(), 1);

        // 占领事件：全局、只含 3 个字段
        let events = &world.resource::<GameplayEvents>().captured;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].owner, Team::Blue);
        assert!(!format!("{:?}", events[0]).contains("Entity"));

        // 死亡事件含击杀者与受击者
        let deaths = &world.resource::<GameplayEvents>().deaths;
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0].killer_net_id, 1);
        assert_eq!(deaths[0].victim_net_id, 2);

        // 野怪 AI 与塔防御组件已挂接并参与系统
        let mut monsters = world.query::<&jungle::MonsterAi>();
        assert_eq!(monsters.iter(world).count(), 1);
        let mut defenses = world.query::<&structures::StructureDefense>();
        assert_eq!(defenses.iter(world).count(), 1);
    }

    // 推进到重生时刻：death_tick=1 → respawn_tick=481
    app.world_mut().resource_mut::<ServerTick>().0 = 480;
    app.update();

    {
        let world = app.world();
        let state = world.get::<respawn::RespawnState>(victim).unwrap();
        assert!(!state.dead, "hero must respawn at tick 481");
        assert_eq!(world.get::<Hero>(victim).unwrap().health, 1000.0);
        assert!(
            world
                .get::<combat::StatusEffects>(victim)
                .unwrap()
                .is_invulnerable()
        );
    }
}
