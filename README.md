# GX-01

原创世界观的 3v3 团队竞技原型（内部工程代号 `GX-01`，产品名待商标检索后确定）。
服务范围、需求与验收标准以 `docs/` 下的文档族为准。

> **本次交付边界**：M0 + M1，验收闸门 G0 / G1。见 `docs/03_REQUIREMENTS.md` §11.1。
> **范围变更（v1.1）**：交付范围由 `M0+M1` 扩展为 `M0+M1 + 玩法逻辑闭环`
> （属性扩展 / 状态效果 / 完整伤害公式 / 商店事务 / 野怪 AI / 占领目标 / 塔护盾 / 死亡重生 / 胜负加时 / PvE 单人对局）。
> M2 剩余差距逐条对照见 `docs/05_COMBAT_SPEC.md` §23，寻路 M2 待办规格见 §24；
> 上线功能（M3）与灰度（M4）仍不在范围。
> **本次文档整理**：纠正表现层章节与代码不符的口径，并把「MC 风格细化」列为下一阶段的正式开发路线（见 §8）。

## 1. 文档族

| 文档 | 编号 | 内容 |
|---|---|---|
| `docs/` 上游报告 | `VG-FS-2026-001` | 上游可行性与法律边界研究（合规留档） |
| `docs/01_TECHNICAL_FEASIBILITY.md` | `GX-TF-2026-001` | 技术假设、POC、性能/带宽预算、技术闸门 |
| `docs/02_ARCHITECTURE.md` | `GX-ARCH-2026-001` | 分层架构、协议、帧循环、ADR、开发注意项 |
| `docs/03_REQUIREMENTS.md` | `GX-PRD-2026-001` | FR / NFR / CR / UC、验收标准、版本切分 |
| `docs/04_DEVELOPMENT.md` | `GX-DEV-2026-001` | DEV 任务卡、TC 用例、CI、DoD、追溯矩阵 |
| `docs/05_COMBAT_SPEC.md` | `GX-CBT-2026-001` | **对战详细规格**：属性 / 伤害 / 普攻 / 技能 / 状态 / 经济 / 装备商店 / 兵线 / 野怪 / 中立目标 / 塔与源核 / 死亡重生 / 操作与自动寻路 / 表现层 / 完整数值表；**§16 表现层规范、§22 玩法详细设计、§23 M2 待办差距、§24 寻路 M2 待办** |
| `ASSET_REGISTRY.md` | `GX-ASSET-REG-2026-001` | 资产来源登记（表现层全部代码生成） |

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

## 5. 操作方案（v1.2）

**键盘 W/A/S/D 与方向键为主操作，直接控制英雄移动方向**（`GX-CBT-2026-001` §3 v1.2）；鼠标点地与自动寻路为辅助：

| 操作 | 效果 |
|---|---|
| **W / A / S / D / 方向键** | 直接产生移动方向（优先级最高）；按下时取消点地寻路目标与 A* 路径 |
| 左键点地面 | 自动寻路（A*）到该点，到点后待命 |
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
| 地形与建筑 | 12×12 大平板三材质轮换、4 面边界石墙、折叠线石径、金矿坑、三路路点条带、基地石台、草丛；哨塔 3 层 / 源核 5 层递减方块堆叠 + 金顶 |
| 非英雄实体 | 小兵 3 方块模型随快照创建 / 回收；训练假人复用石质英雄模型 |
| 战斗特效 | `CombatFxQueue` + 13 个 `tick_*` 系统：近战挥击三段曲线 / 刀光 / 延迟火花、远程弹道插值、暴击大刀光、碎片抛物线、`HitFlash`、`DamageFlash`、`CastEffect`；血条 3 层 billboard + 掉血 lag 残影；4 类方块瞄准 / 选中标记 |
| 大厅与流程 | `AppState{Lobby, InMatch}` + `lobby_ui::draw_lobby` 锁定视口的三列大厅（模式 / 英雄档案 / 详情）；HUD 按状态门禁；连接延后到 `OnEnter(InMatch)`；详见 §6.4 |
| 地图 | `MAP_WIDTH/HEIGHT = 200`，三路各 4 座共 12 座哨塔、z 轴两端基地、中央分隔与四角野区；`collision_system` 做动态互推与静态体推出 |

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
8. **8-bit 音频曾零实现**：客户端无音频依赖与合成代码；此前 `README`、根 `ASSET_REGISTRY.md` 写「已提供」，`docs/ASSET_REGISTRY.md` 写「待资产」，三方互斥。P-V4 已以内存 PCM 合成器落地，两份登记表统一为「已提供」。
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

## 7. M1 实现要点

- **协议**：`PROTOCOL_VERSION = 3`（v3 在 v2 基础上新增 `CH_STATUS` / `CH_SHIELD` 快照位、9 类玩法事件、购买 / 售出上行；bitcode 与 v2 不兼容，由 `negotiate` 拒绝旧端）+ `config_hash` 双版本协商，不匹配时明确 `Reject`，不静默降级。
- **上行**：`InputCmd` 带 `seq` / `client_time_us` / `config_hash` / `checksum` / 真实 RTT；服务端校验序号与校验和。
- **下行**：状态走 `Unreliable` 通道的增量快照（`changed_mask` + 每 2s 基线）；关键事件走 `ReliableOrdered` 且带 `event_id` 幂等去重。
- **视野**：服务端按每玩家 `VisibilitySet` 在序列化前裁剪，规则见 `GX-ARCH-2026-001` §8.5。
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
        └────────── 三项修改：方向键 / 中文 / PvE ──────────────┘
```

最小可玩闭环判据：**单人能开局 → 能用方向键移动 → 能攻击非英雄实体 → 其血量可归零 → 源核归零或硬上限触发胜负事件 → 客户端可见结果**。

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
| 未完成 | 渊卫实体下发；客户端挂 `StatusEffects/ShieldPool`；旧 `HeroItemPurchased` 与新 `ItemPurchased` 合并去重；协议升版兼容用例 | ⛔ |

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
| 野怪 / 塔攻击 | `monster_attack_system` 在 Aggro 态发 `AttackCommand`；`turret_ai_system` 已注册进链 | 已落地 |
| 实体同步 | 塔 / 源核 / 野怪补 `TransformSync` 与阵营，使其可被下发与索敌 | 已落地 |
| 未完成 | `turret_ai_system` 未接 `choose_threat` 优先级与换目标前 2 次减半；野怪刷新未重新生成实体；兵线 / 野怪死亡事件未完整广播；`resolve_pending_hits` 仍走旧 `apply_mitigated_damage` 且致死直接 `despawn`（绕过 `PendingDeaths` 与重生） | ⛔ |

约束：`world_damage_system` 必须严格早于 `damage_system` 且不得移除 `AttackCommand`；同一系统内不得同时出现 `&mut Transform` 与 `&Transform` 查询（B0001，已用资源快照规避）；源核摧毁只能播报一次（`CoreDestroyed` 守卫）。

### P-C 目标与胜负

| 类别 | 内容 | 状态 |
|---|---|---|
| buff 表 | `objective_buff()`：辉矿 `+20 护甲/+15 魔抗/90s`、征募井 `+15 AD/+0.15 攻速/90s`、渊卫 `+15% 伤害放大/120s` | 已落地 |
| 发放 | `objective_buff_system` 消费 `captured`，全队 `add_buff` + `recompute`，发 `ObjectiveCaptured`/`ObjectiveBuffGranted` | 已落地 |
| 过期 | `buff_tick_system` 每 tick 推进，过期后重算属性 | 已落地 |
| 时钟 | `match_clock_system`：Warmup 帧 `start_at(ServerTick)`，之后每 tick `sync(ServerTick)`；22 分钟发 `OvertimeStarted`、25 分钟按双方源核剩余生命比例判胜发 `MatchFinished` | 已落地 |
| 判胜 | `world_damage_system` 源核 `health ≤ EPS` 立即 `clock.finish(对方)` 发 `MatchFinished(reason=1)`；硬上限 `reason=2` | 已落地 |
| 衰竭 | `damage_system` 构造 `DamageRequest` 时读 `MatchClock.exhaustion_active()`，衰竭无视无敌 | 已落地 |
| 停赛 | `ServerState::Finished` 状态切换与 `match_running` 冻结门禁 | 已落地 |
| 未完成 | 客户端胜负结算面板（P-V3）；`MatchFinished.reason` 码表登记；投降；助攻实际结算；渊卫占领点（当前只生成 2 个） | ⛔ |

### P-H 验收与门禁

| 类别 | 内容 | 状态 |
|---|---|---|
| 已有测试调整 | `client/src/input.rs` 单测 `arrow_keys_and_wasd_move_hero`（6 方向断言 + 松键归零）；`e2e_movement.rs` 改 `press_key/release_keys`；`compliance.rs` `TC-CBT-12` 改名并改判据 | 已落地 |
| P-A 新增测试 | 时钟对齐 / Warmup 首帧 / 冻结幂等 / 阈值顺序、聚合层 base 污染与等级成长重算、注册表与 buff 表 ID 一致性、玩法摘要影响握手、权威 tick 序列回放确定性、弹体携带权威 tick | 已落地 |
| 待新增 | `gx-e2e/tests/playable_loop.rs`：单用户 PvE 开局 → AI 对手 → 方向键移动 → 波次 → 打塔 → 源核归零 → `MatchFinished`；键盘 / 中文 / 协议 v3 独立用例 | ⛔ |
| 待新增 | `TC-CBT-05`（普攻前后摇，属 P-D）、`TC-SIM-01~05`、`TC-NET-03`、协议升版兼容、`TC-PRES-01~04`（体素风格检查器，属 P-V1） | ⛔ |
| 验证命令 | `fmt / clippy / test / build` 全量已通过 | 已复验 |
| 文档同步 | 本次已整理 `README.md` §6 / §8、`ASSET_REGISTRY.md`（两份）、`docs/05_COMBAT_SPEC.md` §16；`§19/§20/§23/§24` 与架构 / 需求文档完成状态仍待回写 | 部分完成 |

### 三项同步修改

**方向键移动（已落地 + 规格已同步）**：`client/src/input.rs` 新增 `camera_pair()` 修复多相机下 `.single()` 失败导致点击无响应，键盘计算 `keyboard_dir` 并优先于 A* 路径；规格 §3.1 / `TC-CBT-01` / `NAV-M2-02-C8` 已同步 v1.2。验收：`TC-INPUT-01`、两轴位移断言。

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
