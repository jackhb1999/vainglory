//! `GameplayEvents` → `ServerEvents` 适配器（协议 v3）。
//!
//! 之前玩法系统把事件写进本地 `GameplayEvents`，而网络层只 `std::mem::take`
//! `ServerEvents`；两个汇之间没有任何桥，且本地汇永远不清空。本模块每 tick
//! 做一次转换并清空，补齐购买/占领/死亡/状态/胜负的端到端链路。

use crate::networking::{EventCursor, ServerEvents};
use bevy::prelude::*;
use game_core::components::{NetId, Team};
use game_core::gameplay::{GameplayEvents, NetEvent};
use game_core::net::GameEvent;

pub fn drain_gameplay_events(
    mut events: ResMut<GameplayEvents>,
    mut out: ResMut<ServerEvents>,
    mut cursor: ResMut<EventCursor>,
) {
    let next_id = |cursor: &mut EventCursor| {
        cursor.0 = cursor.0.saturating_add(1);
        cursor.0
    };

    // 内部裁决记录：占领与阵亡
    for captured in events.captured.drain(..) {
        out.0.push(GameEvent::ObjectiveCaptured {
            objective_id: game_core::gameplay::registry::objective_name(captured.objective_id)
                .to_owned(),
            owner: captured.owner,
            event_id: next_id(&mut cursor),
        });
    }
    for death in events.deaths.drain(..) {
        out.0.push(GameEvent::HeroDeath {
            victim: NetId(death.victim_net_id),
            killer: (death.killer_net_id != 0).then_some(NetId(death.killer_net_id)),
            assist: Vec::new(),
            bounty: Some(death.bounty),
        });
    }
    events.purchases.clear();

    for event in events.net.drain(..) {
        let id = next_id(&mut cursor);
        let game_event = match event {
            NetEvent::ItemPurchased {
                net_id,
                item_id,
                paid,
            } => GameEvent::ItemPurchased {
                net_id: NetId(net_id),
                item_id,
                paid,
                event_id: id,
            },
            NetEvent::ItemSold {
                net_id,
                item_id,
                refund,
            } => GameEvent::ItemSold {
                net_id: NetId(net_id),
                item_id,
                refund,
                event_id: id,
            },
            NetEvent::ShopRejected {
                net_id,
                item_id,
                reason,
            } => GameEvent::ShopRejected {
                net_id: NetId(net_id),
                item_id,
                reason,
                event_id: id,
            },
            NetEvent::ObjectiveCaptured {
                objective_id,
                owner,
            } => GameEvent::ObjectiveCaptured {
                objective_id: game_core::gameplay::registry::objective_name(objective_id)
                    .to_owned(),
                owner,
                event_id: id,
            },
            NetEvent::ObjectiveBuffGranted {
                objective_id,
                owner,
                buff_id,
            } => GameEvent::ObjectiveBuffGranted {
                objective_id,
                owner,
                buff_id,
                event_id: id,
            },
            NetEvent::HeroDeath {
                victim,
                killer,
                bounty,
            } => GameEvent::HeroDeath {
                victim: NetId(victim),
                killer: killer.map(NetId),
                assist: Vec::new(),
                bounty: Some(bounty),
            },
            NetEvent::WaveSpawned(index) => GameEvent::WaveSpawned {
                wave_index: index,
                event_id: id,
            },
            NetEvent::OvertimeStarted => GameEvent::OvertimeStarted { event_id: id },
            NetEvent::MatchFinished {
                winner,
                reason,
                duration_ticks,
            } => GameEvent::MatchFinished {
                winning_team: winner,
                reason,
                duration_ticks,
                event_id: id,
            },
            NetEvent::TurretDestroyed { net_id, team } => GameEvent::TurretDestroyed {
                net_id: NetId(net_id),
                destroyer: NetId(0),
                team,
            },
            NetEvent::MinionDeath { net_id } => GameEvent::MinionDeath {
                net_id: NetId(net_id),
                killer: None,
            },
            NetEvent::JungleMonsterDeath { net_id } => GameEvent::JungleMonsterDeath {
                net_id: NetId(net_id),
                killer: NetId(0),
                team: Team::Neutral,
            },
            NetEvent::AbyssWardenSpawn { position } => GameEvent::AbyssWardenSpawn { position },
            NetEvent::AbyssWardenDeath { killer_team } => {
                GameEvent::AbyssWardenDeath { killer_team }
            }
            NetEvent::LodeShrineCapture { team } => GameEvent::LodeShrineCapture { team },
            NetEvent::ObjectiveIncome { owner, amount } => GameEvent::ObjectiveIncome {
                owner,
                amount,
                event_id: id,
            },
        };
        out.0.push(game_event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_maps_every_net_event_and_drains() {
        let mut app = App::new();
        app.init_resource::<GameplayEvents>()
            .init_resource::<ServerEvents>()
            .init_resource::<EventCursor>()
            .add_systems(Update, drain_gameplay_events);
        app.world_mut()
            .resource_mut::<GameplayEvents>()
            .emit(NetEvent::ItemPurchased {
                net_id: 7,
                item_id: 42,
                paid: 300,
            });
        app.world_mut()
            .resource_mut::<GameplayEvents>()
            .emit(NetEvent::WaveSpawned(3));
        app.update();

        let out = app.world().resource::<ServerEvents>();
        assert_eq!(out.0.len(), 2);
        assert!(matches!(
            &out.0[0],
            GameEvent::ItemPurchased { net_id, paid: 300, .. } if net_id.0 == 7
        ));
        assert!(matches!(
            &out.0[1],
            GameEvent::WaveSpawned { wave_index: 3, .. }
        ));
        assert!(app.world().resource::<GameplayEvents>().net.is_empty());
    }
}
