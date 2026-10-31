# Vainglory Rust（王者争霸 / 虚荣 复刻）

用 Rust 复刻 **Vainglory（虚荣 / Halcyon Fold）** 的 3v3 MOBA 原型：服务器权威 + 客户端预测 + 插值的经典网络同步模型。

> 当前状态：**workspace 可 `cargo check` / `cargo build` 通过（仅有少量 dead_code 警告）**，是一个「能跑通联机骨架 + 基础战斗模拟」的可玩原型，尚缺英雄技能、商店、小兵生成等玩法层内容。

---

## 一、当前实现概览

### 技术栈

| 层 | 选型 |
|---|---|
| 客户端引擎 / 渲染 | `bevy` 0.20（wgpu，Vulkan/Metal/DX12/WebGPU） |
| 客户端 UI | `bevy_egui` 0.43（HUD / 小地图 / 商店占位） |
| 数学 | `glam`、`fixed`（`I32F32` 确定性定点）、`bitcode`（紧凑二进制序列化） |
| 网络传输 | `renet2` 0.12（可靠/不可靠通道）+ `renet2_netcode` 0.12（基于 UDP 的 netcode 传输，`NativeSocket`） |
| 服务端运行时 | Headless `bevy`（`MinimalPlugins` + `StatesPlugin`）+ 固定 60Hz `FixedUpdate`；注册 `NetworkingPlugin` / `SimulationPlugin` / `ServerPlugin`（驱动 `ServerState` 状态机）/ `SpawnPlugin`（6 人开局生成防御塔等）。Headless 不渲染，但手动初始化 `Assets<Mesh>` / `Assets<StandardMaterial>` 资源供 `SpawnPlugin` 使用 |
| 匹配服务 | `axum` 0.7 + `tokio`（HTTP 接口，内存队列；预留 Redis） |

- **工作空间成员**：`game-core`（共享逻辑）、`client`、`server`、`matchmaking`
- **Edition 2024** → 需要 Rust ≥ 1.85（`rustup update stable`）
- 三个二进制：`vainglory-client`、`vainglory-server`、`vainglory-mm`

### 已实现功能

**网络与同步**
- 客户端经 `127.0.0.1:5000` 连接服务端；`renet2_netcode` 完成握手、连接/断线事件、UDP 收发包。
- 协议版号 `PROTOCOL_VERSION = 1`，`ClientAuthentication::Unsecure`（开发用）。
- 客户端每帧发送 `InputCmd`（tick / 移动方向 / 瞄准方向 / 技能掩码 / ping）。
- 服务端每 tick 广播 `WorldSnapshot` 与 `GameEvent`（当前均走 `ReliableOrdered`，原型简化；后续可改为不可靠通道 + 增量压缩以降低带宽）。
- 消息用 `bitcode::encode/decode` 序列化；`InputCmd / WorldSnapshot / GameEvent / ClientMessage / ServerMessage` 等全部实现 `Encode/Decode`。

**客户端表现层**
- **输入**：键盘（WASD / 方向键 / 数字键 1-4）、鼠标（世界坐标拾取生成瞄准方向）、触摸（左半屏虚拟摇杆移动、右半屏瞄准） → `PlayerInputState`。
  - 移动方向以**视角为准**：相机固定俯视 `-z`，故 `W/↑` = 前进（屏幕上方、远离相机），`S/↓` = 后退（屏幕下方、靠近相机），`A/←`、`D/→` 为左右；键盘优先级高于点击移动，按下即取消点击目标。左键点地则走点击移动（到达阈值内自动停下）。
  - **技能**：数字键 1-4 = 4 个技能槽（与 `game_core::abilities::default_hero_abilities` 对应），右键 = 普攻（预留 bit0）。技能为**数据驱动**（`AbilityDef`：CD / 蓝耗 / 射程 / 类型 `Instant`/`Projectile`/`PointTarget` / 伤害类型 / 加成），施放后扣蓝并进入冷却（`HeroAbilities.cooldowns`），**命中裁决完全在服务端**（`ability_cast_system` / `projectile_system` / `resolve_pending_hits`），客户端只按快照渲染弹道。能力栏 HUD 显示技能名与冷却遮罩。
- **预测**：`FixedUpdate` 中本地推进 tick、本地模拟移动/朝向，缓存待确认输入历史（`PredictionState`），收到快照后做 **硬修正 / 平滑修正** 回滚对齐。
- **插值**：远端实体按最近两帧在 `Vec3/Quat` 空间插值（`EntityHistory` 保存 ≤60 帧），消除网络抖动。
- **渲染**：3D 场景含地面、河道、三路兵线、防御塔、Vain Crystal、野怪营地、草丛、方向光 + 环境光 + 摄像机（来自 `game-core/map.rs` 的 Halcyon Fold 静态数据）。
  - **摄像机**：固定在英雄后上方偏移 `(0,22,26)` 俯视，带**死区（leash）跟随**——英雄在死区内移动时相机不动、英雄在画面中明显前后/左右滑动，超出死区后相机才平滑追平，使纵深方向移动可感知（早期"只能左右、看不出前后"即源于相机每帧把英雄硬吸到屏幕正中）。
  - **点击落点标记**：左键点地后会在目标处显示一枚平铺地面的金色圆环（`Torus`），有目标时可见、到达/取消后隐藏，便于确认点击方向与落点。
  - **地图边界**：英雄移动被限制在地图形内（`±(MAP/2 - 1)`），不会跑出地图。
- **UI（egui）**：左上角血条/蓝条/等级/金币 HUD、底部 QWER + 回城/召唤师技能栏、右上角小地图（绘制了河道）、商店占位窗口、输入调试面板。
  - **中文字体**：启动时自动从 `C:\Windows\Fonts\` 加载 `simhei.ttf`/`Deng.ttf`/`simkai.ttf` 等系统 CJK 字体作为回退，中文 HUD 正常显示（egui 默认字体不含中文字形，否则中文会显示为方块）。

**服务端模拟**
- 服务端状态机：`WaitingForPlayers → Loading → Running → Paused / Finished`。
- 客户端连接后分配 `NetId` 与阵营（蓝/红交替），生成 `Hero` 实体并广播 `HeroSpawn`。
- 固定 60Hz 运行 `game-core` 的战斗系统：移动、朝向、攻击、伤害结算、技能冷却、升级成长、被动金币、血/能量回复。
- **技能系统（服务端权威）**：`ability_cast_system` 读取 `PlayerInput.ability_mask`（bit1-4 = 技能槽 0-3，bit0 预留普攻）触发施法，校验蓝量/冷却后发起；`Projectile` 弹体由 `projectile_system` 推进并做碰撞命中、经统一 `AttackCommand` 结算；`PointTarget`/`Instant` 经 `PendingHit` 做范围伤害；伤害含护甲/魔抗减免。弹体随 `WorldSnapshot` 下发，客户端按 `EntityType::Projectile` 渲染。
- 野怪 AI（仇恨表 + 追击 / 攻击）、防御塔 AI（范围攻击），均来自 `game-core::systems`。
- 开局生成：所有防御塔、双方 Vain Crystal、各野怪营地（`spawn.rs`）；**小兵生成仍是 TODO**。

**共享核心 `game-core`**
- `components`：Hero / Minion / Turret / JungleMonster / Ability / Item / PlayerInput / NetId / TransformSync / Aggro / Selectable 及 `LocalPlayer`/`ServerAuthority`/`Predicted`/`Interpolated` 标记。
- `net`：向量/四元数类型（`Vec2f/Vec3f/Quatf`，自带与 bevy 类型的双向 `From`）、`InputCmd`、`WorldSnapshot`、`EntitySnapshot`、`GameEvent`、`ClientMessage`、`ServerMessage`、`MatchState`、`PingType`、`JungleMonsterType` 等协议定义与序列化工具。
- `math`：确定性定点 `Fixed/I32F32`、`FixedVec2/3`、`TickClock`（固定步长累加器 + 插值 alpha）、`lerp_*` 助手、`DeterministicRng`（Xorshift64，保证回放一致）。
- `systems`：移动、朝向、攻击、伤害、技能冷却、升级、经济、仇恨、野怪/防御塔 AI、回复等系统。
- `map`：Halcyon Fold 静态数据（地图尺寸、双方基地、Vain Crystal、12 座防御塔位置、野怪营地、兵线路径点、草丛、视野范围常量）。

**匹配服务 `matchmaking`**
- `axum` HTTP 服务（`:8080`）：`GET /health`、`POST /queue/join`、`POST /queue/leave`、`GET /match/status`。
- 内存版 `MatchQueue`：满 6 人触发匹配、生成 `match_id` 与（占位）服务器地址。

---

## 二、架构

```
┌──────────────────── 客户端 vainglory-client（Bevy + wgpu） ────────────────────┐
│                                                                                │
│  input.rs        采集键鼠/触摸 → PlayerInputState(move_dir, aim_dir, mask)      │
│      │  InputCmd(tick, move, aim, abilities)                                   │
│      ▼                                                                         │
│  prediction.rs   本地预测(FixedUpdate) ──► 本地英雄 Transform/Predicted         │
│      │                     │                                                   │
│      │                     ▼                                                   │
│      │              ui.rs / rendering.rs (HUD / 3D 场景 / 插值渲染)            │
│      ▼                                                                         │
│  networking.rs    RenetClient + NetcodeClientTransport ──(UDP 不可靠/可靠)──┐   │
│                       ▲  WorldSnapshot / Event                              │   │
│                       └─────────────────────────────────────────────────────┘   │
│  interpolation.rs  远端实体 EntityHistory 插值 ──► Interpolated 实体平滑        │
└────────────────────────────────────────────────────────────────────────────────┘
                              │  UDP (renet2_netcode)
                              ▼
┌──────────────────── 服务端 vainglory-server（Headless Bevy） ──────────────────┐
│  networking.rs                                                                │
│    start_server(Startup) 绑定 UDP 0.0.0.0:5000                               │
│    poll_connections   连接/断线、分配 NetId 与阵营、生成权威英雄实体           │
│    receive_inputs     收集 InputCmd → PendingInputs                           │
│    broadcast_snapshot 组装 WorldSnapshot + 首帧 MatchStart → 全客户端广播     │
│      │ InputCmd          │  WorldSnapshot / GameEvent                        │
│      ▼                  ▼                                                     │
│  simulation.rs   60Hz FixedUpdate: tick_clock + apply_pending_inputs + 战斗系统│
│  spawn.rs        ServerState::Running 时生成 防御塔 / Vain Crystal / 野怪营地  │
│  systems.rs / game-core::systems  移动/攻击/伤害/技能冷却/升级/经济/回复/AI   │
│  lib.rs          ServerPlugin 驱动 ServerState(WaitingForPlayers→Loading→Running)│
└────────────────────────────────────────────────────────────────────────────────┘
                              │  HTTP (axum)
                              ▼
┌──────────────────── 匹配服务 vainglory-mm（axum + tokio） ─────────────────────┐
│  routes.rs / queue.rs / store.rs  队列 / 6人匹配 / 状态查询（Redis 预留）       │
└────────────────────────────────────────────────────────────────────────────────┘

         game-core（components / net / math / systems / map）
         ↑ 客户端与服务端共用同一份确定性逻辑，保证双方一致
```

### 同步模型要点
- **服务器权威**：所有战斗判定在服务端发生，客户端仅渲染 + 预测。
- **通道划分**：客户端 `InputCmd` 走 `Unreliable`（高频、可丢）；`WorldSnapshot` / `GameEvent`（创建/销毁、购买、击杀、匹配状态）当前均走 `ReliableOrdered`（原型简化，后续可改快照为不可靠 + 增量压缩）。
- **确定性**：核心移动/AI 使用 `fixed` 定点与 `DeterministicRng`，为回放与反作弊打基础（目前快照与预测仍用 f32 推进，定点仅在 `math` 层提供工具）。
- **手感优化**：本地英雄客户端预测 + 快照回滚；远端英雄插值 + 延迟渲染。

---

## 三、后续实现目标 / 构想 / 方向

### 玩法层（优先级高）
- [x] **英雄实体真正落地**：客户端收到 `HeroSpawn` 后生成带 `Mesh3d` + `Transform` + `TransformSync` 的实体，本地英雄打 `LocalPlayer/Predicted`（客户端预测驱动移动）、远端英雄打 `Interpolated`（快照插值）；服务端英雄实体补齐 `Transform` 后由 `movement_system` 权威推进，快照位置随输入实时更新。移动链路已由 `crates/client/tests/e2e_movement.rs`（键盘→预测→网络→服务端→快照）与 `crates/server/tests/net_handshake.rs`（hero_moves_with_input / hero_moves_in_z）覆盖 x/z 两轴验证。
- [ ] **技能系统**：技能定义、CD/资源消耗、弹道（Projectile）、命中裁决、伤害公式（AD/AP + 护甲/魔抗减免）。
- [ ] **小兵与兵线**：`minion_paths()` 路点寻路、波次生成、小兵 AI、经验/金币奖励。
- [ ] **经济与商店**：`Item` / 购买流程（已定义协议 `PurchaseItem`），属性叠加，主动装备效果。
- [ ] **目标点**：Vain Crystal / 防御塔血量与摧毁逻辑、Kraken / Gold Mine / Minion Mine 全局事件。

### 网络与性能
- [ ] **AOI 裁剪 + 增量快照**：只对视野内实体下发，按 tick 做 delta 压缩，降低带宽。
- [ ] **断线重连**：基于快照 + 输入历史的重演（依赖 `DeterministicRng` 与固定种子）。
- [ ] **RTT/ping 计算**：`InputCmd.ping` 与 `WorldSnapshot.server_time` 对齐，驱动插值 alpha 与预测。
- [ ] **反作弊**：每 tick 状态哈希 + 输入日志回放校验。

### 平台与工程
- [ ] **Web 端**：替换 `renet2_netcode`（UDP/原生 socket）为 WebRTC/QUIC 传输（如 `matchbox`/`quinnet`），开启 `bevy` 的 wasm 目标。
- [ ] **移动端**：Android/iOS 构建配置 + 触屏 UI 完善（虚拟摇杆已预留）。
- [ ] **匹配服务生产化**：接入 Redis 队列、battle-server 动态分配（K8s Pod/每局一进程）、账号与战绩（`sqlx` + Postgres）。
- [ ] **资源/美术**：glTF 英雄模型、骨骼动画、技能特效、音效。

---

## 四、如何启动与编译

### 环境要求
- Rust ≥ 1.85（`edition = "2024"`，`rustup update stable`）。
- 操作系统：Windows / Linux / macOS（桌面原生目标，开箱即用）。
- 网络：本地回环即可，无需公网。

### 编译
```bash
# 检查整个 workspace（CI / 快速验证）
cargo check --workspace

# 单独构建各 target
cargo build -p game-core
cargo build -p client
cargo build -p server
cargo build -p matchmaking

# 发布构建（带优化）
cargo build --workspace --release
```

### 运行（先起服务端，再起客户端）
```bash
# 终端 1：服务端（监听 0.0.0.0:5000，headless）
cargo run -p server

# 终端 2：客户端（连接 127.0.0.1:5000，弹出窗口）
cargo run -p client

# 终端 3（可选）：匹配服务（HTTP :8080）
cargo run -p matchmaking
```
> 客户端默认连接 `127.0.0.1:5000`，端口/地址在 `client/src/networking.rs` 的 `connect_to_server` 与 `server/src/networking.rs` 的 `start_server` 中修改。
> 监听端口由 `ServerPort` 资源控制（默认 5000），可在测试或部署时覆盖。
> 当前满 6 人（或手动双开）即可进入对局；服务端状态随连接人数在 `WaitingForPlayers/Loading/Running` 间切换（`ServerPlugin` 驱动 `ServerState`）。

### 各平台编译说明

| 平台 | 状态 | 说明 |
|---|---|---|
| Windows / Linux / macOS（桌面） | ✅ 支持 | 默认目标，`cargo run -p client` 直接出窗口。 |
| WebAssembly（浏览器） | ⚠️ 待改造 | `bevy` 支持 wasm，但 `renet2_netcode` 依赖原生 UDP socket；需替换为 WebRTC/QUIC 传输（如 `matchbox`/`quinnet`）才能联网。 |
| Android / iOS | ⚠️ 待改造 | 需配置 `bevy` 移动后端 + 触屏输入 + 替换原生传输；虚拟摇杆逻辑已在 `input.rs` 预留。 |
| 专用服务器（Linux headless） | ✅ 支持 | `cargo build -p server --release` 即可部署，无渲染依赖。 |

### 目录结构
```
vainglory/
├── Cargo.toml                  # workspace 根（成员 + 共享依赖）
├── README.md
└── crates/
    ├── game-core/              # 共享逻辑：components / net / math / systems / map
    ├── client/                 # Bevy 客户端：input / prediction / interpolation / rendering / ui / networking
    ├── server/                 # Headless 服务端：networking / simulation / spawn / systems
    └── matchmaking/            # axum 匹配服务：routes / queue / store
```

---

## 五、常见问题 / 排错

- **看不到英雄 / 地图（3D 场景空白，但 UI 面板正常）**
  1. **端口 5000 被旧服务端占用**：若之前启动过服务端但没关，新服务端会在 `start_server` 绑定 UDP 时**报错退出**（现已改为打印中文错误并 `exit(1)`，而非静默 panic），导致英雄无法生成。解决办法：先结束所有残留的 `vainglory-server` 进程，再只启动**一个**服务端。
     ```powershell
     Stop-Process -Name "vainglory-server" -Force
     cargo run -p server   # 仅此一个
     ```
  2. **相机朝向**：英雄实体其实已生成，只是相机若朝向天空会看不见任何东西；正常情况下相机固定俯视地面，应能看到英雄与地图。
  3. 确认启动顺序：先 `server` 再 `client`；客户端日志里出现 `已加载中文字体: ...` 即代表 UI 初始化正常。

- **中文显示为方块（□□□）**
  启动时 `setup_cjk_font` 会从 `C:\Windows\Fonts\` 依次尝试 `simhei.ttf` / `Deng.ttf` / `simkai.ttf` / `msyh.ttc` / `simsun.ttc` 并作为回退字体加载（仅 Windows 验证过）。若全部缺失会跳过且不阻塞，但中文会显示为方块。非 Windows 平台需自行放置一个包含中文字形的字体并调整 `client/src/ui.rs` 中的候选路径。

- **前进/后退感觉反了**
  移动方向以当前固定俯视视角为准：`W/↑` 为前进（屏幕上方、远离相机），`S/↓` 为后退。若仍觉别扭，检查 `client/src/input.rs` 中 `handle_input` 的 `keyboard_dir` 符号即可对调。

- **只有左右移动、感觉不出前后**
  早期相机每帧把英雄吸附到屏幕正中所致，现已改为**死区跟随**（英雄在死区内自由滑动、超出才追平）。若仍不直观，可调 `client/src/rendering.rs` 里 `camera_follow_local_player` 的死区半径（当前 6.0）或相机偏移 `(0,22,26)`。

> 小提示：当前代码含有若干 `TODO`（英雄技能、小兵、商店、ping、Redis 等），编译通过但玩法尚不完整，适合作为继续开发的起点。
