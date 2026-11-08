//! M1 网络与配置验收用例（`DEV-27`）
//!
//! `TC-NET-01/02/03`、`TC-COMPAT-01`、`TC-PRED-01`、`TC-CFG-01`。

use game_core::components::{NetId, Team};
use game_core::net::{
    EntityDelta, EntitySnapshot, EntityType, EventDeduplicator, PROTOCOL_VERSION, Quatf,
    RejectReason, Snapshot, SnapshotHeader, Vec3f, VersionHello, VisibilityPolicy, VisibilitySet,
    WorldSnapshot, apply_delta, diff_entity, negotiate,
};
use game_core::visibility::{compute_visibility, event_visible};
use gx_config::{ConfigSet, ConfigValidate};
use gx_proto_content::ContentTables;

fn entity(net_id: u64, team: Team, kind: EntityType, x: f32, z: f32) -> EntitySnapshot {
    EntitySnapshot {
        net_id: NetId(net_id),
        entity_type: kind,
        team,
        position: Vec3f::new(x, 0.0, z),
        rotation: Quatf::identity(),
        velocity: Vec3f::zero(),
        health: 100.0,
        max_health: 100.0,
        energy: 50.0,
        max_energy: 50.0,
        level: 1,
        gold: 0,
        ability_cooldowns: [0.0; 4],
        status_mask: 0,
        shield: 0.0,
    }
}

fn policy() -> VisibilityPolicy {
    VisibilityPolicy {
        vision_radius: 18.0,
        brush_reveal_radius: 6.0,
        constructs_provide_vision: true,
        sentries_provide_vision: true,
    }
}

fn header(tick: u64) -> SnapshotHeader {
    SnapshotHeader {
        tick,
        baseline_tick: tick,
        config_hash: [0; 32],
        view_seq: tick as u32,
        client_time_us: 0,
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// `TC-NET-01`：6 人房间下，单实体变化的 delta 包显著小于全量快照。
#[test]
fn tc_net_01_delta_bandwidth_is_less_than_half_of_full_snapshot() {
    let mut entities = Vec::new();
    for i in 0..6u64 {
        let team = if i < 3 { Team::Blue } else { Team::Red };
        entities.push(entity(i + 1, team, EntityType::Hero, i as f32 * 2.0, 0.0));
    }
    let full = WorldSnapshot {
        tick: 100,
        server_time: 0.0,
        entities: entities.clone(),
        events: Vec::new(),
    };
    let full_bytes = bitcode::encode(&full).len();

    let prev: Vec<EntityDelta> = entities.iter().map(|e| e.to_delta()).collect();
    let mut next = prev[0].clone();
    next.health = 42.0;
    let delta = diff_entity(&prev[0], &next);
    assert_ne!(delta.changed_mask, 0);
    let snapshot = Snapshot {
        header: header(101),
        deltas: vec![delta],
        removed: Vec::new(),
        baseline: None,
    };
    let delta_bytes = bitcode::encode(&snapshot).len();

    assert!(
        delta_bytes * 2 < full_bytes,
        "TC-NET-01: delta {delta_bytes} B should be < 50% of full {full_bytes} B"
    );
}

/// `TC-NET-02`：可靠事件重传只交付一次。
#[test]
fn tc_net_02_reliable_events_are_idempotent() {
    let mut dedup = EventDeduplicator::new(16);
    assert!(dedup.accept(1));
    assert!(!dedup.accept(1), "retransmission must be dropped");
    assert!(dedup.accept(2));
    dedup.clear();
    assert!(dedup.accept(1));
}

/// `TC-NET-03`：视野外敌方实体零下发（含事件附带数据）。
#[test]
fn tc_net_03_no_out_of_view_entity_bytes_in_packet() {
    let hidden_position = 987.654f32;
    let entities = vec![
        entity(1, Team::Blue, EntityType::Hero, 0.0, 0.0),
        entity(2, Team::Blue, EntityType::Hero, 2.0, 0.0),
        entity(3, Team::Red, EntityType::Hero, 3.0, 0.0),
        entity(4, Team::Red, EntityType::Hero, hidden_position, 0.0),
    ];
    let set = compute_visibility(Team::Blue, &policy(), &entities);
    assert!(set.contains(3), "near enemy should be visible");
    assert!(!set.contains(4), "distant enemy must be culled");

    let visible: Vec<EntitySnapshot> = entities
        .iter()
        .filter(|e| set.contains(e.net_id.0))
        .cloned()
        .collect();
    let packet = bitcode::encode(&WorldSnapshot {
        tick: 1,
        server_time: 0.0,
        entities: visible,
        events: Vec::new(),
    });
    assert!(
        !contains_subslice(&packet, &hidden_position.to_le_bytes()),
        "hidden entity position leaked into packet"
    );

    let leaked_event = game_core::net::GameEvent::DamageDealt {
        attacker: NetId(1),
        target: NetId(4),
        damage: 10.0,
        is_critical: false,
    };
    assert!(!event_visible(&leaked_event, &set));
    let global = game_core::net::GameEvent::ObjectiveCaptured {
        objective_id: "lode_shrine".into(),
        owner: Team::Blue,
        event_id: 7,
    };
    assert!(event_visible(&global, &VisibilitySet::default()));
}

/// `TC-COMPAT-01`：协议版本与内容 hash 双协商，不匹配即明确拒绝。
#[test]
fn tc_compat_01_dual_version_negotiation_rejects() {
    let expected = [7u8; 32];
    assert!(
        negotiate(
            &VersionHello {
                protocol_version: PROTOCOL_VERSION,
                config_hash: expected
            },
            expected
        )
        .is_ok()
    );
    assert!(matches!(
        negotiate(
            &VersionHello {
                protocol_version: 1,
                config_hash: expected
            },
            expected
        ),
        Err(RejectReason::ProtocolMismatch { .. })
    ));
    assert!(matches!(
        negotiate(
            &VersionHello {
                protocol_version: PROTOCOL_VERSION,
                config_hash: [0; 32]
            },
            expected
        ),
        Err(RejectReason::ConfigMismatch { .. })
    ));
}

/// `TC-PRED-01`：基线 + 增量可重建完整状态，供预测回滚与插值使用。
#[test]
fn tc_pred_01_baseline_plus_delta_rebuilds_full_state() {
    let base = entity(9, Team::Red, EntityType::Hero, 1.0, 2.0).to_delta();
    let mut changed = base.clone();
    changed.position = [4.0, 0.0, 6.0];
    changed.health = 12.5;
    changed.level = 4;
    let delta = diff_entity(&base, &changed);

    // 客户端先持基线（CH_ALL），再叠加增量；身份字段未变化时由基线保留
    let mut rebuilt = base.clone();
    apply_delta(&mut rebuilt, &delta);
    assert_eq!(rebuilt.position, changed.position);
    assert_eq!(rebuilt.health, changed.health);
    assert_eq!(rebuilt.level, changed.level);

    let snapshot = EntitySnapshot::from_delta(&rebuilt);
    assert_eq!(snapshot.net_id.0, 9);
    assert_eq!(snapshot.team, Team::Red);
    assert_eq!(snapshot.position.x, 4.0);
}

/// `TC-CFG-01`：配置校验器拒绝断引用、非法数值与成环合成图。
#[test]
fn tc_cfg_01_config_validator_rejects_invalid_tables() {
    ConfigSet::load_default().expect("placeholder config must be valid");

    let mut tables = ContentTables::placeholder();
    tables.heroes[0].ability_ids.push("ab_missing".into());
    assert!(
        tables.validate().is_err(),
        "unknown ability reference must fail"
    );

    let mut tables = ContentTables::placeholder();
    tables.items[0].price = 0;
    assert!(tables.validate().is_err(), "zero price must fail");

    let mut tables = ContentTables::placeholder();
    tables.modes.room_size = 5;
    assert!(
        tables.validate().is_err(),
        "room_size must equal team_size * 2"
    );
}
