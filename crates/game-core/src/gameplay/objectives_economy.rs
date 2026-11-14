//! 中立目标的经济与刷新裁决（`GX-CBT-2026-001` §13.2 / §13.4）。
//!
//! 本模块只有纯函数与无副作用的数据结构，由服务端权威系统调用：
//! - **辉矿 / 兵矿**（`LodeShrine`）：占领瞬间全队每人 `+40`、团队池 `+120`；
//!   归属期间每 1800 tick（30 s）再给归属方每人 `+30`，直到被反占。
//! - **渊卫**（中央矿坑的全局首领；项目原创术语，见 §CR-01）：
//!   `T=54000` 首刷、被击败后 7200 tick 重生，**按队伍累计伤害**判定归属，
//!   归属走 `objective_buff(3)` 发 120 s 全队增益。
//!
//! 地图骨架不动：坑位与兵矿坐标已由 `map::GOLD_MINE_POS` / `map::JUNGLE_CAMPS` 给出。

use crate::components::Team;
use bevy::prelude::*;

/// 目标 id 与 `attributes::objective_buff` / `registry::objective_name` 的 1/2/3 对齐。
/// 两座兵矿共用 id 1（同类型、同 buff）；征募井 id 2；渊卫 id 3。
pub const OBJECTIVE_LODE_SHRINE: u32 = 1;
pub const OBJECTIVE_MUSTER_WELL: u32 = 2;
pub const OBJECTIVE_ABYSS_WARDEN: u32 = 3;

/// `T = 54000`（15 min）首次刷新；加时 / 判胜后不再刷新。
pub const ABYSS_WARDEN_FIRST_SPAWN_TICK: u64 = 54_000;
pub const ABYSS_WARDEN_RESPAWN_TICKS: u64 = 7_200;
/// 归属期间每 1800 tick（30 s）发放一次持续收益。
pub const LODE_SHRINE_INCOME_PERIOD_TICKS: u64 = 1_800;
pub const LODE_SHRINE_CAPTURE_PER_PLAYER_GOLD: u32 = 40;
pub const LODE_SHRINE_CAPTURE_TEAM_GOLD: u32 = 120;
pub const LODE_SHRINE_PERIODIC_GOLD: u32 = 30;
/// 渊卫归属：团队池 +300、全队每人 +100（§14.4）。
pub const ABYSS_WARDEN_CAPTURE_TEAM_GOLD: u32 = 300;
pub const ABYSS_WARDEN_CAPTURE_PER_PLAYER_GOLD: u32 = 100;
/// 「渊卫推进」120 s：归属方新生成的战偶 `+50% 生命 / +30% 攻击 / +0.4 移速`（§13.4）。
pub const ABYSS_WARDEN_BUFF_DURATION_TICKS: u64 = 120 * 60;
pub const WARDEN_MINION_HP_MULT: f32 = 1.5;
pub const WARDEN_MINION_AD_MULT: f32 = 1.3;
pub const WARDEN_MINION_SPEED_BONUS: f32 = 0.4;

/// 渊卫是否应在该 tick 生成。纯函数，便于锁定首刷 / 重生 / 加时门禁。
pub const fn should_spawn_abyss_warden(
    tick: u64,
    alive: bool,
    next_spawn_tick: u64,
    match_over: bool,
) -> bool {
    !match_over && !alive && tick >= ABYSS_WARDEN_FIRST_SPAWN_TICK && tick >= next_spawn_tick
}

/// 周期收益是否到期；`last_payout_tick` 为该占点上次发放的权威 tick。
pub const fn income_due(tick: u64, last_payout_tick: u64) -> bool {
    tick >= last_payout_tick.saturating_add(LODE_SHRINE_INCOME_PERIOD_TICKS)
}

/// 按累计伤害判定渊卫归属；无人伤害或完全持平则无人获得（§13.4）。
pub fn abyss_warden_owner(blue_damage: f32, red_damage: f32) -> Option<Team> {
    if blue_damage <= 0.0 && red_damage <= 0.0 {
        return None;
    }
    if (blue_damage - red_damage).abs() < f32::EPSILON {
        return None;
    }
    Some(if blue_damage > red_damage {
        Team::Blue
    } else {
        Team::Red
    })
}

fn team_index(team: Team) -> Option<usize> {
    match team {
        Team::Blue => Some(0),
        Team::Red => Some(1),
        Team::Neutral => None,
    }
}

/// 渊卫全局状态（服务端唯一权威副本）。
///
/// `alive` 由生成 / 击败系统维护，`entity` 用于在重生时清理残留实体；
/// 伤害累计只统计**英雄**对渊卫的实际扣血，避免野怪或小兵误抢归属。
#[derive(Resource, Default, Debug)]
pub struct AbyssWardenState {
    pub alive: bool,
    pub entity: Option<Entity>,
    pub next_spawn_tick: u64,
    pub blue_damage: f32,
    pub red_damage: f32,
}

impl AbyssWardenState {
    pub fn on_spawn(&mut self, entity: Entity) {
        self.alive = true;
        self.entity = Some(entity);
        self.blue_damage = 0.0;
        self.red_damage = 0.0;
    }

    /// 记一次英雄造成的伤害；中立 / 无队伍的攻击者不计入。
    pub fn record_damage(&mut self, team: Option<Team>, amount: f32) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        match team {
            Some(Team::Blue) => self.blue_damage += amount,
            Some(Team::Red) => self.red_damage += amount,
            _ => {}
        }
    }

    /// 渊卫被击败：返回归属队、排下一次重生、清空累计伤害。
    pub fn on_defeated(&mut self, tick: u64) -> Option<Team> {
        let owner = abyss_warden_owner(self.blue_damage, self.red_damage);
        self.alive = false;
        self.entity = None;
        self.blue_damage = 0.0;
        self.red_damage = 0.0;
        self.next_spawn_tick = tick.saturating_add(ABYSS_WARDEN_RESPAWN_TICKS);
        owner
    }
}

/// 金矿归属收益账本。
///
/// `team_gold` 是**团队池**（§13.2 的 `+120`）：项目暂无团队金币消费点，
/// 先按权威账本记录，供后续结算 / 统计使用，不直接发到个人金币。
///
/// 两座兵矿共用 `objective_id = 1`，若按 id 计时会导致「同队占两矿只发一次」。
/// 因此周期收益**按队伍**计时，每 1800 tick 发放 `30 × 该队持有兵矿数` 金/人，
/// 既区分了两队的独立计时，也符合「每占一矿多一份收益」的直觉。
#[derive(Resource, Default, Debug)]
pub struct ObjectiveIncomeState {
    pub last_payout_tick: [u64; 2],
    pub team_gold: [u32; 2],
}

impl ObjectiveIncomeState {
    pub fn credit_team(&mut self, team: Team, gold: u32) {
        if let Some(i) = team_index(team) {
            self.team_gold[i] = self.team_gold[i].saturating_add(gold);
        }
    }

    pub fn team_gold_of(&self, team: Team) -> u32 {
        team_index(team).map(|i| self.team_gold[i]).unwrap_or(0)
    }

    pub fn payout_due(&self, team: Team, tick: u64) -> bool {
        team_index(team).is_some_and(|i| income_due(tick, self.last_payout_tick[i]))
    }

    pub fn mark_paid(&mut self, team: Team, tick: u64) {
        if let Some(i) = team_index(team) {
            self.last_payout_tick[i] = tick;
        }
    }

    /// 占领瞬间记团队池，并把该队上次发放时刻重置为当前 tick，
    /// 避免「刚占领就立刻再发一次 30 金」。
    pub fn on_captured(&mut self, objective_id: u32, owner: Team, tick: u64) {
        if objective_id == OBJECTIVE_LODE_SHRINE {
            self.credit_team(owner, LODE_SHRINE_CAPTURE_TEAM_GOLD);
            self.mark_paid(owner, tick);
        }
    }
}

/// 渊卫推进增益的权威有效期（服务端唯一副本）。
///
/// 一期只作用于**归属期间新生成**的战偶（§13.4 的「附近半径 20 内」简化为全队兵线），
/// 已存在的战偶不回溯增强；真实 LoS / 推线 AI 仍属二期。
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct WardenBuff {
    pub owner: Option<Team>,
    pub expires_tick: u64,
}

impl WardenBuff {
    pub fn grant(&mut self, team: Team, tick: u64) {
        self.owner = Some(team);
        self.expires_tick = tick.saturating_add(ABYSS_WARDEN_BUFF_DURATION_TICKS);
    }

    pub fn active(&self, team: Team, tick: u64) -> bool {
        self.owner == Some(team) && tick < self.expires_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warden_buff_expires_after_120_seconds() {
        let mut buff = WardenBuff::default();
        assert!(!buff.active(Team::Blue, 0));
        buff.grant(Team::Blue, 1_000);
        assert!(buff.active(Team::Blue, 1_000));
        assert!(!buff.active(Team::Red, 1_000));
        assert!(!buff.active(Team::Blue, 1_000 + ABYSS_WARDEN_BUFF_DURATION_TICKS));
        let hero_buff =
            crate::gameplay::attributes::objective_buff(OBJECTIVE_ABYSS_WARDEN, Team::Blue)
                .expect("渊卫 buff 必须存在");
        assert_eq!(
            u64::from(hero_buff.remaining_ticks),
            ABYSS_WARDEN_BUFF_DURATION_TICKS,
            "英雄 buff 与战偶强化必须同寿命"
        );
    }

    #[test]
    fn objective_ids_match_buff_and_registry_tables() {
        assert_eq!(OBJECTIVE_LODE_SHRINE, 1);
        assert_eq!(OBJECTIVE_MUSTER_WELL, 2);
        assert_eq!(OBJECTIVE_ABYSS_WARDEN, 3);
        assert!(
            crate::gameplay::attributes::objective_buff(OBJECTIVE_ABYSS_WARDEN, Team::Blue)
                .is_some()
        );
        assert_eq!(
            crate::gameplay::registry::objective_name(OBJECTIVE_LODE_SHRINE),
            "lode_shrine"
        );
    }

    #[test]
    fn abyss_warden_spawn_respects_first_spawn_and_overtime() {
        assert!(!should_spawn_abyss_warden(53_999, false, 0, false));
        assert!(should_spawn_abyss_warden(54_000, false, 0, false));
        assert!(
            !should_spawn_abyss_warden(54_000, true, 0, false),
            "已存在不得重复生成"
        );
        assert!(
            !should_spawn_abyss_warden(60_000, false, 0, true),
            "判胜后不刷新"
        );
        assert!(!should_spawn_abyss_warden(60_000, false, 61_200, false));
        assert!(should_spawn_abyss_warden(61_200, false, 61_200, false));
    }

    #[test]
    fn abyss_warden_owner_is_decided_by_accumulated_damage() {
        assert_eq!(abyss_warden_owner(0.0, 0.0), None);
        assert_eq!(abyss_warden_owner(100.0, 100.0), None, "完全持平无人获得");
        assert_eq!(abyss_warden_owner(150.0, 100.0), Some(Team::Blue));
        assert_eq!(abyss_warden_owner(100.0, 150.0), Some(Team::Red));
    }

    #[test]
    fn abyss_warden_state_cycles_spawn_damage_and_defeat() {
        let mut world = World::new();
        let e = world.spawn_empty().id();
        let mut state = AbyssWardenState::default();
        state.on_spawn(e);
        assert!(state.alive);
        state.record_damage(Some(Team::Blue), 120.0);
        state.record_damage(Some(Team::Red), 80.0);
        state.record_damage(Some(Team::Neutral), 999.0);
        state.record_damage(None, f32::NAN);
        assert_eq!(state.blue_damage, 120.0);
        assert_eq!(state.red_damage, 80.0);

        assert_eq!(state.on_defeated(55_000), Some(Team::Blue));
        assert!(!state.alive);
        assert_eq!(state.entity, None);
        assert_eq!(state.blue_damage, 0.0);
        assert_eq!(state.next_spawn_tick, 55_000 + ABYSS_WARDEN_RESPAWN_TICKS);
        assert!(should_spawn_abyss_warden(
            state.next_spawn_tick,
            false,
            state.next_spawn_tick,
            false
        ));
    }

    #[test]
    fn lode_shrine_income_is_periodic_and_team_pool_is_tracked() {
        let mut income = ObjectiveIncomeState::default();
        assert!(income.payout_due(Team::Blue, LODE_SHRINE_INCOME_PERIOD_TICKS));
        assert!(!income.payout_due(Team::Blue, 1));

        income.on_captured(OBJECTIVE_LODE_SHRINE, Team::Blue, 900);
        assert_eq!(
            income.team_gold_of(Team::Blue),
            LODE_SHRINE_CAPTURE_TEAM_GOLD
        );
        assert_eq!(income.team_gold_of(Team::Red), 0);
        assert!(!income.payout_due(Team::Blue, 900));
        assert!(!income.payout_due(Team::Blue, 900 + 1_799));
        assert!(income.payout_due(Team::Blue, 900 + 1_800));
        // 两队计时互不影响：Blue 已在 900 记过账，Red 仍按 0 起算（1800 到期）
        assert!(!income.payout_due(Team::Red, 900));
        assert!(income.payout_due(Team::Red, LODE_SHRINE_INCOME_PERIOD_TICKS));

        income.mark_paid(Team::Blue, 2_700);
        assert!(!income.payout_due(Team::Blue, 4_499));
        assert!(income.payout_due(Team::Blue, 4_500));
        // 征募井不进团队池
        income.on_captured(OBJECTIVE_MUSTER_WELL, Team::Red, 3_000);
        assert_eq!(income.team_gold_of(Team::Red), 0);
        assert_eq!(income.team_gold_of(Team::Neutral), 0);
        assert!(!income.payout_due(Team::Neutral, 100_000));
    }

    /// 两座兵矿同队时收益翻倍（按占点数量，而非按 id 去重）。
    #[test]
    fn income_scales_with_owned_gold_mines() {
        use crate::gameplay::objectives::CapturePoint;
        let mut blue = CapturePoint::new(OBJECTIVE_LODE_SHRINE, 100, 5400);
        blue.owner = Some(Team::Blue);
        let mut red = CapturePoint::new(OBJECTIVE_LODE_SHRINE, 100, 5400);
        red.owner = Some(Team::Blue);
        let neutral = CapturePoint::new(OBJECTIVE_MUSTER_WELL, 100, 5400);
        let points = [&blue, &red, &neutral];
        let count = |team: Team| {
            points
                .iter()
                .filter(|cp| cp.objective_id == OBJECTIVE_LODE_SHRINE && cp.owner == Some(team))
                .count() as u32
        };
        assert_eq!(count(Team::Blue), 2);
        assert_eq!(
            count(Team::Blue) * LODE_SHRINE_PERIODIC_GOLD,
            60,
            "占两矿 = 每人 60 金/周期"
        );
        assert_eq!(count(Team::Red), 0);
    }
}
