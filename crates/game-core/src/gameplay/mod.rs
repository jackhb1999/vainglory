//! 玩法核心逻辑（`GX-CBT-2026-001` §4~§15）。
//!
//! 本目录下全部模块为**服务端权威的纯逻辑 + 组件定义**，不含资源加载、
//! 不含客户端预测；ECS 系统由 `server::gameplay` 驱动，客户端只读快照。

pub mod attributes;
pub mod clock;
pub mod combat;
pub mod jungle;
pub mod objectives;
/// 中立目标的经济 / 刷新裁决（§13.2 辉矿、§13.4 渊卫）。
pub mod objectives_economy;
pub mod registry;
pub mod respawn;
pub mod shop;
pub mod structures;
pub mod waves;

use crate::components::Team;
use bevy::prelude::*;
use objectives::ObjectiveCapturedEvent;
use respawn::DeathRecord;
use shop::ShopReceipt;

pub const TICK_RATE: u64 = 60;

/// 服务端确定性伪随机：所有暴击/掉落判定统一使用，回放可复现（§17）。
pub fn deterministic_roll(tick: u64, a: u64, b: u64) -> f32 {
    let mut h =
        tick.wrapping_mul(0x9e37_79b9) ^ a.wrapping_mul(0x85eb_ca6b) ^ b.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    ((h % 1_000_000) as f32) / 1_000_000.0
}

/// 伤害系统写入、死亡系统在同 tick 消费的击杀流水：`(tick, killer_net_id, victim_net_id)`。
/// 单独成资源而非事件汇，避免被网络层当广播事件提前清空。
#[derive(Resource, Default, Debug)]
pub struct PendingDeaths(pub Vec<(u64, u64, u64)>);

/// 待广播的**协议级**玩法事件。`GameplayEvents → ServerEvents` 的 adapter 每 tick
/// 消费并清空；`event_id` 由 `EventEnvelope` 统一分配，避免两套自增游标冲突。
#[derive(Clone, Debug, PartialEq)]
pub enum NetEvent {
    ItemPurchased {
        net_id: u64,
        item_id: u32,
        paid: u32,
    },
    ItemSold {
        net_id: u64,
        item_id: u32,
        refund: u32,
    },
    ShopRejected {
        net_id: u64,
        item_id: u32,
        reason: u8,
    },
    ObjectiveCaptured {
        objective_id: u32,
        owner: Team,
    },
    ObjectiveBuffGranted {
        objective_id: u32,
        owner: Team,
        buff_id: u32,
    },
    HeroDeath {
        victim: u64,
        killer: Option<u64>,
        bounty: u32,
    },
    WaveSpawned(u64),
    OvertimeStarted,
    MatchFinished {
        winner: Team,
        reason: u8,
        duration_ticks: u64,
    },
    TurretDestroyed {
        net_id: u64,
        team: Team,
    },
    /// 小兵死亡：服务端已 despawn（碰撞体移除），仅需告知客户端播放消失动画。
    MinionDeath {
        net_id: u64,
    },
    /// 野怪死亡：同上，并触发客户端特效。
    JungleMonsterDeath {
        net_id: u64,
    },
    /// 渊卫（中央矿坑全局首领）刷新 / 被击败：全局广播，客户端在坑位播放特效与提示。
    AbyssWardenSpawn {
        position: crate::net::Vec3f,
    },
    AbyssWardenDeath {
        killer_team: Team,
    },
    /// 兵矿（辉矿）被占领：客户端在小地图标记归属。
    LodeShrineCapture {
        team: Team,
    },
    /// 兵矿归属期间的周期收益，供 HUD 飘字；金币本身由 `CH_GOLD` 快照裁决。
    ObjectiveIncome {
        owner: Team,
        amount: u32,
    },
}

/// 玩法关键事件汇：内部裁决记录 + 待广播的协议事件。
#[derive(Resource, Default, Debug)]
pub struct GameplayEvents {
    pub captured: Vec<ObjectiveCapturedEvent>,
    pub deaths: Vec<DeathRecord>,
    pub purchases: Vec<ShopReceipt>,
    pub net: Vec<NetEvent>,
}

impl GameplayEvents {
    pub fn emit(&mut self, event: NetEvent) {
        self.net.push(event);
    }

    pub fn clear(&mut self) {
        self.captured.clear();
        self.deaths.clear();
        self.purchases.clear();
        self.net.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_roll_is_reproducible_and_bounded() {
        assert_eq!(deterministic_roll(42, 7, 9), deterministic_roll(42, 7, 9));
        assert_ne!(deterministic_roll(42, 7, 9), deterministic_roll(43, 7, 9));
        for t in 0..1000 {
            let r = deterministic_roll(t, 1, 2);
            assert!((0.0..1.0).contains(&r));
        }
    }

    /// P-A 回放基线：同一权威 tick 序列下，`SimTime` / `MatchClock` / 随机裁决
    /// 三者重放结果必须逐项一致（禁止墙钟参与的回归护栏）。
    #[test]
    fn authoritative_tick_sequence_is_replayable() {
        use crate::gameplay::clock::MatchClock;
        use crate::systems::SimTime;

        let replay = || {
            let mut clock = MatchClock::default();
            clock.start_at(1000);
            let mut rolls = Vec::new();
            for step in 1..=600u64 {
                let server_tick = 1000 + step;
                clock.sync(server_tick);
                let sim = SimTime(server_tick);
                assert_eq!(sim.tick(), server_tick);
                assert_eq!(clock.tick, sim.tick() - clock.start_tick);
                rolls.push(deterministic_roll(sim.tick(), 3, 5).to_bits());
            }
            (clock.tick, clock.phase, rolls)
        };
        assert_eq!(replay(), replay());
    }
}
