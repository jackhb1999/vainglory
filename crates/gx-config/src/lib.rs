//! GX-01 配置框架（`DEV-02` / `FR-90~92` / `NFR-07` / `ARCH §7.3`）。
//!
//! `ConfigSet` 绑定 `version` + `content_hash`；**一局对局内不可变**（`DN-25`），
//! 热更只影响新对局。所有平衡数值来自 `gx-proto-content`，代码中不得出现平衡字面量（`DN-23`）。

use gx_net::ConfigHash;
use gx_proto_content::ContentTables;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("hero `{hero}` references unknown ability `{ability}`")]
    UnknownAbility { hero: String, ability: String },
    #[error("ability `{0}` has non-positive cooldown")]
    InvalidCooldown(String),
    #[error("ability `{0}` has negative energy cost")]
    NegativeEnergyCost(String),
    #[error("ability `{0}` has unknown target_kind")]
    UnknownTargetKind(String),
    #[error("item `{0}` has zero price")]
    ZeroPrice(String),
    #[error("item edge references unknown item `{0}`")]
    UnknownItem(String),
    #[error("item tree contains a cycle at `{0}`")]
    ItemTreeCycle(String),
    #[error("objective `{0}` spawns after the hard match limit")]
    ObjectiveOutOfRange(String),
    #[error("vision radius must be positive and brush radius must not exceed it")]
    InvalidVision,
    #[error("room_size must equal team_size * 2")]
    InvalidRoomSize,
    #[error("hard_limit_ticks must be >= target_match_ticks")]
    InvalidMatchDuration,
    #[error("duplicate id `{0}`")]
    DuplicateId(String),
}

pub trait ConfigValidate {
    fn validate(&self) -> Result<(), ConfigError>;
}

#[derive(Clone, Debug)]
pub struct ConfigSet {
    pub version: String,
    pub content_hash: ConfigHash,
    pub tables: ContentTables,
}

impl ConfigSet {
    pub fn new(tables: ContentTables) -> Result<Self, ConfigError> {
        tables.validate()?;
        let version = tables.schema_version.clone();
        let content_hash = compute_hash(&tables);
        Ok(Self {
            version,
            content_hash,
            tables,
        })
    }

    /// 默认加载自研占位表（M0+M1）。
    pub fn load_default() -> Result<Self, ConfigError> {
        Self::new(ContentTables::placeholder())
    }

    /// 参与版本协商的默认配置（P-A / FR-61）：在内容表 hash 之外混入玩法内容摘要，
    /// 使「协议版本相同但商品/目标/技能表不同」的两端在握手阶段就被拒绝。
    /// `gx-config` 不能依赖 `game-core`（循环依赖），摘要由调用方显式注入。
    pub fn load_default_with_gameplay(gameplay_digest: u64) -> Result<Self, ConfigError> {
        let mut set = Self::load_default()?;
        set.content_hash = mix_gameplay_digest(set.content_hash, gameplay_digest);
        Ok(set)
    }
}

/// 把玩法内容摘要确定性混入 `ConfigHash`（非密码学，仅用于版本比对）。
pub fn mix_gameplay_digest(mut hash: ConfigHash, digest: u64) -> ConfigHash {
    for (i, b) in digest.to_le_bytes().into_iter().enumerate() {
        let idx = i % hash.len();
        hash[idx] = hash[idx].rotate_left(3) ^ b.wrapping_add(0x5b);
    }
    hash
}

/// 内容一致性 hash。**非密码学**：仅用于版本比对与按 hash 分房，不用于安全校验。
pub fn compute_hash(tables: &ContentTables) -> ConfigHash {
    let bytes = bitcode::encode(tables);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let mut out = [0u8; 32];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let v = h.rotate_left((i as u32) * 7) ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        chunk.copy_from_slice(&v.to_le_bytes()[..chunk.len()]);
    }
    out
}

impl ConfigValidate for ContentTables {
    fn validate(&self) -> Result<(), ConfigError> {
        let mut ids = HashSet::new();
        for a in &self.abilities {
            if !ids.insert(a.id.as_str()) {
                return Err(ConfigError::DuplicateId(a.id.clone()));
            }
            if a.cooldown <= 0.0 {
                return Err(ConfigError::InvalidCooldown(a.id.clone()));
            }
            if a.energy_cost < 0.0 {
                return Err(ConfigError::NegativeEnergyCost(a.id.clone()));
            }
            if !matches!(
                a.target_kind.as_str(),
                "self" | "direction" | "point" | "entity" | "projectile"
            ) {
                return Err(ConfigError::UnknownTargetKind(a.id.clone()));
            }
        }
        let ability_ids: HashSet<&str> = self.abilities.iter().map(|a| a.id.as_str()).collect();
        for h in &self.heroes {
            for id in &h.ability_ids {
                if !ability_ids.contains(id.as_str()) {
                    return Err(ConfigError::UnknownAbility {
                        hero: h.id.clone(),
                        ability: id.clone(),
                    });
                }
            }
        }

        let item_ids: HashSet<&str> = self.items.iter().map(|i| i.id.as_str()).collect();
        for i in &self.items {
            if i.price == 0 {
                return Err(ConfigError::ZeroPrice(i.id.clone()));
            }
        }
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &self.item_edges {
            if !item_ids.contains(e.from.as_str()) {
                return Err(ConfigError::UnknownItem(e.from.clone()));
            }
            if !item_ids.contains(e.to.as_str()) {
                return Err(ConfigError::UnknownItem(e.to.clone()));
            }
            edges
                .entry(e.from.as_str())
                .or_default()
                .push(e.to.as_str());
        }
        // DFS 三色标记检测合成图成环（TC-CFG-01）
        let mut state: HashMap<&str, u8> = HashMap::new();
        fn visit<'a>(
            n: &'a str,
            edges: &HashMap<&'a str, Vec<&'a str>>,
            state: &mut HashMap<&'a str, u8>,
        ) -> Result<(), ConfigError> {
            match state.get(n).copied().unwrap_or(0) {
                1 => return Err(ConfigError::ItemTreeCycle(n.to_string())),
                2 => return Ok(()),
                _ => {}
            }
            state.insert(n, 1);
            if let Some(next) = edges.get(n) {
                for &m in next {
                    visit(m, edges, state)?;
                }
            }
            state.insert(n, 2);
            Ok(())
        }
        for &n in item_ids.iter() {
            visit(n, &edges, &mut state)?;
        }

        for o in &self.objectives {
            if o.spawn_tick > self.modes.hard_limit_ticks {
                return Err(ConfigError::ObjectiveOutOfRange(o.id.clone()));
            }
        }
        if self.vision.vision_radius <= 0.0
            || self.vision.brush_reveal_radius <= 0.0
            || self.vision.brush_reveal_radius > self.vision.vision_radius
        {
            return Err(ConfigError::InvalidVision);
        }
        if self.modes.room_size != self.modes.team_size * 2 {
            return Err(ConfigError::InvalidRoomSize);
        }
        if self.modes.hard_limit_ticks < self.modes.target_match_ticks {
            return Err(ConfigError::InvalidMatchDuration);
        }
        Ok(())
    }
}

/// 运行中持有当前配置；热更只影响**新**对局，已有对局通过 hash 绑定旧配置（`DN-25`）。
#[derive(Clone, Debug)]
pub struct ActiveConfig {
    current: ConfigSet,
    /// 在局对局绑定的 hash -> 版本字符串，用于分房与审计。
    in_encounter: HashMap<ConfigHash, u32>,
    revision: u64,
}

impl ActiveConfig {
    pub fn new(config: ConfigSet) -> Self {
        Self {
            current: config,
            in_encounter: HashMap::new(),
            revision: 0,
        }
    }

    pub fn get(&self) -> &ConfigSet {
        &self.current
    }
    pub fn hash(&self) -> ConfigHash {
        self.current.content_hash
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 校验通过后替换当前配置；返回被替换的 hash 供审计。
    pub fn reload(&mut self, next: ConfigSet) -> Result<ConfigHash, ConfigError> {
        next.tables.validate()?;
        let old = self.current.content_hash;
        self.current = next;
        self.revision += 1;
        Ok(old)
    }

    /// 新对局绑定当前 hash。
    pub fn bind_encounter(&mut self, encounter_id: u64) -> ConfigHash {
        let h = self.hash();
        let _ = encounter_id;
        *self.in_encounter.entry(h).or_insert(0) += 1;
        h
    }

    pub fn release_encounter(&mut self, hash: ConfigHash) {
        if let Some(v) = self.in_encounter.get_mut(&hash) {
            *v = v.saturating_sub(1);
            if *v == 0 {
                self.in_encounter.remove(&hash);
            }
        }
    }

    pub fn active_hashes(&self) -> Vec<ConfigHash> {
        self.in_encounter.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_config_is_valid() {
        ConfigSet::load_default().expect("placeholder ConfigSet must validate");
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        let a = ContentTables::placeholder();
        let mut b = a.clone();
        let h1 = compute_hash(&a);
        assert_eq!(h1, compute_hash(&a));
        b.heroes[0].base_health += 1.0;
        assert_ne!(h1, compute_hash(&b));
    }

    #[test]
    fn gameplay_digest_is_part_of_negotiation_hash() {
        let base = ConfigSet::load_default().unwrap().content_hash;
        let a = ConfigSet::load_default_with_gameplay(1)
            .unwrap()
            .content_hash;
        let b = ConfigSet::load_default_with_gameplay(2)
            .unwrap()
            .content_hash;
        assert_ne!(base, a, "玩法内容摘要必须改变 content_hash");
        assert_ne!(a, b, "不同玩法表必须得到不同 hash");
        assert!(matches!(
            gx_net::negotiate(
                &gx_net::VersionHello {
                    protocol_version: gx_net::PROTOCOL_VERSION,
                    config_hash: b,
                },
                a,
            ),
            Err(gx_net::RejectReason::ConfigMismatch { .. })
        ));
    }

    #[test]
    fn rejects_unknown_ability_reference() {
        let mut t = ContentTables::placeholder();
        t.heroes[0].ability_ids.push("ab_missing".into());
        assert!(matches!(
            t.validate(),
            Err(ConfigError::UnknownAbility { .. })
        ));
    }

    #[test]
    fn rejects_item_tree_cycle() {
        let mut t = ContentTables::placeholder();
        t.item_edges.push(gx_proto_content::ItemEdge {
            from: "item_edge_blade".into(),
            to: "item_edge_shard".into(),
        });
        assert!(matches!(t.validate(), Err(ConfigError::ItemTreeCycle(_))));
    }

    #[test]
    fn rejects_zero_price_and_bad_cooldown() {
        let mut t = ContentTables::placeholder();
        t.items[0].price = 0;
        assert!(matches!(t.validate(), Err(ConfigError::ZeroPrice(_))));
        let mut t2 = ContentTables::placeholder();
        t2.abilities[0].cooldown = 0.0;
        assert!(matches!(
            t2.validate(),
            Err(ConfigError::InvalidCooldown(_))
        ));
    }

    #[test]
    fn reload_bumps_revision_and_keeps_bound_hash() {
        let mut active = ActiveConfig::new(ConfigSet::load_default().unwrap());
        let h0 = active.bind_encounter(1);
        let next = ConfigSet::new(ContentTables::placeholder()).unwrap();
        active.reload(next).unwrap();
        assert_eq!(active.revision(), 1);
        assert!(active.active_hashes().contains(&h0));
        active.release_encounter(h0);
        assert!(active.active_hashes().is_empty());
    }
}
