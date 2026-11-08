//! GX-01 协议层（`DEV-03` / `DEV-04` / `DEV-05` / `DEV-06`）。
//!
//! 本 crate **不依赖任何引擎**（含 Bevy/glam），所有几何量用原始数组表示，
//! 由 `game-core` 的适配层与引擎类型转换。对应 `GX-ARCH-2026-001` §4.2 / §8。

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// 协议版本。字段结构演进后由 1 升为 2（`FR-57` / `TF-10`）。
/// v3：`EntityDelta` 新增 `status_mask` / `shield`，快照位新增 `CH_STATUS` / `CH_SHIELD`，
/// 新增玩法事件与购买/售出上行；与 v2 的 bitcode 布局**不兼容**，由 `negotiate` 拒绝旧客户端。
pub const PROTOCOL_VERSION: u32 = 3;
/// 快照下行频率（Hz），与 60Hz 模拟解耦。
pub const SNAPSHOT_HZ: u32 = 20;
/// 基线快照间隔（tick）。60Hz × 2s。
pub const BASELINE_INTERVAL_TICKS: u64 = 120;
/// 状态快照每 tick 的下发间隔。
pub const SNAPSHOT_INTERVAL_TICKS: u64 = 3;

pub type ConfigHash = [u8; 32];
pub type EventId = u64;
pub type PeerId = u64;

// ---------------------------------------------------------------------------
// 版本协商（FR-57 / TC-COMPAT-01）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct VersionHello {
    pub protocol_version: u32,
    pub config_hash: ConfigHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum RejectReason {
    ProtocolMismatch {
        expected: u32,
        got: u32,
    },
    ConfigMismatch {
        expected: ConfigHash,
        got: ConfigHash,
    },
    MissingSessionToken,
    InvalidSessionToken,
    RateLimited,
}

/// 双版本号协商：协议版本与内容 hash 均需匹配，否则明确拒绝（不得静默降级）。
pub fn negotiate(hello: &VersionHello, expected_hash: ConfigHash) -> Result<(), RejectReason> {
    if hello.protocol_version != PROTOCOL_VERSION {
        return Err(RejectReason::ProtocolMismatch {
            expected: PROTOCOL_VERSION,
            got: hello.protocol_version,
        });
    }
    if hello.config_hash != expected_hash {
        return Err(RejectReason::ConfigMismatch {
            expected: expected_hash,
            got: hello.config_hash,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 传输抽象（ADR-04 / DEV-04）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum Channel {
    Unreliable,
    ReliableOrdered,
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("peer not connected: {0}")]
    PeerNotConnected(PeerId),
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(usize),
    #[error("transport closed")]
    Closed,
}

/// 与具体 UDP/Web 实现解耦的最小传输契约。
/// renet2_netcode 为默认实现，移动端/Web 可换实现（ADR-04）。
pub trait Transport: Send + Sync {
    fn send(&mut self, peer: PeerId, channel: Channel, payload: &[u8]) -> Result<(), NetError>;
    fn recv(&mut self) -> Vec<(PeerId, Channel, Vec<u8>)>;
}

// ---------------------------------------------------------------------------
// 增量快照（FR-52 / DEV-06 / TC-NET-01）
// ---------------------------------------------------------------------------

pub const CH_POSITION: u32 = 1 << 0;
pub const CH_ROTATION: u32 = 1 << 1;
pub const CH_VELOCITY: u32 = 1 << 2;
pub const CH_HEALTH: u32 = 1 << 3;
pub const CH_MAX_HEALTH: u32 = 1 << 4;
pub const CH_ENERGY: u32 = 1 << 5;
pub const CH_MAX_ENERGY: u32 = 1 << 6;
pub const CH_LEVEL: u32 = 1 << 7;
pub const CH_GOLD: u32 = 1 << 8;
pub const CH_COOLDOWNS: u32 = 1 << 9;
pub const CH_IDENTITY: u32 = 1 << 10;
/// `PROTOCOL_VERSION = 3`：状态效果位掩码（`StatusEffects` 的 8 类效果压缩为 bit）。
pub const CH_STATUS: u32 = 1 << 11;
/// `PROTOCOL_VERSION = 3`：护盾池当前值。
pub const CH_SHIELD: u32 = 1 << 12;
pub const CH_ALL: u32 = 0x1FFF;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct SnapshotHeader {
    pub tick: u64,
    pub baseline_tick: u64,
    pub config_hash: ConfigHash,
    pub view_seq: u32,
    /// 回显客户端 `client_time_us`，用于真实 RTT 估计（FR-56 / DN-13）。
    pub client_time_us: u64,
}

/// 单个实体的增量。仅 `changed_mask` 标记的字段有效。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct EntityDelta {
    pub net_id: u64,
    pub generation: u16,
    pub changed_mask: u32,
    pub entity_type: u8,
    pub team: u8,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub health: f32,
    pub max_health: f32,
    pub energy: f32,
    pub max_energy: f32,
    pub level: u8,
    pub gold: u32,
    pub ability_cooldowns: [f32; 4],
    /// `PROTOCOL_VERSION = 3`：状态效果位掩码（bit0..7 对应 8 类 `StatusKind`）。
    pub status_mask: u32,
    /// `PROTOCOL_VERSION = 3`：护盾池当前值。
    pub shield: f32,
}

impl EntityDelta {
    pub fn empty(net_id: u64) -> Self {
        Self {
            net_id,
            generation: 0,
            changed_mask: 0,
            entity_type: 0,
            team: 0,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0; 3],
            health: 0.0,
            max_health: 0.0,
            energy: 0.0,
            max_energy: 0.0,
            level: 0,
            gold: 0,
            ability_cooldowns: [0.0; 4],
            status_mask: 0,
            shield: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Snapshot {
    pub header: SnapshotHeader,
    pub deltas: Vec<EntityDelta>,
    /// 本周期消失的实体（视野裁剪 / 销毁），客户端据此清理插值历史。
    pub removed: Vec<u64>,
    pub baseline: Option<Vec<EntityDelta>>,
}

/// 相对上一帧计算 changed_mask（纯函数，便于单测）。
pub fn diff_entity(prev: &EntityDelta, next: &EntityDelta) -> EntityDelta {
    let mut mask = 0u32;
    if prev.entity_type != next.entity_type
        || prev.team != next.team
        || prev.generation != next.generation
    {
        mask |= CH_IDENTITY;
    }
    if prev.position != next.position {
        mask |= CH_POSITION;
    }
    if prev.rotation != next.rotation {
        mask |= CH_ROTATION;
    }
    if prev.velocity != next.velocity {
        mask |= CH_VELOCITY;
    }
    if prev.health != next.health {
        mask |= CH_HEALTH;
    }
    if prev.max_health != next.max_health {
        mask |= CH_MAX_HEALTH;
    }
    if prev.energy != next.energy {
        mask |= CH_ENERGY;
    }
    if prev.max_energy != next.max_energy {
        mask |= CH_MAX_ENERGY;
    }
    if prev.level != next.level {
        mask |= CH_LEVEL;
    }
    if prev.gold != next.gold {
        mask |= CH_GOLD;
    }
    if prev.ability_cooldowns != next.ability_cooldowns {
        mask |= CH_COOLDOWNS;
    }
    if prev.status_mask != next.status_mask {
        mask |= CH_STATUS;
    }
    if prev.shield != next.shield {
        mask |= CH_SHIELD;
    }

    let mut out = next.clone();
    out.changed_mask = mask;
    out
}

/// 把增量应用到基线，得到完整实体状态（客户端重建用）。
pub fn apply_delta(base: &mut EntityDelta, delta: &EntityDelta) {
    if delta.changed_mask & CH_IDENTITY != 0 {
        base.entity_type = delta.entity_type;
        base.team = delta.team;
        base.generation = delta.generation;
    }
    if delta.changed_mask & CH_POSITION != 0 {
        base.position = delta.position;
    }
    if delta.changed_mask & CH_ROTATION != 0 {
        base.rotation = delta.rotation;
    }
    if delta.changed_mask & CH_VELOCITY != 0 {
        base.velocity = delta.velocity;
    }
    if delta.changed_mask & CH_HEALTH != 0 {
        base.health = delta.health;
    }
    if delta.changed_mask & CH_MAX_HEALTH != 0 {
        base.max_health = delta.max_health;
    }
    if delta.changed_mask & CH_ENERGY != 0 {
        base.energy = delta.energy;
    }
    if delta.changed_mask & CH_MAX_ENERGY != 0 {
        base.max_energy = delta.max_energy;
    }
    if delta.changed_mask & CH_LEVEL != 0 {
        base.level = delta.level;
    }
    if delta.changed_mask & CH_GOLD != 0 {
        base.gold = delta.gold;
    }
    if delta.changed_mask & CH_COOLDOWNS != 0 {
        base.ability_cooldowns = delta.ability_cooldowns;
    }
    if delta.changed_mask & CH_STATUS != 0 {
        base.status_mask = delta.status_mask;
    }
    if delta.changed_mask & CH_SHIELD != 0 {
        base.shield = delta.shield;
    }
}

// ---------------------------------------------------------------------------
// 幂等关键事件（FR-53 / DN-14 / TC-NET-02）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub payload: Vec<u8>,
}

/// 客户端去重器：重复/乱序的可靠事件只交付一次。
#[derive(Default, Debug)]
pub struct EventDeduplicator {
    seen: Vec<EventId>,
    max_seen: usize,
}

impl EventDeduplicator {
    pub fn new(max_seen: usize) -> Self {
        Self {
            seen: Vec::new(),
            max_seen: max_seen.max(1),
        }
    }
    /// 返回 true 表示首次见到，应当处理。
    pub fn accept(&mut self, id: EventId) -> bool {
        if self.seen.contains(&id) {
            return false;
        }
        self.seen.push(id);
        if self.seen.len() > self.max_seen {
            self.seen.remove(0);
        }
        true
    }
    pub fn clear(&mut self) {
        self.seen.clear();
    }
}

// ---------------------------------------------------------------------------
// 视野策略数据（FR-60 / ARCH §8.5，判定逻辑在 game-core）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct VisibilityPolicy {
    pub vision_radius: f32,
    pub brush_reveal_radius: f32,
    pub constructs_provide_vision: bool,
    pub sentries_provide_vision: bool,
}

/// 已按玩家可见集裁剪的结果，供抓包断言（TC-NET-03）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisibilitySet {
    pub visible: Vec<u64>,
}

impl VisibilitySet {
    pub fn contains(&self, net_id: u64) -> bool {
        self.visible.contains(&net_id)
    }
}

// ---------------------------------------------------------------------------
// 会话令牌（FR-62 / FR-55 / DEV-04 / DEV-08）
// ---------------------------------------------------------------------------

pub type SessionToken = [u8; 16];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct SessionHello {
    pub token: SessionToken,
    pub hello: VersionHello,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_rejects_mismatched_protocol() {
        let r = negotiate(
            &VersionHello {
                protocol_version: 1,
                config_hash: [0; 32],
            },
            [0; 32],
        );
        assert_eq!(
            r,
            Err(RejectReason::ProtocolMismatch {
                expected: PROTOCOL_VERSION,
                got: 1
            })
        );
    }

    #[test]
    fn negotiate_rejects_mismatched_config() {
        let r = negotiate(
            &VersionHello {
                protocol_version: PROTOCOL_VERSION,
                config_hash: [1; 32],
            },
            [0; 32],
        );
        assert_eq!(
            r,
            Err(RejectReason::ConfigMismatch {
                expected: [0; 32],
                got: [1; 32]
            })
        );
    }

    #[test]
    fn diff_detects_only_changed_fields() {
        let a = EntityDelta::empty(7);
        let mut b = a.clone();
        b.health = 100.0;
        let d = diff_entity(&a, &b);
        assert_ne!(d.changed_mask & CH_HEALTH, 0);
        assert_eq!(d.changed_mask & CH_POSITION, 0);
    }

    #[test]
    fn apply_delta_restores_full_state() {
        let a = EntityDelta::empty(9);
        let mut b = a.clone();
        b.position = [1.0, 0.0, 2.0];
        b.health = 42.0;
        let d = diff_entity(&a, &b);
        let mut rebuilt = EntityDelta::empty(9);
        apply_delta(&mut rebuilt, &d);
        assert_eq!(rebuilt.position, b.position);
        assert_eq!(rebuilt.health, b.health);
    }

    #[test]
    fn event_deduplicator_rejects_retransmission() {
        let mut d = EventDeduplicator::new(8);
        assert!(d.accept(1));
        assert!(!d.accept(1));
        assert!(d.accept(2));
    }
}
