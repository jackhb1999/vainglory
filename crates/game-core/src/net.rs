//! 共享网络协议：输入、快照、事件与会话消息。
//!
//! M1-b 渐进迁移：新增协议组件在 `gx-net` 定义（无引擎依赖），本模块负责
//! 引擎类型适配并 re-export，保持既有 API 兼容（`GX-ARCH-2026-001` §4.3 M1-b）。
//! 旧协议类型的物理迁移随 M2 的 `game-core → gx-sim` 一并完成。

pub use crate::components::{NetId, Team};
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

pub use gx_net::{
    BASELINE_INTERVAL_TICKS, CH_ALL, CH_COOLDOWNS, CH_ENERGY, CH_GOLD, CH_HEALTH, CH_IDENTITY,
    CH_LEVEL, CH_MAX_ENERGY, CH_MAX_HEALTH, CH_POSITION, CH_ROTATION, CH_VELOCITY, Channel,
    ConfigHash, EntityDelta, EventDeduplicator, EventEnvelope, EventId, NetError, PROTOCOL_VERSION,
    PeerId, RejectReason, SNAPSHOT_HZ, SNAPSHOT_INTERVAL_TICKS, SessionHello, SessionToken,
    Snapshot, SnapshotHeader, Transport, VersionHello, VisibilityPolicy, VisibilitySet,
    apply_delta, diff_entity, negotiate,
};

/// 2D 向量（bitcode 友好，不依赖引擎数学类型）
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Vec2f {
    pub x: f32,
    pub y: f32,
}

impl Vec2f {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub const fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// 3D 向量
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3f {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub const fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
    pub fn to_vec3(&self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(self.x, self.y, self.z)
    }
    pub fn from_vec3(v: bevy::math::Vec3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

/// 四元数
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Quatf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quatf {
    pub const fn identity() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
    pub fn to_quat(&self) -> bevy::math::Quat {
        bevy::math::Quat::from_xyzw(self.x, self.y, self.z, self.w)
    }
    pub fn from_quat(q: bevy::math::Quat) -> Self {
        Self::new(q.x, q.y, q.z, q.w)
    }
}

pub type Vec3 = Vec3f;
pub type Quat = Quatf;

// --- 引擎适配层（M2 迁移到 gx-sim 时随类型一并下沉） ---
impl From<Vec2f> for bevy::math::Vec2 {
    fn from(v: Vec2f) -> Self {
        bevy::math::Vec2::new(v.x, v.y)
    }
}
impl From<bevy::math::Vec2> for Vec2f {
    fn from(v: bevy::math::Vec2) -> Self {
        Vec2f::new(v.x, v.y)
    }
}
impl From<Vec3f> for bevy::math::Vec3 {
    fn from(v: Vec3f) -> Self {
        bevy::math::Vec3::new(v.x, v.y, v.z)
    }
}
impl From<bevy::math::Vec3> for Vec3f {
    fn from(v: bevy::math::Vec3) -> Self {
        Vec3f::new(v.x, v.y, v.z)
    }
}
impl From<Quatf> for bevy::math::Quat {
    fn from(q: Quatf) -> Self {
        bevy::math::Quat::from_xyzw(q.x, q.y, q.z, q.w)
    }
}
impl From<bevy::math::Quat> for Quatf {
    fn from(q: bevy::math::Quat) -> Self {
        Quatf::new(q.x, q.y, q.z, q.w)
    }
}

/// 客户端发送给服务端的输入指令（上行只含输入，禁止权威状态字段，`FR-50` / `DR-02`）
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct InputCmd {
    pub tick: u64,
    /// 单调递增序号，用于去重与迟到丢弃（`FR-61` / `DEV-03`）
    pub seq: u32,
    /// 客户端时间戳（微秒），服务端回显以计算真实 RTT（`FR-56` / `DN-13`）
    pub client_time_us: u64,
    pub move_dir: Vec2f,
    pub aim_dir: Vec2f,
    pub ability_mask: u8,
    pub target_net_id: Option<u64>,
    /// 客户端测得的 RTT 估计（ms）；即 `DEV-03` 的 rtt 字段，不再写死 0（`DN-13`）
    pub ping: u32,
    /// 内容 hash，服务端校验后拒绝不匹配会话（`FR-57`）
    pub config_hash: ConfigHash,
    /// 输入校验和（序号 + tick + 方向），服务端复核（`FR-61`）
    pub checksum: u32,
}

impl InputCmd {
    /// 计算输入校验和；纯函数，客户端与服务端共用。
    pub fn compute_checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for b in self.seq.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        for b in self.tick.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        for b in self.move_dir.x.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        for b in self.move_dir.y.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        h
    }
    pub fn with_checksum(mut self) -> Self {
        self.checksum = self.compute_checksum();
        self
    }
    pub fn checksum_valid(&self) -> bool {
        self.checksum == self.compute_checksum()
    }
}

/// 服务端下发的全量世界快照。仅用于基线 / 重连 / 兼容路径；
/// 常规状态走 `ServerMessage::Snapshot` 的增量编码（`FR-52` / `DEV-06`）。
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct WorldSnapshot {
    pub tick: u64,
    pub server_time: f64,
    pub entities: Vec<EntitySnapshot>,
    pub events: Vec<GameEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct EntitySnapshot {
    pub net_id: NetId,
    pub entity_type: EntityType,
    pub team: Team,
    pub position: Vec3f,
    pub rotation: Quatf,
    pub velocity: Vec3f,
    pub health: f32,
    pub max_health: f32,
    pub energy: f32,
    pub max_energy: f32,
    pub level: u8,
    pub gold: u32,
    pub ability_cooldowns: [f32; 4],
    /// `PROTOCOL_VERSION = 3`：状态效果位掩码（bit0..7 对应 8 类状态）。
    pub status_mask: u32,
    /// `PROTOCOL_VERSION = 3`：护盾池当前值。
    pub shield: f32,
}

impl EntitySnapshot {
    /// 转换为 `gx-net` 增量实体（服务端编码用）。
    pub fn to_delta(&self) -> EntityDelta {
        EntityDelta {
            net_id: self.net_id.0,
            generation: 0,
            changed_mask: CH_ALL,
            entity_type: self.entity_type as u8,
            team: match self.team {
                Team::Blue => 0,
                Team::Red => 1,
                Team::Neutral => 2,
            },
            position: [self.position.x, self.position.y, self.position.z],
            rotation: [
                self.rotation.x,
                self.rotation.y,
                self.rotation.z,
                self.rotation.w,
            ],
            velocity: [self.velocity.x, self.velocity.y, self.velocity.z],
            health: self.health,
            max_health: self.max_health,
            energy: self.energy,
            max_energy: self.max_energy,
            level: self.level,
            gold: self.gold,
            ability_cooldowns: self.ability_cooldowns,
            status_mask: self.status_mask,
            shield: self.shield,
        }
    }

    /// 从完整增量重建（客户端 delta 合并用）。
    pub fn from_delta(d: &EntityDelta) -> Self {
        Self {
            net_id: NetId(d.net_id),
            entity_type: entity_type_from_u8(d.entity_type),
            team: team_from_u8(d.team),
            position: Vec3f::new(d.position[0], d.position[1], d.position[2]),
            rotation: Quatf::new(d.rotation[0], d.rotation[1], d.rotation[2], d.rotation[3]),
            velocity: Vec3f::new(d.velocity[0], d.velocity[1], d.velocity[2]),
            health: d.health,
            max_health: d.max_health,
            energy: d.energy,
            max_energy: d.max_energy,
            level: d.level,
            gold: d.gold,
            ability_cooldowns: d.ability_cooldowns,
            status_mask: d.status_mask,
            shield: d.shield,
        }
    }
}

pub fn entity_type_from_u8(v: u8) -> EntityType {
    match v {
        1 => EntityType::Minion,
        2 => EntityType::Turret,
        3 => EntityType::JungleMonster,
        4 => EntityType::Projectile,
        5 => EntityType::Ward,
        _ => EntityType::Hero,
    }
}
pub fn team_from_u8(v: u8) -> Team {
    match v {
        0 => Team::Blue,
        1 => Team::Red,
        _ => Team::Neutral,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum EntityType {
    Hero = 0,
    Minion = 1,
    Turret = 2,
    JungleMonster = 3,
    Projectile = 4,
    Ward = 5,
}

/// 游戏事件（可靠有序通道，配合 `EventEnvelope` 幂等）
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum GameEvent {
    HeroSpawn {
        net_id: NetId,
        team: Team,
        hero_name: String,
        position: Vec3f,
        /// 外观皮肤编号，客户端据此区分 voxel 模型（与数值无关）。
        skin: u8,
    },
    HeroDespawn {
        net_id: NetId,
    },
    HeroDeath {
        victim: NetId,
        killer: Option<NetId>,
        assist: Vec<NetId>,
        /// 协议 v3：击杀赏金（含连杀加成）。
        bounty: Option<u32>,
    },
    HeroLevelUp {
        net_id: NetId,
        new_level: u8,
    },
    HeroGoldChanged {
        net_id: NetId,
        new_gold: u32,
    },
    HeroItemPurchased {
        net_id: NetId,
        item_id: u32,
    },
    AbilityCast {
        caster: NetId,
        ability_index: u8,
        target_position: Vec3f,
        target_entity: Option<NetId>,
    },
    DamageDealt {
        attacker: NetId,
        target: NetId,
        damage: f32,
        is_critical: bool,
    },
    MinionSpawn {
        net_id: NetId,
        team: Team,
        minion_type: MinionType,
        position: Vec3f,
    },
    MinionDeath {
        net_id: NetId,
        killer: Option<NetId>,
    },
    TurretDestroyed {
        net_id: NetId,
        destroyer: NetId,
        team: Team,
    },
    JungleMonsterSpawn {
        net_id: NetId,
        monster_type: JungleMonsterType,
        position: Vec3f,
    },
    JungleMonsterDeath {
        net_id: NetId,
        killer: NetId,
        team: Team,
    },
    // --- 原创术语（CR-01）---
    AbyssWardenSpawn {
        position: Vec3f,
    },
    AbyssWardenDeath {
        killer_team: Team,
    },
    LodeShrineCapture {
        team: Team,
    },
    MatchStart {
        blue_heroes: Vec<(NetId, String)>,
        red_heroes: Vec<(NetId, String)>,
    },
    MatchEnd {
        winning_team: Team,
    },
    Pause {
        reason: String,
    },
    Resume,
    Ping {
        from: NetId,
        position: Vec3f,
        ping_type: PingType,
    },
    // --- M1-b 新增（DEV-03 / ARCH §8.1）---
    ObjectiveCaptured {
        objective_id: String,
        owner: Team,
        event_id: EventId,
    },
    SourceCoreDestroyed {
        team: Team,
        event_id: EventId,
    },
    EncounterSettled {
        winning_team: Team,
        duration_ticks: u64,
        event_id: EventId,
    },
    ReconnectState {
        net_id: NetId,
        token: SessionToken,
    },
    ConfigMismatch {
        expected: ConfigHash,
        got: ConfigHash,
    },
    // --- PROTOCOL_VERSION = 3：玩法事件（由 `GameplayEvents` adapter 广播）---
    /// 购买成功回执；`item_id` 来自 `gameplay::registry` 的稳定 u32 id。
    ItemPurchased {
        net_id: NetId,
        item_id: u32,
        paid: u32,
        event_id: EventId,
    },
    /// 售出成功回执。
    ItemSold {
        net_id: NetId,
        item_id: u32,
        refund: u32,
        event_id: EventId,
    },
    /// 购买/售出被拒；携带服务端裁决的拒绝原因，客户端据此置灰原因提示。
    ShopRejected {
        net_id: NetId,
        item_id: u32,
        reason: u8,
        event_id: EventId,
    },
    /// 中立目标 buff 发放（§13）：不含任何实体字段，可全局广播。
    ObjectiveBuffGranted {
        objective_id: u32,
        owner: Team,
        buff_id: u32,
        event_id: EventId,
    },
    /// 状态效果生效（客户端只需种类与时长显示图标）。
    StatusApplied {
        net_id: NetId,
        kind: u8,
        duration_ticks: u32,
        event_id: EventId,
    },
    /// 护盾值变化（同时经 `CH_SHIELD` 快照下发，此事件用于特效触发）。
    ShieldChanged {
        net_id: NetId,
        value: f32,
        event_id: EventId,
    },
    /// 兵线波次生成；全局节奏事件，不携带实体字段。
    WaveSpawned {
        wave_index: u64,
        event_id: EventId,
    },
    /// 加时衰竭开启。
    OvertimeStarted {
        event_id: EventId,
    },
    /// 带原因与时长的对局结束事件。
    MatchFinished {
        winning_team: Team,
        reason: u8,
        duration_ticks: u64,
        event_id: EventId,
    },
}

impl GameEvent {
    /// 事件涉及的实体（用于按接收方可见集过滤，VIS-7 / `FR-60`）。
    pub fn involved_net_ids(&self) -> Vec<u64> {
        match self {
            GameEvent::HeroSpawn { net_id, .. }
            | GameEvent::HeroDespawn { net_id }
            | GameEvent::HeroLevelUp { net_id, .. }
            | GameEvent::HeroGoldChanged { net_id, .. }
            | GameEvent::HeroItemPurchased { net_id, .. }
            | GameEvent::MinionSpawn { net_id, .. }
            | GameEvent::MinionDeath { net_id, .. }
            | GameEvent::TurretDestroyed { net_id, .. }
            | GameEvent::JungleMonsterSpawn { net_id, .. }
            | GameEvent::ReconnectState { net_id, .. }
            | GameEvent::ItemPurchased { net_id, .. }
            | GameEvent::ItemSold { net_id, .. }
            | GameEvent::ShopRejected { net_id, .. }
            | GameEvent::StatusApplied { net_id, .. }
            | GameEvent::ShieldChanged { net_id, .. } => vec![net_id.0],
            GameEvent::HeroDeath {
                victim,
                killer,
                assist,
                ..
            } => {
                let mut v = vec![victim.0];
                if let Some(k) = killer {
                    v.push(k.0);
                }
                v.extend(assist.iter().map(|n| n.0));
                v
            }
            GameEvent::AbilityCast {
                caster,
                target_entity,
                ..
            } => {
                let mut v = vec![caster.0];
                if let Some(t) = target_entity {
                    v.push(t.0);
                }
                v
            }
            GameEvent::DamageDealt {
                attacker, target, ..
            } => vec![attacker.0, target.0],
            GameEvent::JungleMonsterDeath { net_id, killer, .. } => vec![net_id.0, killer.0],
            GameEvent::Ping { from, .. } => vec![from.0],
            _ => Vec::new(),
        }
    }

    /// 是否为全局游戏状态事件（VIS-6：可全局广播，但不得携带敌方实体字段）。
    pub fn is_global(&self) -> bool {
        matches!(
            self,
            GameEvent::ObjectiveCaptured { .. }
                | GameEvent::SourceCoreDestroyed { .. }
                | GameEvent::EncounterSettled { .. }
                | GameEvent::MatchStart { .. }
                | GameEvent::MatchEnd { .. }
                | GameEvent::LodeShrineCapture { .. }
                | GameEvent::AbyssWardenSpawn { .. }
                | GameEvent::AbyssWardenDeath { .. }
                | GameEvent::ConfigMismatch { .. }
                | GameEvent::Pause { .. }
                | GameEvent::Resume
                | GameEvent::ObjectiveBuffGranted { .. }
                | GameEvent::WaveSpawned { .. }
                | GameEvent::OvertimeStarted { .. }
                | GameEvent::MatchFinished { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum MinionType {
    Melee,
    Ranged,
    Siege,
    Super,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum JungleMonsterType {
    MusterWell,
    LodeShrine,
    AbyssWarden,
    Treant,
    CoreWarden,
    GildedToad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum PingType {
    Normal,
    Danger,
    OnMyWay,
    AssistMe,
}

/// 客户端 -> 服务端消息
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum ClientMessage {
    Input(InputCmd),
    /// `item_id` 是 `gameplay::registry::stable_id` 产生的稳定 u32，服务端反查商品名。
    PurchaseItem {
        item_id: u32,
    },
    /// 售出 6 格栏位中的装备（服务端重新核算返还，不信任客户端金额）。
    SellItem {
        slot: u8,
    },
    UpgradeAbility {
        ability_index: u8,
    },
    Chat {
        message: String,
    },
    Ping {
        position: Vec3f,
        ping_type: PingType,
    },
    SurrenderVote,
    RequestPause,
    // --- M1-b 新增（FR-57 / FR-55 / DEV-08）---
    Hello(SessionHello),
    ReconnectRequest {
        token: SessionToken,
        hello: VersionHello,
    },
    Ack {
        baseline_tick: u64,
    },
    ClientMetrics {
        rtt_ms: u16,
        dropped_inputs: u16,
    },
    /// 大厅模式选择：`MatchMode::code()` 编码（0 = PVE，1 = PVP）。
    /// 服务端只在开局前（`MatchClock.tick == 0`）接受，局中一律忽略。
    SelectMode {
        mode: u8,
    },
}

/// 服务端 -> 客户端消息
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum ServerMessage {
    WorldSnapshot(WorldSnapshot),
    Event(GameEvent),
    /// 幂等关键事件包裹（`FR-53` / `DN-14`）
    ReliableEvent(EventEnvelope),
    /// 增量状态快照（Unreliable 通道，`FR-52`）
    Snapshot(Snapshot),
    Welcome {
        your_net_id: NetId,
        assigned_team: Team,
        match_id: u64,
        server_tick: u64,
        session_token: SessionToken,
    },
    MatchStateChange {
        state: MatchState,
    },
    Error {
        code: u16,
        message: String,
    },
    // --- M1-b 新增（FR-57 / FR-55）---
    ConfigSetOffer {
        version: String,
        config_hash: ConfigHash,
    },
    Reject {
        reason: RejectReason,
    },
    SessionResume {
        net_id: NetId,
        team: Team,
        snapshot: WorldSnapshot,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum MatchState {
    Lobby,
    Loading,
    InProgress,
    Paused,
    Finished,
}

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct ConnectionInfo {
    pub client_id: u64,
    pub protocol_version: u32,
    pub supported_features: Vec<String>,
}

pub mod serialization {
    use super::*;
    pub fn encode<T: Encode>(value: &T) -> Vec<u8> {
        bitcode::encode(value)
    }
    pub fn decode<'a, T: Decode<'a>>(bytes: &'a [u8]) -> Result<T, bitcode::Error> {
        bitcode::decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-U1：大厅模式选择变体必须能被 bitcode 正确往返，
    /// 否则服务端 `handle_session_messages` 会把它当 decode 失败静默丢弃。
    #[test]
    fn select_mode_message_roundtrips() {
        for mode in [0_u8, 1, 255] {
            let msg = ClientMessage::SelectMode { mode };
            let decoded: ClientMessage =
                serialization::decode(&serialization::encode(&msg)).unwrap();
            match decoded {
                ClientMessage::SelectMode { mode: got } => assert_eq!(got, mode),
                _ => panic!("SelectMode decoded into the wrong variant"),
            }
        }
    }

    fn sample_cmd() -> InputCmd {
        InputCmd {
            tick: 100,
            seq: 7,
            client_time_us: 1_000_000,
            move_dir: Vec2f::new(1.0, 0.0),
            aim_dir: Vec2f::new(0.0, 1.0),
            ability_mask: 0b0001,
            target_net_id: None,
            ping: 42,
            config_hash: [0; 32],
            checksum: 0,
        }
        .with_checksum()
    }

    #[test]
    fn test_input_cmd_serialization() {
        let cmd = sample_cmd();
        let decoded: InputCmd = serialization::decode(&serialization::encode(&cmd)).unwrap();
        assert_eq!(cmd.tick, decoded.tick);
        assert_eq!(cmd.seq, decoded.seq);
        assert_eq!(cmd.move_dir, decoded.move_dir);
        assert!(decoded.checksum_valid());
    }

    #[test]
    fn checksum_detects_tampering() {
        let mut cmd = sample_cmd();
        assert!(cmd.checksum_valid());
        cmd.seq += 1;
        assert!(!cmd.checksum_valid());
    }

    #[test]
    fn test_world_snapshot_serialization() {
        let snapshot = WorldSnapshot {
            tick: 1000,
            server_time: 16.67,
            entities: vec![EntitySnapshot {
                net_id: NetId(1),
                entity_type: EntityType::Hero,
                team: Team::Blue,
                position: Vec3f::zero(),
                rotation: Quatf::identity(),
                velocity: Vec3f::zero(),
                health: 1000.0,
                max_health: 1000.0,
                energy: 500.0,
                max_energy: 500.0,
                level: 1,
                gold: 0,
                ability_cooldowns: [0.0; 4],
                status_mask: 0,
                shield: 0.0,
            }],
            events: vec![],
        };
        let decoded: WorldSnapshot =
            serialization::decode(&serialization::encode(&snapshot)).unwrap();
        assert_eq!(snapshot.tick, decoded.tick);
        assert_eq!(snapshot.entities.len(), decoded.entities.len());
    }

    #[test]
    fn entity_delta_roundtrip() {
        let es = EntitySnapshot {
            net_id: NetId(9),
            entity_type: EntityType::Minion,
            team: Team::Red,
            position: Vec3f::new(1.0, 2.0, 3.0),
            rotation: Quatf::identity(),
            velocity: Vec3f::zero(),
            health: 25.0,
            max_health: 30.0,
            energy: 0.0,
            max_energy: 0.0,
            level: 2,
            gold: 99,
            ability_cooldowns: [1.0, 0.0, 3.0, 0.0],
            status_mask: 0b0000_0011,
            shield: 125.0,
        };
        let rebuilt = EntitySnapshot::from_delta(&es.to_delta());
        assert_eq!(rebuilt.net_id.0, 9);
        assert_eq!(rebuilt.position, es.position);
        assert_eq!(rebuilt.gold, es.gold);
        assert_eq!(rebuilt.team, Team::Red);
        assert_eq!(rebuilt.status_mask, es.status_mask);
        assert_eq!(rebuilt.shield, es.shield);
    }
}
