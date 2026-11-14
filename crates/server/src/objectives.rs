//! 中立目标 ECS 系统（`GX-CBT-2026-001` §13.2 辉矿 / §13.4 渊卫）。
//!
//! 纯裁决在 `game_core::gameplay::objectives_economy`，本文件只负责 tick 驱动、
//! 实体生成与金币写回。地图骨架不动：渊卫坑位沿用 `map::GOLD_MINE_POS`，
//! 兵矿沿用 `map::JUNGLE_CAMPS` 里两座 `LodeShrine` 的占领点。

use crate::spawn::spawn_monster;
use bevy::prelude::*;
use game_core::components::{Hero, Team};
use game_core::gameplay::clock::{MatchClock, OVERTIME_START_TICKS};
use game_core::gameplay::objectives::CapturePoint;
use game_core::gameplay::objectives_economy::{
    AbyssWardenState, LODE_SHRINE_PERIODIC_GOLD, OBJECTIVE_LODE_SHRINE, ObjectiveIncomeState,
    should_spawn_abyss_warden,
};
use game_core::gameplay::{GameplayEvents, NetEvent};
use game_core::map::{GOLD_MINE_POS, JungleCamp};
use game_core::net::{JungleMonsterType, Vec3f};

/// 渊卫（中央矿坑全局首领）首刷 / 重生。
///
/// `T=54000`(15min) 首次出现，被击败后 7200 tick(120s) 重生；加时与判胜后不再刷新。
/// 开局 `spawn_jungle_camps` 会跳过 `AbyssWarden`，因此这里是它唯一的生成入口。
/// 生成后 `NavGrid` 会通过 `rebuild_server_nav_grid` 自动把它加进动态障碍。
pub fn abyss_warden_spawn_system(
    clock: Res<MatchClock>,
    mut commands: Commands,
    mut warden: ResMut<AbyssWardenState>,
    mut events: ResMut<GameplayEvents>,
) {
    let tick = clock.tick;
    // 加时与判胜后不再刷新（§13.4）
    let no_more_spawns = clock.is_over() || tick >= OVERTIME_START_TICKS;
    if !should_spawn_abyss_warden(tick, warden.alive, warden.next_spawn_tick, no_more_spawns) {
        return;
    }
    let camp = JungleCamp {
        monster_type: JungleMonsterType::AbyssWarden,
        position: GOLD_MINE_POS,
        team: Team::Neutral,
    };
    let entity = spawn_monster(&mut commands, &camp);
    warden.on_spawn(entity);
    events.emit(NetEvent::AbyssWardenSpawn {
        position: Vec3f::from_vec3(GOLD_MINE_POS),
    });
}

/// 兵矿（辉矿）归属期间的持续收益：每 1800 tick 给归属方每人
/// `30 × 该队持有兵矿数` 金，直到被反占（§13.2）。
///
/// 两座兵矿共用 `objective_id = 1`，因此按**队伍**计时并按占点数量叠加，
/// 而不是按 id 去重（同队占两矿应当拿两份）。金币写回 `Hero.gold`，
/// 由下一次 `CH_GOLD` 增量快照下发；`NetEvent::ObjectiveIncome` 仅供 HUD 飘字。
pub fn objective_income_system(
    clock: Res<MatchClock>,
    points: Query<&CapturePoint>,
    mut heroes: Query<(&Team, &mut Hero)>,
    mut income: ResMut<ObjectiveIncomeState>,
    mut events: ResMut<GameplayEvents>,
) {
    let tick = clock.tick;
    for team in [Team::Blue, Team::Red] {
        if !income.payout_due(team, tick) {
            continue;
        }
        let owned = points
            .iter()
            .filter(|cp| cp.objective_id == OBJECTIVE_LODE_SHRINE && cp.owner == Some(team))
            .count() as u32;
        if owned == 0 {
            continue;
        }
        let amount = LODE_SHRINE_PERIODIC_GOLD.saturating_mul(owned);
        for (hero_team, mut hero) in heroes.iter_mut() {
            if *hero_team == team {
                hero.gold = hero.gold.saturating_add(amount);
            }
        }
        income.mark_paid(team, tick);
        events.emit(NetEvent::ObjectiveIncome {
            owner: team,
            amount,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::gameplay::objectives_economy::{
        ABYSS_WARDEN_FIRST_SPAWN_TICK, ABYSS_WARDEN_RESPAWN_TICKS, LODE_SHRINE_INCOME_PERIOD_TICKS,
    };

    fn app_at(tick: u64) -> App {
        let mut app = App::new();
        app.insert_resource(MatchClock { tick, ..default() })
            .init_resource::<AbyssWardenState>()
            .init_resource::<ObjectiveIncomeState>()
            .init_resource::<GameplayEvents>()
            .add_systems(
                Update,
                (abyss_warden_spawn_system, objective_income_system).chain(),
            );
        app
    }

    #[test]
    fn warden_does_not_spawn_before_first_spawn_tick() {
        let mut app = app_at(ABYSS_WARDEN_FIRST_SPAWN_TICK - 1);
        app.update();
        assert!(!app.world().resource::<AbyssWardenState>().alive);
        assert!(app.world().resource::<GameplayEvents>().net.is_empty());
    }

    #[test]
    fn warden_spawns_once_and_respawns_after_defeat() {
        let mut app = app_at(ABYSS_WARDEN_FIRST_SPAWN_TICK);
        app.update();
        assert!(app.world().resource::<AbyssWardenState>().alive);
        assert_eq!(app.world().resource::<GameplayEvents>().net.len(), 1);
        // 同一 tick 再跑一次：alive 已置位，不得重复生成
        app.update();
        assert_eq!(app.world().resource::<GameplayEvents>().net.len(), 1);

        app.world_mut()
            .resource_mut::<AbyssWardenState>()
            .on_defeated(ABYSS_WARDEN_FIRST_SPAWN_TICK);
        let next = ABYSS_WARDEN_FIRST_SPAWN_TICK + ABYSS_WARDEN_RESPAWN_TICKS;
        app.world_mut().resource_mut::<MatchClock>().tick = next;
        app.update();
        assert!(app.world().resource::<AbyssWardenState>().alive, "重生");
        assert_eq!(app.world().resource::<GameplayEvents>().net.len(), 2);
    }

    #[test]
    fn warden_does_not_spawn_in_overtime() {
        let mut app = app_at(OVERTIME_START_TICKS);
        app.update();
        assert!(!app.world().resource::<AbyssWardenState>().alive);
    }

    #[test]
    fn periodic_income_pays_only_owning_team_and_scales_with_mines() {
        let tick = LODE_SHRINE_INCOME_PERIOD_TICKS;
        let mut app = app_at(tick);
        // 一座蓝方兵矿 + 一座无主征募井；蓝方英雄 1 名，红方英雄 1 名
        app.world_mut().spawn({
            let mut cp = CapturePoint::new(OBJECTIVE_LODE_SHRINE, 100, 5400);
            cp.owner = Some(Team::Blue);
            cp
        });
        app.world_mut().spawn(CapturePoint::new(2, 100, 5400));
        let blue = app.world_mut().spawn((Team::Blue, Hero::default())).id();
        let red = app.world_mut().spawn((Team::Red, Hero::default())).id();
        let blue_start = app.world().entity(blue).get::<Hero>().unwrap().gold;
        let red_start = app.world().entity(red).get::<Hero>().unwrap().gold;

        app.update();
        assert_eq!(
            app.world().entity(blue).get::<Hero>().unwrap().gold,
            blue_start + LODE_SHRINE_PERIODIC_GOLD
        );
        assert_eq!(
            app.world().entity(red).get::<Hero>().unwrap().gold,
            red_start,
            "未占矿的队伍不得获得收益"
        );
        assert_eq!(app.world().resource::<GameplayEvents>().net.len(), 1);
        // 同一 tick 不重复发放
        app.update();
        assert_eq!(
            app.world().entity(blue).get::<Hero>().unwrap().gold,
            blue_start + LODE_SHRINE_PERIODIC_GOLD
        );
    }
}
