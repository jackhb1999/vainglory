# GX-01

原创世界观的 3v3 团队竞技原型（内部工程代号 `GX-01`，产品名待商标检索后确定）。
服务范围、需求与验收标准以 `docs/` 下的文档族为准。

> **本次交付边界**：M0 + M1，验收闸门 G0 / G1。见 `docs/03_REQUIREMENTS.md` §11.1。
> **范围变更（v1.1）**：交付范围由 `M0+M1` 扩展为 `M0+M1 + 玩法逻辑闭环`
> （属性扩展 / 状态效果 / 完整伤害公式 / 商店事务 / 野怪 AI / 占领目标 / 塔护盾 / 死亡重生 / 胜负加时 / PvE 单人对局）。
> M2 剩余差距逐条对照见 `docs/05_COMBAT_SPEC.md` §23；寻路 M2 规格见 §24（P-U8 / P-U9 已落地并通过 `TC-NAV-01~09`）；
> 上线功能（M3）与灰度（M4）仍不在范围。
> **本次文档整理**：纠正表现层章节与代码不符的口径，并把「MC 风格细化」列为下一阶段的正式开发路线（见 §8）。

### 目录

- §1 文档族（编号 / 文件索引）
- §2 产品定位
- §3 工作空间
- §4 构建与运行
- §5 操作方案（v1.3 纯点击控制）
- §6 表现层现状（MC 体素，零外部资产）
- §7 M1 实现要点
- §8 视觉开发路线：MC 表现层细化（P-V 系列）
- §9 可玩闭环实施状态（P-A → P-E → P-B → P-C → P-H）
- §10 已知未完成项与文档同步
- §11 合规与外部依赖
- §12 风险登记
- §13 验证状态

## 1. 文档族

| 文档 | 编号 | 内容 |
|---|---|---|
| `docs/` 上游报告 | `VG-FS-2026-001` | 上游可行性与法律边界研究（合规留档） |
| `docs/01_TECHNICAL_FEASIBILITY.md` | `GX-TF-2026-001` | 技术假设、POC、性能/带宽预算、技术闸门 |
| `docs/02_ARCHITECTURE.md` | `GX-ARCH-2026-001` | 分层架构、协议、帧循环、ADR、开发注意项 |
| `docs/03_REQUIREMENTS.md` | `GX-PRD-2026-001` | FR / NFR / CR / UC、验收标准、版本切分 |
| `docs/04_DEVELOPMENT.md` | `GX-DEV-2026-001` | DEV 任务卡、TC 用例、CI、DoD、追溯矩阵 |
| `docs/05_COMBAT_SPEC.md` | `GX-CBT-2026-001` | **对战详细规格**：属性 / 伤害 / 普攻 / 技能 / 状态 / 经济 / 装备商店 / 兵线 / 野怪 / 中立目标 / 塔与源核 / 死亡重生 / 操作与自动寻路 / 表现层 / 完整数值表；**§16 表现层规范、§22 玩法详细设计、§23 M2 待办差距、§24 寻路 M2 规格与验收（已落地）** |
| `docs/06_VISUAL_SPEC.md` | `GX-VIS-2026-001` | **表现层规范**：视觉方向 / 色彩 / 术语 / 命名 / 四阶段（P-V0~P-V4）交付标准与验收 / 性能约束 |
| `ASSET_REGISTRY.md` | `GX-ASSET-REG-2026-001` | 资产来源登记（表现层全部代码生成） |
| `docs/TERM_DENYLIST.txt` | — | 表达层禁用词表（`TC-COMPLIANCE-01` 扫描源） |

## 2. 产品定位

| 维度 | 决策 | 当前状态 |
|---|---|---|
| 玩法内核 | 复刻 **3v3 半场对战 PC 端**：3v3 半场对抗、路点推进、哨塔与源核、野区、装备与增益、加时衰竭 | 服务端闭环已落地，客户端可见结果待完善 |
| 美术风格 | **MC（Minecraft）方块 / 体素风格**：角色、地图与所有建模实体统一以方块 / 像素化表达，不追求高保真 3D 模型 | 已落地基础版，**明确偏粗糙，下一阶段重点细化**（§8） |
| 音效配乐 | **8-bit 程序化合成**：方波 / 三角 / 噪声 + ADSR + 4-bit 量化，内存 PCM 运行期生成，不引入任何外部音频资产 | 已落地（P-V4）；headless / 无声环境静默降级 |
| 资源策略 | **零外部资产**：几何 / 纹理 / 图标 / 特效全部代码生成；唯一例外是运行时借用宿主 OS 已有中文字形（不随包分发），且必须有 ASCII 回退 | 已落地，合规门禁背书 |

## 3. 工作空间

Edition 2024，工具链锁定见 `rust-toolchain.toml`（Rust 1.99.1）。

| crate | 用途 | 对外二进制 |
|---|---|---|
| `game-core` | 共享组件、协议适配、确定性数学、战斗系统、地图数据 | — |
| `client` | Bevy 客户端：网络 / 输入 / 预测 / 插值 / 体素渲染 / 局内 UI | `gx-client` |
| `server` | Headless 战斗服务端（权威模拟） | `gx-battle` |
| `matchmaking` | `axum` 匹配服务（内存队列） | `gx-matchmaker` |
| `gx-net` | 纯协议层：版本协商、增量快照、幂等事件、传输抽象、视野策略 | — |
| `gx-config` | `ConfigSet`、内容 hash、结构校验、热更 | — |
| `gx-proto-content` | 自研占位内容表（英雄 / 技能 / 装备 / 目标 / 地图 / 经济 / 视野） | — |
| `gx-e2e` | 端到端与合规测试宿主 | — |

> M0+M1 阶段现有 4 个 crate 名保持不变，仅对外二进制与描述去关联；完整重命名随 M2 的 `game-core → gx-sim` 迁移完成。

## 4. 构建与运行

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release --locked --workspace
```

```bash
# 终端 1：战斗服务端（UDP 0.0.0.0:5000，headless）
cargo run -p server --features offline-dev

# 终端 2：客户端（连接 127.0.0.1:5000）
cargo run -p client --features offline-dev

# 终端 3（可选）：匹配服务（HTTP :8080）
cargo run -p matchmaking
```

`offline-dev` 保留「单连接即开局」的调试能力（PvE 单人对局即基于此），发布构建必须禁用该 feature（CI 有门禁）。

## 5. 操作方案（v1.3）

**纯点击控制**：英雄移动只由鼠标 / 触摸点地驱动（`GX-CBT-2026-001` §3 v1.3）。v1.2 的 WASD / 方向键直控已移除 —— 两套移动源同帧互抢路径是「走路随机乱晃」的主因。

| 操作 | 效果 |
|---|---|
| 左键点地面 | 自动寻路（A*）到该点，到点后待命；兵线堤墙参与 A* 障碍，坡道开口可通行 |
| 左键点敌方单位 | 仅选中查看信息，不移动、不攻击 |
| 右键点地面 | 立即移动，打断当前施法前摇 / 普攻后摇，清除锁定 |
| 右键点敌方单位 | 锁定普攻目标，自动追击到射程内并持续普攻 |
| Q / E / R / F | 进入技能瞄准；左键确认施放，Esc 或再按同键取消 |
| P | 商店 |

键盘移动优先级高于自动寻路；触摸端以点触替代左键、长按替代右键，不提供虚拟摇杆。

> v1.1「键盘不得产生移动」已作废；`TC-CBT-01` 判据已翻转为逐键断言方向，`NAV-M2-02-C8` 改为「节流 / 寻路改造不得削弱键盘移动优先级」。

## 6. 表现层现状（MC 体素，零外部资产）

**全部使用代码生成，不依赖任何外部资源，采用 Minecraft 方块 / 体素风格**（`GX-CBT-2026-001` §16，PRES-3 v1.2）。
本章按**代码实际状态**书写，与规格文档 §16 的旧表述不同处以代码为准，规格升版已列入下一阶段。

### 6.1 已落地

| 层 | 实现 |
|---|---|
| 英雄模型 | `crates/client/src/voxel.rs::spawn_voxel_hero`：固定 12 个 `Cuboid`（双腿 / 躯干 / 双臂挂在空肩枢轴 / 头 / 双眼 / 4 条队伍脚环）+ 可变头饰、武器、披风、肩甲，合计约 **13~19 个方块 + 2 个枢轴**；父实体挂 `HeroRig{torso, head, right_arm, left_arm}` |
| 外观区分 | `skin % 6` 决定躯干 / 四肢 / 头三色，队伍色只作用于 `accent` 与脚环；偶数壮硕 / 奇数修长；**6 型头饰**（王冠 / 双角盔 / 大帽宝石 / 竖冠 / 兜帽尖顶 / 光环）、**6 型武器**（长剑 / 战斧 / 巨棒 / 长枪 / 法杖法球 / 巨锤）；`skin=9` 为石质训练假人 |
| 纹理 | `PixelKind` 12 种、`make_pixel_texture` 生成 16×16 RGBA，`Nearest` 采样，`voxel_material` 统一 `StandardMaterial`；每英雄独立 5 套材质，头部纹理以 `skin` 为种子 |
| 地形与建筑 | 32×16 大平板三材质轮换（2:1 长方形）、4 面边界石墙、**兵线南侧 4 段分隔石堤 + 段端门柱**、沿中轴 `x = 0` 的纵向河道与下沉河床 / 水面、兵线跨河桥面石台、纵向河岸、中央矿坑、正下方共用野区商店广场、单条水平兵线路点条带、基地石台、草丛；哨塔 3 层 / 源核 5 层递减方块堆叠 + 金顶 |
| 非英雄实体 | 小兵 3 方块模型随快照创建 / 回收；野怪随快照建模、可右键锁定并显示血条；训练假人复用石质英雄模型 |
| 战斗特效 | `CombatFxQueue` + 13 个 `tick_*` 系统：近战挥击三段曲线 / 刀光 / 延迟火花、远程弹道插值、暴击大刀光、碎片抛物线、`HitFlash`、`DamageFlash`、`CastEffect`；血条 3 层 billboard + 掉血 lag 残影（英雄与野怪 / 小兵通用）；4 类方块瞄准 / 选中标记 |
| 大厅与流程 | `AppState{Lobby, InMatch}` + `lobby_ui::draw_lobby` 锁定视口的三列大厅（模式 / 英雄档案 / 详情）；HUD 按状态门禁；连接延后到 `OnEnter(InMatch)`；详见 §6.4 |
| 地图 | **`MAP_WIDTH × MAP_HEIGHT = 480 × 240`（2:1 横向长方形，`MAP_SCALE = 1.5` 等比放大）**，蓝方左端 / 红方右端；单条兵线沿北侧 `LANE_Z = -60` 水平横贯左右，**只在兵线南侧（靠野区一侧）有 4 段实体石堤，在 5 处对称坡道开口连通**；野区在兵线南侧，纵向河道沿中轴 `x = 0` 把野区切成左右两半，与兵线走向**正交**，交点 `BRIDGE_Z = LANE_Z` 为全速桥面；兵线中点正下方是中央矿坑，正下方中央是双方共用野区商店，左右下角各一座兵矿；每队 **5 座哨塔**（3 线上 + 2 守家）；整图关于中轴 `x = 0` **左右镜像对称**（`mirror_fold(x,z) = (-x,z)`），红蓝基地 / 路点 / 塔 / 营地 / 草丛 / 墙段 / 坡道逐项对等；`collision_system` 做动态互推与静态体推出（含墙） |

### 6.2 尚未落地 / 当前粗糙点

> 用户反馈「当前 MC 过于粗糙」对应的具体技术缺口，逐条如下；这些是 §8 路线的输入。
> **状态：上述 9 项已由 P-V1~P-V4 全部关闭**（第 3 项的 2D HUD 为规范允许例外）。
> 第 4、5、8 项由 P-V4 落地：非英雄动态骨架、7 层地形与河道、8-bit 程序化音频。
> 逐项验收见 `docs/06_VISUAL_SPEC.md` §9~§12；下表保留原始缺口描述作为设计依据。**

1. **纹理几乎无信息量**：只有 4×4 色块级哈希点缀，无逐像素噪声 / 描边 / 明暗面 / AO；所有面共用一张贴图，明暗完全依赖全局光；`PixelKind::Water` 定义后从未使用。
2. **英雄没有朝向与角色动画**：仅攻击时旋转两个肩枢轴 0.34 s，`HeroRig.torso/head` 全程闲置；`facing_system` 只改整体 `Transform.rotation`，而眼睛固定朝 +Z；走路摆臂 / 待机呼吸 / 施法前摇后摇 / 受击硬直 / 死亡全无。
3. **违反自身 PRES-4 体素约束**：弹体是 `Sphere`、血条是 `Rectangle`、小地图大量 `circle_*`。
4. **非英雄实体 3D 表现基本缺失**：客户端只建模 Hero / Minion / Projectile；塔、源核、野怪、守卫没有实体模型，全是地形里的静态装饰，不随血量或存活变化。
5. **地图极薄**：12×12 平板无高度起伏、无河道（代码注释明确「该地图没有河道」），兵线仅贴地条带，草丛每丛 5 方块；而旧文档仍写「2×2 网格 / 水块 / 河道」。
6. **事件视觉覆盖窄**：`handle_game_event` 只处理 6 类，其余（击杀 / 占领 / 塔毁 / 野怪死亡 / 波次 / 加时 / 胜负 / 护盾 / 状态）落入 `_ => {}`；`MatchStart/MatchEnd` 仅日志，无胜负结算面板。
7. **技能表现与技能数据脱节**：4 个技能（火焰弹 / 寒冰新星 / 能量光束 / 自我治疗）共用同一个通用 `CastEffect` 方块，忽略 `ability_index`（**协议已携带**）/ `cast_type` / `aoe_radius` / `damage_type`；两套 `AbilityDef` 与 `target_kind` / `CastType` 并存。
8. **8-bit 音频曾零实现**：客户端无音频依赖与合成代码；此前根 `ASSET_REGISTRY.md` 与 `docs/ASSET_REGISTRY.md` 两份登记表对音频状态互斥（一个写「已提供」一个写「待资产」）。P-V4 已以内存 PCM 合成器落地，两份已合并为单一 `ASSET_REGISTRY.md` 并统一为「已提供」。
9. **无材质复用池**：`voxel_material` 每次调用新建一张 `Image` 与材质，地形循环内重复生成。

### 6.3 零外部资产口径与门禁

- UI：egui 引擎内置字形 + 方块化面板；**唯一例外**是运行时探测宿主 OS 自带中文字体并注入 `FontDefinitions`（不随发布包分发任何字体文件），探测失败时全部文案回退 ASCII。
- 音效：P-V4 已按 §8.6 以纯代码合成器落地（方波 / 三角 / 噪声 + ADSR + 4-bit 量化，内存 PCM 直接交给引擎音频后端）；不引入任何音频文件，也不通过字节内嵌宏引入音频；无声 / headless 环境静默降级。
- 门禁：`TC-CBT-12` 与 `TC-COMPLIANCE-01/02` 扫描断言源码中无外部资源路径、发布树不含 `.ttf/.ttc/.otf/.png/.jpg/.wav/.ogg` 等二进制、根无美术 / 音频资源目录、无字节内嵌宏；资产状态见 `ASSET_REGISTRY.md`。

### 6.4 大厅与信息层（P-U1）

本轮补齐「刚进入没有大厅 / 没有英雄信息 / 商店道具信息不全 / 缺少 PVP-PVE 模式选择」四项：

| 项 | 实现 | 位置 |
|---|---|---|
| 大厅首屏 | `AppState` 默认 `Lobby`；**顶层无标题栏 `Window` 锁定整个视口**（替换旧 `Area` + painter 的静态示意图），三列布局：模式 / 英雄列表 / 英雄详情；egui 0.36 的 `CentralPanel::show` 只接受 `&mut Ui`，顶层必须用 `Window::show(ctx, ..)` | `crates/client/src/lobby_ui.rs::draw_lobby` |
| 英雄档案 | `HERO_PROFILES[6]`：中英名、定位、描述、外观、五项定性评分、打法提示、推荐出装；技能组**数值**读 `get_ability_def()`，文案来自 `ABILITY_NOTES_*`（不复制第二套平衡数值）；局内 HUD 徽标与技能栏悬停同步显示 | `lobby.rs` / `lobby_ui.rs` / `ui.rs` |
| 道具图鉴 | `ITEM_PROFILES[12]` 与 `ShopCatalog::placeholder()` 一一对应（单测锁定），补齐中文名 / 分类 / 描述 / 属性加成文本 / 合成前置；大厅「道具图鉴」窗口按 5 类分组展示 | 同上 |
| 局内商店 | 按 `P` 打开，12 件真实道具 + 分类筛选 + 金币显示；「购买」→ `ClientMessage::PurchaseItem`，「售出栏位」→ `SellItem`；`ShopToast` 给出提交回执，返还与栏位仍由服务端核算 | 同上 / `networking.rs::send_client_commands` |
| PVE / PVP | 大厅两个可点模式；`MatchMode::code()` 编码，客户端 `Hello` **同批**上行 `ClientMessage::SelectMode`；服务端仅在 `MatchPhase::Warmup` 接受，PVP 需两名真人连接才 `Running`，且 `ensure_pve_bot` 不生成 AI 对手 | `game-core/clock.rs` / `net.rs` / `server/networking.rs` / `server/lib.rs` |

**时序护栏**：`ensure_pve_bot` 在 `FixedUpdate` 按默认 PVE 判断是否补 AI，若等客户端连上后再单独发模式消息会抢先生成。因此模式选择与 `Hello` 同一批 `ReliableOrdered` 消息上行，服务端 `handle_session_messages` 与 `handle_gameplay_messages` 两处都能处理（覆盖认证前 / 认证后 / 重连三种到达时机）。

**裁决 / 表现隔离**：英雄档案是**展示层数据**，局内外观 `skin` 仍由服务端 `HeroSpawn` 权威下发；大厅选择不改动任何裁决数值，也不写 `Hero` / `TransformSync`。`ClientMessage` 仅在末尾追加 `SelectMode` 变体，客户端与服务端同仓编译，`PROTOCOL_VERSION` 与 `content_digest()` 不变。

**大厅与对局场景解耦**：启动期只生成「承载 egui 主上下文的相机 + 灯光」（`spawn_game_scene`），**不生成任何游戏世界内容**；体素地形 / 哨塔 / 源核 / 各类标记改由 `OnEnter(AppState::InMatch)` 的 `spawn_match_world` 生成。因此大厅阶段后景是空 3D 场景（由全屏 `Window` 完全覆盖），玩家点「进入对局」后才开始渲染地形与实体，消除「大厅是个弹窗、后景是对局」的观感。护栏单测 `lobby_startup_renders_only_camera_not_match_world` 锁定此不变量。

### 6.5 战斗体验修复（P-U2）

针对「敌人死亡只隐藏血条 / 河道桥面无差异 / 小兵不可锁定 / 普攻存疑 / 经济重复发奖」五项缺陷，本轮的当前实现与修复：

| # | 缺陷 | 当前实现（修复前） | 修复方案 | 位置 | 验证 |
|---|---|---|---|---|---|
| 1 | 敌人死亡应消失 / 播动画并移除碰撞体 | 小兵/野怪死亡后服务端 `despawn`，但客户端缺「消失动画」资源，仅从快照消失（看似瞬间消失）；英雄死亡走 `HeroDeath` 不 despawn（正确） | 新增 `DyingMinions` 资源 + `DYING_TIME=0.4s`；`sync_minions` / `sync_monsters` 检测「快照中实体消失」即登记为缩小消失动画，按 `scale = 剩余/0.4` 收缩、计时结束 `despawn`（碰撞体随之移除）。服务端 `world.rs` 同时广播 `NetEvent::MinionDeath/JungleMonsterDeath` → 客户端 `GameEvent`（野怪额外触发碎裂特效） | `client/interpolation.rs`、`client/entity_visual.rs`、`server/world.rs`、`game-core/gameplay/mod.rs`、`server/net_bridge.rs` | `should_start_dying` 单测；e2e `gameplay_systems_drive_a_full_ecs_match_slice` 死亡/重生断言 |
| 2 | 河道与桥面不同移动效果 | 地图仅有 `PixelKind::Water` 水面视觉，无任何移动惩罚 | 新增 `terrain_speed_mult(pos)`：沿中轴的纵向河道（`river_cross(pos)=\|x\| ≤ 8` 且 `river_axis(pos)=z ≥ RIVER_START`）减速为 0.5；兵线跨河的桥面（`\|z − BRIDGE_Z\| ≤ 10`）保持全速——即**河道浅滩减速、桥面全速**的差异化；仅改速度，不改裁决平面、不阻挡通行。英雄（`systems.rs`）、小兵 AI（`world.rs`）与客户端预测（`prediction.rs`）均乘该系数 | `game-core/map.rs`、`systems.rs`、`world.rs`、`client/prediction.rs` | `longitudinal_river_slows_movement_but_lane_bridge_is_full_speed` 单测 |
| 3 | 小兵可锁定与攻击 | 客户端小兵可视实体**缺 `Selectable`**，右键锁定查询只命中英雄，无法选中/锁定小兵 | 小兵 `MinionVisual` 生成时补挂 `Selectable{radius:0.8,height:2.0}`；右键锁定改为遍历所有 `Selectable`（含小兵）取最近目标。`world.rs` 的攻击系统本就处理小兵/野怪目标并结算 `gold_reward/exp_reward` | `client/interpolation.rs`、`client/input.rs`、`server/world.rs` | 结构校验 + e2e 小兵击杀金币 |
| 4 | 英雄普攻是否存在且生效 | `attack_system` 持续置位普攻意图 → `AttackCommand` → `damage_system` 对英雄/小兵/野怪结算伤害；英雄致死走 `HeroDeath` | 确认普攻链路完整：e2e 中以 1000 伤害 `AttackCommand` 击杀英雄，断言 `health=0`、`RespawnState.dead`、`GameplayEvents::deaths` 含 killer/victim | `game-core/systems.rs`、`server/world.rs`、`server/gameplay.rs` | e2e 致命攻击断言 |
| 5 | 经济：来源/显示/消费 | 英雄击杀时**重复发奖**：`systems.rs` 即时 `+50 金币/+100 经验` 与 `kill_bounty`（基础 180+连杀）叠加 | 移除即时 `+50/+100`，击杀奖励统一由 `death_respawn_system` 经 `kill_bounty` 结算。来源 = 被动 1.3/s（`passive_gold_system`）+ 击杀英雄 `kill_bounty` + 击杀小兵/野怪 `gold_reward`；显示 = HUD 金币；消费 = 商店购买/售出（`P-U1` 已贯通） | `game-core/systems.rs`、`server/gameplay.rs`、`game-core/systems.rs`、`client/ui.rs` | e2e 金币断言由 930 修正为 **880**（1000+180−300） |

**已知限制 / 后续**：消失动画依赖「快照中实体消失」判定，在极端视锥裁剪下可能与真实死亡偶发混淆（小地图全可见场景无碍）；兵线跨河桥面现以 `LAYER_FOLD` 石质平台表示，河岸为沿纵向中线的两条窄石带。

本轮（P-U3）另修复四项体验缺陷：

| 缺陷 | 根因 | 修复 |
|---|---|---|
| 血量归零后人不消失、仍可控制、很久才回泉水 | 服务端只发 `HeroDeath` 不隐藏实体；重生只改 `Transform` 未写 `TransformSync`；客户端无输入门禁 | 服务端以权威血量统一登记死亡（覆盖技能 / 野怪 / 哨塔致死）并去重广播，死亡瞬间吸附泉水且同步快照；客户端按快照整株隐藏、冻结全部输入并显示复活倒计时；重生写回血量 / 位置 / 速度并恢复可视体 |
| 走路随机乱晃 | 20Hz 快照在 60Hz `FixedUpdate` 中被消费 3 次；服务端 tick 与本地 tick 混用裁掉输入历史；朝向被过期快照 slerp；寻路网格只覆盖中心 128×128 | 快照对账按 tick 去重、移动中不做小修正、朝向由本地 aim 独占；预测与服务端共用 `terrain_speed_mult`；动画速度低通 + 移动态滞回；网格扩为覆盖整图（P-U4 后为 64×32 分轴网格覆盖 320×160） |
| 野怪无法攻击、没有血条 | `send_input` 从 `mask = 0` 起算丢掉 bit0 普攻位；服务端快照完全不下发野怪；视野半径 18 会把远处营地裁掉 | 普攻位透传（纯函数 + 单测）；野怪随兵线查询一起进快照、视野豁免、生成时补挂 `Selectable`；`WorldHealthBar` 让野怪 / 小兵共用英雄的三层血条 |
| 地图不像经典 3v3 | 一条斜穿中心的兵线 + 竖直 `x=0` 河道 | P-U3 先改为单条贴边的 L 形兵线 + 沿 `z = x` 与兵线走向正交的斜向河道；**P-U4 已进一步改为 2:1 横向长方形布局**（见下） |

### 6.6 横向长方形地图（P-U4）

用户指出正方形地图不符合参考总览图。根因不是技术限制，而是**上一轮选择的对称方案把尺寸锁死了**：为保公平采用「沿对角线 `z = x` 镜像」，该变换 `(x,z)→(z,x)` 只有在正方形里才把地图映回自身；一旦变成长方形，宽高互换即越界。于是 `MAP_WIDTH == MAP_HEIGHT == 240`，且 `MAP_HALF` 一个常量通吃两轴，从寻路网格、地面拼贴到小地图都顺着这个假设写死。

按参考总览图重做后的骨架：

| 项 | 实现 |
|---|---|
| 尺寸 | `320 × 160`（2:1 横向长方形），`x ∈ [-160,160]`、`z ∈ [-80,80]`；新增 `MAP_W_HALF / MAP_H_HALF` 分轴半径，`MAP_HALF` 降级为 x 轴兼容别名 |
| 兵线 | 唯一兵线沿北侧 `LANE_Z = -40` **水平横贯左右**，11 个路点从蓝方左端 `(-140,-40)` 到红方右端 `(140,-40)`，中心 `(0,-40)` 为跨河桥面 |
| 河道 | 沿中轴 `x = 0` **纵向**贯穿，半宽 8；方向与兵线 `(1,0)` 正交；桥面 `\|z − BRIDGE_Z\| ≤ 10` 全速，其余河段减速一半 |
| 野区 | 位于兵线南侧、被河道分成左右两半；每半 4 座营地（树精 / 大型野怪 / 小型野怪 / 兵矿），兵矿在左下与右下角 |
| 共用目标 | 中央矿坑在兵线中点正下方、河道之上 `(0,8)`；野区商店在正下方中央 `(0,64)`，双方共用 |
| 哨塔 | 每队 5 座（3 座沿兵线、2 座守卫源核），红方逐座为蓝方的中轴镜像 |
| 公平性 | 关于中轴 `x = 0` **左右镜像对称**（`mirror_fold(x,z) = (-x,z)`），长方形下仍严格成立；基地 / 路点 / 塔 / 营地 / 草丛逐项配对，单测锁定 |

同步改动：`game-core/map.rs` 全量重写；`client/pathfinding.rs` 改 64×32 分轴网格；`client/voxel.rs` 改 32×16 地面拼贴、纵向河岸与分轴边界墙，并补共用商店广场；`client/ui.rs` 小地图改 228×114 长方形且分轴归一化、大厅示意图重画；`server/systems.rs` 的 aim 越界校验与 `client/prediction.rs` 的边界钳制均改分轴；`gx-proto-content` 的 `MapDef` 同步为 320×160。

护栏单测：`whole_map_is_uniformly_scaled_landscape_rectangle`、`lane_is_horizontal_and_river_is_orthogonal`、`classic_landmarks_are_in_the_right_places`、`map_is_mirror_symmetric_about_center_axis`、`grid_covers_whole_rectangular_map`、`minimap_normalizes_both_axes_for_a_landscape_map`、`river_is_longitudinal_and_orthogonal_to_lane`。

> 注：本小节的 `320 × 160` 及所有坐标是 **P-U4 当时的基准布局**；P-U5 已把它整体等比放大 1.5 倍，见下。

### 6.7 等比放大与兵线 / 野区分隔墙（P-U5）

用户指出参考总览图里**兵线和野区之间是有墙的**，并要求地图再等比扩大一圈。两件事一起做，因为放大后才有足够空间让堤墙、坡道与野区各自成立。

**等比放大**：把 P-U4 的布局抽成「基准坐标」，全部经单一常量 `MAP_SCALE = 1.5` 派生 —— `s(v)` 缩放标量、`p(x,z)` 缩放坐标。改这一个常量即可整体放大 / 缩小，尺寸、11 个路点、10 座营地、10 座塔、12 丛草丛、河道半宽、兵线宽度、墙段与坡道会一起变，不会破坏 2:1 比例和左右镜像。

| 项 | P-U4 | P-U5 |
|---|---|---|
| 尺寸 | `320 × 160` | **`480 × 240`** |
| 基地 | `(-140,-40)` / `(140,-40)` | `(-210,-60)` / `(210,-60)` |
| 兵线 / 桥面 z | `-40` | `-60` |
| 河道半宽 / 桥半长 | `8` / `10` | `12` / `15` |
| 寻路网格 | 64×32，每格 5 | 64×32，每格 **7.5**（由 `MAP_WIDTH` 派生，自动跟随） |
| 地面拼贴 | 32×16，每块 10 | 32×16，每块 **15** |

只有**地图尺度量**随缩放变化；英雄 / 建筑模型尺寸、碰撞半径、攻击距离、视野半径等**物件尺度**数值保持不变 —— 放大得到的是更开阔的空间与更长转线，而不是把角色一起放大。

**分隔墙**：`game-core/map.rs` 新增唯一真相源 `LANE_WALLS` 与 `LANE_WALL_GAPS`（5 处坡道开口：两端基地入口、两处中段坡道、中央河道交汇口）。P-U5 初版在兵线南北两侧各放了 4 段；**P-U6 按用户反馈删掉北侧 4 段**，现在只在兵线靠野区的南侧保留 4 段 —— 兵线北侧就是地图边界方向，不需要墙。小兵不会跑进野区，英雄转线必须走坡道，路径可被预判。全部墙段与开口关于 `x = 0` 逐项镜像，双方转线距离完全对等。

服务端 `collision_system` 只支持**圆形** `Collider`，所以 `wall_colliders()` 把每段长墙采样成一串连续圆（间距 < 直径，段端封口）。五个消费方全部读同一份数据：

| 消费方 | 接入点 |
|---|---|
| 服务端权威碰撞 | `server/spawn.rs::spawn_lane_walls` 生成 `Collider + StaticCollider`（不挂 `NetId`，不进快照） |
| 客户端本地预测 | `client/prediction.rs::static_obstacles()` 追加同一批圆，避免贴墙回滚抖动 |
| 点击寻路 A* | `client/pathfinding.rs::lane_wall_cells()` 栅格化成阻挡格；`PathState::set_goal` 传入（旧版恒传 `&[]`），并排除起终点格以免贴墙时无法移动 |
| 3D 体素渲染 | `client/voxel.rs` 按段渲染石墙 + 压顶 + 段端门柱，门柱标出坡道入口 |
| 小地图 / 大厅示意 | `client/ui.rs` 用同一批段绘制，坡道断口可见 |

护栏单测：`whole_map_is_uniformly_scaled_landscape_rectangle`、`lane_walls_are_a_mirrored_barrier_with_passable_gaps`、`wall_colliders_form_a_continuous_barrier`、`lane_walls_are_rasterized_with_open_gaps`、`astar_detours_through_wall_gaps`、`lane_walls_are_rendered_from_the_authoritative_segments`，以及草丛视野测试改为从 `BUSHES` 取坐标（不再硬编码，等比放大后仍成立）。

### 6.8 单侧墙、涉水伤害与普攻可读性（P-U6）

用户确认分隔墙本身没问题，但提出三点：**兵线上方（北侧）不必有墙**、**过河应该受伤**、**普攻看不出是怎么打出去的**。

**① 只有南侧一道墙**：从 `LANE_WALLS` 删掉北侧 4 段（并删除 `WALL_NORTH_Z` 常量），现在只剩南侧 4 段。服务端静态碰撞、客户端预测、A* 阻挡格、体素渲染、小地图 / 大厅示意全部读同一份常量，删段后自动同步，无需分别改 6 处。新增护栏断言「所有墙段 z 必须大于 `LANE_Z`」「兵线北侧那一带不得存在任何墙格」。

**② 涉水伤害**：河道此前只有 0.5 倍减速，没有任何代价。现在 `game-core/map.rs` 提供 `is_hazardous_river(pos) = is_river && !is_bridge`：

| 项 | 规则 |
|---|---|
| 危险区 | 河道浅滩（与减速区**完全一致**）；桥面 `\|z − BRIDGE_Z\| ≤ 15` 是唯一安全过河点 |
| 伤害 | 每 30 tick（0.5 s）扣 `RIVER_HAZARD_DAMAGE = 25` 真实伤害 = 50 HP/s；横穿一次约吃 300 点 |
| 作用对象 | 只有**英雄**；小兵沿兵线走桥、野怪是固定设施，都不被环境伤害磨死 |
| 免伤 | 阵亡中 / 复活无敌期（`invulnerable_until_tick`） |
| 裁决 | `server/systems.rs::river_hazard_system` 只写 `Hero.health`；淹死由既有的 `death_respawn_system` 以权威血量统一登记，走正常复活与 `HeroDeath` 广播 |
| 协议 | 新增 `GameEvent::EnvironmentDamage`（`involved_net_ids` 只含目标）。**不能复用 `DamageDealt`** —— 那条链路会驱动攻击者的挥击 / 刀光，变成「自己打自己」 |
| 客户端 | 冷蓝色伤害飘字 + 受击闪光；本地英雄站在危险水域时 HUD 显示「河水灼烧 −50 HP/s · 请走桥面过河」，判据与服务端同源 |

**③ 普攻可读性**：旧实现里挥击 / 刀光 / 弹道**全部**由服务端 `DamageDealt` 事件驱动，而 `attack_system` 只在「射程内有目标且造成伤害」时才产生该事件。于是右键锁定后若目标还差几步、或打空了，玩家看不到任何攻击动作；命中了也要等一整个 RTT 才播。同时 `CombatFxRequest.damage` 被 `let _ = req.damage;` 丢掉，没有伤害数字。

| 缺口 | 修复 |
|---|---|
| 起手滞后 / 挥空无表现 | `rendering.rs::local_attack_anticipation` 用本地输入按攻速即时插 `AttackAnim` 并朝锁定目标（或鼠标落点）甩出短刀光；服务端命中回包再刷新挥击并播完整刀光 / 弹道 |
| 打中了多少看不出 | `FloatingTexts` 世界空间飘字：以 `NetId` 为锚点（目标移动时数字跟着走），由 egui HUD 层经相机投影绘制，上浮淡出，同屏上限 40 条；暴击加 `!` 并换橙色 |
| 环境伤害与普攻混淆 | `CombatFxRequest.attacker` 改为 `Option<NetId>`；`None` 时渲染层只播目标端火花 + 冷蓝色数字，绝不触发挥击 |

护栏单测：`hazardous_water_is_the_slowed_river_except_the_bridge`、`river_hazard_tests::*`（浅滩扣血 / 桥面·陆地·非周期 tick 安全 / 阵亡与无敌免伤 / 不会扣成负血）、`damage_labels_are_readable`、`floating_texts_are_anchored_and_budgeted`、`environment_damage_has_no_attacker`、`lane_walls_are_rasterized_with_open_gaps`。

### 6.9 基础经济与商店点击穿透（P-U7）

用户要求：英雄要有**基础经济**（开局即可买第一件小件），且**打开商店后点击不应穿透去控制英雄移动**，开局阶段同样如此。

**① 基础经济**：
- `game-core/components.rs` 新增唯一常量 `STARTING_GOLD = 300`（等于商店最便宜的起始小件价）与 `PASSIVE_GOLD_PER_SECOND = 3.0`；`Hero::default().gold = STARTING_GOLD`，所有英雄（玩家 / PvE 对手 / 客户端镜像）与所有 spawn 路径（`world.rs` / `spawn.rs` 用 `..default()`）自动继承，无需逐处手写。
- 旧 `passive_gold_system` 是 `hero.gold += (1.3 * dt) as u32`：每 tick（60Hz）不足 1 金被 `u32` 截断成 **0**，且没有累加器 → 被动收入空转。现用 `Local<f32>` 攒小数余量，每 20 tick 发放 1 金，不丢精度、不与整数快照 delta 冲突；所有英雄收入对等。
- 与配置层 `EconomyDef.passive_gold_per_tick = 0.05` 在 60Hz 下等价（0.05 × 60 = 3.0），`GameEvent::GoldChanged` / `collect_entities` / `sync_hero_stats` 链路不变，HUD 自然显示 300 起跳、随时间增长。

**② 商店点击穿透**：根因是 `lobby_ui::draw_shop` 在 UI pass 里用**自己的 `Local<bool>` + P 键切换**，与 `handle_input` 的冻结判定不是同一份状态，二者会失同步；而 `handle_input` 旧有的 `over_ui` 又依赖**上一帧**的 egui 命中布局、egui 主上下文未就绪时直接返回 false，于是「开局 / 开商店」时点击穿透到地面。

修复：引入单一真相源 `ShopOpen(pub bool)` 资源，由 `input::update_input_gate`（在 `Update`、早于 `handle_input`）统一切换 P 键并算出 `InputGate.frozen = !in_match || !hero_ready || modal_open`；`handle_input` 据此**当帧**冻结，并清空残留的 `MoveTarget` / 路径 / 待发施法，避免英雄沿旧目标继续走。`draw_shop` 改用 `ResMut<ShopOpen>`（`.open(&mut open.0)` 同时支持窗口 X 关闭），仍存在的归档占位 `ui::draw_shop`（重复 P 键处理）已删除。`over_ui` 保留作对局内的二级防护。

护栏单测：`every_hero_starts_with_purchasable_gold`、`passive_gold_accrues_every_twenty_ticks`、`input_frozen_rules`、`update_input_gate_maps_shop_open_to_frozen`。

### 6.10 纯点击控制、寻路节流、野怪攻击与服务端校验（P-U8）

用户选定用「**去掉 WASD、只用点击控制**」替代原来的键盘/点击互斥方案，并要求补齐野怪攻击与服务端合法性校验。

**① 纯点击控制（操作方案 v1.3）**：从 `handle_input` 删除整个键盘方向分支。旧版「键盘清 `MoveTarget`/A* 路径 → 同帧左键又写入 → 下一帧键盘再清」的交替，是「走路随机乱晃」的决定性成因。现在左键点地、右键 force、触摸点触三条分支**只登记一个 `move_request`**，帧末统一规划一次并计算方向；键盘仅保留 Q/E/R/F/Esc/P 等非移动功能。`update_egui_input`、HUD、大厅说明、`gx-e2e/tests/e2e_movement.rs` 全部改为注入 `MoveTarget`（headless 无 Window/Camera，无法真实投射鼠标射线）并新增「键盘不得移动」护栏。

**② 到达阈值随格尺寸 + A* 重算节流**：
- `ARRIVE_RADIUS = pathfinding::CELL_SIZE * 0.2`（1.5），取代硬编码 `0.4`；地图等比放大后自动跟随。
- 新增 `path_move_dir`：在**同一帧内循环跳过**所有已进入到达半径的 waypoint。旧实现到达帧直接返回 `Vec2::ZERO`、下一个点要等下一帧，每过一个路径点顿一下 —— 这是乱晃的第二成因。
- `PathState::request_goal`：同目标（`< GOAL_EPS = 0.5`）连点且路径未失效时不重算；目标明显变化立即重算；右键 `force` 当帧强制重算（`NAV-M2-02-C4`）。`lane_wall_cells()` 的静态阻挡格改用 `OnceLock` 缓存，避免每次点击重扫 64×32 格并 `sort/dedup`。

**③ 野怪攻击冷却 + 静止型反击**：旧版 `monster_attack_system` 要求 `state == Aggro`，而 `step_monster` 让 `move_speed = 0` 的征募井 / 两座辉矿恒为 `Idle` → 3 个营地永不反击；同时无冷却，每 tick 插一次 `AttackCommand` 并当 tick 消费 = 60 次/秒。
- `JungleMonster` 新增 `attack_interval_ticks` / `next_attack_tick`，按权威 `SimTime::tick()` 裁决（0 兜底 60 tick）。
- 静止型放开 Aggro 门槛，射程内有存活英雄即反击。
- 修正攻击射程：静止型 2.0、近战型 1.5 都小于双方 `Collider` 推出距离（野怪 2.0 + 英雄半径），碰撞解算后永远打不到人；现按类型提到 4.5~6.0。

**④ 服务端速度与碰撞校验（`NAV-M2-03`）**：新增 `game-core/src/navigation.rs`，把 §3.3「服务端负责合法性」落到代码。
- `sanitize_move_dir` 在 `apply_pending_inputs` 写入 `PlayerInput` **之前**清洗上行方向：NaN / Inf 一旦进入积分会永久污染权威坐标与之后所有快照，超长向量按方向归一化、不采纳长度意图。
- `validate_movement(prev, proposed, move_speed, dt)` 在 `movement_system` 积分后调用，按危害顺序判定 `InvalidInput / Teleport / OutOfBounds / SpeedExceeded / WallCollision`；拒绝时回退到 `prev`（越界钳制到边界内），由下一次权威快照让客户端回滚预测。`move_speed` 取服务端 `Hero` 值，不信任客户端。
- 墙判定直接复用 `map::wall_colliders()` 的**圆形碰撞体**（缓存），不使用 AABB 近似 —— AABB 会把段端外扩成方角、悄悄缩窄坡道开口。起点意外落在墙内时放宽墙校验让英雄自行走出，避免永久锁死。P-U9 起另叠加 `NavGrid` 的**动态障碍层**（见 §6.11）：当时未建 `ServerNavGrid` 是因为静态墙已有精确圆判定，动态障碍则用独立报文下发。
- `MovementViolations` 按实体记录连续违规，连续 3 次写 `warn`；本阶段只审计，是否断开留给 M3 反作弊策略。

护栏单测：`keyboard_does_not_move_hero_only_click_target_does`、`path_move_dir_skips_arrived_waypoints_without_zero_frame`、`request_goal_deduplicates_but_force_always_recomputes`、`e2e_movement::{click_target_moves_hero_in_z, click_target_moves_hero_in_x, keyboard_no_longer_moves_hero}`、`world::monster_attack_tests::*`、`navigation::tests::*`（正常位移 / 加速与瞬移 / 非有限输入 / 越界 / 撞墙线段 / 墙内自愈）。

### 6.11 动态障碍网格与 `TC-NAV-01~09` 完整验收（P-U9）

用户要求落地 §24 里剩下的 `NAV-M2-01` 动态障碍（哨塔 / 渊卫纳入 `walls`）与 `TC-NAV-01~09` 完整验收。

**① 双端同源 `NavGrid`**：新增 `game-core/src/navgrid.rs`。规格把 `NavGrid` 放客户端、`ServerNavGrid` 放服务端并要求「同一 blocked 数据」，本实现把**类型与几何常量下沉到 `game-core`**，两端共用同一个 `NavGrid`，同源由类型系统保证而不是靠两处代码对齐。`new()` 只烘焙静态堤墙；动态障碍只能来自 `NavObstacleSnapshot`（C1）。`blocked = 静态层 | 动态层`，`version` 仅在集合真的变化时 `saturating_add(1)`（u32 单调、不取模，C3）；`rebuild` 对同一 `server_tick` 幂等，集合变化当 tick 重建、无变化按 30 tick 周期（C4）。半径 → 格数 `ceil(r / CELL_SIZE)`：哨塔 / 源核 `r=3`、渊卫 `r=4`（C2）。

**② 服务端权威全局障碍视图**：`simulation.rs::rebuild_server_nav_grid` 每 tick 从 ECS 生成 `NavObstacleSnapshot` —— 哨塔被摧毁后实体仍以 `health=0` 常驻且继续下发，因此**存活位必须由服务端显式给出**；渊卫是死亡即 despawn 的 `JungleMonster`，存活时才带坐标。`networking.rs::broadcast_nav_grid` 通过新增的 `ServerMessage::NavGridUpdate` 在 ReliableOrdered 通道广播，**不经逐玩家视野裁剪**（视野外的塔也必须进 walls），变化立即发、无变化每 30 tick 补发以覆盖新加入 / 重连。新变体追加在枚举末尾，避免平移既有 bitcode 判别值。客户端 `ClientNavObstacles` → `sync_nav_grid` → `PathState.grid`，`version` 由本地 `rebuild` 自行裁决，不采信报文字段（C8）。

**③ 客户端 A* 改读网格**：`pathfinding.rs` 的 `astar(start, goal, grid)` 现在吃 `&NavGrid`；新增 `find_path(..., fallback)`，实现 4096 节点预算、超预算返回上一次有效路径而非空路径（C5，避免急停），以及起 / 终点落在障碍格时钳制到 ≤3 格的可通行格（C6）。禁穿角、octile 启发式、线段可见性平滑保持不变。

**④ §24.2 重算策略补齐**：`PathState` 增加 `grid_version / last_recompute_tick / stuck_ticks / stalled_until_tick / last_valid_path / recompute_calls`；新增纯函数 `should_recompute` 与单帧统一入口 `process_requests`，按 `NewGoal（含 force）> GridChanged > Stuck > Periodic > None` 裁决（C1），单 tick 最多一次 A*（C5）。`Periodic` 间隔 30 tick、待命时不触发；`GridChanged` 依 `grid.version != state.grid_version` 立即重算（C2）；连续 60 tick 位移 < 0.1 触发 `Stuck`，重算仍失败则清路径、`move_dir = 0`、静默 30 tick（C3）；重算后 `index` 按新路径最近点重定位（C7）。客户端 60Hz `ClientTick` 专门给这些计时用，不用渲染帧数或 20Hz 快照 tick。

**⑤ C7 落地到客户端预测**：`prediction.rs::static_obstacles()` 旧版把 `map::TURRET_POSITIONS` 全部写死为静态圆，塔被摧毁后客户端仍把英雄推开、与权威位置持续偏离。现按服务端存活位过滤，未收到视图时退回常量避免开局穿塔。

**⑥ `TC-NAV-01~09` 完整验收**：

| 用例 | 位置 |
|---|---|
| 01 / 02 / 03 | `client/src/pathfinding.rs::tests::tc_nav_01..03_*`（哨塔绕行 / 摧毁后直线 / 渊卫占位与同 tick 幂等） |
| 04 / 05 / 06 | `client/src/input.rs::tests::tc_nav_04..06_*`（周期 30 tick / 5 请求只跑 1 次 A* / 卡住静默） |
| 07 / 08 | `game-core/src/navigation.rs::tests::tc_nav_07/08_*`（1.06× 超速 / 塔格穿越与边缘擦过） |
| 09 | `game-core/src/navigation.rs::tests::tc_nav_09_*` + `gx-e2e/tests/nav_acceptance.rs`（抓包 `corrected_position` / 视野零下发 / 违规计数） |
| 跨 crate 接线 | `gx-e2e/tests/nav_acceptance.rs`：服务端网格 → bitcode 报文 → 客户端网格 → A*，以及 `GridChanged` 触发重算 |

**与规格的三处偏差（已在 §24.4.1 登记）**：① 规格 `NAV_GRID_N=64` 写于正方形地图时期，当前 2:1 长方形按 `64×32 / CELL_SIZE=7.5` 派生，`TC-NAV-01` 的格 `(32,32)` 对应 `(32,16)`；② 静态墙有精确圆形碰撞体，`NAV-M2-03` 的格判定只取 `NavGrid` **动态层**，否则 7.5 单位粗格会把坡道通行带凭空缩窄两格；③ 校验仍在 `movement_system` 积分**之后**（须先得到 `proposed` 才能判断位移合法性），文档 §24.3-C8 的「之前」不成立。自愈也收窄为：起点在墙圆内整段放行，起点仅在动态格内只跳过格判定、静态墙判定仍生效，避免「出生在源核格」变成穿墙许可证。

### 6.12 渊卫、兵矿收益与战争迷雾（P-U10）

用户确认**当前地图布置正确**（兵线、野区、河道都不动），只在此基础上补齐三样内容。新增代码不修改 `game-core/src/map.rs` 的任何坐标 / 墙段 / 路点常量，`gx-e2e/tests/objectives_fog.rs::map_layout_is_unchanged_by_objectives_and_fog` 把「2:1 长方形、坑位在中轴、唯一渊卫营地、两座兵矿、一座征募井」锁成回归护栏。

**① 渊卫（中央矿坑全局首领）**：项目原创名 `AbyssWarden`，坑位沿用 `map::GOLD_MINE_POS`。
- 新增 `game-core/src/gameplay/objectives_economy.rs`（纯裁决）与 `server/src/objectives.rs`（tick 驱动）。`T=54000`（15 min）首刷、被击败后 7200 tick（120 s）重生，**加时与判胜后不再刷新**。开局 `spawn_jungle_camps` 跳过渊卫，改由 `abyss_warden_spawn_system` 用同一个 `spawn_monster` 生成 —— 数值表只有一份，不会两处漂移。生成后 `rebuild_server_nav_grid` 当 tick 把它纳入 `NavGrid` 动态障碍，A* 与服务端校验同步生效。
- 归属按**队伍累计伤害**（§13.4），不是最后一击：`world_damage_system` 对渊卫的每次英雄伤害按攻击者队伍累加到 `AbyssWardenState`，完全持平或无人伤害则无人获得。归属复用既有 `GameplayEvents.captured` 流水 → `objective_buff_system` 发 120 s 全队增益并广播 `ObjectiveCaptured` / `ObjectiveBuffGranted`，不另写一套 buff 代码。
- 归属同时结算 §14.4 赏金（每人 100、团队池 300）与 `WardenBuff`：有效期内归属方**新生成**的战偶 `+50% 生命 / +30% 攻击 / +0.4 移速`。小地图在渊卫存活时给出金色目标环（野怪实体本就全局下发，不算泄露视野）。

**② 兵矿（辉矿）持续收益**：占领瞬间全队每人 +40、团队池 +120；归属期间每 1800 tick（30 s）按队发放 `30 × 该队持有兵矿数` 金/人，直到被反占。两座兵矿共用 `objective_id = 1`，因此 `ObjectiveIncomeState` **按队伍**计时并按占点数量叠加，避免「同队占两矿只发一份」；金币写回 `Hero.gold`，由 `CH_GOLD` 增量快照下发，`ObjectiveIncome` 事件只供 HUD 表现。

顺带修正一处既有错配：`attach_world_gameplay` 旧版用枚举下标 `idx+1` 分配占领点 id，得到 `[辉矿, 辉矿, 征募井]`，而 `attributes::objective_buff` / `registry::objective_name` 的 1/2/3 是 `[辉矿, 征募井, 渊卫]` —— 第二座兵矿发的是征募井 buff、征募井发的是渊卫 buff。现改为按类型固定分配，id 3 留给渊卫。

**③ 战争迷雾**：服务端原有的逐玩家视野裁剪一直存在，缺的是客户端表现。
- `game-core/src/visibility.rs` 抽出 `vision_sources()` 纯函数，`compute_visibility()` 与新的 `ServerMessage::VisionUpdate` **共用同一份可见源与半径**；报文逐玩家在 20Hz 快照 tick 走 Unreliable 下发，客户端不自行从（已被裁剪的）渲染实体反推视野。新增协议变体一律追加在枚举末尾，不平移既有 bitcode 判别值。
- 新增 `client/src/fog.rs` 与 `FogPlugin`：`FogOfWar` 维护 64×32 网格的三档记忆（当前可见 / 已探明 / 未探明），草丛格复用 `visibility::in_brush` 与更小的 `brush_reveal_radius`，与服务端对草丛内敌方实体的裁剪一致。迟到的旧 tick 报文直接丢弃，避免遮罩回退闪烁。
- 表现两层：小地图在实体之后叠加按行合并的暗幕（`FR-81`）；3D 世界用共享网格 + 2 个半透明材质的 32×16 拼贴层，贴在地形最高层之上、英雄脚底之下，`Visible` 档直接 `Visibility::Hidden`。`ui.rs::draw_minimap` 用 `Option<Res<...>>` 兼容未挂 `FogPlugin` 的 headless 夹具。

**验收**（`gx-e2e/tests/objectives_fog.rs`，6 例）：地图骨架不变、渊卫首刷/重生/加时门禁 + 网格障碍 + 伤害归属 + 120 s buff、兵矿占领与周期收益流水线、渊卫赏金与战偶强化只作用于归属方、迷雾可见区与服务端 `VisibilitySet` 一致、`VisionUpdate` 的 bitcode 往返。另有 `objectives_economy` 8 例、`server::objectives` 4 例、`client::fog` 5 例纯逻辑单测。

**与规格的偏差（登记在 §13 / §23）**：① §13.4 的「附近半径 20 内战偶」简化为归属方整条兵线、且只作用于归属期间**新生成**的战偶；② 渊卫尚无沿兵线推进的 AI 与捕获进度条，当前是坑位内的静止野怪 + 全队增益；③ 迷雾是「可见源圆盘 + 已探明记忆」，不是真实视线（LoS）判定，墙体不遮挡视野；④ §13.3 征募井对战偶的强化仍未接入波次系统；⑤ 团队金币池只记权威账本，暂无消费点。

## 7. M1 实现要点

- **协议**：`PROTOCOL_VERSION = 3`（v3 在 v2 基础上新增 `CH_STATUS` / `CH_SHIELD` 快照位、9 类玩法事件、购买 / 售出上行；bitcode 与 v2 不兼容，由 `negotiate` 拒绝旧端）+ `config_hash` 双版本协商，不匹配时明确 `Reject`，不静默降级。
- **上行**：`InputCmd` 带 `seq` / `client_time_us` / `config_hash` / `checksum` / 真实 RTT；服务端校验序号与校验和。
- **下行**：状态走 `Unreliable` 通道的增量快照（`changed_mask` + 每 2s 基线）；关键事件走 `ReliableOrdered` 且带 `event_id` 幂等去重。
- **视野**：服务端按每玩家 `VisibilitySet` 在序列化前裁剪，规则见 `GX-ARCH-2026-001` §8.5；P-U10 起逐玩家下发可见源，客户端渲染战争迷雾（见 §6.12）。
- **重连**：会话令牌恢复原 `NetId` 与阵营，不生成新英雄，并立即补发基线快照。
- **配置**：所有平衡数值来自 `gx-proto-content`，`ConfigSet` 在局内不可变，热更只影响新对局；`content_digest()` 已混入握手 hash。
- **合规**：源码 / 配置 / 资源文件名禁用词由 `crates/gx-e2e/tests/compliance.rs` 扫描；`docs/**` 与 `LEGAL/**` 为白名单。

## 8. 视觉开发路线：MC 表现层细化（P-V 系列）

> **实施状态：P-V0 → P-V4 五个阶段已全部逐阶段落地并验收**（含 P-V4 的三项交付与两个试玩缺陷修复）。
> 唯一交付文档为 `docs/06_VISUAL_SPEC.md`（含各阶段资源清单、设计说明、复用规则、触发时机、
> 表现层级、性能约束与验收结论）。本章保留原始设计意图，**与规范冲突处以 `docs/06_VISUAL_SPEC.md` 为准**。
>
> **结论：能再细化，且不需要引入任何外部资产。**
> 现有 `make_pixel_texture` / `voxel_material` / `hero_palette` + 6 型头饰武器查表 / `HeroRig` 肩枢轴 /
> 13 个 `tick_*` 系统 / `CombatFxQueue` 已经是完整的程序化底座，细化是**数据扩展 + 分层生成**，
> 不换引擎、不加二进制资源、不违反 PRES-1/2。主要成本是处理器生成开销与 draw call，
> 由「材质复用池 + chunk 合批」控制。

**范围界定**：P-V 是**客户端表现层**支线，不改动服务端权威模拟与 `TransformSync`。
唯一需要协议配合的两项（技能事件携带 `ability_index`、野怪实体下发）已在 P-E / P-B 未完成项中登记。

### 8.1 细化原则

| 原则 | 说明 |
|---|---|
| 剪影优先 | 低分辨率与远距离下靠「体型 + 头部特征 + 武器轮廓」辨识角色，不依赖贴图细节或血条 |
| 队伍色与角色色分离 | 队伍色只占小面积（脚环 / 肩章 / 披风内衬 / 刃光），角色主色由 `skin` 决定，避免全队一个样 |
| 轴对齐、阶梯近似 | 所有部件轴对齐，禁止曲面 / 平滑；弧线、圆环、斜面一律用阶梯体素近似 |
| 数据驱动 | 新增英雄 / 技能 = 加一行内容表数据，不改几何与特效代码 |
| 表现与裁决隔离 | 动画只改客户端表现节点的 `Transform`，绝不回写服务端同步组件 |

### 8.2 英雄设计：从硬编码查表到造型体系

**8.2.1 造型原型矩阵**（先定语言，再填英雄；以下为原型，不绑定具体英雄名）

| 原型 | 体型 | 头部特征 | 武器轮廓 | 一眼可辨的细节 |
|---|---|---|---|---|
| 重甲护卫 | 宽厚偏矮 | 全覆盔 + 面甲 | 巨盾 + 短锤 | 肩甲外扩、脚环厚、移动慢摆 |
| 剑士 | 中等匀称 | 王冠 / 短发 | 长剑 | 披风摆动、刃脊亮线 |
| 游侠 | 修长窄肩 | 兜帽 | 长枪 / 长弓 | 武器长度超过身高、细长剪影 |
| 法师 | 修长 | 尖顶帽 / 光环 | 法杖 + 浮空法球 | 法球悬浮自转、粒子色 |
| 治疗 | 中等 | 兜帽 + 光环 | 短杖 | 绿白配色、环形脉冲 |
| 攻城 | 极宽厚 | 双角盔 | 巨锤 / 战斧 | 超大武器、三段慢挥 |

**8.2.2 `HeroVisual` 数据表**（把 `match skin` 硬编码迁到 `gx-proto-content`）

字段：`id, archetype, palette{torso, limb, head, accent}, build{shoulder, height, limb_thickness},
headgear, weapon, cape, pauldrons, trail_color, cast_color, sfx_id`。
与 `ConfigSet` / `content_digest()` 打通，使外观表变更同样触发握手拒绝；`skin` 退化为表索引。

**8.2.3 部件细化清单**

| 项 | 现状 | 目标 |
|---|---|---|
| 方块数 | 13~19 | 24~40（护腕、手指、腰带扣、靴、肩甲分层、披风分节、武器握柄 / 护手 / 刃脊 / 刃尖、头饰分层、箭袋） |
| 面材质 | 头 / 躯干 / 武器共用少数材质 | 按部件指定 `PixelKind`；眼睛 / 宝石 / 刃使用 emissive；脸随 `head` 子节点转向 |
| 最小单元 | 1 体素 | 近景关键部件（头 / 武器）细分到 1/4 体素，远景维持 1 体素 |
| 材质数量 | 每英雄 5 套、每次新建 | 按 `(PixelKind, palette, face)` 缓存复用，全局共享 |

**8.2.4 骨架与动画状态机**

扩展 `HeroRig` 为分层骨架：
`{root, torso, head, shoulder_l/r, forearm_l/r, thigh_l/r, shin_l/r, weapon_mount, cape_mount}`。
沿用现有「组件 + 时间插值」范式（非骨骼、非逐帧），新增状态机：

| 状态 | 表现 | 触发 |
|---|---|---|
| Idle | 躯干呼吸缩放、头部微摆、披风低频摆动 | 默认 |
| Move | 双腿交替前后摆 + 反向摆臂，频率随速度 | 速度 > 阈值 |
| Attack | 现有三段挥击曲线，扩展为「抬手 → 挥出 → 收势」并联动躯干扭转 | `AttackAnim` |
| Cast | 前摇蓄力 → 吟唱 → 释放 → 后摇，与 §8.3 五阶段对齐 | `AbilityCast` |
| Hit | 受击硬直：躯干后仰 + 短暂定格 | `DamageDealt` |
| Death | 方块散落 + 下沉淡出，替代当前直接 `despawn` | `HeroDeath` |
| Respawn | 方块自下而上重组 | 快照重建 |

**朝向**：面部与眼睛挂到 `head` 子节点随 `rotation` 转动；转身用角速度插值；上半身可相对下半身偏转，实现「边走边瞄准」而不打断移动。

### 8.3 技能设计：从数值表到视觉序列

**8.3.1 先统一技能表**

现状：`game-core/src/abilities.rs::AbilityDef`（12 字段 + `CastType`）与
`gx-proto-content::AbilityDef.target_kind`（5 种语义）并存，且 `README` §9 已登记「三套数值表待合并」。
P-V 前置任务：合并为单一 `AbilityDef`，并派生只读的 `AbilityVisual` 描述：
`{cast_type, element, palette, windup_ticks, projectile_shape, trail, impact_pattern, aoe_shape, recover_ticks, sfx_id}`。
**所有尺寸 / 速度 / 时长必须读 `range` / `aoe_radius` / `projectile_speed` / `cooldown`，不再写死 `radius = 4`。**

**8.3.2 四技能视觉差异化方案**

| 技能 | 类型 | 前摇 | 主体 | 命中 | 余烬 |
|---|---|---|---|---|---|
| 火焰弹 | Projectile / 物理 | 手心橙红方块收缩 0.18 s | 橙红方块弹体 + 阶梯拖尾，速度取 `projectile_speed` | 碎片迸射 + 地面焦痕方块 0.4 s | 火星上浮淡出 |
| 寒冰新星 | PointTarget / 魔法 | 脚下蓝白同心方块环收缩 0.25 s | 以 `aoe_radius` 为半径生成阶梯方块环 | 环外扩 + 地面霜纹方块阵 | 霜块停留 0.6 s 后逐格消失 |
| 能量光束 | Projectile / 魔法 | 双手青色方块聚拢 0.15 s | 沿线段按 0.5 体素步长采样的连续方块束 | 命中格点亮 + 溅射小方块 | 束体逐段熄灭 |
| 自我治疗 | Instant / 真实 | 绿白方块自下而上环绕 0.3 s | 环形脉冲 + 上升方块粒子 | 无 | 治疗数字 + 头顶十字方块 |

**8.3.3 施法序列与通道分离**

五阶段 `Windup → Release → Travel → Impact → Recover`，每阶段由独立的 `tick_*` 系统驱动，
与 P-D 战斗时间轴（前后摇）对齐，但**只在表现层**实现，不改变服务端裁决时序。
`CombatFxQueue` 拆分为 `BasicAttack / Ability / Objective` 三通道，避免普攻与技能特效互相覆盖。

> **口径对齐（P-V0 / C-06）**：`GameEvent::AbilityCast` **已携带 `ability_index: u8`**，
> 当前 `handle_game_event` 以 `..` 忽略该字段，才导致四技能共用通用方块。
> P-V3 直接读取该字段即可，**无需协议升版**；统一口径见 `docs/06_VISUAL_SPEC.md` §1。

### 8.4 体素细化技术路径（五层）

| 层 | 现状 | 细化手段 |
|---|---|---|
| 纹理 | 4×4 色块哈希、两色、无描边 | 逐像素确定性噪声 + 部件专属图案（眼睛 / 铆钉 / 刃线 / 布纹）+ 1 px 描边 + 边缘磨损；按面法线给顶 / 侧 / 底面不同亮度系数实现**伪 AO 明暗**，无需法线贴图 |
| 几何 | 单一体素尺寸 | 近景 1/4 体素细分、阶梯体素近似弧 / 圆 / 斜面；新增可破坏装饰（草丛分层、石块碎裂） |
| 光照 | 明暗全靠全局平行光 | 面级 emissive 与亮度系数，减少全局光依赖；特效体素统一 unlit |
| 地形 | 12×12 平板、无高度、无水面 | 引入 y 分层地质（草 / 土 / 石）、启用已定义未使用的 `PixelKind::Water` 生成横向河道、三路高差与台阶、边界墙纹理；200×200 地图按 chunk 分块生成并合批 |
| 合规 | `Sphere` 弹体 / `Rectangle` 血条 / `circle_*` 小地图 | 3D 弹体改 `Cuboid`；血条与小地图明确为 2D HUD 例外并方块化；把 `compliance.rs` 从「体素数文本计数」扩展为真正的体素风格检查器（`TC-PRES-01~04`） |

### 8.5 非英雄实体与事件视觉

- **实体**：为野怪、哨塔、源核、守卫建立客户端实体模型，由快照驱动血量与状态——塔按剩余血量逐层缺失、源核出现裂纹方块、野怪有肢体与待机动画；静态地形装饰退化为底座。
- **事件覆盖**：把 `handle_game_event` 的 `_ => {}` 逐类补齐：击杀（方块散落）、占领（进度方块环）、塔毁（坍塌碎片）、波次（兵线出口脉冲）、加时（全局色调 + 衰竭方块雨）、护盾（六面壳体）、状态（头顶图标方块）、胜负（结算面板）。
- **结算**：`MatchFinished` 驱动局内结算面板（时长 / 击杀 / 经济 / 胜负原因），当前仅日志。

### 8.6 8-bit 程序化音频

- **状态（P-V4 已落地）**：`crates/client/src/audio.rs` 提供 `SFX_TABLE[7]` + `synthesize()`，
  `ChipSource` / `ChipDecoder` 以自定义 `Decodable` 把内存 PCM 直接交给引擎音频后端，
  无需解码 feature 或外部文件；完整规格见 `docs/06_VISUAL_SPEC.md` §12.2.3。
- 合成链：方波 / 三角 / 噪声（确定性 LCG）+ 频率滑移 + ADSR 包络 + 4-bit 幅度量化，22.05 kHz 单声道。
- 触发：`SfxQueue` 与 `EventFxQueue` 共用同一次消费，`SFX_EVENT_MAP` 把事件种子稀疏映射到
  技能 / 击杀 / 塔毁 / 占领 / 加时 / 胜负；野怪死亡 / 波次等高频事件静默，避免刷屏。
- 约束：不引入音频资源文件（PRES-1/2），单帧 ≤4 次播放、单音效 ≤0.5 s；
  `Assets<ChipSource>` 未注册时 `play_sfx` 静默丢弃，headless / 无声环境安全降级。

### 8.7 分阶段计划与验收

| 阶段 | 交付 | 验收判据 |
|---|---|---|
| **P-V0 口径与合规** | 本次文档整理（§6 / §8 / §16 / 两份资产表）；PRES-4 违规清单登记 | 合规门禁全绿、文档与代码无互斥表述 |
| **P-V1 基线** | 纹理增强 + 伪 AO 明暗 + 材质复用池；弹体方块化；8-bit 合成器 MVP | `TC-PRES-01/02`；材质实例数不随地形规模增长；零外部资产 |
| **P-V2 英雄** | `HeroVisual` 数据表、部件 24~40、分层骨架 + 状态机、朝向、6 原型落地 | 远距离剪影辨识用例；动画不改服务端 tick 断言；新增英雄只改数据 |
| **P-V3 技能与事件** | 技能表统一 + `AbilityVisual`、四技能差异化、施法五阶段、全事件视觉、结算面板 | 四技能视觉互异、事件覆盖 100%、与 P-D 时间轴对齐 |
| **P-V4 地图与实体** | 7 层高度 / 河道 / 共享方块合批、野怪快照实体与塔 / 源核动态骨架、8-bit 音频 | **已完成**：层间隔 ≥0.05 单测、客户端 42/42、合规 4/4 |

> **五阶段全部完成**：各阶段资源清单 / 设计说明 / 复用规则 / 验收结论见 `docs/06_VISUAL_SPEC.md` §8~§12。
> P-V4 另修复「地图随英雄移动闪烁」与「技能不落在鼠标点击处」两个试玩缺陷（§12.4）。

**依赖**：P-V0 → P-V1 为串行（材质池是 V4 合批的前置）；V2 与 V3 可并行（分别基于 `HeroRig` 与 `CombatFxQueue`）；V3 **复用已有 `AbilityCast.ability_index`，无需协议改动**；V4 复用快照中已下发的 `EntityType::JungleMonster`，无需 P-B 升版。
**每阶段 DoD**：`fmt / clippy / test / build` 全量通过 + 合规门禁全绿 + 无禁用词 + 零外部资产 + 规格文档同步。

### 8.8 与既有路线的关系

`P-A → P-E → P-B → P-C → P-H` 是**服务端权威闭环**主线，P-V 是并行的**客户端表现支线**：
不修改 `SimTime` / `MatchClock` / `AttributeSources` / 协议裁决，只消费事件与快照。
因此 P-V 可与 `P-D`（战斗时间轴）、`P-F`（商店 UI 与状态护盾渲染）、`P-G`（寻路）并行推进，
其中 P-F 的状态 / 护盾渲染与 §8.5 的事件视觉直接复用同一套方块组件。

## 9. 可玩闭环实施状态（P-A → P-E → P-B → P-C → P-H）

### 顺序与目标

```
P-A 权威底座  →  P-E 协议 v3  →  P-B 实体驱动  →  P-C 目标与胜负  →  P-H 验收与文档
   │               │                 │                  │
   └ 统一时钟/ID/属性  └ 快照位/事件/上行  └ 兵线/塔/野怪伤害   └ buff/加时/判胜
        ▲                                                        │
        └────────── 三项修改：点击移动 / 中文 / PvE ──────────────┘
```

最小可玩闭环判据：**单人能开局 → 能点击地面移动 → 能攻击非英雄实体 → 其血量可归零 → 源核归零或硬上限触发胜负事件 → 客户端可见结果**。

### P-A 权威底座

| 类别 | 内容 | 状态 |
|---|---|---|
| 时钟 | `SimTime(u64)`（`delta_secs() ≡ 1/60`、`tick()`）；`movement/attack/ability_cooldown/passive_gold/health_regen/energy_regen/turret_ai` 全部改 `Res<SimTime>`；`tick_clock_system` 每 `FixedUpdate` 同步 `ServerTick → SimTime`；`projectile_system` 已去 `Res<Time>` 墙钟，`AttackCommand.tick` 填权威 tick | 已落地 |
| 对局时钟 | `MatchClock{tick, start_tick, mode, phase, winner, frozen}`；`start_at(ServerTick)` 记录开局基准，`sync(ServerTick)` 为唯一推进路径（`tick ≡ ServerTick - start_tick`）；`finish()` 幂等，`frozen` 短路玩法链并切换 `ServerState::Finished`；`18/22/25` 分钟阈值与加时衰竭 | 已落地 |
| ID 注册表 | `stable_id()`（FNV-1a）、商品 / 目标 / 野怪 / 技能 ID 双向映射；`item_id/objective_id/monster_name` 均接入生产路径；`content_digest()` 经 `ConfigSet::load_default_with_gameplay()` 混入握手 `content_hash`（FR-61） | 已落地 |
| 属性聚合 | `base + level_delta + equipment + Σbuffs → Hero/CombatStats`，`recompute()` 按 max 变化量补足当前 HP/EN（不回满）；`add_buff` 同类刷新；`new()` 扣除挂载前已有等级成长，避免 base 污染 | 已落地 |
| 接线 | `attach_hero_gameplay` 幂等挂 `AttributeSources`；购买 / 售出 / 升级全部走聚合层重算，删除直写 `Hero` 的 `level_up_system` 与 legacy 分支；`damage_system` 读 `MatchClock.exhaustion_active()` | 已落地 |
| 系统注册 | `SimulationPlugin` 分组严格 `.chain()`：时钟组与事件下发组无条件执行，玩法组受 `match_running` 冻结门禁；注册此前死代码系统；删除兵线空壳 | 已落地 |
| 未完成 | 合并 `gx-proto-content` / `abilities.rs` / `shop.rs` 三套数值表（当前以 `content_digest` 摘要保证握手一致性，表本身仍重复）；P-V3 需在此基础上派生 `AbilityVisual` | ⛔ |

约束：任何裁决系统不得读 `bevy::time::Time`；纯函数只接受 `u64 tick`；`MatchClock.tick ≡ ServerTick - start_tick`；`AttributeSources` 是唯一属性写入口；`MatchMode::Pve` 为默认值保证单人可开局。

### P-E 协议 v3

| 类别 | 内容 | 状态 |
|---|---|---|
| 升版 | `PROTOCOL_VERSION = 3`（旧端由 `negotiate` 拒绝） | 已落地 |
| 快照位 | `CH_STATUS(1<<11)` / `CH_SHIELD(1<<12)` / `CH_ALL = 0x1FFF`；`EntityDelta/EntitySnapshot` 加 `status_mask: u32`、`shield: f32` | 已落地 |
| 位序 | `StatusKind::as_bit()`（8 类按枚举序）、`StatusEffects::status_mask()` | 已落地 |
| 事件 | `ItemPurchased/ItemSold/ShopRejected/ObjectiveBuffGranted/StatusApplied/ShieldChanged/WaveSpawned/OvertimeStarted/MatchFinished`；`HeroDeath.bounty` | 已落地 |
| 上行 | `ClientMessage::SellItem`；`handle_gameplay_messages` 解 ReliableOrdered → `ShopIntents/SellIntents`，`u32` 经 `registry::item_name` 反查 | 已落地 |
| 事件桥 | `drain_gameplay_events` 每 tick 把 `GameplayEvents` 转 `GameEvent` 推入 `ServerEvents` 并清空；`event_id` 单一自增 | 已落地 |
| 实体下发 | 英雄带状态 / 护盾；塔、兵线纳入 `collect_entities` | 已落地 |
| 未完成 | 客户端挂 `StatusEffects/ShieldPool`；旧 `HeroItemPurchased` 与新 `ItemPurchased` 合并去重；协议升版兼容用例 | ⛔ |
| P-U10 | `ServerMessage::VisionUpdate`（逐玩家可见源）、`GameEvent::ObjectiveIncome`、`NetEvent` 四类中立目标事件；新变体均追加在枚举末尾 | 已落地 |

> 已完成并从本表移除：野怪快照下发（P-V4 已复用）、`handle_game_event` 9 类事件分支（P-V3）、
> 商店 UI 发购买 / 售出请求（P-U1，见 §6.4）。

约束：全局事件不含 `NetId`/坐标（VIS-6）；`event_id` 不允许两套自增并存；服务端重算 `distance_to_source/is_dead/death_tick/current_tick`，绝不采信客户端；`ShopReject → u8 reason` 码表需在协议文档登记。

### P-B 实体驱动层

| 类别 | 内容 | 状态 |
|---|---|---|
| 波次 | `WaveDef`（首波 `T=900`、间隔 `1500`）、`wave_composition`（每 3 波加攻城、6 波后超级兵）、`wave_scale`、三路 `lane_waypoints` | 已落地 |
| 生成 | `wave_spawn_system` 为双方三路生成带 `NetId/Team/TransformSync/MinionAi` 的战偶，发 `WaveSpawned` | 已落地 |
| 兵线 AI | `minion_ai_system` 沿路点推进、射程内索敌停下攻击、发 `AttackCommand` | 已落地（未验证） |
| 索敌统一 | `TargetCandidates` 资源 + `collect_target_candidates_system`；`attack_system` 按 `Team` 过滤（含 `Neutral` 训练假人 / 野怪）、锁定优先、最近敌人兜底 | 已落地（未验证） |
| 非英雄伤害 | `world_damage_system` 在 `damage_system` **之前**处理塔 / 兵线 / 野怪，接 `structures::apply_incoming`；死亡结算金币经验并 despawn | 已落地 |
| 野怪 / 塔攻击 | `monster_attack_system` 发 `AttackCommand`（P-U8 起：权威 tick 冷却 + 静止型营地反击 + 射程覆盖碰撞距离）；`turret_ai_system` 已注册进链 | 已落地 |
| 实体同步 | 塔 / 源核 / 野怪补 `TransformSync` 与阵营，使其可被下发与索敌 | 已落地 |
| 未完成 | `turret_ai_system` 未接 `choose_threat` 优先级与换目标前 2 次减半；野怪刷新未重新生成实体；兵线 / 野怪死亡事件未完整广播；`resolve_pending_hits` 仍走旧 `apply_mitigated_damage` 且致死直接 `despawn`（绕过 `PendingDeaths` 与重生） | ⛔ |

约束：`world_damage_system` 必须严格早于 `damage_system` 且不得移除 `AttackCommand`；同一系统内不得同时出现 `&mut Transform` 与 `&Transform` 查询（B0001，已用资源快照规避）；源核摧毁只能播报一次（`CoreDestroyed` 守卫）。

### P-C 目标与胜负

| 类别 | 内容 | 状态 |
|---|---|---|
| buff 表 | `objective_buff()`：辉矿 `+20 护甲/+15 魔抗/90s`、征募井 `+15 AD/+0.15 攻速/90s`、渊卫 `+15% 伤害放大/120s` | 已落地 |
| 发放 | `objective_buff_system` 消费 `captured`，全队 `add_buff` + `recompute`，发 `ObjectiveCaptured`/`ObjectiveBuffGranted`；P-U10 起另结算兵矿/渊卫赏金与 `WardenBuff` | 已落地 |
| 中立目标 | 渊卫定时首刷 / 伤害贡献归属 / 重生 / 战偶强化；兵矿占领与 30 s 周期收益 | 已落地（P-U10，见 §6.12） |
| 过期 | `buff_tick_system` 每 tick 推进，过期后重算属性 | 已落地 |
| 时钟 | `match_clock_system`：Warmup 帧 `start_at(ServerTick)`，之后每 tick `sync(ServerTick)`；22 分钟发 `OvertimeStarted`、25 分钟按双方源核剩余生命比例判胜发 `MatchFinished` | 已落地 |
| 判胜 | `world_damage_system` 源核 `health ≤ EPS` 立即 `clock.finish(对方)` 发 `MatchFinished(reason=1)`；硬上限 `reason=2` | 已落地 |
| 衰竭 | `damage_system` 构造 `DamageRequest` 时读 `MatchClock.exhaustion_active()`，衰竭无视无敌 | 已落地 |
| 停赛 | `ServerState::Finished` 状态切换与 `match_running` 冻结门禁 | 已落地 |
| 未完成 | 客户端胜负结算面板（P-V3）；`MatchFinished.reason` 码表登记；投降；助攻实际结算；渊卫推线 AI 与捕获进度条；征募井战偶强化 | ⛔ |

### P-H 验收与门禁

| 类别 | 内容 | 状态 |
|---|---|---|
| 已有测试调整 | `client/src/input.rs` 单测（P-U8 起改为 `keyboard_does_not_move_hero_only_click_target_does` 反向断言）；`e2e_movement.rs` 改为注入点击目标驱动；`compliance.rs` `TC-CBT-12` 改名并改判据 | 已落地 |
| P-A 新增测试 | 时钟对齐 / Warmup 首帧 / 冻结幂等 / 阈值顺序、聚合层 base 污染与等级成长重算、注册表与 buff 表 ID 一致性、玩法摘要影响握手、权威 tick 序列回放确定性、弹体携带权威 tick | 已落地 |
| 待新增 | `gx-e2e/tests/playable_loop.rs`：单用户 PvE 开局 → AI 对手 → 点击地面移动 → 波次 → 打塔 → 源核归零 → `MatchFinished`；中文 / 协议 v3 独立用例 | ⛔ |
| 待新增 | `TC-CBT-05`（普攻前后摇，属 P-D）、`TC-SIM-01~05`、`TC-NET-03`、协议升版兼容、`TC-PRES-01~04`（体素风格检查器，属 P-V1） | ⛔ |
| 验证命令 | `fmt / clippy / test / build` 全量已通过 | 已复验 |
| 文档同步 | 本次已整理 `README.md` §6 / §8、原根与 `docs/` 两份 `ASSET_REGISTRY.md`（已合并为单一文件）、`docs/05_COMBAT_SPEC.md` §16；`§19/§20/§23/§24` 与架构 / 需求文档完成状态仍待回写 | 部分完成 |

### 三项同步修改

**点击移动（v1.3 现行；v1.2 的键盘直控已移除）**：`client/src/input.rs` 的 `camera_pair()` / `primary_camera()` 修复多相机下 `.single()` 失败导致点击无响应；左键 A* 寻路、右键 force 立即移动、触摸点触等效左键。P-U8 起键盘不再产生 `move_dir`，理由与验收见 §6.10。

**中文显示（已落地，规格已同步）**：`client/src/ui.rs` 运行时探测 OS 中文字体注入，失败回退 ASCII；`TC-CBT-12` 门禁改为「发布树不含打包资产 + 必须有 ASCII 回退」。PRES-3 v1.2 = 发布包零外部资源，唯一例外是运行时借用 OS 中文字形。本次已把 `docs/05_COMBAT_SPEC.md` §16 的 PRES-3 / PRES-9 同步为实际口径。

**PvE 单人对局（已落地）**：`MatchMode::Pve` 为默认；单人连接即 `Running`；`ensure_pve_bot` 在有真人且 Pve 时生成红方 AI 英雄并写入 `DummyRegistry`；`pve_bot_ai_system` 追击最近真人并持续普攻。PvE 内容由 P-B / P-C 提供。未完成：模式选择开关、AI 施法与走位、断线重连 bot 归属、难度与多 AI、客户端正式 HUD / 结算面板。

## 10. 已知未完成项与文档同步

- **`GAP-01~18` 差距清单**：见 `docs/05_COMBAT_SPEC.md` §23。本轮 P-A..P-C 已关闭其中「兵线波次 / 胜负加时 / 网络协议字段 / 商店 opcode」等多项，但 §23 文字尚未回写，逐条对照仍标记为未实现。
- **规格文档族待升版**：`docs/05_COMBAT_SPEC.md` §19 / §20（完成状态）、§23 / §24（差距）；`docs/02_ARCHITECTURE.md` 的 DN-06/DN-32、`docs/03_REQUIREMENTS.md` 的 FR-86，需同步为 MC 体素 + 运行时 CJK 字体例外 + 音频规划中。§16 与资产注册表已本次同步。
- **P-V 表现层细化**：见 §8，为下一阶段正式开发路线。
- **平台与上线**：账号实名支付、匹配编排与部署、运营后台、反作弊、局外 UI、多平台适配。
- **外部闸门仍缺**：律师书面意见、产品名 FTO、目标机型与真实网络阈值实测、`TF-01` 容量压测与成本实测。
- **后续玩法（本轮顺序外）**：`P-D` 战斗时间轴 / 技能加成 / 普攻前后摇；`P-F` 完整商店 UI 与状态护盾渲染；`P-G` 寻路动态障碍 / 节流 / 服务端校验（其前置「仅上报 `move_dir` 的协议下速度上限校验恒为真」属规格与协议模型不自洽，需重新设计而非补 `validate_movement`）。

## 11. 合规与外部依赖

- 第三方版权资料已隔离到 `LEGAL/`，**不得进入发布包**，也不得用于生成商品化数值表。
- 表现层无美术 / 音频 / 字体外部资产，全部代码生成或运行期合成（中文字形为运行时借用 OS 字体）；音频若后续引入须登记许可。
- 律师书面意见、产品名商标检索、目标机型清单、目标市场与版号、防沉迷实名验证系统接入、留存 / 手感指标与成本实测均为**外部依赖**，见 `docs/03_REQUIREMENTS.md` §11.1。技术侧门禁已就绪，但这些项未完成前相应闸门不成立。

## 12. 风险登记

1. **文档与代码口径冲突（已大幅收敛）**：本次已修正「6 个方块 / 2×2 网格 / 水块 / 河道 / 8-bit 已提供 / 禁 OS 字体」等互斥表述，并同步 §16 与两份资产表；剩余 `GAP-01~18` 完成状态与 §19/§20/§23 仍待升版，否则验收无法逐条执行。
2. **表现层成熟度不足（已关闭）**：§6.2 的 9 项粗糙点已由 P-V1~P-V4 全部解决；纹理 / 动画 / 技能与事件视觉 / 地形分层 / 非英雄模型 / 8-bit 音频均有数据表与单测护栏，性能预算见 `docs/06_VISUAL_SPEC.md` §7 与 §12.2.5。
3. **`B0001` 冲突已关闭**：`minion_ai_system`/`attack_system` 改为 `TargetCandidates` 资源快照 + `collect_target_candidates_system`，`cargo test --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings` 均已复验通过。
4. **协议不兼容**：`PROTOCOL_VERSION = 3` 改变 bitcode 布局，旧客户端 / 回放被拒；`content_hash` 已混入 `content_digest()`，「同版本不同内容表」由 `negotiate` 拒绝，但协议升版兼容用例仍待补。P-V3 复用现有 `AbilityCast.ability_index`，**不改变布局**（P-V0 已对齐口径）。
5. **事件重复**：`HeroItemPurchased`（旧）与 `ItemPurchased`（新）并存；`objectives.rs` 的 `(tick<<8)|id` 与 `EventCursor` 两套 `event_id` 需统一。
6. **运行时字体**：Linux / macOS 无候选字体时全部文案回退 ASCII，中文需求在这些平台仍不满足，需在文档中明确为「尽力而为」。
7. **旧伤害管线**：`resolve_pending_hits` 致死仍 `despawn`，技能击杀会绕过重生系统；虽属 P-D，但会影响 P-H 的可玩闭环断言。

## 13. 验证状态

当前可重复执行的验证命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

已知状态：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --workspace --locked` 与合规门禁 4/4 全部通过；`B0001` 冲突与 `gx-e2e/tests/gameplay.rs` 缺 `SellIntents` 资源初始化两处历史问题均已修复并复验。P-V0~P-V4 五阶段全部验收完成，客户端单测 50/50（含 P-U1 大厅 / 档案 / 图鉴 / 商店 7 例 + 场景解耦 1 例）。大厅、英雄档案、12 件道具图鉴、真实购买售出上行与 PVP / PVE 模式选择已落地（§6.4），且大厅阶段不渲染后景对局、点开始后才进入对局。剩余待办为 `playable_loop` 端到端用例、规格文档族（§19/§20/§23/§24）回写，以及高塔点击的几何 raycast 视差优化（见 `docs/06_VISUAL_SPEC.md` §12.4.2 已知限制）。

---

**结论**：按 `P-A → P-E → P-B → P-C → P-H` 的服务端权威闭环代码主体已写入工作树，P-A 底座三套 tick 统一、`content_digest` 混入握手 hash、属性聚合唯一写入口与冻结状态机均已落地并通过全量验证。表现层沿纹理 / 几何 / 动画 / 地形 / 音频五层，配合 `HeroVisual`、`AbilityVisual`、`NON_HERO_VISUALS` 与 `SFX_TABLE` 数据表，按 `P-V0 → P-V4` 五阶段全部落地并逐阶段验收（§8，交付文档 `docs/06_VISUAL_SPEC.md`）。同时修复「地图随英雄移动闪烁」与「技能不落在鼠标点击处」两个试玩缺陷。P-U1 补齐进入即见的大厅、6 位英雄档案、12 件道具图鉴与真实商店交互，并让 PVE / PVP 在大厅可选、贯通到服务端开局人数门槛与 AI 生成。全程保持零外部资产，合规门禁持续全绿。
