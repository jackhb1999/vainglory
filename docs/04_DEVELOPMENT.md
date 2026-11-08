# GX-01 开发实施规范（对应需求文档）

| 项 | 内容 |
|---|---|
| 文档编号 | `GX-DEV-2026-001` |
| 版本 / 日期 | v1.1 / 2026-11-07 |
| 上游 | `GX-TF-2026-001`（闸门与预算）、`GX-ARCH-2026-001`（架构与 DN）、`GX-PRD-2026-001`（需求基线）、`GX-CBT-2026-001`（对战详细规格） |
| 对应 | 本文档是 `GX-PRD-2026-001` 的**实现侧文档**：每条 `FR/NFR/CR` 均可经本文档的 `DEV` 任务与 `TC` 用例反向追溯 |
| 适用对象 | 客户端 / 服务端 / 后端 / 工具链工程师、QA、技术美术、技术策划 |

> **v1.1 范围变更**：按本轮新指令，玩法逻辑闭环（`DEV-12~16` 的核心规则部分）已由 M2 提前落地，
> 接口与系统链见 `GX-CBT-2026-001` §22，逐条剩余差距见其 §23，寻路 M2 待办规格见 §24。
> 本次交付边界、闸门与排期的**正式修订仍以 `GX-PRD-2026-001` §11.1 为准**；本文件未擅自改动 `G0/G1/G2` 定义。

---

## 0. 如何使用本文档族

```
VG-FS-2026-001  可行性研究（为什么做 L2、法律与商业边界）
      ↓
GX-TF-2026-001  技术可行性（做不做得成、量化阈值、闸门 G0~G4）
      ↓
GX-ARCH-2026-001 架构（怎么切分、ADR、40 条开发注意项 DN）
      ↓
GX-PRD-2026-001 需求（要交付什么、验收标准）  ←→  GX-DEV-2026-001（本文档：怎么实现、怎么测）
```

**追溯规则**：提交信息、PR、测试用例、发布清单都必须带 ID。任一 `DEV` 任务必须能回答"覆盖哪些 `FR`、由哪个 `TC` 验证、落在哪个 `G` 闸门"。§12 给出反向追溯矩阵（DEV/TC → FR）。

---

## 1. 开发环境与工具链

| 项 | 要求 | 关联 |
|---|---|---|
| Rust | edition 2024，用 `rust-toolchain.toml` 固定版本（≥1.85），CI 与本地一致 | DN-37 / NFR-09 |
| 依赖锁定 | 提交 `Cargo.lock`；禁止在 CI 里 `cargo update` | DN-36 / NFR-09 |
| Workspace | 根 `Cargo.toml` 用 `[workspace.dependencies]` 统一版本；各 crate 用 `workspace = true` | 现状已部分如此 |
| 格式化 / Lint | `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` | CI |
| 本地一键调试 | M0+M1：`cargo run -p client --features offline-dev` + `cargo run -p server --features offline-dev`；M2 后为目标名 `gx-client` / `gx-battle`；**无需任何后端服务** | AS-05 / DN-21 / DN-40 |
| 日志 | `tracing` + `RUST_LOG`；结构化字段 `encounter_id/player_id/tick/event_id`；禁止 `println!` 调试残留 | DN-10 |
| 网络损伤测试 | 本地代理可注入 RTT / 抖动 / 丢包（150ms / 50ms / 3% 为基线档） | TF-04 / TC-PRED-01 |
| 压测 | headless `gx-battle` + 合成客户端，产出 tick P95、内存、带宽报告 | TF-01 / TC-PERF-01 |
| 机型 | M0 确定目标机型清单（低/中/高三档），作为 NFR-02/08 校准依据 | AS-02 |

**Feature 约定**：
| feature | 用途 | 发布构建 |
|---|---|---|
| `offline-dev` | 1 人开局、无认证、回环地址、egui 调试面板 | **必须禁用** |
| `dev-tools` | egui 调试面板、协议检查器、回放查看器 | **必须禁用** |
| `telemetry` | 遥测上报 | 启用 |

---

## 2. 代码组织与演进式迁移路线

> **红线（AS-06 / ADR-01）**：不推倒重写。每个阶段结束都必须满足 `cargo test --workspace` 通过、`offline-dev` 可一键启动、且现有 E2E（连接→开局→移动→技能命中→掉血）不回归。

### 2.1 目标 crate 结构（与 `GX-ARCH-2026-001` §4.2 一致）

```
crates/
  gx-sim/            # 仅 bevy_ecs / bevy_math；服务端·预测·回放·平衡模拟共用
  gx-net/            # 协议、编解码、Transport trait
  gx-config/         # ConfigSet 定义、加载、校验、版本
  gx-proto-content/  # 纯数据：heroes/abilities/items/map/economy/objectives/modes
  gx-client/         # 完整 Bevy：渲染、输入、预测、产品 UI
  gx-battle/         # headless 权威服
  gx-gateway/        # 接入、认证、限流
  gx-matchmaker/     # 匹配、编排
  gx-platform/       # 账号/实名/支付/配置中心/运营后台
  gx-tools/          # 回放、平衡模拟、资产 lint、协议兼容
```

> **M0+M1 命名约定（经澄清 Q-11）**：现有 4 个 crate（`game-core` / `client` / `server` / `matchmaking`）名称**保持不变**，仅修改对外二进制名与 `description`；M1 新增的组件 crate 采用 `gx-*` 命名，完整重命名与 `game-core → gx-sim` 合并到 M2 阶段。因此 M1 的一键调试命令仍为 `cargo run -p client --features offline-dev` 与 `cargo run -p server --features offline-dev`（§1 中的 `gx-client`/`gx-battle` 为 M2 后的目标名）。

### 2.2 迁移阶段（每阶段有独立验收）

| 阶段 | 动作 | 对应 DEV | 阶段验收（不可跳过） |
|---|---|---|---|
| **M0 合规与技术基线** | 命名/描述/二进制名清理；第三方数据隔离到 `LEGAL/`；确认 `Cargo.lock` 已跟踪、补 `rust-toolchain.toml`；CI 骨架；禁词与资产门禁；临时诊断日志清理；法律意见归档（外部依赖） | DEV-01、DEV-26、DEV-29 | **G0**：禁词 0 命中、资产登记 100%、可复现构建；律师意见由委托方提供 |
| **M1-a 工程清理** | 移除死依赖（`bevy_renet2`/`glam`/`redis`）；解除 `client → server` 反向依赖；`interpolation` 改 `pub mod` 并注册 `InterpolationPlugin`；`ClientPlugin` 处置；README 与代码对齐 | DEV-07（部分）、DEV-29 | 全测试通过；新增"远端实体被插值"集成测试；`-D warnings` 清零 |
| **M1-b 协议抽离** | `net.rs` → `gx-net`；删除协议中对 `bevy::math` 的直接 `From` 实现，改适配层；`game-core` 先 re-export 保持兼容；引入 `seq/checksum` 与版本协商 | DEV-03、DEV-04 | 协议单测与现有 E2E 通过；`TC-COMPAT-01` 通过 |
| **M1-c 配置落地** | `gx-config` + `gx-proto-content`；先迁"技能数值 / 英雄基础值 / 哨塔与源核血量"三类硬编码；配置校验器接入 CI | DEV-02、DEV-10（准备） | `TC-CFG-01` 通过；代码中平衡字面量清零检查 |
| **M1-d 网络加固** | 服务端视野裁剪；delta + 通道分级；预测/插值闭环；真实 RTT；状态恢复式重连 | DEV-05、DEV-06、DEV-07、DEV-08 | **G1**：`TC-NET-01/02/03/04`、`TC-PRED-01` 全部通过；抓包零视野外信息（TF-01 已按澄清 Q-03 移出 G1） |
| **M2 垂直切片** | 兵线/营地/占领目标/哨塔/源核/胜负；3 英雄完整技能；经济与装备；**鼠标点触自动寻路输入（无键盘移动）**；基础局内 UI；回放；**TF-01 容量压测（自 G1 移入）**与成本实测 | DEV-09~DEV-16、DEV-22、DEV-23、DEV-27、DEV-30 | **G2**：`TC-SIM-01~05`、`TC-CBT-01~11` 与 `TC-PERF-01` 通过；产出 `cost_per_match` 实测值 |
| **M3 上线就绪** | 账号实名防沉迷、支付、匹配编排、运营后台、反作弊、局外 UI、本地化、机型适配、CI/CD 完整流水线 | DEV-17~DEV-21、DEV-24、DEV-25、DEV-28 | **G3**：全部 P1 + CR + NFR 达标，合规验收签字 |
| **M4 灰度** | 小范围放量、单位经济复核、SLO 观察 | — | **G4**：LTV > 获客成本 |

### 2.3 阶段间的兼容策略
- `game-core` → `gx-sim` 迁移期间用 re-export 保持 API 兼容，**禁止同时维护两套逻辑**；
- 协议与配置双版本号（`PROTOCOL_VERSION` + `config_hash`），新旧客户端按 hash 分房（DN-25 / TR-06）；
- `gx-client` 与 `gx-battle` 共享 `gx-sim`，移动/技能常量**只允许**从 `ConfigSet` 读取（DN-09）。

---

## 3. 关键接口约定（示意，最终 API 以代码为准）

> 以下签名为设计约束的表达，非可直接编译的最终代码。所有 ID 与 `GX-ARCH-2026-001` §5/§7/§8 对应。

```rust
// --- gx-config (ARC-10 / DEV-02) ---
pub struct ConfigSet {
    pub version: semver::Version,
    pub content_hash: [u8; 32],
    pub tables: ConfigTables,          // heroes/abilities/items/... 见架构 §7.3
}
pub trait ConfigValidate { fn validate(&self) -> Result<(), ConfigError>; }
// 一局对局内 ConfigSet 不可变；热更只影响新对局 (DN-25)

// --- gx-net (ARC-04 / DEV-03) ---
pub trait Transport: Send + Sync {
    fn send(&mut self, peer: PeerId, packet: &Packet) -> Result<(), NetError>;
    fn recv(&mut self) -> impl Iterator<Item = (PeerId, Packet)> + '_;
}
pub struct InputCmd { pub seq: u32, pub tick: u64, pub client_time_us: u64,
                      pub intent: Intent, pub config_hash: [u8; 32],
                      pub rtt_ms: u16, pub checksum: u32 }
pub struct Snapshot { pub header: SnapshotHeader, pub deltas: Vec<EntityDelta>,
                      pub baseline: Option<BaselineSnapshot> }
// 上行不得含 health/gold/level 等权威字段 (FR-50 / DR-02)

// --- gx-sim (ARC-06 / DEV-12~15) ---
pub struct EncounterWorld { /* ECS World + ConfigSet + ServerTick */ }
impl EncounterWorld {
    pub fn tick(&mut self, inputs: &[ValidatedInput]) -> TickOutcome;
}
// 服务端、客户端预测、回放、平衡模拟调用同一 tick 实现 (ADR-02/03)

// --- 视野 (ARC-07 / DEV-05) ---
pub fn visibility_set(world: &World, player: PlayerId) -> VisibilitySet;
// 必须在序列化之前调用；裁剪覆盖快照/事件/小地图/统计 (FR-60 / DN-12)

// --- 目标状态机 (ARC-08 / DEV-13) ---
pub struct CaptureObjective { pub owner: Option<Team>, pub progress: u16,
                              pub reward: RewardRef, pub respawn_tick: u64 }

// --- 回放 (ARC-13 / DEV-22) ---
pub struct ReplayWriter { /* inputs + baselines + config_hash + events */ }
pub fn verify_replay(path: &Path) -> Result<ReplayDiff, ReplayError>;
```

**接口不变量**：
1. `EncounterWorld::tick` 不得读取系统时钟（DN-27）；
2. 所有伤害写入 `DamageSink`，由 `flush_damage_events` 统一发事件（架构 §6.1 顺序不变量 2）；
3. 未知 id 一律返回错误而非占位（DN-26 / FR-26）；
4. `NetId` 与 `Entity` 的映射仅服务端维护（架构 §7.2 / DN-29 相关）；
5. 每 tick 分配受控，复用缓冲（DN-28）。

---

## 4. 开发任务卡（DEV-01 ~ DEV-30）

> 列说明：**覆盖** = 需求文档 FR/NFR/CR/UC；**阶段** = §2.2；**要点** = 实现约束（含 DN/ADR）；**验收** = TC 用例或闸门。所有任务的通用 DoD 见 §9。

### 4.1 基线与平台（DEV-01~08）

| ID | 任务 | 覆盖 | 阶段 | 关键实现要点 | 验收 |
|---|---|---|---|---|---|
| DEV-01 | 工程基线与合规清理 | CR-01、NFR-09、DN-29/30/36/37 | M0 | 对外二进制名与描述去关联（**crate 名保持不变**）；第三方数据移入 `LEGAL/` 并由发布流水线排除；确认 `Cargo.lock` 已跟踪、新增 `rust-toolchain.toml`；清理 `[DMG]/[SNAP]/...` 临时日志 | **G0**（技术侧）；`TC-COMPLIANCE-01/02` |
| DEV-02 | `gx-config` 配置框架 | FR-90/91/92、NFR-07 | M1-c | `ConfigSet{version,hash,tables}`；启动/CI 校验器；热更 API；一局内版本锁定；静态检查"代码中无平衡字面量" | `TC-CFG-01/02` |
| DEV-03 | `gx-net` 协议演进 | FR-50/56/57、NFR-03 | M1-b | `InputCmd` 加 `seq/checksum/rtt/config_hash`；`Snapshot` 改基线+delta；`GameEvent` 补目标/结算/重连/配置不匹配事件；双版本协商 | `TC-NET-01`、`TC-COMPAT-01` |
| DEV-04 | `Transport` 抽象与网关 | FR-62、CR-05 | M1-b / M3 | `Transport` trait；renet2_netcode 为默认实现；令牌校验替换 `Unsecure`；加密、连接/包速率限制；对外令牌地址由网关下发 | `TC-SEC-01`；压测限流生效 |
| DEV-05 | **服务端视野裁剪** | FR-60、FR-81、CR-05 | M1-d | 空间哈希 + 每玩家可见集；裁剪作用于快照、事件附带数据、小地图、统计；抓包自动化断言 | **`TC-NET-03`**（TF-03） |
| DEV-06 | delta 编码与通道分级 | FR-52/53、NFR-03 | M1-d | `changed_mask` + 周期基线；状态走 Unreliable、关键事件 ReliableOrdered 且带 `event_id` 幂等；替换"全量 ReliableOrdered" | `TC-NET-01/02`（TF-02） |
| DEV-07 | 预测 / 插值闭环 | FR-51/54、NFR-04 | M1-a / M1-d | 注册 `InterpolationPlugin`（现状死代码）；`PredictionState` 加 `seq`；共享 `gx-sim` 与同份常量；平滑/硬修正阈值；抖动缓冲 | `TC-PRED-01`（TF-04） |
| DEV-08 | 状态恢复式断线重连 | FR-55、UC-06 | M1-d | 会话令牌恢复 `player_id/net_id`；`SessionResume` + 完整基线快照；30s 窗口；掉线托管；禁止换新 client_id | `TC-NET-04` |

### 4.2 玩法核心（DEV-09~16）

| ID | 任务 | 覆盖 | 阶段 | 关键实现要点 | 验收 |
|---|---|---|---|---|---|
| DEV-09 | 输入层：鼠标点触自动寻路 | FR-40~44、UC-01/02、`GX-CBT-2026-001` §3 | M2（**M1 先移除键盘移动**） | 左键点地 A* 寻路、右键点地立即移动（`force`）、右键点敌锁定普攻并自动追击、左键点敌仅选中；**键盘 W/A/S/D 与方向键不得产生 `move_dir`**；触摸点触/长按等效左右键；服务端校验速度上限与碰撞 | `TC-CBT-01/02`、`TC-UI-01` |
| DEV-10 | 英雄与属性重构 | FR-20/21/34、FR-85 | M1-c / M2 | `Hero` 拆 `HeroBase`/`HeroState`/`Attributes`；派生属性统一计算；定位标签；双构筑至少 1 位英雄可验证 | `TC-SIM-04`、`TC-CFG-01` |
| DEV-11 | 技能框架与状态效果 | FR-22~26 | M2 | 5 类目标类型；CD/消耗/射程/前后摇服务端校验；8 类状态效果与叠加/驱散/免疫规则；未知 id 拒绝（DN-26） | `TC-SIM-04`、`TC-SEC-01` |
| DEV-12 | 战偶波次与林隙营地 | FR-01/02/03 | M2 | 路点来自配置；生成后 `ApplyDeferred`（DN-17）；`construct_ai_system`/`neutral_ai_system`；营地重生计时；碰撞不穿墙 | `TC-SIM-01/02` |
| DEV-13 | 占领型目标状态机 | FR-04/05/06、UC-03/04 | M2 | 归属切换、全队收益、战偶强化在下一波生效、渊卫捕获进度与最后一击归属、推线 AI 不误伤己方 | `TC-SIM-03` |
| DEV-14 | 哨塔 / 源核 / 死亡 / 胜负 | FR-07/08/09/10 | M2 | 仇恨规则与护盾；源核脱战回血；死亡计时重生且属性装备不丢；`EncounterSettled` 全局仅一次 | `TC-SIM-04/05` |
| DEV-15 | 对局阶段与房间规则 | FR-11~16 | M2 | 阶段机驱动 18±4 分钟与 25 分钟硬上限；房间人数来自配置（默认 6）；`offline-dev` 例外；投降投票幂等 | `TC-SIM-05`、G2 |
| DEV-16 | 经济与装备 | FR-14、FR-30~35 | M2 | 补刀归属唯一；`EconomyLedger` 记录每笔收支；购买/售出/主动道具服务端校验金币与距离；合成图 DAG 校验；双构筑属性叠加 | `TC-SIM-02`、`TC-CFG-01`、`TC-SEC-01` |

### 4.3 局外、安全、数据与 UI（DEV-17~30）

| ID | 任务 | 覆盖 | 阶段 | 关键实现要点 | 验收 |
|---|---|---|---|---|---|
| DEV-17 | 账号 / 实名 / 防沉迷 | FR-70~73、CR-02/03/06 | M3 | 实名验证系统对接；未实名（含游客）不得进入；时段与消费限额；隐私政策与查/改/删；战斗服零 PII | `TC-ACCOUNT-01`；合规验收 |
| DEV-18 | 支付与订单 | FR-72/77、CR-04 | M3 | 订单幂等、发货/退款/对账；概率公示；配置校验"无数值型付费项" | `TC-ACCOUNT-01`；配置校验 |
| DEV-19 | 匹配与编排 | FR-74~76、UC-05 | M3 | 队列 + MMR 实际参与；`allocate_battle_server()` 替换写死地址；健康检查、优雅排空不中断在局 | `TC-PERF-01` |
| DEV-20 | 运营后台 / 热更 / 内容安全 | FR-78、FR-91、FR-95 | M3 | 配置/公告/封禁/客服/灰度开关，操作留审计；聊天昵称审核与举报闭环；热更按 hash 分房 | `TC-CFG-02` |
| DEV-21 | 反作弊与审计 | FR-61/63、CR-05 | M3 | 序号去重、时间窗、速度上限、CD/能量/射程二次校验；审计日志；离线行为检测 | `TC-SEC-01`、`TC-SEC-02` |
| DEV-22 | 回放与遥测 | FR-10/16/35/63/93/94、NFR-10 | M2 / M3 | 输入流+基线+配置 hash+事件；`verify_replay` 逐 tick 比对；遥测 → 数据仓库；SLO 与告警 | `TC-SEC-02`（TF-08） |
| DEV-23 | 局内产品 UI | FR-80~83、FR-44、FR-81 | M2 | Bevy UI 替换 egui 调试面板（ADR-09/DN-04）；HUD/小地图/商店/结算与快照一致；**小地图不显示视野外敌方** | `TC-NET-03`、`TC-UI-01` |
| DEV-24 | 局外 UI 与本地化 | FR-84/86、CR-08 | M3 | 登录/大厅/匹配/选英雄/加载/设置；多分辨率与安全区；UI 使用引擎内置字形的 ASCII 文案，**禁止加载操作系统字体**（DN-06/32）；多语言字形须走代码生成方案 | `TC-COMPLIANCE-02`、`TC-CBT-12` |
| DEV-25 | 体素表现层与机型分级 | FR-85、NFR-02/08、`GX-CBT-2026-001` §16 | M3（**M1 先完成基线改造**） | Minecraft 式体素：几何/纹理/材质/图标/特效全部代码生成，零外部资源；LOD 由体素网格降采样实现、机型分档；命中反馈与服务端 tick 对齐 | `TC-CBT-12`、`TC-PERF-02`（TF-09） |
| DEV-26 | 资产 lint 与术语门禁 | FR-96、CR-01/08、NFR-12 | M0（门禁）/ 持续 | 禁词扫描、资产来源登记、**外部资源路径扫描**（禁止运行时读取字体/图片/音频）、配置引用完整性；例外目录 `docs/**` + `LEGAL/**`（澄清 Q-10） | `TC-COMPLIANCE-01/02`、`TC-CBT-12`；**G0** |
| DEV-27 | 测试框架与 TC 实现 | NFR-11 | M1 / M2 | 按 §5 实现全部自动化用例；headless 服务端夹具、协议回放夹具、抓包断言、网络损伤注入 | 全部 P0 用例绿灯 |
| DEV-28 | CI/CD 与发布流水线 | NFR-09/11 | M0 骨架 / M3 完整 | 按 §6 九阶段；发布构建禁用 `dev-tools`/`offline-dev`，排除 `LEGAL/` 与第三方数据目录 | 可复现构建；G3 |
| DEV-29 | 文档与一致性 | DN-38、AS-06 | 持续 | README 与代码同步；消除"技能已实现 vs 未完成""满 6 人 vs 1 人开局"矛盾；已知 Bug 登记 issue | 评审检查表 |
| DEV-30 | 垂直切片集成与成本实测 | G2、TF-01/05 | M2 | 3 英雄 + 完整一局 + 20 局自动对局；采集 tick P95/内存/带宽 → 计算 `cost_per_match`；产出实测报告 | **G2**：100% 对局自然结束 + 成本报告 |

---

## 5. 测试策略与 TC 用例清单

### 5.1 分层

| 层 | 范围 | 工具 | 触发 |
|---|---|---|---|
| 单元 | 伤害公式、属性叠加、状态效果、配置校验、编解码 | `#[test]` | 每次提交 |
| 集成 | 系统链调度、Query 冲突、`ApplyDeferred` 时序、插件装配 | headless App + `#[cfg(test)]` | 每次提交 |
| E2E | 连接→开局→移动→技能命中→掉血→结算 | 客户端/服务端同进程夹具 + UDP 回环 | 每次提交 |
| 协议 | delta 收敛、幂等、版本兼容、抓包 | `gx-tools protocol-compat` | 每次提交 |
| 安全 | 视野零泄漏、输入篡改注入、回放一致性 | 抓包 + 注入夹具 | 每次提交 |
| 性能 | tick P95、容量、带宽、机型帧率 | 压测 + 真机 | 每日 / 闸门 |
| 合规 | 禁词、资产登记、实名/防沉迷 | CI + 人工签字 | 每次提交 + 发布 |

### 5.2 用例清单（对应需求文档 §12）

| TC | 覆盖 | 类型 | 方法 | 通过判据 |
|---|---|---|---|---|
| **TC-NET-03** | FR-60/81、CR-05、DN-12 | 安全 | 构造"敌方在视野外"场景，抓取服务端下行全部包体反序列化 | 包中**不出现**该实体的任何字段（含事件附带数据、小地图、统计）；0 命中 |
| TC-NET-01 | FR-52、NFR-03 | 协议 | 全量 vs delta 对比，注入基线丢失/乱序 | 带宽降 ≥50%；状态在 N 帧内收敛一致 |
| TC-NET-02 | FR-53 | 协议 | 重传/重复 `event_id` | 无重复结算；可靠事件有序 |
| TC-NET-04 | FR-55 | E2E | 断开 30s 内重连 | 恢复原 `player_id/net_id`、装备/等级/位置一致 |
| TC-PRED-01 | FR-51/54、NFR-04 | 网络损伤 | 150ms/3% 丢包/50ms 抖动下跑 5 分钟 | 无可感拉扯；硬修正 ≤1 次/分；远端实体确实被插值（DN-11） |
| TC-SIM-01 | FR-01/02 | E2E | 20 局自动对局 | 波次按配置；0 卡死、0 穿墙 |
| TC-SIM-02 | FR-03/30/35 | 集成 | 多实体同帧死亡、营地连续击杀、账本重算 | 补刀归属唯一；赏金不重复；账本与回放一致 |
| TC-SIM-03 | FR-04/05/06 | 集成 | 占领/反占领/首领捕获/重生 | 归属唯一、奖励正确、推线 AI 不误伤 |
| TC-SIM-04 | FR-07/09/21/23/24 | 集成 | 塔仇恨与护盾、死亡重生、状态效果叠加/驱散/免疫 | 属性装备不丢；叠加规则符合配置 |
| TC-SIM-05 | FR-08/10/12 | E2E | 100 局自动对局 | 每局有且仅有 1 次结算；时长分布 18±4 分钟（P50）；硬上限生效 |
| TC-SEC-01 | FR-25/32/61、CR-05 | 注入 | 变速齿轮、改包提前施法、越距购买、负金币 | 全部被拒且记审计日志 |
| TC-SEC-02 | FR-35/63/93、TF-08 | 回放 | `verify_replay` 重算 | 逐 tick 状态/账本一致；无差异 |
| TC-CFG-01 | FR-20/33/90 | 校验 | 非法配置（未解析引用、负价格、合成环、未知技能 id） | CI 全部拒绝；合法配置通过 |
| TC-CFG-02 | FR-91、DN-25 | 集成 | 运行中切配置，同时在局 | 新配置只影响新对局；在局 `config_hash` 不变 |
| TC-COMPAT-01 | FR-57 | 协议 | 新旧 `PROTOCOL_VERSION` / `config_hash` 混合 | 明确 `Reject`，不崩溃、不静默降级 |
| TC-COMPLIANCE-01 | CR-01、NFR-12 | CI | 全仓禁词扫描（例外目录显式白名单） | 0 命中 |
| TC-COMPLIANCE-02 | CR-08、FR-86/96 | CI + 人工 | 资产来源登记、**表现层来源全部为「代码生成」**、发布包内容 | 登记 100%；发布包不含 `LEGAL/` 与第三方数据目录；无外部字体/图片/音频文件 |
| TC-PERF-01 | NFR-01/05、TF-01 | 压测 | N 房间 × 6 合成客户端 | tick P95 ≤4 ms；单核 ≥8 房间；内存 ≤128 MB/房 |
| TC-PERF-02 | NFR-02/08、FR-85 | 真机 | 三档机型 × 连续 2 局 | 中端 60fps、低端 ≥30fps；内存 ≤700 MB |
| TC-UI-01 | FR-40/41/42 | 真机 | 左键寻路、右键移动/锁定、触摸点触与长按、热区测量 | 键盘不产生位移；无丢失触点；热区 ≥44dp |
| TC-CBT-01~12 | `GX-CBT-2026-001` §19 | 单元 + 集成 + 门禁 | 操作/寻路/伤害/状态/商店/野怪/目标/塔/表现层 | 已通过 11 项，`TC-CBT-05`（普攻前后摇）待实现；细则见规格 §19.1 |
| TC-GP-01 | `GX-CBT-2026-001` §22 | 集成 | ECS 系统链真实驱动护盾→阵亡→购买→占领→重生 | ✅ 通过；`gx-e2e/tests/gameplay.rs` |
| TC-NAV-01~09 | `GX-CBT-2026-001` §24 | 单元 + 集成 + 抓包 | 动态障碍 / 重算节流 / 服务端速度与碰撞校验 | ⛔ M2 待办，仅规格已定义 |
| TC-ACCOUNT-01 | FR-71/72、CR-02/03 | 集成 + 人工 | 未实名、时段外、超额付费 | 全部被拒；合规验收签字 |

### 5.3 TF → TC 映射（回填 `GX-TF-2026-001` 的验证结果）

| TF | 主要 TC | 回填字段 |
|---|---|---|
| TF-01 | TC-PERF-01 | tick P95、单核房间数、内存、`cost_per_match` |
| TF-02 | TC-NET-01 | 带宽降幅、收敛帧数 |
| TF-03 | TC-NET-03 | 抓包命中数、裁剪耗时 |
| TF-04 | TC-PRED-01 | 硬修正率、拉扯评分 |
| TF-05 | TC-SIM-01/05 | 对局完成率、时长分布 |
| TF-06 | TC-ACCOUNT-01 | PII 隔离审计 |
| TF-07 | TC-CFG-02 | 热更一致性 |
| TF-08 | TC-SEC-02 | 回放差异 |
| TF-09 | TC-PERF-02 | 机型帧率 |
| TF-10 | TC-COMPAT-01 | 兼容矩阵 |
| TF-11 | TC-COMPLIANCE-01/02 | 命中数、登记覆盖率 |

---

## 6. CI/CD 与发布流水线

| # | 阶段 | 命令 / 工具 | 失败处理 |
|---|---|---|---|
| 1 | 格式 | `cargo fmt --all -- --check` | 阻断 |
| 2 | 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | 阻断（M1-a 前需先清零 DN-35） |
| 3 | 资产与术语门禁 | `gx-tools lint-assets` | 阻断（CR-01） |
| 4 | 配置校验 | `gx-tools config-validate` | 阻断（TC-CFG-01） |
| 5 | 测试 | `cargo test --workspace`（单元/集成/E2E） | 阻断 |
| 6 | 协议兼容 | `gx-tools protocol-compat` | 阻断（TC-COMPAT-01） |
| 7 | 安全断言 | 抓包夹具（TC-NET-03）+ 注入夹具（TC-SEC-01） | 阻断 |
| 8 | 构建 | `cargo build --release --locked`；校验可复现 | 阻断 |
| 9 | 性能冒烟（每日） | headless 压测 + 真机冒烟 | 超阈值告警，不阻断合并但阻断闸门 |

**发布包检查清单**（DN-29/30/32、CR-01）：
- `--no-default-features`，确认 `dev-tools`、`offline-dev` 未启用；
- 包体不包含 `LEGAL/`、第三方数据目录、遥测样本、`docs/` 合规留档；
- 表现层全部代码生成、零外部资源路径；音频若引入须登记许可；
- 窗口标题、包名、二进制名、描述均为 GX-01 代号或最终产品名；
- 禁词扫描对**发布包**再跑一次（不只是源码）。

---

## 7. 分支、提交与评审规范

| 项 | 约定 |
|---|---|
| 分支 | `main`（受保护）/ `develop` / `feature/DEV-xx-<slug>` / `fix/TC-xx-<slug>` / `release/v*` |
| 提交信息 | `DEV-12: 实现战偶波次生成` 或 `FR-02: 修正路点配置`；必须含 ID |
| PR 必填 | 覆盖的 `FR/NFR/CR`、对应 `TC`、受影响的 `DN`、`ConfigSet` 是否变更、是否需要数据迁移 |
| 评审重点 | ① 视野裁剪是否被绕过 ② 是否新增硬编码数值 ③ 是否引入禁用词/未登记资产 ④ 系统链顺序与 `ApplyDeferred` ⑤ 客户端/服务端是否仍共享同一份 `gx-sim` |
| 禁止 | 无 ID 的提交；绕过 CI 的直接合并；把规则层映射表写进代码注释或包体 |
| 变更控制 | 需求变更须同步更新 `GX-PRD-2026-001`、架构 §15 追溯矩阵与本文档任务卡 |

---

## 8. 开发注意项（DN）落地检查表

> 完整 40 条见 `GX-ARCH-2026-001` §14。此处定义**检查时机**与**自动化手段**。

| DN 分组 | 编号 | 检查时机 | 自动化 |
|---|---|---|---|
| Bevy / egui 集成 | DN-01~06 | M1-a、每次 UI 改动 | 启动冒烟（无 panic、UI 可见）+ 集成测试 + 代码评审 |
| 网络与同步 | DN-07~14 | M1-d、每次协议改动 | `TC-NET-01/02/03`、`TC-PRED-01`、抓包断言 |
| ECS 与调度 | DN-15~22 | 每次新增系统 | 调度构建用例（Query 冲突）、`cargo test`、启动冒烟 |
| 数据与配置 | DN-23~28 | 每次数值改动 | `TC-CFG-01/02`、静态检查（平衡字面量）、压测分配量 |
| 合规与资产 | DN-29~33 | M0、每次提交、发布前 | `TC-COMPLIANCE-01/02`、发布包扫描 |
| 工程与测试 | DN-34~40 | M1-a、CI、发布前 | CI 全阶段、`--locked` 构建、发布清单 |

**必须自动化的 10 条**（不得以人工评审替代）：DN-01（唯一主上下文 / 启动冒烟）、DN-09（同份常量）、DN-11（插值已注册）、DN-12（视野零泄漏）、DN-23（无硬编码数值）、DN-26（未知 id 拒绝）、DN-29（第三方数据不进包）、DN-30（命名清理）、DN-36（`Cargo.lock`）、DN-39（测试覆盖）。

### 8.1 DN-01 特别提示（本项目已实际发生的故障）
`EguiGlobalSettings{auto_create_primary_context:false}` + **仅**在带 `CameraRenderGraph` 的游戏相机上挂 `PrimaryEguiContext`，二者必须成对出现：
- 只关自动创建、不手动挂 → `EguiPrimaryContextPass` 不运行，UI 全消失且不报错；
- 只手动挂、不关自动创建 → 两个上下文共享同一调度 → panic `Each Egui context must have a unique schedule`。

该约束在 M1-a 回归用例中固化为"启动后 UI 系统确实执行过一次"的断言。

---

## 9. Definition of Done（每个 DEV 任务的通用出口）

- [ ] 代码中含任务 ID 与覆盖的 FR/NFR/CR ID（提交信息、模块文档）；
- [ ] 对应 `TC-*` 用例已实现并通过；
- [ ] `cargo fmt --check` / `clippy -D warnings` / `cargo test --workspace` 全绿；
- [ ] 无新增硬编码平衡数值；配置表与校验器已更新（DN-23）；
- [ ] 无禁用词、无未登记资产；发布包检查通过（CR-01/08）；
- [ ] 若改动系统链：顺序不变量 1~5（架构 §6.1）复核，`ApplyDeferred` 位置正确（DN-17）；
- [ ] 若改动协议：版本协商与兼容用例通过（FR-57），上行仍不含权威字段（FR-50）；
- [ ] 若改动视野/事件/小地图：`TC-NET-03` 重新跑抓包断言；
- [ ] 客户端与服务端仍共享同一份 `gx-sim` 与同一 `ConfigSet`（DN-09）；
- [ ] `offline-dev` 一键启动路径仍可用，且发布构建禁用该 feature（DN-40）；
- [ ] README / 架构追溯矩阵 / 本文档任务卡同步更新（DN-38、DEV-29）；
- [ ] 若为 P0 任务：对应闸门（G0/G1/G2）判据已回填到 `GX-TF-2026-001` 的实测表。

---

## 10. 附录 A：配置表片段示例（示意）

```ron
// gx-proto-content/heroes.ron —— 所有数值为 GX-01 自研占位值，须经平衡验证
Hero(
  id: "hero_ashwarden",
  display_name: "灰烬守望",          // 原创命名，禁用词表扫描
  role_tags: [GUARD, SPEAR],
  base: ( health: 720.0, energy: 260.0, move_speed: 3.8,
          armor: 22.0, shield: 18.0, attack: 54.0 ),
  growth: ( health: 68.0, energy: 18.0, attack: 4.2 ),
  abilities: ["ab_ashwarden_q", "ab_ashwarden_w", "ab_ashwarden_r"],
  build_paths: [BLADE, ARCANE],      // FR-34 双构筑
)
```

```ron
// gx-proto-content/objectives.ron
CaptureObjective(
  id: "lode_shrine", kind: LodeShrine,
  spawn_tick: 900, respawn_ticks: 5400,
  capture_threshold: 100,
  reward: ( team_gold: 120, per_player_gold: 40 ),
)
```

校验器必须拒绝：未解析引用、负价格/负赏金、合成图成环、`spawn_tick` 超出对局时长上限、未知技能 id、禁用词。

## 11. 附录 B：协议字段片段示例（示意）

```rust
// 上行：仅输入与请求，禁止权威状态字段 (FR-50 / DR-02)
InputCmd { seq, tick, client_time_us, intent: Intent::Cast { ability_id, target },
           rtt_ms, config_hash, checksum }

// 下行：基线 + 增量，已按玩家可见集裁剪 (FR-52/60)
Snapshot { header: SnapshotHeader { tick, baseline_tick, config_hash, view_seq },
           deltas: Vec<EntityDelta>, baseline: Option<BaselineSnapshot> }
EntityDelta { net_id, generation, changed_mask, position, facing,
              health, energy, level, gold, cooldowns, status_bits }

// 关键事件：可靠有序 + 幂等 (FR-53 / DN-14)
GameEvent::ObjectiveCaptured { event_id, tick, objective, owner }
GameEvent::EncounterSettled  { event_id, tick, winner, stats }
```

## 12. 附录 C：反向追溯矩阵（DEV / TC → FR）

| DEV | 覆盖需求 | 主要 TC |
|---|---|---|
| DEV-01 | CR-01、NFR-09 | TC-COMPLIANCE-01/02 |
| DEV-02 | FR-90/91/92 | TC-CFG-01/02 |
| DEV-03 | FR-50/56/57 | TC-NET-01、TC-COMPAT-01 |
| DEV-04 | FR-62 | TC-SEC-01 |
| DEV-05 | FR-60/81 | **TC-NET-03** |
| DEV-06 | FR-52/53 | TC-NET-01/02 |
| DEV-07 | FR-51/54 | TC-PRED-01 |
| DEV-08 | FR-55 | TC-NET-04 |
| DEV-09 | FR-40~44 | TC-UI-01 |
| DEV-10 | FR-20/21/34 | TC-SIM-04、TC-CFG-01 |
| DEV-11 | FR-22~26 | TC-SIM-04、TC-SEC-01 |
| DEV-12 | FR-01/02/03 | TC-SIM-01/02 |
| DEV-13 | FR-04/05/06 | TC-SIM-03 |
| DEV-14 | FR-07/08/09/10 | TC-SIM-04/05 |
| DEV-15 | FR-11~16 | TC-SIM-05 |
| DEV-16 | FR-14、FR-30~35 | TC-SIM-02、TC-CFG-01、TC-SEC-01 |
| DEV-17 | FR-70~73、CR-02/03/06 | TC-ACCOUNT-01 |
| DEV-18 | FR-72/77、CR-04 | TC-ACCOUNT-01 |
| DEV-19 | FR-74~76 | TC-PERF-01 |
| DEV-20 | FR-78/91/95 | TC-CFG-02 |
| DEV-21 | FR-61/63 | TC-SEC-01/02 |
| DEV-22 | FR-10/16/35/63/93/94 | TC-SEC-02 |
| DEV-23 | FR-44、FR-80~83 | TC-NET-03、TC-UI-01 |
| DEV-24 | FR-84/86 | TC-COMPLIANCE-02 |
| DEV-25 | FR-85、NFR-02/08 | TC-PERF-02 |
| DEV-26 | FR-96、CR-01/08 | TC-COMPLIANCE-01/02 |
| DEV-27 | NFR-11 | 全部 TC |
| DEV-28 | NFR-09/11 | CI 全阶段 |
| DEV-29 | DN-38 | 评审检查表 |
| DEV-30 | G2（闸门） | TC-SIM-01/05、TC-PERF-01 |

> 需求文档 §12 是"FR → ARC → DEV → TC"的正向矩阵；本附录是其反向视图。两者必须同步维护。

---

*所有阈值、成本与工期口径沿用 `GX-TF-2026-001` 与 `VG-FS-2026-001`；实测数据回填后四份文档须同步升版。*
