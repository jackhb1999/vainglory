# 对战详细规格 GX-CBT-2026-001

| 项 | 值 |
|---|---|
| 文档编号 | `GX-CBT-2026-001` |
| 版本 | v1.5（v1.1 新增 §22 / §23 / §24；v1.3 改为纯点击控制；v1.4 = P-U9 落地 §24 三项与 `TC-NAV-01~09`；v1.5 = P-U10 落地 §13 中立目标、兵矿收益与战争迷雾） |
| 日期 | 2026-11-07（v1.1）；2026-11-14 更新实现状态 |
| 状态 | 设计规格 + 玩法逻辑已实现（数值为自研占位，待平衡验证）；§24 三项待办已闭环，§13 中立目标主体已落地 |
| 上游 | `GX-PRD-2026-001` §3 / §9 / §11.1、`GX-ARCH-2026-001` §6 / §8.5、`GX-DEV-2026-001` §2.2 |
| 适用阶段 | M0+M1 基线 + **本轮按新指令扩展的玩法逻辑闭环**；M2 剩余差距见 §23，寻路规格与验收见 §24（已落地） |
| 术语约束 | 遵守 `GX-ARCH-2026-001` §2.2 禁用词表与 `docs/TERM_DENYLIST.txt` |

> **本文的目的**：把 `GX-PRD-2026-001` 中"攻击、属性、商店、野怪"等只有一句话的条目，展开为**可直接编码、可直接写测试用例**的规格。所有数值、公式、状态机、边界条件在此唯一定义；实现与 `gx-proto-content` 配置表必须以此为准。
>
> **数值声明**：本文全部数值为**本项目自研占位值**，用于让一局对战能完整跑通与验证，不作为上线平衡数值。按 `AS-04` / `GX-PRD-2026-001` §11.1 标注为外部依赖（需 M2 平衡验证回填）。

> **文档族（GX-01）**：本文档与下列文档共享同一套 ID 前缀（`FR`/`NFR`/`CR`/`UC`/`DEV`/`TC`/`GAP`/`ARC`/`ADR`/`TA`/`TF`/`DN`/`TR`），禁止各自另起编号；术语以 `docs/02_ARCHITECTURE.md` §2.1 为准。
>
> | 文档 | 编号 | 文件 |
> |---|---|---|
> | 技术可行性 | `GX-TF-2026-001` | `docs/01_TECHNICAL_FEASIBILITY.md` |
> | 系统架构 | `GX-ARCH-2026-001` | `docs/02_ARCHITECTURE.md` |
> | 需求规格 | `GX-PRD-2026-001` | `docs/03_REQUIREMENTS.md` |
> | 开发实施 | `GX-DEV-2026-001` | `docs/04_DEVELOPMENT.md` |
> | 对战详细规格 | `GX-CBT-2026-001` | `docs/05_COMBAT_SPEC.md` |
> | 表现层规范 | `GX-VIS-2026-001` | `docs/06_VISUAL_SPEC.md` |
> | 上游可行性研究（合规留档，非本项目原创内容） | `VG-FS-2026-001` | `docs/VAINGLORY_FEASIBILITY_STUDY.md` |
> | 资产登记 | `GX-ASSET-REG-2026-001` | `ASSET_REGISTRY.md` |
> | 表达层禁用词 | — | `docs/TERM_DENYLIST.txt` |

### 目录

- §0 适用范围与约定
- §1 对局总览
- §2 地图与空间
- §3 操作与输入规范
- §4 单位属性系统
- §5 战斗结算
- §6 普攻
- §7 技能系统
- §8 状态效果
- §9 经济与等级
- §10 装备与商店
- §11 兵线战偶（Construct）
- §12 野怪与营地
- §13 中立目标（渊卫 / 兵矿）
- §14 哨塔与源核
- §15 死亡、重生与胜负
- §16 表现层规范（程序化 / 零外部资源）
- §17 服务端权威与一致性
- §18 差距表（v1.0 历史快照，已被 §23 取代）
- §19 验收用例
- §20 追溯矩阵
- §21 待外部输入
- §22 玩法系统详细设计规格（已实现）
- §23 M2 待办差距清单
- §24 寻路系统 M2 规格与验收（已落地）

---

## 0. 适用范围与约定

### 0.1 时间单位

| 记号 | 含义 |
|---|---|
| `tick` | 服务端固定步长，`TICK_RATE = 60`，即 `1 tick = 1/60 s ≈ 16.67 ms` |
| `s` | 秒；本文所有"秒"在服务端一律换算为 tick 后参与运算，禁止用 `f32` 累计时间判定冷却 |
| `T` | 当前服务端 `ServerTick` 值，从 0 单调递增 |

### 0.2 数值精度

- 服务端模拟与结算使用 `f32`；**比较一律带容差** `EPS = 1e-4`。
- 金币、经验、计数、tick、层数为整型，禁止用浮点累加（避免跨平台漂移）。
- 所有"概率"判定使用服务端确定性伪随机（种子 = `match_id × 2^32 + T`），禁止客户端参与掷骰。

### 0.3 阵营与实体

| 阵营 | 值 | 说明 |
|---|---|---|
| `Blue` | 0 | 西南方基地，源核 `(-56, 0, -56)` |
| `Red` | 1 | 东北方基地，源核 `(56, 0, 56)` |
| `Neutral` | 2 | 野怪、中立目标 |

实体类别（`EntityType`）：`Hero`（英雄）、`Construct`（战偶）、`Sentry`（哨塔）、`SourceCore`（源核）、`Monster`（野怪）、`Projectile`（弹体）、`Ward`（视野装置，M2 可选）。

---

## 1. 对局总览

### 1.1 一局的完整闭环

```
T=0         双方 3 名英雄在己方源核生成，属性按 §5 初始化，金币 = 500
T=0~900     开局准备：兵线未出发，可购买初始装备；15 s 后关闭"出门补给"提示
T=900       首波战偶出发；辉矿 / 征募井可争夺
T=1500      第二波战偶；此后每 1500 tick 一波
T=54000     渊卫（AbyssWarden）刷新
T=64800     目标时长（18 min）；若未分胜负，进入 overtime
T=90000     硬上限（25 min），按 §16.3 强制判定
任意 T      敌方源核生命 ≤ 0 → 立即结算
```

### 1.2 阶段划分

| 阶段 | tick 区间 | 设计意图 | 关键规则 |
|---|---|---|---|
| 准备期 | `[0, 900)` | 买初始装、站位 | 无敌方可交战区域（跨中线前 20 单位内不能攻击） |
| 对线期 | `[900, 18000)` | 补刀、压制、首塔 | 战偶每 25 s 一波，辉矿 / 征募井 90 s 刷新 |
| 中期 | `[18000, 54000)` | 抱团、争夺目标 | 外塔倒后开放野区捷径；赏金开始递增 |
| 后期 | `[54000, 64800)` | 渊卫决胜 | 渊卫刷新；重生时间达到上限区间 |
| 加时 | `[64800, 90000)` | 强制收敛 | 所有单位每秒受到 `1% 最大生命` 的真实伤害（"衰竭"），防止僵局 |
| 终局 | `≥90000` | 判定 | §16.3 |

> **衰竭机制**是实现 18±4 / 25 上限的收敛装置：加时期间源核护盾按 `0.5%/s` 衰减，避免双方互不清塔。`NFR` 的对局时长指标据此验收。

---

## 2. 地图与空间

### 2.1 全局尺寸

| 常量 | 值 | 说明 |
|---|---|---|
| `MAP_SCALE` | **1.5** | 唯一缩放真相源。所有尺寸 / 坐标 / 宽度都由「基准布局 × `MAP_SCALE`」派生；改这一个常量即可整体等比放大 |
| `MAP_WIDTH` / `MAP_HEIGHT` | **480 / 240（2:1 横向长方形）** | `x ∈ [-240,240]`、`z ∈ [-120,120]`，`y = 0` 为地面；分轴半径 `MAP_W_HALF=240` / `MAP_H_HALF=120` |
| `LANE_Z` / `LANE_WIDTH` | -60 / 19.5 | 兵线在北侧水平横贯左右（基准 -40 / 13） |
| `RIVER_HALF_WIDTH` | 12 | 纵向河道半宽；到中轴 `x = 0` 的距离 `\|x\| ≤ 12` 为水域 |
| 基地 | 蓝方 `(-210,0,-60)` / 红方 `(210,0,-60)` | 兵线左右两端，互为中轴镜像 |
| 兵线走向 | 沿 `z = LANE_Z` 从左端到右端 | 唯一兵线，11 个路点（间距 42），中心 `(0,-60)` 为跨河桥面 |
| 河道走向 | 沿 `x = 0`，自 `z = -120` 至 `z = 120` | 方向 `(0,1)` 与兵线方向 `(1,0)` **正交** |
| **兵线 / 野区分隔墙** | **只在兵线南侧（靠野区一侧）4 段**，中心 z = `-47.1` | 兵线北侧是地图边界方向，不设墙；半厚 2.4，由 `wall_colliders()` 采样成连续圆形静态碰撞体 |
| **坡道开口** | 5 处（两基地入口、两中段坡道、中央交汇口） | 只在开口处可从兵线进野区；全部关于 `x = 0` 镜像 |
| 公平性 | 关于中轴 `x = 0` 左右镜像对称（`mirror_fold(x,z) = (-x,z)`） | 蓝方每个实体镜像到红方对应实体；墙段与坡道同样逐项配对 |
| 兵线路点 | 见 §12.2 | 11 个路点（等距，关于 x=0 镜像） |
| 共用目标 | 中央矿坑 `(0,0,12)` / 野区商店 `(0,0,96)` | 均在中轴上，双方共享 |
| 哨塔 | 每队 5 座（3 线上 + 2 守家） | 红方逐座为蓝方中轴镜像；线上塔收进兵线通道内，不与堤墙重叠 |
| 商店区 | 以己方源核为圆心、半径 12 | 仅此处可购买（§11.4） |

> 只有**地图尺度量**（尺寸、坐标、宽度、草丛 / 河道半径、墙段）随 `MAP_SCALE` 缩放；
> 英雄 / 建筑模型尺寸、碰撞半径、攻击距离、视野半径等**物件尺度**数值保持不变。

### 2.2 坐标与区块

- 客户端寻路使用 **64×32 分轴网格**（`GRID_NX=64`、`GRID_NZ=32`），
  `CELL_SIZE = MAP_WIDTH / GRID_NX = 7.5`，正好覆盖整张 480×240 地图；格坐标
  `(gx, gz) = (floor((x+240)/7.5), floor((z+120)/7.5))`。
  网格尺寸由地图尺寸派生，地图继续等比放大时自动跟随；宽高比必须与地图一致，
  否则 z 轴会整片越界。
- 不可通行格：哨塔 / 源核底座（半径 3）、地图边界外 1 格、
  **兵线分隔墙**（`lane_wall_cells()` 由 `LANE_WALLS` 栅格化，z 方向外扩半格保证不斜穿）。
  起终点所在格在 `PathState::set_goal` 中排除，避免英雄贴墙站立时 A* 直接返回空。
  河道**不阻挡通行**，仅在 `terrain_speed_mult` 中减速一半。
- 桥区：河道内且 `|z − BRIDGE_Z| ≤ 15`（`BRIDGE_Z = LANE_Z = -60`）保持全速；
  此处是兵线跨河处，中央矿坑在桥面正下方的河道上。
- **涉水伤害（P-U6）**：`is_hazardous_river(pos) = is_river && !is_bridge`，
  即危险水域与减速区完全一致、桥面是唯一安全过河点。服务端
  `river_hazard_system` 每 30 tick（0.5 s）对危险水域里的**英雄**扣
  `RIVER_HAZARD_DAMAGE = 25` 真实伤害（50 HP/s）；小兵走桥、野怪为固定设施，均不受影响；
  阵亡 / 复活无敌期免伤。扣血只写 `Hero.health`，淹死由 `death_respawn_system`
  以权威血量统一登记，走正常复活流程。广播 `GameEvent::EnvironmentDamage`
  （`involved_net_ids` 只含目标），**不得复用 `DamageDealt`**：后者会驱动攻击者的
  挥击 / 刀光，造成「自己打自己」的错误表现。

### 2.3 草丛（遮蔽区）

`BUSHES` 定义掩体圆（中心 + 半径），全部关于 `x = 0` 镜像成对，
主要分布在兵线两端、中央矿坑周围与正下方共用商店周围。
下表为**基准布局坐标**，实际值统一乘 `MAP_SCALE = 1.5`：
`(-24,0,-8) r=4` / `(24,0,-8) r=4`、`(-124,0,-24) r=5` / `(124,0,-24) r=5`、
`(-124,0,-56) r=5` / `(124,0,-56) r=5`、`(-20,0,28) r=5` / `(20,0,28) r=5`、
`(-24,0,64) r=5` / `(24,0,64) r=5`、`(-76,0,24) r=5` / `(76,0,24) r=5`。

- 草丛内敌方单位仅在 `brush_reveal_radius = 6` 内可见（`VIS-5`）。
- 草丛**不影响寻路**（可穿行），但进入/离开触发视野集合重算。
- 草丛内单位不自动暴露给未进入草丛的敌方可见源。

---

## 3. 操作与输入规范

> 本节取代此前所有描述。**v1.3 操作方案：纯点击控制**，英雄移动只由鼠标 / 触摸点地驱动。v1.1「键盘不得产生移动」与 v1.2「键盘直控优先」均已作废 —— 键盘与点击两套移动源同帧互抢 `MoveTarget` / A* 路径，是「走路随机乱晃」的决定性成因（P-U8）。

### 3.1 按键分工（唯一定义）

| 输入 | 语义 | 产生的命令 | 备注 |
|---|---|---|---|
| **左键点击地面** | 自动寻路到该点 | `PathRequest { goal, force: false }` | 到点后待命；保留当前锁定目标 |
| **左键点击敌方单位** | 仅选中查看信息面板 | `SelectTarget { net_id }` | **不移动、不攻击** |
| **右键点击地面** | 立即移动（打断当前动作） | `PathRequest { goal, force: true }` | 清除锁定目标、取消施法前摇与普攻后摇 |
| **右键点击敌方单位** | 锁定普攻目标 | `LockAttackTarget { net_id }` | 持续普攻，自动追击到射程内；直到目标死亡/超距/切换/被控 |
| **Q / E / R / F** | 进入对应技能槽瞄准模式 | `EnterAiming { slot }` | 再按同键或 `Esc` 取消 |
| **左键（瞄准模式下）** | 确认施放 | `Cast { slot, aim }` | 左键的寻路语义在瞄准模式下被屏蔽 |
| **Esc** | 取消瞄准 / 关闭面板 | — | 不清除移动 |
| **P** | 开/关商店 | — | 仅在商店区可购买 |
| **1~6 / Space** | 主动道具 / 表情 | `UseItem { slot }` | M2 可选 |
| **WASD / 方向键** | **不产生移动（v1.3）** | — | 键盘与点地寻路互抢路径会导致随机乱晃，已移除；键盘仅保留技能 / 商店等非移动功能 |
| **触摸：点触地面** | 同左键点地 | `PathRequest` | 移动端不提供虚拟摇杆 |
| **触摸：长按敌方** | 同右键锁定 | `LockAttackTarget` | 长按时长阈值 350 ms |

### 3.2 输入状态机

```
Idle ──左键点地──> Pathing ──到达(距目标 <0.4)──> Idle
  │                   │
  │                   ├──右键点地(force)──> Pathing (重算路径, 清锁定)
  │                   └──右键点敌──> Chasing ──进入射程──> Attacking
  │                                          │
  │                                          └──目标死亡/超距──> Idle
  ├──Q/E/R/F──> Aiming ──左键──> Casting(前摇)──> Idle
  │                └──Esc/同键──> Idle
  └──P──> ShopOpen ──P/Esc──> Idle
```

**优先级**（同一帧多个事件时）：`Esc / 取消` > `右键` > `技能键` > `左键` > `触摸`。

### 3.3 自动寻路规格

| 项 | 定义 |
|---|---|
| 算法 | A*，8 邻域，对角移动代价 `√2`；**禁止穿角**（两相邻正交格均不可通行时，对角也不可通行） |
| 启发式 | 八方向距离（octile） |
| 障碍 | §2.2 不可通行格 + 动态占位（哨塔、源核、渊卫） |
| 重算触发 | 目标变化、路径下一格被占用、每 `30 tick`（0.5 s）一次 |
| 路径平滑 | 线段可见性合并：若从当前点到第 `i+k` 个路点的直线不穿越障碍，删除中间点（简化漏斗） |
| 到达判定 | 与当前路点距离 `< 0.4` 则推进下一路点；最后一个路点到达后 `move_dir = 0` |
| 卡住处理 | 连续 60 tick 位移 `< 0.1` → 重算一次；仍失败则清空路径并停止 |
| 输出 | 每帧只向服务端发送**归一化方向向量 `move_dir`**（`InputCmd`），服务端校验速度上限 `move_speed × 1.05` 与碰撞 |
| 服务端职责 | 不重算路径；只做位置积分、边界钳制、速度/穿墙校验，防作弊 |

> 客户端寻路、服务端校验是刻意的职责划分：路径质量由客户端负责（体验），合法性由服务端负责（公平）。

### 3.4 锁定普攻与移动的关系

- 锁定目标后进入 `Chasing`：`PathRequest` 的目标 = 敌方当前位置，每 15 tick 刷新一次。
- 进入 `attack_range` 后停止移动（`move_dir = 0`），进入 `Attacking`。
- 目标逃出 `attack_range × 1.6` 时恢复追击；逃出 `vision_radius` 或死亡则解除锁定。
- 锁定期间 **bit0 置位**，服务端 `attack_system` 持续攻击该 `NetId`。

---

## 4. 单位属性系统

### 4.1 一级属性（基础值，来自 `HeroDef`）

| 属性 | 符号 | 基础 | 每级成长 | 来源 |
|---|---|---|---|---|
| 最大生命 | `max_health` | 720 | +68 | 配置表 |
| 最大能量 | `max_energy` | 260 | +22 | 配置表 |
| 攻击力 | `attack_damage` (AD) | 54 | +4.2 | 配置表 |
| 法术强度 | `ability_power` (AP) | 0 | 0 | 仅装备 |
| 护甲 | `armor` | 22 | +3.5 | 配置表 |
| 魔抗 | `magic_resist` | 18 | +2.4 | 配置表 |
| 攻速 | `attack_speed` | 1.00 次/s | +2.5%/级 | 配置表 |
| 移速 | `move_speed` | 8.0 单位/s | 0 | 配置表（当前代码常量 `HERO_BASE_MOVE_SPEED = 8.0`） |
| 普攻射程 | `attack_range` | 5.5 | 0 | 配置表 |
| 生命回复 | `hp_regen` | 4.0 /s | +0.4/级 | 派生 |
| 能量回复 | `en_regen` | 6.0 /s | +0.5/级 | 派生 |

### 4.2 二级属性（由装备/状态提供，默认 0）

`crit_chance`（上限 100%）、`crit_multiplier`（默认 2.0）、`flat_armor_pen`、`pct_armor_pen`（上限 40%）、`flat_mr_pen`、`pct_mr_pen`（上限 40%）、`lifesteal`（上限 40%）、`cooldown_reduction`（上限 40%）、`damage_amp`（上限 +50%）、`damage_reduction`（上限 −50%）、`shield`（无上限，先于生命扣除）。

### 4.3 派生属性与公式

```
attack_interval   = 1 / clamp(attack_speed, 0.2, 2.5)
effective_armor   = max(0, armor - flat_armor_pen) * (1 - clamp(pct_armor_pen, 0, 0.4))
effective_mr      = max(0, magic_resist - flat_mr_pen) * (1 - clamp(pct_mr_pen, 0, 0.4))
effective_cd      = base_cd * (1 - clamp(cooldown_reduction, 0, 0.4))
final_move_speed  = clamp(move_speed * (1 + slow_sum) + flat_bonus, 1.0, 12.0)
```

### 4.4 等级与经验

```
XP(L → L+1) = round(280 * 1.25^(L-1)),  L ∈ [1, 11]
max_level   = 12
```
| L | 1→2 | 2→3 | 3→4 | 4→5 | 5→6 | 6→7 | 7→8 | 8→9 | 9→10 | 10→11 | 11→12 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| XP | 280 | 350 | 438 | 547 | 683 | 854 | 1068 | 1335 | 1669 | 2086 | 2608 |

经验来源：战偶补刀 30、英雄击杀 200（助攻每人 100）、野怪见 §13、中立目标见 §14。等级提升即刻结算属性成长，并按**成长量**补足当前生命/能量（不回满）。

---

## 5. 战斗结算

### 5.1 伤害类型

`DamageType`：`Physical`（吃护甲）、`Magical`（吃魔抗）、`True`（无视减免）。

### 5.2 完整伤害公式（服务端唯一裁决点）

```
1. raw        = base_damage + Σ(scaling_i * stat_i)          // stat = AD 或 AP
2. mitigation = Physical : 100 / (100 + effective_armor)
                Magical  : 100 / (100 + effective_mr)
                True     : 1.0
3. crit       = (rand() < crit_chance) ? crit_multiplier : 1.0   // 仅普攻与标记可暴击的技能
4. amplified  = raw * mitigation * crit * (1 + damage_amp) * (1 - damage_reduction)
5. shield_hit = min(target.shield, amplified)
6. dealt      = amplified - shield_hit
7. target.shield -= shield_hit
   target.health  = max(0, target.health - dealt)
8. lifesteal  = dealt * lifesteal * factor        // factor: 英雄 1.0 / 战偶 0.33 / 野怪 0.33 / 建筑 0
9. if health == 0 → DeathEvent（§16.1）
```

当前 `game_core::systems::apply_mitigated_damage` 实现的是步骤 1~2 与 7 的简化版（无暴击/护盾/吸血/放大）；M2 需按本式扩展，且**必须保持纯函数可单测**。

### 5.3 减免边界

- `effective_armor < 0` 时按 0 处理（不做负甲增伤），避免除零与无穷。
- `dealt` 下限 0，上限 `1e6`；溢出视为异常并记日志（不发往客户端）。
- 真实伤害仍受护盾与 `damage_reduction` 影响，但不受护甲/魔抗。
- 衰竭（§1.2）按 `True` 伤害结算，且**不受护盾影响**。

### 5.4 伤害归属与补刀

| 情形 | 金币归属 | 经验归属 |
|---|---|---|
| 英雄对单位造成最后一击 | 该英雄 | 该英雄 |
| 战偶/哨塔造成最后一击 | 无（不产生金币） | 附近 8 单位内己方英雄平分 |
| 队友协助（近 8 单位、近 5 s 内造成过伤害） | 击杀者得全额，助攻者各得 `hero_kill_gold × 0.35` | 击杀者 200，助攻者各 100 |
| 野怪 / 中立目标 | 最后一击者 | 半径 12 内己方英雄平分 |

---

## 6. 普攻

| 项 | 定义 |
|---|---|
| 攻击间隔 | `attack_interval = 1 / attack_speed`（基础 1.0 s） |
| 前摇 `windup` | `0.25 × attack_interval`（近战 0.20、远程 0.30，乘以对应系数） |
| 伤害时点 | 前摇结束的那一 tick，服务端立即结算（近战）或生成弹体（远程） |
| 后摇 | `0.15 × attack_interval`；**移动指令可取消后摇**（不取消已结算的伤害） |
| 弹体速度 | 远程 30 单位/s；近战不生成弹体 |
| 索敌 | 无锁定时选射程内最近敌方，优先级 `Hero > Construct > Monster > Sentry > SourceCore` |
| 锁定 | 右键指定 `NetId`；目标在射程内则优先攻击它，忽略其它目标 |
| 射程判定 | 双方**中心距离** ≤ `attack_range + target.radius` |
| 冷却字段 | `Hero.attack_cooldown`（秒），每 tick 减 `FIXED_TIMESTEP`，`> 0` 时不可普攻 |
| 特殊 | 被致盲时普攻必定落空但仍消耗冷却；被眩晕/沉默时不可发起普攻（沉默只禁技能） |

**取消规则**：右键点地 / 锁定新目标 / 进入 `Casting` 前摇 / 阵亡，均可打断普攻后摇；前摇一旦过半则不可取消（防止"卡前摇"）。

---

## 7. 技能系统

### 7.1 技能槽

每英雄：**1 被动 + 3 主动（Q/E/R）+ 1 终极（F，等级 6 解锁）**。M1 占位英雄「灰烬守望」当前仅实现 3 主动，被动与终极的填充属 M2。

### 7.2 目标类型（`target_kind`）

| 类型 | 语义 | 瞄准输入 | 结算方式 |
|---|---|---|---|
| `self` | 以自身为中心 | 无需选点 | 立即结算周身 `aoe_radius` |
| `direction` | 朝指定方向 | 光标地面点 | 沿方向生成矩形/弹体 |
| `point` | 指定落点 | 光标地面点 | 落点 `aoe_radius` 内结算 |
| `entity` | 指定单位 | 光标附近可选实体 | 单体结算 |
| `projectile` | 直线弹道 | 光标地面点 | 弹体飞行，首个命中单位结算 |

### 7.3 施法流程

```
按键 → Aiming（显示射程环 / 落点 / 高亮目标）
     → 左键确认：校验 (a) 未沉默/眩晕 (b) 能量 ≥ cost (c) CD = 0 (d) 目标在 range 内
     → 扣能量、进入 Casting（前摇 cast_windup 秒）
     → 前摇结束：按 target_kind 结算、置 CD = effective_cd
     → 后摇 0.1 s → Idle
```

**打断**：前摇期间被眩晕 / 沉默 / 位移 / 阵亡 → 施法取消，`CD = effective_cd × 0.5`，**能量不返还**。被位移打断时若技能已结算则不回滚。

**射程夹紧**：客户端提交的落点超出 `range` 时，服务端把落点钳制到 `caster + normalize(aim − caster) × range`，不拒绝输入（避免网络抖动导致技能空放）。

### 7.4 技能数值表（占位）

| id | 名称 | 槽 | CD (s) | 能量 | 射程 | 前摇 (s) | 基础伤害 | 加成 | 类型 | 目标 |
|---|---|---|---|---|---|---|---|---|---|---|
| `ab_ashwarden_q` | 余烬突刺 | Q | 6.0 | 40 | 8.0 | 0.15 | 80 | 0.65 AD | Physical | direction |
| `ab_ashwarden_w` | 灰幕 | E | 12.0 | 60 | 10.0 | 0.25 | 40 | 0.45 AP | Magical | point，附带 30% 减速 2 s |
| `ab_ashwarden_r` | 守望裁决 | R | 45.0 | 120 | 14.0 | 0.40 | 220 | 0.90 AD | Physical | projectile |
| `ab_frostwarden_q` | 霜痕 | Q | 7.0 | 35 | 9.0 | 0.20 | 70 | 0.55 AP | Magical | entity，附带 1.0 s 定身 |
| `passive_ashwarden` | 余温 | 被动 | — | — | — | — | — | — | — | 每第 3 次普攻附加 `20 + 0.3 AD` 真实伤害（M2） |
| `ult_ashwarden` | 长夜守望 | F | 90.0 | 160 | 16.0 | 0.60 | 300 | 1.10 AD | Physical | point，半径 5，6 级解锁（M2） |

> `gx-proto-content::AbilityDef` 当前仅含 `cooldown / energy_cost / range / cast_windup / damage / target_kind`；M2 需补 `scaling / damage_type / aoe_radius / status_effect / cooldown_reduction` 字段，并让 `game_core::abilities::AbilityDef` 与配置表合并为单一数据源（消除 `DN-38` 的双表问题）。

---

## 8. 状态效果

### 8.1 八类效果

| id | 名称 | 作用 | 可否被驱散 |
|---|---|---|---|
| `stun` | 眩晕 | 禁止移动/普攻/施法，打断前摇 | 是（仅高阶净化） |
| `root` | 定身 | 禁止移动，可普攻/施法 | 是 |
| `slow` | 减速 | `move_speed × (1 − pct)` | 是 |
| `silence` | 沉默 | 禁止施法，可普攻/移动 | 是 |
| `blind` | 致盲 | 普攻必落空 | 是 |
| `shield` | 护盾 | 先于生命吸收伤害 | 否（按数值消耗） |
| `invulnerable` | 无敌 | 免疫一切伤害与控制 | 否 |
| `burn` | 灼烧 | 每 0.5 s 结算一次 `True` 伤害 | 是 |

### 8.2 叠加规则

- **同类效果不叠乘**：取强度最大者，时长刷新为 `max(剩余时长, 新时长)`（不累加）。
- **不同类共存**：`stun` 与 `root` 同时存在时取更严格者；`slow` 取最大值（不求和）。
- **控制递减**：同一目标在 8 s 内被连续控制时，第 2/3/4 次控制时长分别 `×0.65 / ×0.42 / ×0.28`，之后 `×0.28`；8 s 无控制则重置。
- **驱散优先级**：`invulnerable` > `shield` > 控制类 > `burn`；一次净化只移除一层。
- **无敌与衰竭**：衰竭（§1.2）无视 `invulnerable`，否则加时无法收敛。

### 8.3 状态存储

`StatusEffects` 组件保存 `Vec<EffectInstance { id, strength, remaining_ticks, source_net_id }>`；每 tick 递减 `remaining_ticks`，到 0 移除。状态**必须进入服务端快照**（当前 `EntityDelta` 无状态字段，M2 需扩 `CH_STATUS` 位）。

---

## 9. 经济与等级

### 9.1 金币来源

| 来源 | 数值 | 结算条件 |
|---|---|---|
| 初始金币 | 500 | `T=0` |
| 被动收入 | `0.05 金币/tick = 3 金币/s = 180/分钟` | 每 tick 结算，存活或阵亡均发放 |
| 战偶补刀 | 22 | 英雄最后一击 |
| 野怪 | 见 §13 | 最后一击 |
| 英雄击杀 | 180（基础） | 见 §9.2 |
| 助攻 | `180 × 0.35 = 63` | 见 §5.4 |
| 哨塔摧毁 | 全队每人 120 | 最后一击所在队 |
| 辉矿占领 | 团队 120 + 每人 40 | §14.2 |
| 渊卫 | 团队 300 + 每人 100 | §14.4 |

### 9.2 连杀赏金

```
streak_bonus = min(kill_streak - 1, 5) * 45
kill_gold    = 180 + streak_bonus          // 上限 405
```
`kill_streak` 在阵亡时归零；连续击杀间隔超过 90 s 也归零。被终结连杀时，终结者额外获得 `min(streak,5) × 30` 的"终结赏金"。

### 9.3 金币用途与限制

- 金币只能购买装备（§11）；不可交易、不可转移。
- 出售装备返还 `floor(price × 0.6)`，合成品按**总投入价**计算。
- 装备栏 6 格 + 1 主动道具格，满栏时禁止购买（UI 需明确提示）。

---

## 10. 装备与商店

### 10.1 完整商品表（占位）

| id | 名称 | 价格 | AD | AP | HP | 护甲 | 魔抗 | 其它 | 构筑 |
|---|---|---|---|---|---|---|---|---|---|
| `item_edge_shard` | 棱刃碎片 | 300 | +12 | 0 | 0 | 0 | 0 | — | BLADE |
| `item_arcane_shard` | 秘纹碎片 | 300 | 0 | +12 | 0 | 0 | 0 | — | ARCANE |
| `item_guard_shard` | 守护碎片 | 300 | 0 | 0 | +120 | 0 | 0 | — | GUARD |
| `item_swift_boots` | 迅行靴 | 600 | 0 | 0 | 0 | 0 | 0 | 移速 +0.6 | UTILITY |
| `item_edge_blade` | 棱锋长刃 | 1200 | +40 | 0 | 0 | 0 | 0 | — | BLADE |
| `item_arcane_staff` | 秘纹法杖 | 1200 | 0 | +45 | 0 | 0 | 0 | — | ARCANE |
| `item_bulwark` | 壁垒护甲 | 1100 | 0 | 0 | 0 | +35 | 0 | — | GUARD |
| `item_lumin_charm` | 辉光护符 | 1000 | 0 | 0 | 0 | 0 | +25 | — | GUARD |
| `item_rift_cleaver` | 裂空巨刃 | 3200 | +90 | 0 | 0 | 0 | 0 | 暴击 +15% | BLADE |
| `item_star_core` | 星轨法核 | 3200 | 0 | +110 | 0 | 0 | 0 | 冷却缩减 +20% | ARCANE |
| `item_warden_wall` | 守望壁垒 | 2800 | 0 | 0 | +400 | +45 | 0 | — | GUARD |
| `item_blink_cloak` | 瞬行披风 | 2400 | 0 | 0 | 0 | 0 | 0 | 主动：位移 6 单位，CD 45 s | UTILITY |

### 10.2 合成 DAG

```
棱刃碎片 ──> 棱锋长刃 ──> 裂空巨刃
秘纹碎片 ──> 秘纹法杖 ──> 星轨法核
守护碎片 ──> 壁垒护甲 ──> 守望壁垒
            辉光护符 ──┘
迅行靴 / 瞬行披风 : 独立成品
```
- `ItemEdge { from, to }` 表示"持有 `from` + 支付 `(price(to) − price(from))` 差价 → 升级为 `to`"。
- 校验器要求 DAG（`gx_config` 已实现）；禁止环、禁止悬空引用、禁止价格为 0。
- 双构筑要求（`FR-34`）：`BLADE` 与 `ARCANE` 两条路径对同一英雄均可用，且终极装备总差价 ≤ 3200。

### 10.3 购买规则

```
preconditions:
  1. 英雄存活，或处于阵亡等待期且距上次死亡 ≤ 5 s
  2. 位于己方商店区（距己方源核 ≤ 12）
  3. gold ≥ 差价（或售价，若未持有前置）
  4. 目标栏位未满；若为升级则前置装备存在
effects (原子事务，服务端单 tick 内完成):
  gold -= cost
  remove(from)；add(to) 或 add(item)
  重算派生属性（§4.3）；按成长量补足当前生命/能量
  广播 InventoryChanged 事件（ReliableOrdered，含 event_id）
```
购买失败必须返回明确 `RejectReason::ShopConditionFailed`（具体子码），不得静默吞掉。

### 10.4 商店 UI 规格

- 按 `P` 打开；按构筑（BLADE / ARCANE / GUARD / UTILITY）分页；每格显示图标（程序化像素纹理）、名称、价格、属性增量、可负担状态。
- 不可购买时按钮置灰并给出原因文本。
- 显示当前金币、6 格装备栏、售出按钮（返还 60%）。

> **P-U1 落地状态**（`docs/06_VISUAL_SPEC.md` §13）：按 `P` 打开、5 类分页筛选、12 件商品
> 中文名 / 价格 / 8 项属性增量 / 合成前置 / 描述、当前金币、6 个售出栏位按钮均已实现；
> 「购买」→ `ClientMessage::PurchaseItem { item_id }`、「售出」→ `SellItem { slot }`，
> 由 `ClientCommandQueue` 统一上行，`ShopToast` 显示「已提交…等待服务端裁决」。
> **仍缺**：按钮置灰与具体拒绝原因文本、主动道具格、6 格栏位的真实内容显示（当前用编号按钮）；
> 数值、栏位、距离、阵亡与返还仍全部以服务端 `try_buy/try_sell` 为准。

---

## 11. 兵线战偶（Construct）

### 11.1 属性

| 项 | 基础 | 成长 |
|---|---|---|
| 生命 | 240 | `+20 / 分钟`（按 `T / 3600` 取整） |
| 攻击 | 18 | `+2 / 分钟` |
| 护甲 / 魔抗 | 10 / 10 | 0 |
| 移速 | 2.4 单位/s | 0 |
| 攻距 / 攻速 | 1.5 / 1.0 次/s | 0 |
| 补刀金币 / 经验 | 22 / 30 | 0 |

### 11.2 波次与路点

- 首波 `T=900`，此后每 `1500 tick`（25 s）一波。
- 每波 **4 近战 + 1 远程**（每 3 波追加 1 远程，上限 3）。
- 路点：`己方源核（左/右端）→ 沿北侧水平兵线推进 → 跨河桥面 → 敌方源核`（单条兵线，11 个路点关于 `x=0` 镜像）；到达路点半径 2 内推进下一路点。
- 遇敌判定：半径 6 内出现敌方单位 → 停止推进并攻击（优先级 英雄若正在攻击己方英雄 > 战偶 > 哨塔）。
- 目标 `征募井` 被己方占领后：己方战偶 `+25% 生命 / +20% 攻击`（§14.3）。
- 战偶不提供视野给敌方；己方战偶作为可见源由 `constructs_provide_vision` 开关控制（`VIS-4`）。

---

## 12. 野怪与营地

### 12.1 完整野怪表（占位）

| 名称 | 类型 | 生命 | 攻击 | 护甲/魔抗 | 移速 | 攻距 | 攻速 | 金币 | 经验 | 仇恨半径 | 脱战半径 | 刷新 (tick) | 首次 (tick) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 树卫 | `Treant` | 2500 | 80 | 30/30 | 3.5 | 1.5 | 1.0 | 60 | 100 | 10 | 15 | 5400 | 900 |
| 核卫 | `CoreWarden` | 1800 | 60 | 20/20 | 3.5 | 5.0 | 0.8 | 40 | 60 | 12 | 15 | 5400 | 900 |
| 鎏金蟾 | `GildedToad` | 1200 | 40 | 10/10 | 4.0 | 1.5 | 1.2 | 80 | 50 | 8 | 12 | 5400 | 900 |
| 辉矿 | `LodeShrine` | 4000 | 100 | 40/40 | 0 | 2.0 | 1.0 | 200 | 300 | 0 | 0 | 5400 | 900 |
| 征募井 | `MusterWell` | 5000 | 120 | 50/50 | 0 | 2.0 | 1.0 | 300 | 500 | 0 | 0 | 5400 | 900 |
| 渊卫 | `AbyssWarden` | 20000 | 250 | 100/100 | 3.0 | 3.0 | 0.7 | 500 | 1000 | 20 | 30 | 7200 | 54000 |

> 表中数值与 `server/src/spawn.rs::spawn_jungle_camps` 现有实现一致；M2 需把该 `match` 迁移到 `ConfigSet`（`DEV-02` 遗留项）。

### 12.2 营地坐标

> 下表为**基准布局坐标**，实际世界坐标统一乘 `MAP_SCALE = 1.5`
> （例如蓝方树精 `(-44,0,20)` → `(-66,0,30)`）。哨塔 / 草丛 / 路点同理。

| 阵营侧 | 坐标 | 内容 |
|---|---|---|
| Blue 侧（左半野区） | `(-44,0,20)` | 树精 `Treant` |
| Blue 侧 | `(-116,0,8)` | 后方大型野怪 `CoreWarden` |
| Blue 侧 | `(-76,0,44)` | 小型野怪 `GildedToad` |
| Blue 侧（左下角） | `(-120,0,60)` | 兵矿 `LodeShrine` |
| Red 侧（右半野区） | `(44,0,20)` | 树精 `Treant` |
| Red 侧 | `(116,0,8)` | 后方大型野怪 `CoreWarden` |
| Red 侧 | `(76,0,44)` | 小型野怪 `GildedToad` |
| Red 侧（右下角） | `(120,0,60)` | 兵矿 `LodeShrine` |
| 正下方中央（共用） | `(0,0,64)` | 野区商店 `MusterWell` |
| 兵线中点正下方 / 河道上 | `(0,0,8)` | 渊卫 / 中央矿坑 `AbyssWarden` |

> 每座阵营营地都是另一侧对应营地关于中轴 `x = 0` 的镜像；
> 共用商店与中央矿坑落在镜像轴上，保证双方野区资源与可达距离完全对等。
> 营地 `team` 只标记归属哪半边野区，服务端实际把野怪生成为中立。

### 12.3 AI 行为（野怪）

```
Idle ──敌方单位进入 aggro_range──> Aggro
Aggro ──追击/攻击──> 距离出生点 > leash_range ──> Returning
Aggro ──目标死亡/丢失──> Returning
Returning ──回到出生点──> 每秒回复 8% 最大生命 ──> Idle
```
- 静止型（移速 0）不进入 `Aggro`，被攻击时仅反击 `attack_range` 内目标。
- 脱战（`Returning`）期间**不可被再次拉仇恨**，且免疫来自野怪之外的伤害衰减。
- 野怪不追击到河道桥之外；不会攻击战偶以外的建筑。

---

## 13. 中立目标

### 13.1 通用占领模型

```
capture_progress ∈ [-100, 100]   // 负值代表 Red 方进度
每 tick:
  blue_n = 半径 8 内存活 Blue 英雄数
  red_n  = 半径 8 内存活 Red 英雄数
  delta  = blue_n - red_n
  capture_progress += sign(delta) * min(|delta|, 3)
  // 争夺时（双方同时在场）进度冻结
  if blue_n > 0 && red_n > 0: delta = 0
  if |progress| >= capture_threshold → 归属该方，广播 ObjectiveCaptured
```
- 占领期间目标本体**可被攻击**，生命降至 0 时归属最后一击所在队（击杀判定优先于占领判定）。
- 归属后进入 `cooldown = respawn_ticks`，期间不可再被占领或攻击。
- `ObjectiveCaptured` 属全局事件（`VIS-6`）：**只含 objective_id / owner / event_id，不得携带任何实体字段**。

### 13.2 辉矿（LodeShrine）

- 阈值 100；占领后全队每人 +40 金币、团队 +120 金币。
- 归属期间每 `1800 tick`（30 s）向归属方每人发放 30 金币，直到被重新争夺。
- 视觉：占领进度环，程序化像素纹理。

### 13.3 征募井（MusterWell）

- 阈值 100；占领后归属方战偶获得 `+25% 生命 / +20% 攻击`（§11.2）。
- 效果持续到对方重新占领；不发放金币。
- 效果作用于**已存在与后续生成**的全部战偶，服务端在每次波次生成时读取当前归属。

### 13.4 渊卫（AbyssWarden）

- `T=54000`（15 min）首次刷新，之后每 `7200 tick`（120 s）。
- 阈值 200（击杀制不适用，按占领/伤害贡献判定：伤害最高的队伍获得归属）。
- 归属方获得「渊卫推进」buff 120 s：附近半径 20 内己方战偶 `+50% 生命 / +30% 攻击 / +0.4 移速`。
- 加时期间（§1.2）不再刷新，避免无限延长。

> **P-U10 落地状态与偏差**：§13.2 的占领赏金、30 s 周期收益，§13.4 的首刷 / 重生 / 加时门禁、
> 伤害贡献归属、120 s 增益，以及 §14.4 的渊卫赏金均已落地（见 §22.5、README §6.12）。
> 三处简化：① 「附近半径 20 内战偶」实现为归属方整条兵线，且只作用于归属期间**新生成**的
> 战偶，不回溯增强已存在单位；② 渊卫当前是坑位内的静止野怪，**尚无捕获进度条与沿兵线推进
> 的 AI**，归属通过队伍累计伤害直接判定；③ 阈值 200 尚未作为独立捕获计量表落地。
> §13.3 征募井的战偶强化仍未接入波次系统。战争迷雾为可见源圆盘 + 已探明记忆，
> 不是真实 LoS（墙体不遮挡视野），与 `GX-ARCH-2026-001` §8.5 VIS-9 的一期约定一致。

---

## 14. 哨塔与源核

### 14.1 数值

| 类型 | 生命 | 攻击 | 攻距 | 攻速 | 护甲/魔抗 |
|---|---|---|---|---|---|
| 外塔 `Outer` | 5000 | 120 | 12 | 1.0 | 40/40 |
| 中塔 `Inner` | 6000 | 140 | 12 | 1.0 | 50/50 |
| 内塔 `Base` | 7000 | 160 | 12 | 1.0 | 60/60 |
| 源核 `SourceCore` | 12000 | 200 | 14 | 1.0 | 70/70 |

### 14.2 护盾与脱战

- `shielded_without_constructs = true`：当射程内**无己方战偶**时，哨塔受到的伤害 `×0.4`（60% 减伤）。源核不受此规则影响。
- 前置塔未被摧毁时，后续塔与源核获得 `×0.5` 减伤（"逐塔推进"规则）。
- 脱战 8 s（480 tick）后每秒回复 `2% 最大生命`，上限 100%；被攻击即重置脱战计时。
- 哨塔每次切换目标时，前 2 次攻击伤害 `×0.5`（防止瞬间秒杀换塔英雄）。

### 14.3 仇恨优先级

1. 正在攻击己方英雄的敌方英雄
2. 射程内最近的敌方战偶
3. 射程内最近的敌方英雄
4. 无目标 → 待机

---

## 15. 死亡、重生与胜负

### 15.1 死亡

```
DeathEvent { victim, killer, assists[], T }
victim.status = Dead
victim 立即停止一切结算（不可被攻击、不可造成伤害、不可施法）
金币/经验按 §5.4、§9.2 结算
kill_streak(victim) = 0
```

### 15.2 重生

```
respawn_ticks = (8 + 2 * (level - 1)) * 60, clamp(..., 480, 2400)   // 8~40 s
重生位置 = 己方源核外 6 单位的固定点
重生瞬间：health = max_health, energy = max_energy
           shield = 0, 清除全部状态效果
           获得 invulnerable 3 s + 移速 +50% 3 s（主动攻击/施法会立即移除两者）
```

### 15.3 胜负判定

1. 任一方源核 `health ≤ 0` → 另一方胜利，立即 `MatchEnd`。
2. `T ≥ 90000` 未分胜负 → 依次比较：源核剩余生命比例 > 存活哨塔数 > 队伍总金币 > 总击杀数；全部相等判平局。
3. 一方全部英雄同时离线超过 180 s → 判负（由 M3 网关判定，M2 用占位桩）。
4. 投降（P2）：`T ≥ 54000` 可发起，需 ≥2/3 队员同意；本文不实现，仅预留接口。

---

## 16. 表现层规范（程序化 / 零外部资源）

> 本节为需求变更新增，取代 `FR-85` / `FR-86` / `DN-06` / `DN-32` 中关于"原创 3D 资产、商用授权字体"的表述。
> **v1.3 修订**：按 `README.md` §6 的代码实际状态回写 PRES-3 / 4 / 5 / 6 / 7 / 9 与 §16.2 / §16.3，
> 消除「6 个 `Cuboid`」「2×2 网格 / 水块」「禁 OS 字体」「不存在的生成函数」等占位表述；新增 §16.4 记录细化路线。

### 16.1 硬约束

| 编号 | 约束 | 现状 |
|---|---|---|
| PRES-1 | 表现层**全部由代码生成**：几何、纹理、材质、特效、图标、字形，均不得读取任何外部资源文件 | 符合 |
| PRES-2 | 工作树根不得存在 `assets/` 等美术资源目录；`LEGAL/` 内第三方数据不得被运行时读取 | 符合 |
| PRES-3 | 发布包内不得含字体二进制；运行时可借用宿主 OS 已有中文字形并注入 `FontDefinitions`，但必须有 ASCII 回退 | 符合（`ui.rs::setup_cjk_font`） |
| PRES-4 | 风格为 **Minecraft 式体素**：所有 **3D 可见体**由轴对齐立方体拼装，纹理为低分辨率像素图且使用 `Nearest` 采样，禁止平滑/抗锯齿滤镜；2D HUD / 小地图允许矩形但须方块化 | **部分违规**：弹体仍为 `Sphere`，见 §16.4 |
| PRES-5 | 配色为有限的方块化调色板，单材质不超过 16 色 | 符合（12 种 `PixelKind`，每材质 2 色） |
| PRES-6 | 英雄为分层「方块人」：`Cuboid` 子实体数 ≥ 6，含头 / 躯干 / 双臂 / 双腿与可变头饰 / 武器 / 披风 / 肩甲；队伍色只作用于脚环与点缀色 | 符合（约 13~19 个方块 + 2 肩枢轴） |
| PRES-7 | 地形与建筑由代码生成的方块拼装：7 层高度地表、下沉河道与河岸、路点条带、边界墙、基地、营地、草丛与动态骨架建筑 | 符合；P-V4 落地 `TERRAIN_LAYERS` 与 `RIVER_HALF_WIDTH`，`PixelKind::Water` 已启用（见 `docs/06_VISUAL_SPEC.md` §12.2.2） |
| PRES-8 | 特效（命中、施法、占领进度）由临时体素与像素纹理实现，生命周期 ≤ 1 s，不得引入音频或着色器文件 | 符合 |
| PRES-9 | UI 以 egui 内置字形为主；运行时注入 OS 中文字形属允许例外，但不得随包分发，探测失败全部文案回退 ASCII；面板为方块边框 + 纯色块，无圆角渐变 | 符合 |

### 16.2 生成函数契约（按现有代码回写）

| 函数（`crates/client/src/`） | 输出 |
|---|---|
| `voxel::make_pixel_texture` | 16×16 RGBA 像素纹理（`Nearest` 采样、确定性哈希噪声） |
| `voxel::voxel_material` | `StandardMaterial` + 程序化纹理；每个英雄独立 5 套材质 |
| `voxel::spawn_voxel_hero` | 分层 `HeroRig` + 约 13~19 个 `Cuboid` 子实体 |
| `voxel::spawn_voxel_minion` | 小兵 3 方块（队伍色躯干 / 头 / 武器） |
| `voxel::spawn_voxel_stack` | 哨塔 / 源核层级递减方块堆叠 + 金顶 |
| `voxel::spawn_ground_strip` | 可旋转路面 / 单条水平兵线条带 / 纵向河岸石径 |
| `voxel::spawn_voxel_marker` | 4 类方块指示框（选中 / 瞄准 / 范围 / 落点） |
| `voxel::spawn_voxel_terrain` | 地面、边界墙、兵线 / 野区分隔堤墙与门柱、基地、野区营地、草丛一次性生成 |
| `rendering::spawn_combat_fx` | 消费 `CombatFxQueue`，生成挥击 / 刀光 / 弹道 / 碎片 / 闪光 |
| `rendering::tick_*`（13 个系统） | 临时体素的生命周期、缩放、旋转与位移插值 |

> 旧版 §16.2 中的 `spawn_voxel_structure` / `spawn_voxel_effect` 从未实现，已从契约表移除。

### 16.3 合规门禁（按 `crates/gx-e2e/tests/compliance.rs` 回写）

| 用例 | 断言 |
|---|---|
| `TC-COMPLIANCE-01` | 非白名单树的禁用词扫描（`docs/**`、`LEGAL/**` 等为白名单）；文件名与内容均检查 |
| `TC-COMPLIANCE-02` | `ASSET_REGISTRY.md` 的登记项、许可列、待资产 / 外部依赖标注；`LEGAL/` 三个隔离目录存在 |
| `TC-CBT-12` | 发布树不含字体 / 图片 / 音频二进制，根无 6 类资源目录，无字节内嵌宏；`ui.rs` 具备 `UiLanguage` / `setup_cjk_font` / ASCII 回退；资产表 A-001~A-005 来源为代码生成且已提供；`spawn_voxel_hero` 体素数 ≥ 6 |
| `TC-PRES-01~04`（待新增） | P-V1 落地体素风格检查器：3D 弹体不得为 `Sphere`、不得使用平滑 / 抗锯齿采样、纹理分辨率与调色板上限、材质复用池 |

> 旧版 §16.3 声称扫描「OS 字体路径 / 字体加载调用」，与实际门禁相反（实际要求 `setup_cjk_font` 存在），已回写。

### 16.4 体素细化路线（P-V 系列）

> **P-V4 收口状态：以下五层细化已全部落地并验收**（`docs/06_VISUAL_SPEC.md` §9~§12）。
> 本段保留原始「细化前」描述作为设计依据，不再是当前状态。

细化前表现层「可玩但粗糙」：纹理仅 4×4 色块噪声、无逐像素图案与明暗面；英雄无朝向与走路 / 待机 / 施法前摇 / 硬直 / 死亡动画；
野怪与塔 / 源核无客户端实体模型；4 个技能共用一个施法方块；多数关键事件无视觉；8-bit 音频零实现。

细化在**零外部资产**约束下分五层推进，均已落地：纹理（逐像素噪声 + 描边 + 伪 AO 面明暗 + 材质池）、
几何（非英雄动态骨架 + 共享方块合批）、动画（分层骨架 + 7 态状态机 + 朝向）、
地形（`TERRAIN_LAYERS` 7 层高度 + 河道 + 合批）、
音频（8-bit 程序化合成器 + 内存 PCM），并清零 PRES-4 的 3D 违规项。

英雄设计走 `HeroVisual` 数据表 + 6 种剪影原型；技能设计走统一 `AbilityDef` → `AbilityVisual` 与
`Windup → Release → Travel → Impact → Recover` 五阶段序列。
完整路线、造型原型矩阵、四技能差异化表、分阶段计划与验收判据见 `README.md` §8。

> **P-U1 追加（信息层）**：进入即见的三列大厅、6 位英雄档案（定位 / 评分 / 技能 / 出装）、
> 12 件道具图鉴与真实商店交互、PVE / PVP 模式选择已落地；
> `ClientMessage::SelectMode` 只在 `MatchPhase::Warmup` 生效，PVP 需 2 名真人才 `Running`。
> 详见 `docs/06_VISUAL_SPEC.md` §13 与 `README.md` §6.4。

---

## 17. 服务端权威与一致性

- 全部裁决（伤害、命中、金币、等级、占领、塔仇恨、死亡）**只在服务端**执行；客户端仅做预测（位移）与表现。
- 客户端预测只对**本地英雄位移**生效；属性、金币、状态、技能 CD 一律以服务端快照为准，禁止客户端预测。
- `InputCmd` 携带 `seq / client_time_us / config_hash / checksum`；服务端校验序号单调、校验和正确、内容 hash 一致（`FR-61`）。
- 状态快照按 `GX-ARCH-2026-001` §8.5 做逐玩家视野裁剪，20 Hz delta + 2 s 基线，关键事件 `ReliableOrdered` + `event_id` 幂等。
- 对局中所有随机判定使用服务端确定性 PRNG，回放可复现。

---

## 18. 差距表（v1.0 历史快照，已被 §23 取代）

> **本节保留为 v1.0 的历史记录**：下表描述的是玩法代码落地**之前**的状态，不得作为当前实现状态引用。
> 当前状态、本轮实现范围与剩余差距一律以 **§23（M2 待办差距清单）** 为准；玩法接口以 **§22** 为准。

| 章节 | v1.0 时状态 | v1.0 时缺口 |
|---|---|---|
| §3 操作 | **已落地（v1.3 + P-U9）**：纯点击控制；`NavGrid` 动态障碍（哨塔 / 源核 / 渊卫）纳入 walls，A* 带 4096 节点预算与起终点钳制；`should_recompute` 完整优先级链；服务端同源网格校验。`TC-NAV-01~09` 全部通过 | 无需：§24 三项待办已闭环 |
| §4 属性 | `Hero` 组件有 12 个字段；`HeroDef` 配置表独立 | 缺 `hp/en_regen`、暴击/穿透/吸血/冷却缩减/护盾；双表未合并 |
| §5 伤害 | `apply_mitigated_damage` 仅 `100/(100+armor)` 简化式 | 缺暴击、护盾、放大、吸血、穿透、衰竭 |
| §6 普攻 | `attack_system` 有冷却与锁定目标 | 缺前摇/后摇、弹体速度分化、致盲、取消规则 |
| §7 技能 | `game_core::abilities` 4 个硬编码技能 + `gx-proto-content` 4 条 | 双表、缺被动/终极/加成/状态、缺打断与射程夹紧 |
| §8 状态 | 无 | 全缺（`StatusEffects` 组件 + `CH_STATUS` 快照位） |
| §9 经济 | `EconomyDef` 有占位字段 | 缺连杀赏金、助攻、被动收入实际结算 |
| §10 商店 | `ui.rs` 8 个占位格子，无购买逻辑 | 全缺（商品表需补全、事务、栏位、售出、UI） |
| §11 战偶 | `minion_spawn_system` 为空壳 | 全缺（波次、路点、成长、目标强化） |
| §12 野怪 | `spawn.rs` 硬编码 6 类数值 | 已有数值，缺 AI 状态机、刷新、脱战、配置化 |
| §13 目标 | **已落地（P-U10）**：占领点 id 按类型固定、兵矿占领与 30 s 周期收益、渊卫定时首刷 / 伤害贡献归属 / 重生 / 赏金 / 战偶强化 / 全局事件；迷雾表现见 §6.12（README） | 仍缺：渊卫捕获进度条与推线 AI、征募井战偶强化、真实 LoS 迷雾 |
| §14 塔 | `spawn_turrets` 生成实体 | 缺护盾、逐塔减伤、脱战回复、仇恨优先级 |
| §15 死亡 | 无 | 全缺 |
| §16 表现 | **本轮已提前落地**：体素方块英雄 / 体素地形与建筑 / 16×16 像素纹理 / 方块指示框 / 体素特效 / 系统字体已移除 | M2 仍需：主动画状态（行走摆臂）、程序化像素字形（中文等多语言）、机型分档 LOD |

> 本表是 `DEV-09~DEV-16`（M2 垂直切片）的**原始实现说明书**；v1.1 起玩法逻辑已按新指令部分落地（§22），剩余差距逐条列于 §23。

---

## 19. 验收用例

| 编号 | 判据 | 类型 |
|---|---|---|
| `TC-CBT-01` | **v1.3**：键盘 W/A/S/D/方向键按下时 `move_dir` 恒为 `Vec2::ZERO`；点击目标产生指向它的方向；左键点地产生非空路径；右键点地 `force=true` 且清除锁定 | 集成 |
| `TC-CBT-02` | A* 在含障碍地图上返回连通路径，且不穿越不可通行格、不穿角 | 单元 |
| `TC-CBT-03` | `raw=100, armor=22` → `dealt ≈ 81.97`；护甲穿透与魔抗公式同样有界 | 单元 |
| `TC-CBT-04` | 护盾先于生命扣除；真实伤害无视减免但受 `damage_reduction` | 单元 |
| `TC-CBT-05` | 普攻前摇内被眩晕 → 取消且 CD = 50%；后摇被移动取消但伤害已结算 | 集成 |
| `TC-CBT-06` | 同类减速不叠乘、控制递减序列 `1.0/0.65/0.42/0.28` 正确 | 单元 |
| `TC-CBT-07` | 金币不足以支付差价 / 不在商店区 / 栏位满 → 购买被拒且金币不变（原子性） | 集成 |
| `TC-CBT-08` | 合成图为 DAG，无环、无悬空引用、价格均 > 0 | 单元 |
| `TC-CBT-09` | 野怪脱战回血、不超出 `leash_range`、静止型不进入 `Aggro` | 集成 |
| `TC-CBT-10` | 辉矿双方在场时进度冻结；`ObjectiveCaptured` 事件不含任何实体字段 | 集成 + 抓包 |
| `TC-CBT-11` | 前塔未倒时内塔减伤 0.5；无己方战偶时外塔减伤 0.4 | 单元 |
| `TC-CBT-12` | 表现层扫描：发布树不含字体 / 图片 / 音频二进制、根无 6 类资源目录、无字节内嵌宏；`ui.rs` 具备 ASCII 回退；资产表 A-001~A-005 来源为代码生成且已提供；英雄由 ≥6 个体素构成 | 门禁 |
| `TC-PRES-01~04` | 体素风格检查器：3D 弹体不得为 `Sphere`、禁止平滑 / 抗锯齿采样、纹理分辨率与调色板上限、材质复用池 | 门禁（P-V1 待新增） |

### 19.1 v1.1 实现状态

| 编号 | 判据 | 类型 | 状态 | 实现位置 |
|---|---|---|---|---|
| `TC-CBT-01/02/12` | 操作（v1.2 键盘主操作）/ 寻路基线、零外部资源门禁 | 集成 + 门禁 | ✅ 已通过 | `client/src/input.rs`、`client/src/pathfinding.rs`、`gx-e2e/tests/compliance.rs` |
| `TC-CBT-03` | 伤害减免、穿透有界 | 单元 | ✅ 已通过 | `game-core/src/gameplay/combat.rs` |
| `TC-CBT-04` | 护盾先扣、真实伤害与减伤 | 单元 | ✅ 已通过 | 同上 |
| `TC-CBT-05` | 普攻前摇/后摇、取消规则 | 集成 | ⛔ 待实现 | 见 §23 `GAP-04` |
| `TC-CBT-06` | 同类不叠乘、控制递减序列 | 单元 | ✅ 已通过 | 同上 |
| `TC-CBT-07` | 购买事务原子性 | 集成 | ✅ 已通过 | `game-core/src/gameplay/shop.rs` |
| `TC-CBT-08` | 商品图 DAG / 无悬空 / 价格 > 0 | 单元 | ✅ 已通过 | 同上 |
| `TC-CBT-09` | 脱战回血、牵引半径、静止型不进 Aggro | 集成 | ✅ 已通过 | `game-core/src/gameplay/jungle.rs` |
| `TC-CBT-10` | 争夺冻结、事件不含实体字段 | 集成 + 抓包 | ◐ 逻辑已通过，抓包断言待补 | `game-core/src/gameplay/objectives.rs` |
| `TC-CBT-11` | 前塔减伤 0.5、无战偶减伤 0.4 | 单元 | ✅ 已通过 | `game-core/src/gameplay/structures.rs` |
| `TC-GP-01` | ECS 系统链真实驱动：护盾吸收 → 阵亡 → 购买 → 占领 → 重生 | 集成 | ✅ 已通过 | `gx-e2e/tests/gameplay.rs` |
| `TC-NAV-01~09` | 寻路 M2 三项待办的验收（定义见 §24） | 单元 + 集成 + 抓包 | ✅ 已通过（P-U9） | 01~03 `client/src/pathfinding.rs`；04~06 `client/src/input.rs`；07~09 `game-core/src/navigation.rs` + `gx-e2e/tests/nav_acceptance.rs` |

---

## 20. 追溯矩阵

| FR / CR | 本规格章节 | DEV | TC |
|---|---|---|---|
| `FR-12`（18±4 / 25 上限） | §1.2、§15.3 | DEV-15 | `TC-SIM-05` |
| `FR-20~23`（英雄 / 属性 / 技能） | §4、§7 | DEV-12 | `TC-CBT-03/05` |
| `FR-24~27`（普攻 / 伤害 / 状态） | §5、§6、§8 | DEV-12、DEV-13 | `TC-CBT-03/04/05/06` |
| `FR-28~31`（经济 / 装备 / 商店） | §9、§10 | DEV-14 | `TC-CBT-07/08` |
| `FR-32~35`（兵线 / 野怪 / 目标 / 塔） | §11~§14 | DEV-10、DEV-11 | `TC-CBT-09/10/11` |
| `FR-40~45`（操作 / 点触） | §3 | DEV-09 | `TC-CBT-01/02` |
| `FR-60` / `CR-05`（视野） | §2.3、`ARCH §8.5` | DEV-05 | `TC-NET-03`、`TC-CBT-10` |
| `FR-85` / `FR-86` / `CR-08`（表现层） | §16 | DEV-01、DEV-26 | `TC-CBT-12`、`TC-COMPLIANCE-01/02` |
| `CR-01`（术语） | 全文 | DEV-26 | `TC-COMPLIANCE-01` |
| 玩法落地（本轮新增） | §22.1~§22.7 | DEV-12~16（提前） | `TC-CBT-03/04/06/07/08/09/10/11`、`TC-GP-01` |
| 寻路 M2（P-U8/P-U9 已落地） | §24 | DEV-09 | `TC-NAV-01~09` ✅ |
| M2 差距逐条对照 | §23 | DEV-09~16 | §23 验收列 |

---

## 21. 待外部输入

1. 全部数值需 M2 平衡验证后回填（当前为自研占位）。
2. 12 位英雄的完整技能组与被动/终极设计（`AS-04`）。
3. 状态效果的美术/音效反馈映射（表现层由代码生成，但节奏参数需手感测试）。
4. 目标市场带来的合规差异（`CR-02/03`）不影响本规格。

> 本文与 `GX-PRD-2026-001` 冲突时，以本文为**对战细节**的细化依据；需求范围与优先级仍以 PRD 为准。四份既有文档的对应条目应同步升版，见各文档修订记录。

---

## 22. 玩法系统详细设计规格（v1.1 新增，已实现）

> 本节把 §4~§15 的规则落成**可执行接口**。除 Bevy `Component` 派生外，
> `game-core::gameplay` 全部为**纯函数**（无 IO、不读系统时间、`tick` 由参数传入），
> 由 `server::gameplay` 的 ECS 系统按固定顺序驱动。客户端不做任何裁决。
>
> | 层 | 位置 | 职责 |
> |---|---|---|
> | 纯逻辑 + 组件 | `crates/game-core/src/gameplay/{mod,combat,shop,jungle,objectives,structures,respawn}.rs` | 规则裁决、状态机、事务、常量 |
> | ECS 系统 | `crates/server/src/gameplay.rs` | 组件挂接、tick 驱动、快照、事件汇、写回既有 `Hero/Turret/JungleMonster` |
> | 系统链 | `crates/server/src/simulation.rs` | `FixedUpdate` 严格 `.chain()` 顺序 |
> | 验收 | `game-core` 单测、`gx-e2e/tests/gameplay.rs` | `TC-CBT-*`、`TC-GP-01` |

### 22.0 公共约定

| 约定 | 内容 |
|---|---|
| 时间 | 一切时长以 `tick` 表示；`TICK_RATE = 60`，`tick` 来自 `Res<ServerTick>`，纯函数只接受 `u64` |
| 随机 | `deterministic_roll(tick: u64, a: u64, b: u64) -> f32 ∈ [0,1)`，服务端确定性 PRNG；客户端不得掷骰 |
| 精度 | `f32` 比较带 `EPS = 1e-4`；金币/经验/tick/层数为整型 |
| 事件汇 | `GameplayEvents { captured, deaths, purchases }`（`Resource`）；`PendingDeaths(pub Vec<(tick, killer_net_id, victim_net_id)>)` 为内部流水，不与广播事件共用 |
| 组件挂接 | `attach_hero_gameplay` / `attach_world_gameplay` 用 `Option<&T>` 幂等插入，不改动既有生成代码 |
| 世界快照 | `WorldSnapshot { heroes, minions, turrets }` 每 tick 重建，避免多个 `&mut Transform` / `&Transform` 查询触发 B0001 |
| 系统顺序 | `tick_clock → build_world_snapshot → attach_hero → attach_world → apply_pending_inputs → movement → attack → ability_cast → projectile → damage → resolve_pending_hits → flush_damage_events → ability_cd → level_up → passive_gold → health/energy_regen → status_tick → regen → monster_ai → capture → structure_defense → shop_purchase → death_respawn` |

### 22.1 属性扩展（`§4` 落地）

**接口**（`gameplay::combat`）

```rust
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CombatStats {
    pub crit_chance: f32, pub crit_multiplier: f32,
    pub flat_armor_pen: f32, pub pct_armor_pen: f32,
    pub flat_mr_pen: f32, pub pct_mr_pen: f32,
    pub lifesteal: f32, pub cooldown_reduction: f32,
    pub damage_amp: f32, pub damage_reduction: f32,
}
impl Default for CombatStats        // 全 0，仅 crit_multiplier = 2.0
impl CombatStats { pub fn clamp(&mut self); pub fn clamped(self) -> Self }

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShieldPool { pub value: f32 }

#[derive(Component, Clone, Copy, Debug)]
pub struct RegenStats { pub hp_regen: f32, pub en_regen: f32 }  // 默认 4.0 / 6.0 每秒

pub fn apply_regen(hero: &mut Hero, regen: &RegenStats);
pub fn compute_raw(base: f32, ad: f32, ap: f32, scaling_ad: f32, scaling_ap: f32) -> f32;
```

**约束条件**

| 项 | 上限 / 规则 | 由谁保证 |
|---|---|---|
| `crit_chance` | `[0, 1]` | `clamp()` |
| `pct_armor_pen` / `pct_mr_pen` / `lifesteal` / `cooldown_reduction` | `[0, 0.4]` | `clamp()` |
| `damage_amp` | `[-0.5, 0.5]` | `clamp()` |
| `damage_reduction` | `[0, 0.5]` | `clamp()` |
| 有效抗性 | `max(0, x − flat_pen) × (1 − clamp(pct_pen, 0, 0.4))`，**负甲按 0，不做负甲增伤** | `effective_resist()` |
| 回复 | 每 tick `regen / 60`，钳制到 `max_health` / `max_energy`；阵亡或 `health ≤ EPS` 不回复 | `apply_regen` / `regen_system` |
| 升级补足 | 按成长量补足当前生命/能量，**不回满** | §4.4（M2 待办 `GAP-02`） |

**ECS 接线**：`attach_hero_gameplay` 为所有 `Hero` 幂等挂接 `CombatStats / ShieldPool / RegenStats / StatusEffects / Inventory / RespawnState / KillStreak`；`shop_purchase_system` 把装备增量写回 `Hero` 与 `CombatStats` 并调用 `clamp()`。

**验收**：`TC-CBT-03`（回复不超上限、穿透有界）、`TC-GP-01`（组件挂接 + 装备增量写回）。

### 22.2 状态效果（`§8` 落地）

**接口**

```rust
pub enum StatusKind { Stun, Root, Slow, Silence, Blind, Shield, Invulnerable, Burn }
impl StatusKind {
    pub fn is_control(self) -> bool;        // Stun/Root/Silence/Blind；Slow 属软控不参与递减
    pub fn dispel_priority(self) -> u8;     // 0 无敌 > 1 护盾 > 2 硬控 > 3 减速 > 4 灼烧
}
pub struct EffectInstance { pub kind: StatusKind, pub strength: f32,
                            pub remaining_ticks: u32, pub source_net_id: u64 }
#[derive(Component, Clone, Debug, Default)]
pub struct StatusEffects { pub effects: Vec<EffectInstance>,
                           pub control_chain: u8, pub last_control_tick: u64 }

pub fn apply(&mut self, effect: EffectInstance, tick: u64) -> u32;  // 返回实际生效时长
pub fn tick(&mut self, tick: u64) -> Vec<EffectInstance>;            // 待结算的灼烧 + 到期移除
pub fn dispel_one(&mut self) -> Option<StatusKind>;
pub fn has / strength / is_stunned / is_rooted / is_silenced / is_blind / is_invulnerable
```

**常量**：`CONTROL_WINDOW_TICKS = 480`（8 s）、`CONTROL_DR = [1.0, 0.65, 0.42, 0.28]`、`BURN_PERIOD_TICKS = 30`（0.5 s）。

**约束条件**

1. 同类效果**不叠乘**：`strength = max(旧, 新)`，`remaining_ticks = max(旧, 新)`，**不累加**。
2. 硬控按 `CONTROL_DR[min(chain,3)]` 缩时长，`ceil` 后至少 1 tick；窗口内 `chain` 递增，超过 480 tick 无控制则归零。
3. 灼烧每 30 tick 返回一次结算，`status_tick_system` 以**真实伤害**写回 `Hero.health`（不走护甲减免）。
4. 一次净化只移除一层，按 `dispel_priority` 取最小者。
5. `Invulnerable` 免疫一切伤害与控制；**加时衰竭（`DamageRequest.exhaustion`）无视无敌**（§1.2 收敛要求）。

**ECS 接线**：`status_tick_system`（推进 + 灼烧写回）→ `regen_system`（跳过阵亡）→ `death_respawn_system`（阵亡清空、重生挂 180 tick 无敌）。

**验收**：`TC-CBT-06`（递减序列 `60/39/26/17/17`、同类不叠乘）、驱散优先级单测、无敌/衰竭穿透单测、`TC-GP-01`（重生无敌）。

**未落地**：`CH_STATUS` / `CH_SHIELD` 快照位，客户端目前看不到状态与护盾（§23 `GAP-03`）。

### 22.3 商店购买事务（`§10` 落地）

**接口**（`gameplay::shop`）

```rust
pub const INVENTORY_SLOTS: usize = 6;
pub const ACTIVE_ITEM_SLOTS: usize = 1;
pub const SHOP_ZONE_RADIUS: f32 = 12.0;
pub const SELL_REFUND_PERCENT: u32 = 60;
pub const DEAD_PURCHASE_GRACE_TICKS: u64 = 300;   // 阵亡后 5 s 宽限

pub struct ItemStats { /* ad/ap/max_health/armor/mr/crit_chance/cdr/move_speed */ }
pub struct ItemDef { pub id: &'static str, pub price: u32,
                     pub from: Option<&'static str>, pub stats: ItemStats, pub active: bool }
pub enum ShopDefect { ZeroPrice, DanglingUpgrade, Cycle, DuplicateId }
pub struct ShopCatalog { pub items: Vec<ItemDef> }
    pub fn placeholder() -> Self;                 // §10.1 的 12 件商品
    pub fn get(&self, id: &str) -> Option<&ItemDef>;
    pub fn validate(&self) -> Result<(), ShopDefect>;   // DAG / 悬空 / 价格 / 重复
    pub fn total_invested(&self, id: &str) -> u32;       // 沿 from 链累加**实际支付过的差价**

#[derive(Component)] pub struct Inventory { pub slots: Vec<Option<String>> }  // 默认 6 空位
pub struct ShopRequest { item_id, distance_to_source, is_dead, death_tick, current_tick }
pub enum ShopReject { UnknownItem, DeadTooLong, NotInShopZone,
                      InsufficientGold, MissingPrerequisite, InventoryFull }
pub struct ShopReceipt { item_id, paid, removed, slot, delta: ItemStats }

pub fn try_buy(catalog: &ShopCatalog, inventory: &mut Inventory,
               gold: &mut u32, request: &ShopRequest) -> Result<ShopReceipt, ShopReject>;
pub fn try_sell(catalog: &ShopCatalog, inventory: &mut Inventory,
                gold: &mut u32, slot: usize) -> Result<u32, ShopReject>;
```

**校验顺序（唯一定义，测试据此断言）**：`UnknownItem → DeadTooLong → NotInShopZone → 计算差价 → MissingPrerequisite → InsufficientGold → InventoryFull → 事务提交`。

**约束条件**

1. **原子事务**：所有校验通过后才有唯一写入点；任何 `Err` 都不改动 `gold` 或 `slots`。
2. 升级差价 = `price(to) − price(from)`，且必须持有 `from`；合成品 `delta = stats(to) − stats(from)`，避免属性重复叠加。
3. 售出返还 `total_invested × 60%`；`total_invested` 用差价累加（碎片 300 + 长刃补 900 + 巨刃补 2000 = 3200，而非三件标价之和）。
4. `validate()` 保证图无环、无悬空引用、无重复 id、价格 > 0；用三色 DFS 检测环；`total_invested()` 的链长上限 32 用于防病态数据。
5. 商品表是 `Resource`（`ShopCatalogResource`），**局内不可变**，热更只影响新对局（`FR-61`）。

**ECS 接线**：`ShopIntents` 队列 → `shop_purchase_system` 按 `NetId` 定位英雄、按队伍源核常量计算 `distance_to_source`、覆盖 `is_dead/death_tick/current_tick`（**客户端提交的距离不可信**）→ `try_buy` → 写回金币与属性 → `GameplayEvents.purchases`。

**验收**：`TC-CBT-07`（三类拒绝均不改金币/栏位、升级差价）、`TC-CBT-08`（DAG / 环 / 悬空 / 重复）、`TC-GP-01`（ECS 内扣款与 +12 AD）。

**已落地（P-U1）**：购买 / 售出的协议 opcode 与客户端商店 UI（`§10.4` 状态注）。
**仍缺**：主动道具格、UI 置灰原因文本、6 格栏位真实内容显示（§23 `GAP-07`）。

### 22.4 野怪 AI（`§12` 落地）

**接口**（`gameplay::jungle`）

```rust
pub enum MonsterState { Idle, Aggro, Returning }
#[derive(Component)] pub struct MonsterAi {
    state, spawn: Vec2, aggro_range, leash_range, attack_range, move_speed,
    max_health, health, out_of_combat_ticks: u32,
}
pub struct MonsterInput  { position: Vec2, nearest_enemy: Option<Vec2> }
pub struct MonsterOutput { state, desired_move_dir: Vec2, should_attack: bool, health_after: f32 }
pub fn step_monster(ai: &mut MonsterAi, input: &MonsterInput) -> MonsterOutput;

#[derive(Component)] pub struct MonsterRespawn { first_spawn_tick, respawn_ticks,
                                                 alive, respawn_at_tick }
pub fn on_monster_death(timer: &mut MonsterRespawn, tick: u64);
pub fn step_monster_respawn(timer: &mut MonsterRespawn, tick: u64) -> bool;
```

**状态机（两阶段：`状态迁移` → `按迁移后状态执行行为`，避免切换当帧空转）**

| 当前 | 条件 | 下一状态 | 行为 |
|---|---|---|---|
| `Idle` | `enemy within aggro_range` 且非静止型 | `Aggro` | 追击或攻击 |
| `Aggro` | `spawn_dist > leash_range` 或目标丢失 | `Returning` | 回出生点、持续回血、**不可再次拉仇恨** |
| `Returning` | `spawn_dist ≤ 0.4` | `Idle` | 回血 |
| 静止型（`move_speed = 0`） | 任意 | 恒 `Idle` | 只反击 `attack_range` 内目标，不追击 |

**约束**：脱战与待命每秒回 `8% max_health`（`0.08/60` per tick）；`desired_move_dir` 已乘 `move_speed`，ECS 每 tick 按 `/60` 位移；刷新 `first_spawn_tick` 与阵亡后 `respawn_ticks` 分离，`step_monster_respawn` 单次触发。

**ECS 接线**：`attach_world_gameplay` 用 `JungleMonster` 既有字段构造 `MonsterAi`（含 `aggro_range/leash_range/attack_range`）并挂 `MonsterRespawn`；`monster_ai_system` 从 `WorldSnapshot.heroes` 取最近存活敌方英雄，避免查询冲突。

**验收**：`TC-CBT-09`（静止型不进 Aggro、超牵引半径脱战并回血、脱战期忽略新仇恨）、刷新单次触发单测、`TC-GP-01`（挂接并参与系统）。

**未落地**：野怪刷新未真正重新生成实体、数值仍硬编码在 `spawn.rs`（§23 `GAP-10`）。
**已落地**：野怪攻击已接入 `AttackCommand`（`monster_attack_system`）；P-U8 另补权威 tick 冷却、静止型（征募井 / 辉矿）反击，以及可覆盖双方 `Collider` 推出距离的攻击射程。

### 22.5 占领目标（`§13` 落地）

**接口**（`gameplay::objectives`）

```rust
pub const MAX_CAPTURE_RATE: i32 = 3;
pub const CAPTURE_RADIUS: f32 = 8.0;
#[derive(Component)] pub struct CapturePoint {
    objective_id: u32, progress: i32, threshold: i32, owner: Option<Team>,
    radius: f32, respawn_ticks: u64, cooldown_until_tick: u64, contested: bool,
}
pub struct CaptureInput { blue_count: u32, red_count: u32, tick: u64 }
pub enum CaptureOutcome { None, Contested, Captured(Team), OnCooldown }
pub fn step_capture(cp: &mut CapturePoint, input: CaptureInput) -> CaptureOutcome;

/// 字段集合固定为 3 项，禁止加入任何实体信息（VIS-6）
pub struct ObjectiveCapturedEvent { objective_id: u32, owner: Team, event_id: u64 }
pub fn capture_event(cp: &CapturePoint, tick: u64) -> Option<ObjectiveCapturedEvent>;
```

**约束条件**

1. `progress ∈ [−threshold, +threshold]`，正值 Blue / 负值 Red；每 tick `delta = sign(blue−red) × min(|blue−red|, 3)`。
2. **双方同时在场 → `Contested`，进度冻结**，不衰减、不推进。
3. `|progress| ≥ threshold` → 归属该方、`progress` 归零、`cooldown_until_tick = tick + respawn_ticks`；冷却内返回 `OnCooldown`。
4. `event_id = (tick << 8) | (objective_id & 0xff)`，同 tick 重复广播幂等；事件字段数 `EVENT_FIELD_COUNT = 3`，不含 `NetId`/`Entity`/坐标。
5. 占领状态由服务端独占裁剪后下发，禁止把实体列表塞进全局事件。

**ECS 接线**：`attach_world_gameplay` 生成 3 个占领点（两座辉矿 + 一座征募井，坐标直接取 `map::JUNGLE_CAMPS`，`threshold = 100`、`respawn_ticks = 5400`）；`capture_system` 用 `WorldSnapshot.heroes` 统计范围内存活英雄，占领成功时把事件推入 `GameplayEvents.captured`。

**验收**：`TC-CBT-10`（争夺冻结、事件无实体字段）、速率上限/冷却/红方负进度单测、`TC-GP-01`（ECS 内占领事件）。

**P-U10 落地修订**：
- 旧版本节的「2 个占领点 / 未落地 buff / 未落地广播」均已过时。`objective_buff_system` 早已消费 `captured` 发全队 buff 并广播 `ObjectiveCaptured`/`ObjectiveBuffGranted`；`drain_gameplay_events` 用单一自增游标统一分配 `event_id`。
- 占领点 id 改为**按类型固定**：两座辉矿 = 1、征募井 = 2、渊卫 = 3（预留）。旧版用 `idx+1` 得到 `[辉矿, 辉矿, 征募井]`，与 `attributes::objective_buff` 的 1/2/3 语义错配，导致第二座兵矿发征募井 buff、征募井发渊卫 buff。两座辉矿共用 id 1 不影响 `capture_system`（按实体遍历，无 id 索引）；周期收益按**队伍**计时，因此也不会漏发。
- §13.2 的占领 +40/人、+120 团队与 1800 tick 周期 +30/人已由 `objective_buff_system` + `objective_income_system` 落地；团队池记在 `ObjectiveIncomeState.team_gold`（暂无消费点）。
- §13.4 渊卫走独立链路：`abyss_warden_spawn_system`（T=54000 首刷 / 7200 tick 重生 / 加时停刷）→ `world_damage_system` 按队伍累计英雄伤害 → `AbyssWardenState::on_defeated` 判归属 → 复用 `GameplayEvents.captured` 发 120 s 增益与全局事件，另发赏金与 `WardenBuff`（归属方新生成战偶 +50% 生命 / +30% 攻击 / +0.4 移速）。

**仍未落地**：渊卫捕获进度条与沿兵线推进 AI（当前为坑位静止野怪）、§13.3 征募井战偶强化、真实 LoS 迷雾。详见 README §6.12 的偏差登记。

### 22.6 哨塔护盾（`§14` 落地）

**接口**（`gameplay::structures`）

```rust
pub const NO_CONSTRUCT_MULTIPLIER: f32 = 0.4;   // 射程内无己方战偶
pub const FRONT_TOWER_MULTIPLIER: f32 = 0.5;    // 前置塔未倒
pub const TARGET_SWITCH_ATTACKS: u8 = 2;
pub const TARGET_SWITCH_MULTIPLIER: f32 = 0.5;
pub const OUT_OF_COMBAT_TICKS: u32 = 480;       // 8 s
pub const REGEN_PER_TICK: f32 = 0.02 / 60.0;

#[derive(Component)] pub struct StructureDefense {
    front_tower_alive, friendly_constructs_in_range, out_of_combat_ticks,
    max_health, health, attacks_on_current_target, is_source_core,
}
pub fn incoming_multiplier(def: &StructureDefense) -> f32;
pub fn apply_incoming(def: &mut StructureDefense, raw: f32) -> f32;
pub fn tick_out_of_combat(def: &mut StructureDefense) -> f32;
pub enum ThreatClass { AttackingFriendlyHero = 0, Construct = 1, Hero = 2 }
pub fn choose_threat(candidates: &[(ThreatClass, f32)]) -> Option<usize>;
```

**约束条件**

1. 两条减伤规则**相乘**：外塔无战偶且前置塔存活时倍率 `0.4 × 0.5 = 0.2`；**源核不受"无战偶"规则影响**。
2. 切换目标后前 2 次攻击伤害 `× 0.5`，`notify_target_switch()` 清零计数。
3. 每次受击重置 `out_of_combat_ticks`；满 480 tick 后每秒回 `2% max_health`，上限 100%。
4. 仇恨按 `ThreatClass` 升序、同级按距离升序；`AttackingFriendlyHero` 优先于最近战偶。

**ECS 接线**：`attach_world_gameplay` 为 `Turret` 挂 `StructureDefense`（`is_source_core` 由 `TurretType` 判定）；`structure_defense_system` 从 `WorldSnapshot` 计算 `friendly_constructs_in_range`（同队战偶在 `attack_range` 内）与 `front_tower_alive`（同队存活外塔），并推进脱战回复。

**验收**：`TC-CBT-11`（0.4 / 0.5 / 0.2 倍率、源核例外）、换目标前两次减半、480 tick 后回血、仇恨优先级单测。

**未落地**：`apply_incoming` 尚未接入实际对塔伤害路径、`choose_threat` 未驱动塔攻击（§23 `GAP-14`）。

### 22.7 死亡重生（`§15` 落地）

**接口**（`gameplay::respawn`）

```rust
pub const RESPAWN_MIN_TICKS: u64 = 480;   // 8 s
pub const RESPAWN_MAX_TICKS: u64 = 2400;  // 40 s
pub const INVULNERABLE_TICKS / HASTE_TICKS: u64 = 180;
pub const HERO_KILL_BASE_GOLD: u32 = 180;
pub const STREAK_BONUS: u32 = 45;
pub const MAX_STREAK_BONUS_STEPS: u8 = 5;      // 赏金上限 405
pub const STREAK_EXPIRE_TICKS: u64 = 5400;     // 90 s
pub const ASSIST_GOLD_PERCENT: u32 = 35;

#[derive(Component)] pub struct RespawnState {
    dead, death_tick, respawn_tick, level, spawn_point: Vec2,
    invulnerable_until_tick, haste_until_tick,
}
#[derive(Component, Default)] pub struct KillStreak { count, last_kill_tick }
pub struct DeathRecord { victim_net_id, killer_net_id, tick, bounty }

pub fn respawn_ticks(level: u8) -> u64;    // clamp((8+2×(L−1))×60, 480, 2400)
pub fn mark_dead(&mut RespawnState, tick: u64);
pub fn step_respawn(&mut RespawnState, tick: u64) -> bool;   // 单次触发
pub fn consume_on_action(&mut RespawnState, tick: u64);
pub fn kill_bounty(&mut KillStreak, tick: u64) -> u32;
pub fn register_kill / reset_streak / effective_streak / assist_gold
```

**流程（跨系统，顺序由 `SimulationPlugin` 保证）**

```
damage_system   : health ≤ EPS → 写 PendingDeaths(tick, killer_net_id, victim_net_id)
                : 目标带 RespawnState ⇒ **不 despawn**（否则旧路径直接删除实体）
death_respawn   : 消费流水 → 击杀者 kill_bounty + register_kill + 加金币
                : 受击者 mark_dead + reset_streak + 清状态/护盾 + 写 DeathRecord
                : step_respawn 到点 ⇒ 回满 HP/EN、清状态、挂 180 tick 无敌、移回源核
```

**约束条件**：阵亡期间不可操作、不可被攻击、不回复（§22.1）；`step_respawn` 只触发一次；主动攻击/施法调用 `consume_on_action` 立即移除无敌与加速；金币/装备/等级保留；连杀超过 90 s 无击杀自动归零。

**验收**：重生时长钳制、状态迁移、出生保护消费、赏金上限 405 与 90 s 过期、助攻 35% 单测；`TC-GP-01`（ECS 内阵亡不 despawn → tick 481 重生 → 回满血 + 无敌）。

**未落地**：助攻实际结算、加时衰竭、源核归零判胜、投降（§23 `GAP-16`）。

### 22.8 ECS 系统链与资源清单

| 系统 | 读 | 写 | 位置 |
|---|---|---|---|
| `build_world_snapshot` | `Hero/Turret/Minion + Transform + Team` | `WorldSnapshot` | `server::gameplay` |
| `attach_hero_gameplay` | `Hero + Option<&Team>` | 7 个玩法组件 | 同上 |
| `attach_world_gameplay` | `JungleMonster/Turret` | `MonsterAi/MonsterRespawn/StructureDefense` + 2 个 `CapturePoint` | 同上 |
| `damage_system` | `AttackCommand/CombatStats/StatusEffects/NetId` | `Hero.health/ShieldPool/DamageSink/PendingDeaths` | `game-core::systems` |
| `status_tick_system` | `ServerTick` | `StatusEffects/Hero.health`（灼烧） | `server::gameplay` |
| `regen_system` | `RegenStats/RespawnState` | `Hero.health/energy` | 同上 |
| `death_respawn_system` | `PendingDeaths/ServerTick` | `RespawnState/Hero/StatusEffects/ShieldPool/KillStreak/Transform/GameplayEvents` | 同上 |
| `monster_ai_system` | `WorldSnapshot` | `MonsterAi/Transform` | 同上 |
| `capture_system` | `WorldSnapshot/ServerTick` | `CapturePoint/GameplayEvents` | 同上 |
| `structure_defense_system` | `WorldSnapshot` | `StructureDefense` | 同上 |
| `shop_purchase_system` | `ShopIntents/ShopCatalogResource/ServerTick` | `Hero/Inventory/CombatStats/GameplayEvents` | 同上 |

> `GameplayEvents` 目前只在本地事件汇累积，**未接入网络广播**（§23 `GAP-17`）；事件量受实际对局事件数约束，但长期运行需加清理或广播后 drain。

---

## 23. M2 待办差距清单（v1.1 新增）

### 23.0 范围声明与对照口径

| 口径 | 说明 |
|---|---|
| **M0+M1 原边界** | 经澄清 `Q-01` 确认，本次交付终点为 **M0 + M1**，验收闸门 `G0/G1`；`DEV-09~DEV-16`（兵线、野怪、目标、塔、经济、装备、技能组）与 `DEV-24/25` 明确列为**范围外**，PRD §11.1 与 DEV §2.2 均写明"本次不实现玩法系统"。v1.0 规格 §18 即按此边界编写。 |
| **本轮范围变更** | 按本轮新指令"**实现玩法代码本身**"，把交付范围从 `M0+M1` 扩展为 `M0+M1 + 玩法逻辑闭环`（§22）。这是对原边界的**显式扩范围**，不是补做已承诺项。 |
| **仍不等于 M2 完成** | M2 垂直切片还包含网络同步、UI、内容量、平衡验证、真机指标与 `TF-01` 容量压测；下表 `GAP-*` 逐条列出。 |
| **闸门影响** | 玩法代码通过本地自动化验收，**不改变** `G0/G1` 的外部闸门条件：律师书面意见、产品名 FTO、真实网络阈值实测仍缺；`TF-01` 仍归 `G2`。玩法验收建议作为 `G2` 的前置检查，不追溯修改 `G1` 判定。 |

### 23.1 逐条差距对照

> 图例：✅ 本轮已实现并通过自动化验收；◐ 部分实现；⛔ 未实现（M2 待办）。

| 编号 | 系统 | M0+M1 原边界 | 本轮已实现 | 仍属 M2 的差距 | 状态 | 验收 |
|---|---|---|---|---|---|---|
| `GAP-01` | 二级属性扩展 | 明确范围外，仅 `Hero` 12 字段 | `CombatStats/ShieldPool/RegenStats` + `clamp()` 上限、`apply_regen`、装备增量写回 | 与 `HeroDef` 配置表合并为单一数据源；`hp/en_regen` 未随等级成长 | ◐ | `TC-GP-01`、`TC-CFG-01` |
| `GAP-02` | 升级/属性成长 | 范围外 | 沿用既有 `level_up_system` | 按成长量补足当前生命/能量（现为回满）、等级曲线 `280×1.25^(L−1)` 未落地、`max_level = 12` 未生效（现为 20） | ⛔ | `TC-SIM-01` |
| `GAP-03` | 状态效果 | **全缺** | 8 类效果、叠加/递减/驱散/灼烧、出生无敌 | `CH_STATUS`/`CH_SHIELD` 快照位、`EntityDelta` 字段、客户端状态可视化与净化道具 | ◐ | `TC-CBT-06`、`TC-NET-03` |
| `GAP-04` | 普攻前后摇 | 范围外，仅冷却 + 锁定 | 无 | 前摇 `0.25×间隔`、后摇 `0.15×间隔`、移动取消后摇、致盲必落空、弹体速度分化 | ⛔ | `TC-CBT-05` |
| `GAP-05` | 完整伤害公式 | 仅 `100/(100+armor)` | 减免/穿透/暴击/护盾/放大/吸血/无敌/衰竭 + `deterministic_roll`，已接入 `damage_system` 双路径 | 吸血写回攻击者生命未接、主动技能未走 `compute_raw` 加成、衰竭未随加时启用 | ◐ | `TC-CBT-03/04` |
| `GAP-06` | 技能组与配置化 | 范围外，4 个硬编码技能 + 配置表双表 | 无 | 被动/终极、`scaling/damage_type/aoe/status` 字段、打断与射程夹紧、消除 `abilities` 双表 | ⛔ | `TC-CBT-05` |
| `GAP-07` | 商店事务 | **全缺**，UI 8 个占位格 | 12 件商品、DAG 校验、原子购买/升级差价/售出返还、ECS 事务系统；P-U1 补协议 opcode 与客户端商店 UI（12 件真实商品 / 分类 / 购买 / 售出回执） | UI 置灰原因、主动道具格、6 格栏位真实内容、`RejectReason` 回执展示 | ◐ | `TC-CBT-07/08`、`TC-GP-01` |
| `GAP-08` | 经济结算 | 范围外，被动金币 1.3/s | 连杀赏金（上限 405）、助攻 35% 纯函数、击杀者金币写回 | 被动收入改为 3/s、补刀 22 金币、助攻实际发放、赏金重置与 90 s 过期接入 ECS | ◐ | `TC-SIM-02` |
| `GAP-09` | 兵线战偶 | `minion_spawn_system` 空壳 | 无 | 波次（首波 `T=900`、每 1500 tick）、路点推进、属性成长、**征募井强化**（渊卫推进 buff 已由 P-U10 落地，仅作用于归属期间新生成战偶） | ⛔ | `TC-SIM-03` |
| `GAP-10` | 野怪 AI | 范围外，`spawn.rs` 硬编码 6 类数值 | `Idle/Aggro/Returning` 状态机、牵引/脱战回血/静止型、刷新计时、ECS 挂接与推进 | 攻击已接 `AttackCommand`（P-U8 补冷却 / 静止型反击）；仍缺：刷新未重新生成实体、数值未迁移到 `ConfigSet`、野怪伤害/奖励未结算 | ◐ | `TC-CBT-09` |
| `GAP-11` | 中立目标（占领） | **全缺** | 进度模型、争夺冻结、速率上限、冷却、3 字段全局事件、3 个占领点生成 | **P-U10 已落地**：渊卫定时首刷 / 伤害贡献归属 / 重生 / 赏金 / 战偶强化、兵矿占领与 30 s 周期收益、buff 发放与全局事件广播、占领点 id 按类型修正。仍缺：渊卫捕获进度条与推线 AI、征募井战偶强化、客户端占领环表现 | ◐ | `TC-CBT-10`、`objectives_fog.rs` |
| `GAP-12` | 哨塔护盾 | 范围外，仅生成实体 | 双减伤倍率、逐塔判定、换目标减半、脱战回复、仇恨优先级纯函数 | `apply_incoming` 未接入实际对塔伤害路径、`choose_threat` 未驱动塔攻击、前置塔拓扑硬编码为 `Outer` | ◐ | `TC-CBT-11` |
| `GAP-13` | 死亡重生 | **全缺**，致死即 despawn | 不 despawn、重生计时、出生保护、连杀赏金、死亡事件、ECS 全流程 | 助攻结算、阵亡期间禁止一切结算的门禁、源核归零判胜、加时衰竭、投降 | ◐ | `TC-GP-01`、`TC-SIM-05` |
| `GAP-14` | 胜负与节奏 | 范围外 | 无 | 18±4 / 25 分钟强制判定、加时衰竭、目标时长收敛、`MatchEnd` 事件 | ⛔ | `TC-SIM-05` |
| `GAP-15` | 寻路（操作层） | v1.0 已提前落地 A* 基线 | 保持 | 动态障碍、重算节流、服务端速度/碰撞校验 —— **详见 §24；P-U8 + P-U9 已全部落地** | ✅ | `TC-NAV-01~09` ✅ |
| `GAP-16` | 表现层反馈 | v1.0 已提前落地体素基线 | 保持 | 行走摆臂动画（P-V2 已落地）、状态效果体素特效（P-V3 已落地）、程序化多语言字形、机型分档 LOD；商店 UI 已由 P-U1 落地 | ⛔ | `TC-PERF-02`、`TC-CBT-12` |
| `GAP-17` | 网络同步与事件广播 | M1 已实现视野裁剪 + delta + 幂等事件 | 玩法事件写入本地 `GameplayEvents` | `GameplayEvents` 未接 `ReliableOrdered`、无 `event_id` 去重、玩法事件未做逐玩家视野裁剪、事件汇无清理 | ⛔ | `TC-NET-02/03` |
| `GAP-18` | 数值平衡与内容量 | 明确为外部依赖（`AS-04`） | 全部沿用自研占位值 | 12 位英雄、完整技能组、全部平衡数值、留存/手感指标、单局成本实测 | ⛔ | `TF-01`、`TC-PERF-01` |

### 23.2 本轮不计入"已实现"的内容

1. **没有可玩的完整对局**：兵线、目标 buff、塔攻击、胜负判定、技能加成均未接通，客户端无商店 UI 与状态显示。
2. **没有玩法网络协议**：购买/占领/死亡/状态效果均未在 `ServerMessage`/`GameEvent` 中定义消息，客户端只能看到原有 `Hero` 快照字段。
3. **没有 M2 闸门证据**：`TF-01` 容量压测、`TC-PERF-01`、真机指标、平衡验证均未执行。
4. **范围变更未回写 PRD/DEV 的交付终点**：本轮以本文件 §23.0 为范围声明的唯一依据；如需把玩法正式纳入合同范围，须同步升版 `GX-PRD-2026-001` §11.1、`GX-DEV-2026-001` §2.2 与 `G0/G1/G2` 闸门定义。

---

## 24. 寻路系统 M2 规格（v1.1 定义；P-U8 / P-U9 已落地，含 `TC-NAV-01~09`）

> **本节原为纯设计规格**（v1.1 三条待办均不实现）。现状：
> - **P-U8** 落地 `NAV-M2-02` 基础版（单帧单次 A* / 同目标去重 / 静态阻挡格缓存）
>   与 `NAV-M2-03`（服务端速度与碰撞校验）；
> - **P-U9** 落地 `NAV-M2-01` 动态障碍网格、补齐 `NAV-M2-02` 的完整重算策略，
>   并实现 `TC-NAV-01~09` 全部验收（单元 + 跨 crate 集成 + 抓包）；
> - 操作方案已由 v1.2 键盘优先改为 v1.3 纯点击控制，`NAV-M2-02-C8` 相应作废。
>
> 实现与规格的偏差统一登记在 **§24.4.1**。对应 `GAP-15`，归属 `DEV-09`（M2）。

### 24.0 历史基线（v1.0 当时的实现；现行状态见 §24.4.1）

| 项 | 现状 | 位置 |
|---|---|---|
| 网格 | 64×32 分轴网格，`MAP_WIDTH / GRID_NX = 7.5` 单位/格，覆盖 480×240 长方形；`world_to_cell` / `cell_to_world` | `client/src/pathfinding.rs` |
| 算法 | A*，八邻域、octile 启发式、禁穿角、线段可见性平滑 | 同上 |
| 障碍参数 | **v1.0 历史**：`astar(start, goal, walls: &[(i32,i32)])`，调用方传 `lane_wall_cells()` 并排除起终点格。**P-U9 现行**：`astar(start, goal, grid: &NavGrid)`，静态堤墙烘焙一次、动态障碍来自服务端 `NavObstacleSnapshot` | `client/src/pathfinding.rs` / `game-core/src/navgrid.rs` |
| 重算 | **P-U8 基础版**：单帧最多一次 A*、同目标 `< 0.5` 去重、静态阻挡格 `OnceLock` 缓存。**P-U9**：补 `grid_version` / `stuck_ticks` / `last_recompute_tick` 与 `should_recompute` 优先级链 | `client/src/input.rs::PathState` |
| 到达阈值 | **P-U8**：`ARRIVE_RADIUS = CELL_SIZE × 0.2`，并在同帧循环跳过已到达 waypoint（取代硬编码 `0.4` 与到达帧零方向） | `client/src/input.rs::path_move_dir` |
| 服务端 | **P-U8**：`navigation::validate_movement` 校验速度 / 瞬移 / 越界 / 撞墙 / 非有限值，拒绝回退 `prev`；`sanitize_move_dir` 清洗上行方向。**P-U9**：`validate_movement_with_grid` 叠加 `NavGrid` 动态障碍层 | `game-core/src/navigation.rs` |
| 职责划分 | 客户端负责路径质量，服务端负责合法性（§3.3）；合法性已落地，连续违规记入 `MovementViolations`（只审计，不踢人）。**P-U9**：双端共用 `game-core::navgrid::NavGrid`，服务端下发全局障碍视图与版本，客户端不上报网格 | 本文 §3.3 / §24.4.1 |

### 24.1 `NAV-M2-01` 动态障碍（哨塔 / 渊卫）纳入 `walls`

**目标**：把静态地形与随对局变化的建筑/大型野怪统一成有版本的导航网格，替换当前的 `&[(i32,i32)]` 参数。

**接口约定**

```rust
// client/src/pathfinding.rs（建议新增）
pub const NAV_GRID_N: i32 = 64;
pub const NAV_REBUILD_INTERVAL_TICKS: u64 = 30;   // 0.5 s

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObstacleKind {
    StaticTerrain,  // 墙体 / 河道深水 / 边界（预烘焙，不随对局变化）
    Turret,         // 存活哨塔
    SourceCore,     // 源核底座
    AbyssWarden,    // 渊卫存在期间
    // 草丛 Brush 可通行，不进入网格
}

/// 由服务端 20 Hz 增量快照投影出的障碍视图；客户端不得自行推断建筑存活
#[derive(Clone, Debug, Default)]
pub struct NavObstacleSnapshot {
    pub turrets: Vec<(Vec2, bool)>,      // (位置, 是否存活)
    pub source_cores: Vec<Vec2>,
    pub abyss_warden: Option<Vec2>,
    pub server_tick: u64,
}

#[derive(Clone, Debug, Resource)]
pub struct NavGrid {
    pub blocked: Vec<bool>,   // 长度固定 NAV_GRID_N² = 4096
    pub version: u32,         // 每次变更单调递增
}
impl NavGrid {
    pub fn new() -> Self;                                   // 仅烘焙 StaticTerrain
    pub fn mark(&mut self, kind: ObstacleKind, center: Vec2, radius: f32, alive: bool);
    pub fn rebuild(&mut self, snapshot: &NavObstacleSnapshot) -> u32;  // 返回新 version
    pub fn is_blocked(&self, cell: (i32, i32)) -> bool;
}

/// 替换现有签名；只读网格，不持有可变状态
pub fn astar(start: Vec2, goal: Vec2, grid: &NavGrid) -> Vec<Vec2>;
```

**约束条件**

| 编号 | 约束 |
|---|---|
| NAV-M2-01-C1 | `NavGrid::new()` 只烘焙静态地形；动态障碍必须来自 `NavObstacleSnapshot`，**禁止客户端从渲染实体推断** |
| NAV-M2-01-C2 | 半径 → 格数取 `ceil(radius / CELL_SIZE)`；哨塔 `r=3`、源核 `r=3`、渊卫 `r=4`（占位） |
| NAV-M2-01-C3 | 摧毁/刷新导致障碍集合变化时，`version` 必须在同一 tick 内递增；`version` 用 `u32` 单调计数，不取模 |
| NAV-M2-01-C4 | `rebuild` 周期 `30 tick`，与快照 20 Hz 对齐；同一 `server_tick` 的重复快照不得重复 `rebuild` |
| NAV-M2-01-C5 | `astar` 节点访问上限 `4096`；超预算返回**上一次有效路径**而非空路径，避免英雄急停 |
| NAV-M2-01-C6 | 禁穿角与线段可见性平滑规则保持不变；`start`/`goal` 落在障碍格时钳制到最近可通行格，钳制半径 ≤ 3 格，否则返回空路径 |
| NAV-M2-01-C7 | 死亡的野怪、已摧毁的哨塔不得再阻挡；`Brush` 可通行 |
| NAV-M2-01-C8 | 服务端侧另建 `ServerNavGrid`（同一 `blocked` 数据），供 `NAV-M2-03` 校验使用；双端网格版本由服务端下发，禁止客户端上报网格 |

**验收标准**

| 用例 | 判据 |
|---|---|
| `TC-NAV-01` | 在 `(32,32)` 置 `Turret` 障碍，A* 返回路径且 `blocked` 格命中数为 0；路径不穿角 |
| `TC-NAV-02` | 哨塔由存活改为摧毁并 `rebuild` 后，`version` 严格递增，原本绕行的路径变为直线 |
| `TC-NAV-03` | 渊卫出现/消失各一次，`blocked` 格集合与预期半径一致；同一 `server_tick` 重复快照 `version` 不变 |

### 24.2 `NAV-M2-02` 路径重算节流策略

**目标**：把"每次点击全量 A*"改成事件驱动 + 周期节流，避免每帧或高频点击导致 CPU 峰值。

**接口约定**

```rust
// client/src/input.rs（扩展现有 PathState）
#[derive(Clone, Debug)]
pub struct PathRequest { pub goal: Vec2, pub force: bool, pub version: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecomputeReason { None, NewGoal, GridChanged, Stuck, Periodic }

#[derive(Clone, Debug, Resource)]
pub struct PathState {
    pub waypoints: Vec<Vec2>,
    pub index: usize,
    pub force: bool,
    pub last_recompute_tick: u64,
    pub grid_version: u32,
    pub stuck_ticks: u32,
    pub goal: Option<Vec2>,
}

/// 纯函数：判定本 tick 是否需要重算；不执行 A*
pub fn should_recompute(
    state: &PathState,
    request: Option<&PathRequest>,
    tick: u64,
    grid_version: u32,
    player_pos: Vec2,
) -> RecomputeReason;

pub fn recompute_path(
    state: &mut PathState,
    request: &PathRequest,
    from: Vec2,
    grid: &NavGrid,
    tick: u64,
) -> bool;   // false = 预算耗尽，沿用旧路径
```

**约束条件**

| 编号 | 约束 |
|---|---|
| NAV-M2-02-C1 | 触发优先级：`NewGoal`（含 `force` 右键点地）> `GridChanged` > `Stuck` > `Periodic` > `None`；单 tick 最多执行一次 A* |
| NAV-M2-02-C2 | `Periodic` 间隔 `30 tick`；`GridChanged` 判定依据 `grid_version != state.grid_version`，**立即**重算 |
| NAV-M2-02-C3 | `Stuck`：连续 `60 tick` 位移 `< 0.1` 触发一次；重算后仍失败则清空路径、`move_dir = 0` 并保持 30 tick 不再触发 |
| NAV-M2-02-C4 | `force = true` 的右键点地必须**当帧**重算，且清除锁定目标 / 施法前摇 / 普攻后摇（§3.1 语义不得被节流削弱） |
| NAV-M2-02-C5 | A* 预算 ≤ 4096 节点 / ≤ 2 ms（桌面基准）；超预算返回 `false` 并沿用旧路径，禁止阻塞主线程 |
| NAV-M2-02-C6 | 目标点变化阈值 `0.5` 单位；小于阈值视为同一目标，不产生 `NewGoal` |
| NAV-M2-02-C7 | 重算不得重置已走过的路径点索引到错误位置；`index` 按新路径最近点重新定位 |
| NAV-M2-02-C8 | **已作废（v1.3）**：键盘不再产生移动，改为纯点击控制；节流只需保证右键 `force` 当帧重算（§3.1 / `TC-CBT-01`） |

**验收标准**

| 用例 | 判据 |
|---|---|
| `TC-NAV-04` | 静止 60 tick 内 `should_recompute` 仅在 grid/目标变化时返回非 `None`；周期触发间隔恰为 30 tick |
| `TC-NAV-05` | 同一 tick 连续注入 5 个 `PathRequest`，A* 实际调用次数为 1；`force` 请求当帧生效 |
| `TC-NAV-06` | 模拟卡住 60 tick → 返回 `Stuck`；重算仍失败后 `move_dir == 0` 且 30 tick 内不再触发 |

### 24.3 `NAV-M2-03` 服务端速度与碰撞校验

**目标**：把 §3.3 的"服务端负责合法性"落到代码，防止加速、穿墙、瞬移类输入。

**接口约定**

```rust
// game-core/src/navigation.rs（建议新增，服务端权威；客户端只读回执）
pub const SPEED_TOLERANCE: f32 = 1.05;
pub const COLLISION_SAMPLE_STEP: f32 = 1.0;   // CELL_SIZE / 2
pub const MAP_LIMIT: f32 = 63.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason { SpeedExceeded, WallCollision, OutOfBounds, Teleport }

#[derive(Clone, Copy, Debug)]
pub struct MovementValidation {
    pub accepted: bool,
    pub corrected_position: Vec2,
    pub reject_reason: Option<RejectReason>,
}

#[derive(Clone, Debug, Resource)]
pub struct ServerNavGrid { pub blocked: Vec<bool>, pub version: u32 }

/// 纯函数：在服务端 FixedUpdate 中、位置积分**之前**调用
pub fn validate_movement(
    prev: Vec2,
    proposed: Vec2,
    move_speed: f32,
    dt: f32,
    grid: &ServerNavGrid,
) -> MovementValidation;

/// 连续违规计数与处置策略（审计，不改变裁决结果）
#[derive(Clone, Debug, Resource, Default)]
pub struct MovementViolations { pub per_client: HashMap<u64, (u8, u64)> } // (连续次数, 最后 tick)
```

**约束条件**

| 编号 | 约束 |
|---|---|
| NAV-M2-03-C1 | 允许位移 ≤ `move_speed × dt × 1.05 + EPS`，`dt = FIXED_TIMESTEP = 1/60`；`move_speed` 取服务端 `Hero.move_speed`，**不信任客户端上报值** |
| NAV-M2-03-C2 | 碰撞用线段采样，步长 ≤ `CELL_SIZE/2 = 1.0` 单位；任一样本落在 `blocked` 格 → `WallCollision` |
| NAV-M2-03-C3 | 越界（任意 `|x|` 或 `|z|` > 63）→ `OutOfBounds`；`corrected_position` 钳制到边界内 |
| NAV-M2-03-C4 | 单次位移 > `move_speed × dt × 3` 视为 `Teleport`（兼容重连后的基线补发，重连 tick 单独豁免） |
| NAV-M2-03-C5 | 拒绝时**位置回退到 `prev`**（或钳制后的安全点），不做静默瞬移；`corrected_position` 随下一次快照下发，客户端据此回滚预测 |
| NAV-M2-03-C6 | 连续 3 次拒绝写 `warn` 日志并计入 `MovementViolations`；是否断开由 M3 反作弊策略决定，本阶段只记录 |
| NAV-M2-03-C7 | 服务端**不重算路径**，只做位置合法性裁决；`ServerNavGrid` 版本由服务端维护，与客户端 `NavGrid` 同源但客户端不能上报 |
| NAV-M2-03-C8 | 校验必须发生在 `apply_pending_inputs` 之后、`movement_system` 之前，且不得改变 `InputCmd.seq / checksum / config_hash` 的既有校验链（`FR-61`） |
| NAV-M2-03-C9 | `corrected_position` 下发仍受逐玩家视野裁剪约束（§8.5 / `TC-NET-03`），不得泄露视野外实体 |

**验收标准**

| 用例 | 判据 |
|---|---|
| `TC-NAV-07` | 正常速度输入 `accepted = true`；`1.06×` 超限时 `reject_reason = SpeedExceeded` 且 `corrected_position == prev` |
| `TC-NAV-08` | 穿越 `Turret` 障碍格的线段被拒；沿障碍边缘擦过的合法路径不被误判 |
| `TC-NAV-09` | 抓包断言：拒绝帧的快照中 `corrected_position` 存在；视野外实体的位置**零下发**；连续 3 次违规后日志计数为 3 |

### 24.4 依赖、顺序与不做项

| 待办 | 依赖 | 建议顺序 | 关联用例 |
|---|---|---|---|
| `NAV-M2-03` 服务端校验 | `ServerNavGrid`（与 `NAV-M2-01` 同源数据）、`InputCmd` 既有校验 | **1**（安全优先，先堵住加速/穿墙） | `TC-NAV-07/08/09` |
| `NAV-M2-01` 动态障碍 | 规格建议 `CH_ALIVE` 语义；**实际改为独立全局报文**（见下） | **2** | `TC-NAV-01/02/03` ✅ |
| `NAV-M2-02` 重算节流 | `NAV-M2-01` 的 `version`、`PathState` 扩展 | **3**（依赖网格版本） | `TC-NAV-04/05/06` ✅ |

#### 24.4.1 P-U8 / P-U9 落地现状（对上表与 §24.0 的修订）

| 待办 | 状态 | 实际实现与差异 |
|---|---|---|
| `NAV-M2-01` | **已落地（P-U9）** | `game-core/src/navgrid.rs` 定义双端同源 `NavGrid`（网格几何 / 静态烘焙 / `NavObstacleSnapshot` / `version`）。规格要求「服务端另建 `ServerNavGrid`（同一 blocked 数据）」，实现把**类型与常量下沉到 `game-core`**，两端共用同一个 `NavGrid`，同源由类型系统保证。动态障碍来自 `simulation.rs::rebuild_server_nav_grid` 每 tick 从权威 ECS 生成的全局视图：哨塔死亡后仍以 `health=0` 常驻下发，故必须带存活位；渊卫死亡即 despawn，存活时才有坐标。`ServerMessage::NavGridUpdate` 走 ReliableOrdered 向**所有**客户端广播，**不经视野裁剪**（C1 要求禁止从渲染实体推断，而渲染实体是裁剪过的），变化立即发、无变化每 30 tick 补发。新变体追加在枚举末尾，不平移既有 bitcode 判别值。客户端 `ClientNavObstacles` → `sync_nav_grid` → `PathState.grid`，版本由本地 `rebuild` 裁决、不采信报文字段（C8） |
| `NAV-M2-02` | **已落地（P-U8 基础版 + P-U9 完整版）** | `should_recompute` 纯函数按 `NewGoal（含 force）> GridChanged > Stuck > Periodic > None` 裁决（C1）；`PathState::process_requests` 单 tick 最多一次 A*（C5），`GridChanged` 依 `grid.version != state.grid_version` 立即重算（C2），连续 60 tick 位移 < 0.1 触发 `Stuck`、失败后清路径 / `move_dir=0` / 静默 30 tick（C3），`index` 按新路径最近点重定位（C7）。`ClientTick` 提供 60Hz 基准，不用渲染帧或 20Hz 快照 tick。A* 另有 4096 节点预算与 ≤3 格起终点钳制（§24.1 C5/C6） |
| `NAV-M2-03` | **已落地（P-U8 + P-U9 动态层）** | `sanitize_move_dir` 在 `apply_pending_inputs` 清洗上行方向（挡 NaN / Inf / 超长向量）；`validate_movement` 在 `movement_system` 位置积分**之后**裁决速度 / 瞬移 / 越界 / 撞墙 / 非有限值，拒绝时回退 `prev`。**C8 原文「校验在 `movement_system` 之前」不成立**：须先得到 `proposed` 才能判断位移是否合法；`InputCmd.seq / checksum / config_hash` 校验链不变。P-U9 的 `validate_movement_with_grid` 叠加 `NavGrid` 的**动态障碍层**；静态墙仍用缓存的 `map::wall_colliders()` 圆形碰撞体 —— 若整张 `blocked`（含 7.5 单位粗格的静态层）都参与采样，会把坡道通行带凭空缩窄两格，属误判。C7「服务端不重算路径」成立 |

**规格偏差登记**：
1. `NAV_GRID_N = 64` 写于正方形地图时期；当前 2:1 长方形（480×240）按 `64×32 / CELL_SIZE = 7.5` 派生。`TC-NAV-01` 的格 `(32,32)` 在现行网格里越界，验收改用中心格 `(32,16)`。
2. 自愈收窄：起点在墙圆内整段放行；起点**仅**在动态障碍格内时只跳过格判定，静态墙判定仍生效 —— 否则「英雄出生在源核格」会变成穿墙许可证。
3. 规格建议借 `CH_ALIVE` 在 20Hz 增量快照里带存活位；实现改用独立全局报文，避免与逐玩家视野裁剪纠缠，也不改 `EntityDelta` / `SnapshotHeader` 字段布局。
4. `NAV-M2-02-C8` 已作废（v1.3 纯点击控制）。

**`TC-NAV-01~09` 验收分布**：

| 用例 | 位置 |
|---|---|
| 01 / 02 / 03 | `client/src/pathfinding.rs::tests::tc_nav_01..03_*` |
| 04 / 05 / 06 | `client/src/input.rs::tests::tc_nav_04..06_*` |
| 07 / 08 | `game-core/src/navigation.rs::tests::tc_nav_07/08_*` |
| 09 | `game-core/src/navigation.rs::tests::tc_nav_09_*`（违规计数）+ `gx-e2e/tests/nav_acceptance.rs`（抓包 `corrected_position` / 视野零下发） |
| 跨 crate 接线 | `gx-e2e/tests/nav_acceptance.rs`：服务端网格 → bitcode 报文 → 客户端网格 → A*；`GridChanged` 触发重算 |

> 护栏单测：`navgrid::tests::*`（网格覆盖 / 半径换算 / 版本递增 / 同 tick 幂等 / 渊卫出现消失 / 钳制）、`navigation::tests::*`（含 `tc_nav_07/08/09`）、`pathfinding::tests::tc_nav_01..03_*`、`input::tests::tc_nav_04..06_*`、`gx-e2e/tests/nav_acceptance.rs`。

**明确不做**（避免范围蔓延）：服务端重算路径 / 服务端寻路；客户端预测路径的网络同步；跨格子连续碰撞体物理；动态障碍的移动动画；把 `walls` 暴露给 UI 编辑器。

**追溯**：`NAV-M2-01~03` → `FR-40/41/61`、`CR-05`、`DEV-09` → `TC-NAV-01~09` → `G2`。截至 P-U9，三项待办与 `TC-NAV-01~09` 全部落地并通过验收；剩余 M2 工作主要是地图骨架复刻与其它 §23 差距项。

