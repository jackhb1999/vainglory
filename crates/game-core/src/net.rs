//! 网络协议定义：InputCmd, WorldSnapshot, Event

use crate::components::{NetId, Team};
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// 2D 向量 (bitcode 友好, 不依赖 Bevy/glam)
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

/// 3D 向量 (bitcode 友好, 不依赖 Bevy/glam)
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
        Self { x: 0.0, y: 0.0, z: 0.0 }
    }
}

/// 四元数 (bitcode 友好, 不依赖 Bevy/glam)
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Quatf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quatf {
    pub const fn identity() -> Self {
        Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

/// 原有 Vec3f 结构保持向后兼容
pub type Vec3 = Vec3f;

/// 四元数类型别名
pub type Quat = Quatf;

// 与 Bevy 数学类型的双向转换（网络层用 bitcode 友好的 `*f` 类型，渲染/逻辑层用 bevy 类型）
impl Vec2f {
    pub fn to_vec2(&self) -> bevy::math::Vec2 {
        bevy::math::Vec2::new(self.x, self.y)
    }
    pub fn from_vec2(v: bevy::math::Vec2) -> Self {
        Self::new(v.x, v.y)
    }
}

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

impl Vec3f {
    pub fn to_vec3(&self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(self.x, self.y, self.z)
    }
    pub fn from_vec3(v: bevy::math::Vec3) -> Self {
        Self::new(v.x, v.y, v.z)
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

impl Quatf {
    pub fn to_quat(&self) -> bevy::math::Quat {
        bevy::math::Quat::from_xyzw(self.x, self.y, self.z, self.w)
    }
    pub fn from_quat(q: bevy::math::Quat) -> Self {
        Self::new(q.x, q.y, q.z, q.w)
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

/// 客户端发送给服务端的输入指令
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct InputCmd {
    pub tick: u64,
    pub move_dir: Vec2f,
    pub aim_dir: Vec2f,
    pub ability_mask: u8,
    pub ping: u32, // 客户端测量的 RTT
}

/// 服务端下发的世界快照
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct WorldSnapshot {
    pub tick: u64,
    pub server_time: f64,
    pub entities: Vec<EntitySnapshot>,
    pub events: Vec<GameEvent>,
}

/// 单个实体的快照数据
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum EntityType {
    Hero,
    Minion,
    Turret,
    JungleMonster,
    Projectile,
    Ward,
}

/// 游戏事件（可靠通道）
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum GameEvent {
    HeroSpawn { net_id: NetId, team: Team, hero_name: String, position: Vec3f },
    HeroDespawn { net_id: NetId },
    HeroDeath { victim: NetId, killer: Option<NetId>, assist: Vec<NetId> },
    HeroLevelUp { net_id: NetId, new_level: u8 },
    HeroGoldChanged { net_id: NetId, new_gold: u32 },
    HeroItemPurchased { net_id: NetId, item_id: u32 },
    AbilityCast { caster: NetId, ability_index: u8, target_position: Vec3f, target_entity: Option<NetId> },
    DamageDealt { attacker: NetId, target: NetId, damage: f32, is_critical: bool },
    MinionSpawn { net_id: NetId, team: Team, minion_type: MinionType, position: Vec3f },
    MinionDeath { net_id: NetId, killer: Option<NetId> },
    TurretDestroyed { net_id: NetId, destroyer: NetId, team: Team },
    JungleMonsterSpawn { net_id: NetId, monster_type: JungleMonsterType, position: Vec3f },
    JungleMonsterDeath { net_id: NetId, killer: NetId, team: Team },
    KrakenSpawn { position: Vec3f },
    KrakenDeath { killer_team: Team },
    GoldMineCapture { team: Team },
    MatchStart { blue_heroes: Vec<(NetId, String)>, red_heroes: Vec<(NetId, String)> },
    MatchEnd { winning_team: Team },
    Pause { reason: String },
    Resume,
    Ping { from: NetId, position: Vec3f, ping_type: PingType },
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
    MinionMine,
    GoldMine,
    Kraken,
    Treant,
    CrystalSentry,
    GoldToad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum PingType {
    Normal,
    Danger,
    OnMyWay,
    AssistMe,
}

/// 客户端 -> 服务端可靠消息
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum ClientMessage {
    Input(InputCmd),
    PurchaseItem { item_id: u32 },
    UpgradeAbility { ability_index: u8 },
    Chat { message: String },
    Ping { position: Vec3f, ping_type: PingType },
    SurrenderVote,
    RequestPause,
}

/// 服务端 -> 客户端可靠消息
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub enum ServerMessage {
    WorldSnapshot(WorldSnapshot),
    Event(GameEvent),
    Welcome { your_net_id: NetId, assigned_team: Team, match_id: u64, server_tick: u64 },
    MatchStateChange { state: MatchState },
    Error { code: u16, message: String },
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

/// 连接信息
#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct ConnectionInfo {
    pub client_id: u64,
    pub protocol_version: u32,
    pub supported_features: Vec<String>,
}

pub const PROTOCOL_VERSION: u32 = 1;

/// 序列化工具
pub mod serialization {
    use super::*;
    use bitcode;

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

    #[test]
    fn test_input_cmd_serialization() {
        let cmd = InputCmd {
            tick: 100,
            move_dir: Vec2f::new(1.0, 0.0),
            aim_dir: Vec2f::new(0.0, 1.0),
            ability_mask: 0b0001,
            ping: 50,
        };
        
        let encoded = serialization::encode(&cmd);
        let decoded: InputCmd = serialization::decode(&encoded).unwrap();
        
        assert_eq!(cmd.tick, decoded.tick);
        assert_eq!(cmd.move_dir, decoded.move_dir);
        assert_eq!(cmd.aim_dir, decoded.aim_dir);
        assert_eq!(cmd.ability_mask, decoded.ability_mask);
    }
    
    #[test]
    fn test_world_snapshot_serialization() {
        let snapshot = WorldSnapshot {
            tick: 1000,
            server_time: 16.67,
            entities: vec![
                EntitySnapshot {
                    net_id: NetId(1),
                    entity_type: EntityType::Hero,
                    team: Team::Blue,
                    position: Vec3f::new(0.0, 0.0, 0.0),
                    rotation: Quatf::identity(),
                    velocity: Vec3f::zero(),
                    health: 1000.0,
                    max_health: 1000.0,
                    energy: 500.0,
                    max_energy: 500.0,
                    level: 1,
                    gold: 0,
                    ability_cooldowns: [0.0, 0.0, 0.0, 0.0],
                }
            ],
            events: vec![],
        };
        
        let encoded = serialization::encode(&snapshot);
        let decoded: WorldSnapshot = serialization::decode(&encoded).unwrap();
        
        assert_eq!(snapshot.tick, decoded.tick);
        assert_eq!(snapshot.entities.len(), decoded.entities.len());
    }
}