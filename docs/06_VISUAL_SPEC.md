# GX-01 视觉开发规范（P-V 系列）

| 项 | 内容 |
|---|---|
| 文档编号 | `GX-VIS-2026-001` |
| 版本 | v1.0（P-V0 口径收口） |
| 关联 | `PRES-1~9` / `GX-CBT-2026-001` §16 / `ASSET_REGISTRY.md` / `README.md` §8 |
| 适用范围 | `client` crate 全部表现层资产：几何、纹理、材质、特效、动画、HUD 标记 |
| 硬约束 | **零外部资产**，全部代码生成；`docs/**` 与 `LEGAL/**` 为术语白名单，其余发布树受合规门禁扫描 |

> 本文是 P-V0 的交付物，也是 P-V1 / P-V2 / P-V3 的**唯一口径来源**。
> 后续阶段若发现与本规范冲突，必须先修订本规范并同步 `README.md` / §16 / 资产登记表，再继续实现。

## 1. 背景：需要收口的口径冲突

| 编号 | 旧表述 | 实际情况 | 本次处置 |
|---|---|---|---|
| C-01 | 英雄为「头/躯干/双臂/双腿共 6 个方块」 | 12 个固定方块 + 可变头饰/武器/披风/肩甲，约 13~19 块 + 2 肩枢轴 | 统一为「分层方块人，≥6 块」 |
| C-02 | 地形为「2×2 网格 / 水块 / 河道」 | 12×12 平板 + 路点条带，`PixelKind::Water` 定义未启用，无河道 | 按代码回写，河道列为 P-V4 |
| C-03 | 8-bit 音频「已提供」/「本期不提供」三方互斥 | 客户端零音频实现 | 统一为「规划中，P-V1 落地」 |
| C-04 | PRES-3/9「禁 OS 字体 / 禁 `FontData::from_owned`」 | `ui.rs` 运行时借用 OS 字形，合规门禁反而要求 `setup_cjk_font` 存在 | §16 v1.3 已回写 |
| C-05 | §16.2 的 `spawn_voxel_structure` / `spawn_voxel_effect` | 两个函数从未实现 | 契约表已换成真实函数 |
| C-06 | 「`AbilityCast` 不携带 `ability_index`，需 P-E 补协议字段」 | `GameEvent::AbilityCast` **已携带 `ability_index: u8`**，客户端以 `..` 忽略 | **P-V0 对齐**：P-V3 直接读取，无需协议升版 |
| C-07 | PRES-4「所有可见体均为方块」 | 弹体 `Sphere`、血条 `Rectangle`、小地图 `circle_*` | 3D 体素化 + 2D HUD 例外明示 |

## 2. 视觉方向

### 2.1 风格定义（统一口径）

| 维度 | 规范 |
|---|---|
| 几何 | 所有 **3D 可见体**由轴对齐 `Cuboid` 拼装；弧线 / 圆环 / 斜面用**阶梯体素**近似；禁止 `Sphere` / 胶囊 / 曲面网格 |
| 纹理 | 16×16 RGBA 程序化像素图，`Nearest` 采样，有限调色板，禁止平滑 / 抗锯齿 / 渐变滤镜 |
| 光照 | 不依赖法线贴图；用**按面朝向选择材质变体**实现伪 AO 明暗；特效统一 `unlit` + `emissive` |
| 剪影 | 低分辨率与远距离下，角色靠「体型 + 头部特征 + 武器轮廓」辨识，不依赖贴图细节与血条 |
| 色彩 | 队伍色只占小面积（脚环 / 肩章 / 披风内衬 / 刃光），角色主色由造型数据决定 |
| 2D 例外 | HUD 血条、小地图允许矩形与 2D 绘制，但须方块化、无圆角、无渐变 |
| 隔离 | 动画 / 特效只改客户端表现节点的 `Transform`，**绝不回写服务端 `TransformSync` 与裁决组件** |

### 2.2 明确禁止

- 任何外部资源文件（几何 / 贴图 / 字体 / 音频二进制）与字节内嵌宏；
- 平滑采样、抗锯齿、渐变、圆角、法线 / 高光贴图；
- 表现层直接写 `Hero` 数值、`MatchClock`、输入裁决结果；
- 每次 spawn 都新建同参数的纹理 / 材质（必须走 P-V1 材质池）。

## 3. 色彩规范

### 3.1 基础调色板（与 `voxel::PixelKind` 同源，12 色）

| `PixelKind` | 底色 RGB | 点缀 RGB | 语义用途 |
|---|---|---|---|
| `Grass` | 86,156,70 | 104,174,82 | 地面植被 |
| `Dirt` | 134,96,67 | 155,116,82 | 土层 / 野区台地 |
| `Stone` | 128,128,132 | 155,155,158 | 边界墙 / 石径 / 假人 |
| `Water` | 52,104,190 | 72,134,214 | 河道（P-V4 启用） |
| `Lane` | 168,148,112 | 190,170,132 | 兵线路面 |
| `Brush` | 46,108,52 | 62,132,66 | 草丛 / 树冠 |
| `Blue` | 46,96,214 | 78,132,240 | 蓝方语义色 |
| `Red` | 200,56,52 | 232,84,76 | 红方语义色 |
| `Gold` | 226,184,62 | 246,210,92 | 金币 / 塔顶 / 中立 |
| `Core` | 148,96,214 | 182,130,236 | 源核 / 中立核心 |
| `Marker` | 240,240,240 | 255,255,255 | 选中 / 瞄准标记 |
| `Effect` | 250,148,52 | 255,194,92 | 通用特效 |

### 3.2 语义色（不新增调色板项，由 `Color` 常量派生）

| 类别 | 颜色 | 用途 |
|---|---|---|
| 队伍-蓝 | `srgb(0.55,0.85,1.0)` | 蓝方命中 / 弹道 / 占领环 |
| 队伍-红 | `srgb(1.0,0.55,0.4)` | 红方命中 / 弹道 / 占领环 |
| 中立 | `srgb(1.0,0.82,0.35)` | 视野外攻击者 / 野怪反馈 |
| 元素-火 | 橙红系（`Effect` 派生） | 火焰弹及余烬 |
| 元素-冰 | 蓝白系（`Blue`/`Marker` 派生） | 寒冰新星 / 霜纹 |
| 元素-能量 | 青系（`Blue` 派生） | 能量光束 |
| 元素-治疗 | 绿白系（`Brush`/`Marker` 派生） | 自我治疗 / 恢复 |
| 暴击 | `srgb(1.0,0.55,0.2)` | 暴击额外刀光 |

### 3.3 明暗规范（伪 AO）

| `FaceShade` | 亮度系数 | 适用 |
|---|---|---|
| `Top` | 1.15 | 朝上的地面 / 台面 / 头顶 |
| `Side` | 1.00 | 默认立面 |
| `Bottom` | 0.78 | 底面 / 背光层 |

实现方式为**按物体朝向选择材质变体**（同一 `PixelKind` 的三种亮度 `Handle`），不修改网格 UV。

### 3.4 使用规则

1. 单材质不超过 16 色；每个 `PixelKind` 只有底色与点缀色。
2. 队伍色在单个角色模型上的可见面积占比 ≤ 20%。
3. 特效材质统一 `unlit`，不参与全局光；生命周期 ≤ 1 s。
4. 所有随机性必须来自确定性 `hash(x, y, seed)`，禁止运行时随机源，保证回放一致。

## 4. 术语口径

| 术语 | 定义 | 易混淆点 |
|---|---|---|
| 体素 / `Cuboid` | 轴对齐立方体网格，本项目最小 3D 可见单元 | 不是体素引擎，不使用 chunk 数据结构（P-V4 再引入） |
| 资源 | **代码生成后持有的 `Handle<Image>` / `Handle<StandardMaterial>` / `Handle<Mesh>`** | 不是磁盘文件，仓库内不存在资源目录 |
| 材质池 | `VoxelMaterialPool`：按 `MaterialKey` 缓存并复用上述 Handle 的 ECS 资源 | 与 `Assets<T>` 的区别：Assets 是存储，池是去重索引 |
| `MaterialKey` | `(PixelKind, seed, TexturePattern, FaceShade, glow)` | 五元组全部参与哈希，任一不同即不同材质 |
| `TexturePattern` | 纹理图案枚举：`Flat` / `Speckle` / `Border` / `Stripe` / `Grid` | 决定像素级排布，不改变底色 |
| `seed` | 确定性哈希种子，相同 `(kind, seed, pattern)` 必得相同纹理 | 不是随机数，不可来自系统时间 |
| `HeroRig` | 英雄分层骨架的实体句柄集合 | 是表现层组件，与服务端骨骼无关 |
| `HeroVisual` | 英雄造型数据表条目（配色 / 体型 / 头饰 / 武器 / 披风 / 肩甲） | 与数值 `Hero` 组件无关，只驱动外观 |
| `AbilityVisual` | 技能视觉描述（元素 / 色 / 前摇 / 主体 / 命中 / 余烬 / 时长） | 由 `ability_index` 索引，不参与伤害计算 |
| 剪影原型 | 6 种角色轮廓分类（重甲 / 剑士 / 游侠 / 法师 / 治疗 / 攻城） | 是造型语言，不是职业数值 |
| 特效通道 | `CombatFxQueue` 中区分 `BasicAttack` / `Ability` / `Objective` 的逻辑分流 | 通道分离避免普攻与技能特效互相覆盖 |
| 生命周期 | 临时特效组件从生成到 `despawn` 的时长 | 硬上限 1 s，受同屏数量预算约束 |
| `skin` | 服务端下发的外观编号，客户端用作 `HeroVisual` 索引 | 与数值、等级、装备无关 |

## 5. 命名规范

| 对象 | 规则 | 示例 |
|---|---|---|
| 资源调试名 | `VIS-{域}-{类别}-{语义}-{seed}` | `VIS-HERO-MAT-HEAD-13` |
| `MaterialKey` | 五元组结构化，禁止拼接字符串做键 | `MaterialKey { kind, seed, pattern, face, glow }` |
| 造型数据 | `HERO_VISUALS[skin % 6]`，条目名用剪影原型 | `HeroVisual::Ranger` |
| 技能视觉 | `ABILITY_VISUALS[ability_index]`，条目名用元素语义 | `AbilityVisual::Frost` |
| 组件 | 表现层语义 + 用途，禁止 `Mgr` / `Data` 后缀 | `HeroAnim`、`CastWindup`、`ObjectiveRing` |
| 系统 | `tick_*`（逐帧）/ `spawn_*`（生成）/ `update_*`（同步） | `tick_hero_anim`、`spawn_ability_fx` |
| 事件视觉 | 与 `GameEvent` 变体同名 + `Fx` 后缀 | `HeroDeathFx`、`OvertimeFx` |

## 6. 四阶段交付标准与验收条件

| 阶段 | 必须交付 | 验收条件 | 前置 |
|---|---|---|---|
| **P-V0** | 本规范；口径冲突修正清单 | C-01~C-07 全部标注处置；合规门禁全绿；文档无互斥表述 | — |
| **P-V1** | `VoxelMaterialPool` + 增强纹理工厂；3D 弹体方块化 | 同 `MaterialKey` 复用同一 Handle；纹理确定性；地形不再循环内新建材质；`TC-PRES-01/02` 通过 | P-V0 |
| **P-V2** | `HeroVisual` 数据表；分层骨架；动画状态机；朝向修正 | 6 原型配色唯一；待机/移动/攻击/施法/受击/死亡/重生 7 态可触发；动画不改服务端 tick | P-V1 |
| **P-V3** | `AbilityVisual` 表；四技能差异化；五阶段施法序列；关键事件视觉 | 四技能视觉互异且读 `ability_index`；事件覆盖登记齐全；同屏特效数与材质复用达标 | P-V1（可与 V2 并行） |

### 6.1 通用 DoD（每阶段均适用）

1. `cargo fmt --all -- --check` 通过；
2. `cargo clippy --workspace --all-targets -- -D warnings` 通过；
3. `cargo test --workspace` 通过，且本阶段新增用例全绿；
4. `cargo test -p gx-e2e --test compliance` 通过（禁用词 / 零外部资产 / 字体回退 / 体素数门禁）；
5. 本阶段新增资源**全部代码生成**，`ASSET_REGISTRY.md` 状态同步；
6. 本文对应阶段章节补齐「资源清单 / 设计说明 / 复用规则 / 阶段验收结论」四项；
7. 若发现与前一阶段口径冲突，**先修订规范与相关文档再进入下一阶段**。

## 7. 性能约束

| 项 | 预算 | 说明 |
|---|---|---|
| 纹理 | 固定 16×16 RGBA，单张 1 KiB | 由池复用，总量 = 去重后 `MaterialKey` 数 |
| 材质 | 全局去重后 ≤ 128 个 `Handle` | 超出需审查 `seed` 是否滥用 |
| 网格 | 共享单位 `Cuboid` 为主，特殊尺寸按需 | 同尺寸必须复用同一 `Handle<Mesh>` |
| 同屏临时特效 | ≤ 96 个实体 | 超限时按最旧优先丢弃 |
| 特效生命周期 | ≤ 1 s（`PRES-8`） | 事件类庆祝特效可到 1.5 s，须登记 |
| 每事件特效实体 | ≤ 12 | 碎片数量按此上限裁剪 |
| 动画 | 每英雄每帧 ≤ 10 次 `Transform` 写入 | 只驱动 `HeroRig` 节点 |
| headless / 无声 | 表现层正常构建但不播音频、不依赖窗口 | 测试环境静默降级 |

## 8. P-V0 阶段交付

### 8.1 资源清单

本阶段**不新增运行时资源**，产出为规范与口径修正：

| 编号 | 交付物 | 类型 |
|---|---|---|
| `V0-01` | `docs/06_VISUAL_SPEC.md`（本文） | 规范文档 |
| `V0-02` | C-01~C-07 口径冲突处置表（§1） | 规范文档 |
| `V0-03` | `README.md` 中 C-06 相关表述修正（§8.3.3 / §8.7 / §9 / §12） | 文档同步 |
| `V0-04` | `crates/client/src/networking.rs` 英雄模型注释修正 | 注释同步 |
| `V0-05` | `docs/05_COMBAT_SPEC.md` §16 v1.3（上一轮已回写，本阶段复核） | 规格复核 |

### 8.2 设计说明

P-V0 不改动运行时行为，只把「视觉方向、美术风格、色彩规范、术语口径」固化为唯一来源，
使后续三个阶段在实现时不再依赖 `README.md` 的概述性描述或代码注释里的零散约定。
关键是明确两条边界：**3D 必须体素化、2D HUD 允许方块化例外**；**表现层绝不回写裁决组件**。
色彩规范直接复用代码里已存在的 12 色 `PixelKind`，避免规范与实现各有一套调色板。

### 8.3 复用规则

1. 后续阶段所有颜色必须来自 §3.1 或 §3.2 的派生，禁止在特效代码里散落新的 `Color::srgb` 常量；
2. 所有纹理 / 材质必须经 P-V1 的 `VoxelMaterialPool`，`MaterialKey` 命名遵循 §5；
3. 所有随机性使用确定性 `hash(x, y, seed)`，保证回放一致；
4. 事件视觉组件按 `GameEvent` 变体命名，统一在 `handle_game_event` 一处分发；
5. 任何新术语先登记 §4，任何新资源先登记 `ASSET_REGISTRY.md`。

### 8.4 阶段验收结论

| 检查项 | 结论 |
|---|---|
| C-01~C-05、C-07 | 上一轮文档整理已回写，本阶段复核无回退 |
| C-06（`AbilityCast.ability_index`） | **已对齐**：协议字段存在，修正 `README.md` 四处误述；P-V3 无需升版 |
| 术语与色彩唯一来源 | 已建立（§3 / §4） |
| 四阶段交付标准与验收条件 | 已建立（§6 / §6.1 / §7） |
| 合规门禁 | `cargo test -p gx-e2e --test compliance` 通过 |
| 是否可进入 P-V1 | **是**（前提：P-V1 实现不得引入 §2.2 禁止项） |

---

## 9. P-V1 阶段交付：纹理与材质池

### 9.1 资源清单

| 编号 | 交付物 | 类型 | 位置 |
|---|---|---|---|
| V1-01 | `TexturePattern`（`Flat` / `Speckle` / `Border` / `Stripe` / `Grid`） | 资源类型 | `crates/client/src/visual.rs` |
| V1-02 | `FaceShade`（`Top` 1.15 / `Side` 1.0 / `Bottom` 0.78） | 资源类型 | 同上 |
| V1-03 | `MaterialKey` 五元组 + `debug_name()` 命名 | 资源键 | 同上 |
| V1-04 | `pattern_texture_data()` 纯函数纹理工厂 | 生成函数 | 同上 |
| V1-05 | `make_pattern_texture()` → `voxel::make_pixel_texture()` | 生成函数 | `visual.rs` / `voxel.rs` |
| V1-06 | `voxel_material_keyed()` + `voxel_mat()` 语义快捷入口 | 生成函数 | `voxel.rs` |
| V1-07 | `VoxelMaterialPool`（纹理 / 材质 / 特效材质 / 共享网格 四类缓存） | ECS 资源 | `visual.rs`，注册于 `RenderingPlugin` |
| V1-08 | `fx_material()` 特效材质入口、`PoolStats` 规模快照 | 复用接口 | `visual.rs` |
| V1-09 | 3D 弹体 `Sphere → Cuboid`（PRES-4 违规清零） | 合规修正 | `interpolation.rs::sync_projectiles` |
| V1-10 | 场景 / 道具 / 角色三类的图案与明暗分配 | 风格落地 | `voxel.rs` 各生成函数 |
| V1-11 | 6 个新增单测 | 验收用例 | `visual.rs::tests` |
| V1-12 | 根 / `docs` 两份资产登记表 A-002 / A-005 状态同步 | 文档 | `ASSET_REGISTRY.md`、`docs/ASSET_REGISTRY.md` |

**资源覆盖矩阵**

| 类别 | 对象 | 图案 | 明暗 |
|---|---|---|---|
| 场景 | 草地 / 草丛 | `Speckle` | `Top` |
| 场景 | 土层 | `Flat` | `Bottom` |
| 场景 | 石墙 | `Stripe` | `Side` |
| 场景 | 兵线路面 | `Stripe` | `Top` |
| 场景 | 源核 | `Grid`（发光） | `Top` |
| 道具 | 选中 / 瞄准标记 | `Border`（发光） | `Top` |
| 道具 | 弹体 | `Border`（发光，池化） | `Top` |
| 角色 | 躯干 / 四肢 | `Speckle` | `Side` |
| 角色 | 头部 | `Border` | `Top` |
| 角色 | 队伍点缀 / 脚环 | `Grid`（发光） | `Top` |

### 9.2 设计说明

1. **纹理升级**：旧实现是 4×4 色块（每 4 像素才变一次色），现改为 16×16 **逐像素**确定性噪点，
   叠加 5 种图案与 1px 边缘压暗；所有随机性来自纯函数 `hash(x, y, seed)`，回放一致。
2. **伪 AO**：不引入法线贴图或多材质网格，而是按物体朝向选择 `FaceShade` 材质变体
   （顶面提亮、底面压暗），以最小改动获得体素层次感，符合 §2.1「轴对齐 / 无平滑」。
3. **池化**：`VoxelMaterialPool` 用 `HashMap<MaterialKey, Handle<..>>` 去重，
   同键必返回同一 Handle；共享单位 `Cuboid` 网格由 `cube()` 提供。
   弹体生成（逐帧按快照创建）已改为走池，消除反复 `Assets::add`。
4. **兼容升级**：`make_pixel_texture` / `voxel_material` 保留原签名并委托新工厂，
   使未显式指定图案的调用点（小兵、建筑堆叠、动态装饰）自动获得统一风格，
   无需逐点改签名，降低回归风险。
5. **合规**：`Sphere` 弹体替换为方块，关闭 §1 C-07 的 3D 违规项；2D HUD 例外保持不变。

### 9.3 复用规则

1. 新纹理一律经 `pattern_texture_data`，禁止在生成函数里自行计算像素；
2. 新材质一律经 `voxel_mat`（静态对象）或 `VoxelMaterialPool::material`（运行期反复创建）；
3. 逐帧 / 事件生成的材质一律经 `VoxelMaterialPool::fx_material`（P-V3 直接复用）；
4. 1×1×1 体素一律经 `VoxelMaterialPool::cube`，禁止 `meshes.add(Cuboid::new(1,1,1))` 散落；
5. 资源键只用 `MaterialKey` 五元组，**禁止字符串拼接键**，调试名走 `debug_name()`；
6. `seed` 按域分段避免碰撞：角色 `11~19`、标记 `21`、建筑 `31~32`、地形 `41~48`、
   动态装饰 `51~52`、弹体 `61`、小兵 `71~73`；新增域须在本文登记；
7. 新增 `PixelKind` 必须同步 §3.1 调色板表与 `PALETTE` 常量，保持「规范即实现」。

### 9.4 阶段验收结论

| 检查项 | 结论 | 证据 |
|---|---|---|
| §6 交付项 | 全部落地 | V1-01~V1-12 |
| 纹理确定性 / 图案差异 / 明暗顺序 / key 唯一性 / 命名规范 | 通过 | `visual::tests` 6/6 |
| 同 `MaterialKey` 复用同一 Handle | 通过 | `VoxelMaterialPool::material` / `fx_material` 短路返回 |
| 场景 / 道具 / 角色三类覆盖 | 通过 | §9.1 覆盖矩阵 |
| 3D 体素合规（PRES-4） | 通过 | 全仓仅剩 HUD 2D 绘制，`Sphere` 已清除 |
| 零外部资产 | 通过 | 合规门禁 4/4；无新增资源文件 |
| 工作区编译 / 客户端单测 | 通过 | `cargo check --workspace --all-targets`；`cargo test -p client --lib` 13/13 |
| 与 P-V0 口径冲突 | **无**；§1 C-07 的 3D 部分已按规范关闭 | — |
| 是否可进入 P-V2 | **是** | P-V2 必须复用 `VoxelMaterialPool`，不得新建平行资源库 |

---

## 10. P-V2 阶段交付：英雄造型与动画

### 10.0 与前阶段口径对齐（先对齐再继续）

| 编号 | 冲突 | 处置 |
|---|---|---|
| A-01 | `TC-CBT-12` 用字面 `b.spawn(` 计数校验 PRES-6「≥6 体素」。P-V2 把双腿改为「髋部枢轴 + 腿方块」、眼睛改为 head 子节点后，rustfmt 将链式调用拆成 `b\n    .spawn(`，**实际 9 个体素被误判为 3 个**，门禁失败 | 门禁改为统计 `.spawn(` 调用（同时覆盖两种写法），判据语义「≥6 个体素」**未放宽**，只修判据脆弱性；已在 `compliance.rs` 注明原因与阶段 |
| A-02 | 复核 §1 C-06：`AbilityCast` 是否携带足够信息触发施法动画 | 已确认协议含 `caster: NetId` 与 `ability_index: u8`，`CastAnim` 直接复用，**无协议改动**；`ability_index` 预留给 P-V3 |

### 10.1 资源清单

| 编号 | 交付物 | 类型 | 位置 |
|---|---|---|---|
| V2-01 | `Silhouette` 6 剪影原型枚举 | 资源类型 | `crates/client/src/visual.rs` |
| V2-02 | `HeroVisual` 造型表 + `HERO_VISUALS[6]` + `TRAINING_DUMMY_VISUAL` | 数据表 | 同上 |
| V2-03 | `hero_visual(skin)` 唯一索引入口 | 查询接口 | 同上 |
| V2-04 | `HeroRigBase` 静止姿态（7 节点） | 资源类型 | `crates/client/src/voxel.rs` |
| V2-05 | `HeroRig` 扩展为 7 节点 + `cape` 可选节点 + `base` | 骨架组件 | 同上 |
| V2-06 | 双腿髋部枢轴化、眼睛挂 head 子节点 | 几何 / 朝向修正 | `spawn_voxel_hero` |
| V2-07 | `HeroAnimState` 7 态 + `HeroAnim` 组件 | 状态机 | `crates/client/src/hero_anim.rs` |
| V2-08 | `CastAnim` / `HitAnim` / `DeathAnim` 触发标记 | 事件组件 | 同上 |
| V2-09 | `update_hero_motion` + `tick_hero_anim` + `apply_pose` | 系统 | 同上；注册于 `RenderingPlugin` 的顺序链 |
| V2-10 | 事件接线：`AbilityCast` / `DamageDealt` / `HeroDeath` | 触发接线 | `crates/client/src/networking.rs` |
| V2-11 | 7 个新增单测（含运行时无查询冲突回归） | 验收用例 | `visual.rs::tests`、`hero_anim.rs::tests` |
| V2-12 | `TC-CBT-12` 判据健壮性修正 | 门禁 | `crates/gx-e2e/tests/compliance.rs` |

**造型原型矩阵（代码即规范）**

| 原型 | 体型 (body_scale.x) | 头饰 | 披风 | 肩甲 | 主色 (躯干/四肢/头) | 技能色 |
|---|---|---|---|---|---|---|
| `Bulwark` 重甲 | 1.12 | 冠 | 否 | 是 | 蓝 / 石 / 金 | 蓝 |
| `Blade` 剑士 | 0.92 | 冠 | 是 | 否 | 红 / 土 / 红 | 火 |
| `Ranger` 游侠 | 1.02 | 尖顶 | 是 | 是 | 核心 / 石 / 核心 | 核心 |
| `Mage` 法师 | 0.88 | 冠 | 否 | 否 | 石 / 蓝 / 白 | 蓝 |
| `Healer` 治疗 | 1.00 | 尖顶 | 是 | 是 | 土 / 石 / 火 | 草 |
| `Siege` 攻城 | 1.10 | 冠 | 否 | 否 | 金 / 红 / 白 | 金 |

> 头饰 / 武器的 6 型轮廓仍由 `spawn_headgear` / `spawn_weapon` 按 `skin % 6` 生成，
> 与数据表的 6 原型共用同一索引，不产生第二套 ID。

### 10.2 设计说明

1. **造型数据化**：把原先散落在 `hero_palette` 的 `match skin % 6`、体型 `if/else`、
   披风 / 肩甲条件判断，全部收敛到 `HERO_VISUALS` 一行一原型。新增英雄 = 加一行数据，
   不改几何代码；训练假人（`skin = 9`）单列，不占原型索引。
2. **剪影优先**：6 原型的 `(torso, limb, head)` 配色组合唯一（单测锁定），
   体型宽度极差 ≥ 0.2（单测锁定），保证低分辨率 / 远距离下不靠血条即可辨识。
3. **分层骨架**：`HeroRig` 从 4 节点扩到 7 节点（+ 双腿 + 可选披风），
   并保存 `HeroRigBase` 静止姿态；动画每帧在 base 上**重新计算**而非累加，杜绝漂移。
4. **朝向修正**：眼睛从父实体的直接子节点改为 `head` 子节点，转头 / 抬头时脸始终跟随，
   为「上半身瞄准不打断移动」预留结构。
5. **7 态状态机**：优先级 `Death > Hit > Cast > Attack > Move > Idle`；
   `Idle` 呼吸 + 头部微摆 + 披风低频摆动，`Move` 双腿交替 + 反向摆臂 + 躯干起伏，
   `Attack` 躯干扭转（双臂让给既有三段挥击），`Cast` 双手抬起 + 微仰头 + 蓄力脉冲，
   `Hit` 后仰硬直，`Death` 倾倒下沉，`Respawn` 方块自下而上重组（0.6 s 后自动回 Idle）。
6. **裁决隔离 / 冲突规避**：只读父 `Transform` 的 `update_hero_motion`、
   只写子节点的 `tick_hero_anim` 拆成两个系统，避免同系统内读写 `Transform` 的 `B0001`；
   与 `tick_attack_anim` 组成 `.chain()` 固定顺序，`Attack` 态 `skip_arms` 保证两段动画不互相覆盖。
   动画**不触碰** `Hero` / `TransformSync` / 服务端同步组件。
7. **触发接线**：`AbilityCast → CastAnim(0.45s)`、`DamageDealt → HitAnim(0.28s)`、
   `HeroDeath → DeathAnim(1.2s)`；标记组件由状态机倒计时并移除，状态自动回落。
   连续受击用 `insert` 刷新而非 `try_insert` 忽略，避免硬直卡住。

**动画曲线摘要**

| 状态 | 驱动节点 | 曲线 |
|---|---|---|
| Idle | 躯干 / 头 / 披风 | `sin(2t)` 呼吸、`sin(1.3t)` 摆头、`sin(1.7t)` 披风 |
| Move | 双腿 / 双臂 / 躯干 / 披风 | `sin(9t)` 反向摆腿摆臂、`|sin(18t)|` 起伏 |
| Attack | 躯干（双臂跳过） | `sin(12t)` 扭转 |
| Cast | 双臂 / 头 / 躯干 | 固定抬起 -1.0 rad + `sin(8t)` 蓄力 |
| Hit | 躯干 / 头 | 固定后仰，随标记结束回落 |
| Death | 躯干 / 头 | `min(2t,1)` 倾倒下沉 |
| Respawn | 躯干 / 头 | `min(t/0.6,1)` 重组 |

### 10.3 复用规则

1. 新增英雄只新增 / 修改 `HERO_VISUALS` 一行，**禁止**在 `spawn_voxel_hero` 里加 `skin` 分支；
2. 新增动画状态先登记 `HeroAnimState` 与优先级表，再实现姿态分支；优先级变更须同步单测 `state_priority_is_deterministic`；
3. 所有骨架写入必须经 `apply_pose`，禁止系统里散落 `nodes.get_mut(rig.xxx)`；
4. `HeroRigBase` 必须与 `spawn_voxel_hero` 生成时的 `Transform` 严格一致，改几何须同步改 base；
5. 触发标记统一在 `handle_game_event` 对应事件分支插入，不在渲染系统里猜测状态；
6. 动画系统不得读写任何裁决组件 / 服务端同步组件（表现与裁决隔离，§2.1）；
7. 继续复用 P-V1 的 `VoxelMaterialPool` 与 12 色调色板，P-V2 未新增任何材质或纹理资源；
8. 每英雄每帧写入 ≤ 7 个节点（§7 预算），新增节点须重新评估。

### 10.4 阶段验收结论

| 检查项 | 结论 | 证据 |
|---|---|---|
| §6 交付项（数据表 / 骨架 / 7 态 / 朝向） | 全部落地 | V2-01~V2-10 |
| 6 原型配色唯一、体型差 ≥0.2、skin 索引稳定 | 通过 | `visual::tests` 3 例 |
| 7 态优先级、入场 Respawn、状态互异 | 通过 | `hero_anim::tests` 3 例 |
| 运行时无 `B0001` 查询冲突且姿态确实写回 | 通过 | `anim_systems_run_without_query_conflict` |
| 客户端单测 | 20/20 通过 | `cargo test -p client --lib` |
| 合规门禁（含修正后的体素计数） | 4/4 通过 | `cargo test -p gx-e2e --test compliance` |
| 严格静态检查 | 通过 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 工作区编译 | 通过 | `cargo check --workspace --all-targets` |
| 与 P-V0 / P-V1 口径冲突 | **已先行对齐**（A-01 / A-02），未放宽验收标准 | §10.0 |
| 零外部资产 | 通过 | 全部代码生成，资产登记表已同步 |
| 是否可进入 P-V3 | **是** | P-V3 直接复用 `CastAnim.ability_index` 与 `VoxelMaterialPool::fx_material` |

---

## 11. P-V3 阶段交付：技能与事件视觉

### 11.0 与前阶段口径对齐（先对齐再继续）

| 编号 | 冲突 | 处置 |
|---|---|---|
| A-03 | P-V1 §9.3 复用规则 3 要求「逐帧 / 事件生成的材质一律经 `VoxelMaterialPool::fx_material`」，但 `AbilityCast` 主体初版仍用一次性 `voxel::voxel_material` | 给 `handle_game_event` 增加 `VoxelMaterialPool` 参数，技能主体与命中碎片**统一走池**；仅不含池资源的测试夹具保留回退分支。**规则未放宽** |
| A-04 | 复核 §1 C-06 / P-V0：`ability_index` 是否可用 | 已用于 `ability_visual()` 索引、`AbilityDef` 数值读取、材质 seed 与 `CastAnim`，无协议改动 |

### 11.1 资源清单

| 编号 | 交付物 | 类型 | 位置 |
|---|---|---|---|
| V3-01 | `FxShape`（`Orb` / `Ring` / `Beam` / `Pulse`） | 资源类型 | `crates/client/src/visual.rs` |
| V3-02 | `AbilityVisual` + `ABILITY_VISUALS[4]` + `ability_visual(index)` | 数据表 | 同上 |
| V3-03 | `CastEffect` 扩展（阶段 / 起终点 / 技能索引 / 单次迸射标记） | 组件 | `crates/client/src/rendering.rs` |
| V3-04 | 五阶段技能序列 `Windup → Release/Travel → Impact → Recover` | 系统 | `tick_cast_effect` |
| V3-05 | `EventFxQueue` / `EventFxRequest` | 请求队列 | `rendering.rs` |
| V3-06 | `EventBurst` 碎片组件 + `spawn_event_fx` / `tick_event_burst` | 系统 | 同上 |
| V3-07 | 性能预算常量 `MAX_CONCURRENT_FX` / `MAX_SHARDS_PER_EVENT` / `EVENT_FX_TTL` | 约束 | 同上 |
| V3-08 | `team_kind()` 队伍语义色映射 | 色彩映射 | `crates/client/src/networking.rs` |
| V3-09 | 14 类关键事件的视觉请求分支 | 事件接线 | 同上 |
| V3-10 | 技能主体材质池化（A-03 对齐） | 复用 | 同上 |
| V3-11 | 8 个新增单测（技能表唯一性 / 越界回退 / 预算 / 阶段 / 运行时复用） | 验收用例 | `visual.rs::tests`、`rendering.rs::pv3_tests` |

### 11.2 设计说明

#### 11.2.1 四技能差异化

| `ability_index` | 技能 | 语义色 | 形状 | 碎片 | 主体表现 | 尺寸 / 时长来源 |
|---|---|---|---|---|---|---|
| 0 | 火焰弹 | `Effect` 橙红 | `Orb` | 8 | 收缩蓄力 → 单方块飞行 → 命中迸射火星 | `range` / `projectile_speed` |
| 1 | 寒冰新星 | `Marker` 白 | `Ring` | 6 | 扁平方块 → 按 `aoe_radius` 展开阶梯环 → 霜块四散 | `aoe_radius` |
| 2 | 能量光束 | `Blue` 蓝 | `Beam` | 4 | 沿瞄准线拉长的连续方块束 → 命中格点亮 | `range` / `projectile_speed` |
| 3 | 自我治疗 | `Brush` 绿 | `Pulse` | 6 | 竖长方块 → 自下而上上升脉冲 → 绿白方块上浮 | 默认值 + `range` |

> **四色 × 四形互异**（单测锁定），视觉表只描述「风格」；
> `radius` / `max` 全部读 `game_core::abilities::AbilityDef` 的
> `aoe_radius` / `range` / `projectile_speed`，不复制第二套平衡数值。
> 未注册索引回退到 0，不会 panic。

#### 11.2.2 五阶段施法序列与触发时机

| 阶段 | 进度 `p` | 触发时机 | 表现 |
|---|---|---|---|
| `Windup` | 0.00–0.20 | 收到 `AbilityCast` 当帧起 | 施法点上方蓄力方块收缩；同时插 `CastAnim` 驱动英雄抬手 |
| `Release / Travel` | 0.20–0.55 | 蓄力结束 | 从近似起点插值移向落点，按 `FxShape` 切换方块比例 |
| `Impact` | 0.55–0.80 | 抵达落点 | 按 `radius` 展开阶梯方块环，**一次性**推入碎片请求 |
| `Recover` | 0.80–1.00 | 命中后 | 按固定基准收缩（不逐帧累乘，无漂移） |
| 结束 | `t ≤ 0` | — | `despawn`，总时长钳制在 0.6–1.2 s |

> `from` 为**表现层近似起点**（权威施法者位置由服务端裁决，客户端不参与判定）；
> 动画与特效均不回写 `Hero` / `TransformSync`（§2.1 隔离）。

#### 11.2.3 表现层级

| 层 | 内容 | 交付阶段 | 生命周期 |
|---|---|---|---|
| `L0` 常驻 | 地形 / 建筑 / 英雄模型 / 材质 | P-V1 / P-V2 | 常驻 |
| `L1` 战斗反馈 | 普攻挥击、刀光、弹道、受击闪光、血条 | 既有 | ≤ 1 s |
| `L2` 技能序列 | 五阶段主体 + 命中碎片 | P-V3 | 0.6–1.2 s / 0.8 s |
| `L3` 事件广播 | 击杀 / 占领 / 塔毁 / 波次 / 加时 / 胜负 / 护盾 / 状态 | P-V3 | 0.8 s |

层级越高越不依赖具体实体：`L3` 事件多为全局广播、协议不携带坐标，
统一以地图中心为表现锚点；`L1/L2` 由 `EventFxQueue` 之外/之内的既有链路分别处理。

#### 11.2.4 事件视觉覆盖表

| `GameEvent` | 触发时机 | 语义色 | 形状 | 碎片 | 此前状态 |
|---|---|---|---|---|---|
| `AbilityCast` | 事件当帧 | 按技能表 | 按技能表 | 4–8 | 仅通用方块 |
| `HeroDeath` | 事件当帧 | `Gold` | `Orb` | 10 | 仅动画 |
| `MatchEnd` | 事件当帧 | 胜方色 | `Ring` | 12 | 仅日志 |
| `MatchFinished` / `EncounterSettled` | 事件当帧 | 胜方色 | `Ring` | 12 | 落入 `_ => {}` |
| `TurretDestroyed` | 事件当帧 | `Stone` | `Orb` | 8 | 落入 `_ => {}` |
| `SourceCoreDestroyed` | 事件当帧 | `Core` | `Orb` | 12 | 落入 `_ => {}` |
| `JungleMonsterDeath` / `AbyssWardenDeath` | 事件当帧 | `Brush` | `Orb` | 6 | 落入 `_ => {}` |
| `LodeShrineCapture` / `ObjectiveCaptured` | 占领当帧 | 占领方色 | `Ring` | 8 | 落入 `_ => {}` |
| `WaveSpawned` | 波次生成 | `Blue` | `Pulse` | 4 | 落入 `_ => {}` |
| `OvertimeStarted` | 加时开启 | `Effect` | `Ring` | 12 | 落入 `_ => {}` |
| `ShieldChanged`（`value > 0`） | 护盾生效 | `Marker` | `Ring` | 6 | 落入 `_ => {}` |
| `StatusApplied` | 状态生效 | `Core` | `Pulse` | 4 | 落入 `_ => {}` |

> 仍由 `_ => {}` 处理的是**无世界空间表现需求**的事件：
> `HeroGoldChanged` / `ItemPurchased` / `ItemSold` / `ShopRejected`（HUD 反馈）、
> `Pause` / `Resume` / `Ping`（流程或信号）、`ReconnectState` / `ConfigMismatch`（握手）、
> `HeroLevelUp` / `HeroSpawn` / `HeroDespawn`（已有模型或 UI 表现）、
> `MinionSpawn` / `MinionDeath`（高频，避免特效刷屏）。
> 高频事件刻意不生成方块，属于 §7 性能预算的一部分。

#### 11.2.5 性能约束

| 约束 | 值 | 实现位置 | 验收 |
|---|---|---|---|
| 同屏临时特效 | ≤ 96 实体 | `MAX_CONCURRENT_FX`；`spawn_event_fx` 超限时优先 despawn 最旧碎片 | 常量单测 + 生成路径 |
| 每事件碎片 | ≤ 12 | `MAX_SHARDS_PER_EVENT` + `clamp_shards()` | 单测覆盖 0/4/12/99 |
| 碎片生命周期 | 0.8 s（< PRES-8 的 1 s） | `EVENT_FX_TTL` + `tick_event_burst` | 单测 |
| 技能序列时长 | 0.6–1.2 s | `AbilityCast` 分支 `clamp` | 代码 + 文档 |
| 纹理 / 材质 | 全部经 `VoxelMaterialPool` 去重 | `spawn_event_fx` / `AbilityCast` 分支 | 运行时单测 `stats.effects == 1` |
| 网格 | 共享单位 `Cuboid` | `VoxelMaterialPool::cube` | 运行时单测 `mesh_shared` |
| 随机性 | 确定性 `hash(i, seed, ...)` | 碎片排布 | 回放一致 |
| 每事件请求 | 1 次 `Impact` 迸射 | `CastEffect.burst_done` | 代码 |

### 11.3 复用规则

1. 新技能只加 `ABILITY_VISUALS` 一行 + `AbilityDef` 一项；**禁止**在 `tick_cast_effect` 里加 `ability_index` 分支；
2. 技能尺寸 / 时长必须从 `AbilityDef` 读取，视觉表不得复制数值；
3. 所有事件视觉请求经 `EventFxQueue` → `spawn_event_fx`，**禁止**在 `handle_game_event` 里直接 `commands.spawn` 碎片；
4. 所有碎片材质经 `fx_material`、网格经 `pool.cube`；新增事件只调用 `push_fx(kind, shape, pos, shards, seed)`；
5. `seed` 按事件类别分段（技能 13~310，事件 81~97+），保证同名事件碎片排布可复现、不同事件互不串色；
6. 新事件须登记 §11.2.4 覆盖表，并说明层级与生命周期；高频事件默认不生成世界空间特效；
7. 碎片数、生命周期、同屏上限三项常量不得在调用点绕过；
8. 继续复用 P-V1 调色板、P-V2 `CastAnim`，不新建平行资源库或事件通道。

### 11.4 阶段验收结论

| 检查项 | 结论 | 证据 |
|---|---|---|
| §6 交付项（技能差异化 / 五阶段 / 事件覆盖 / 性能约束） | 全部落地 | V3-01~V3-11 |
| 四技能语义色与形状互异、越界回退 | 通过 | `visual::tests` 2 例 |
| 性能预算（≤12 碎片 / <1 s / ≤96 同屏） | 通过 | `pv3_tests::performance_budget_is_bounded` |
| 五阶段边界有序且覆盖完整生命周期 | 通过 | `pv3_tests::ability_sequence_phases_are_ordered_and_cover_full_lifetime` |
| 事件队列 → 生成 → 生命周期无查询冲突，且网格/材质确实复用 | 通过 | `pv3_tests::event_fx_systems_run_without_conflict` |
| 客户端单测 | 25/25 通过 | `cargo test -p client --lib` |
| 工作区编译 | 通过 | `cargo check --workspace --all-targets` |
| 与前阶段口径冲突 | **已先行对齐**（A-03 材质池、A-04 技能索引），未放宽 §9.3 | §11.0 |
| 零外部资产 | 通过 | 全部代码生成，资产登记表已同步 |
| P-V0~P-V3 四阶段是否全部完成 | **是**：逐阶段验收通过，无遗留冲突 | §8.4 / §9.4 / §10.4 / §11.4 |

---

## 12. P-V4 阶段交付：非英雄实体、地形分层与 8-bit 音频

> 本阶段同时修复了试玩反馈的两个缺陷：**地图随英雄移动异常闪烁**、**技能不落在鼠标点击处**。
> 缺陷根因与护栏见 §12.4。

### 12.0 与前阶段口径对齐（先对齐再继续）

| 编号 | 冲突 | 处置 |
|---|---|---|
| A-05 | 前四阶段资产表 / `README.md` §8.6 / `docs/05_COMBAT_SPEC.md` §16.4 均写「8-bit 音频零实现 / 待资产」，本阶段变为**已提供** | 两份 `ASSET_REGISTRY.md`、README、§16 全部回写；**门禁要求的「待资产 / 外部依赖」标注仍由 A-006 中文字形条目承担**，不删除、不放宽 |
| A-06 | §16.1 PRES-7 与资产表写「当前无高度层与河道，`PixelKind::Water` 已定义未启用」 | 本阶段启用河道与 7 层高度表，逐处回写；`Water` 从「定义未启用」变为「河道水面」 |
| A-07 | §11.2.4 明确「高频事件刻意不生成世界空间特效」，音频若逐事件发声会违反同一性能意图 | 音频采用**稀疏显式映射表**（`SFX_EVENT_MAP`），野怪死亡 / 波次 / 状态等高频事件不发声；视觉与听觉共用同一次事件消费 |
| A-08 | §7 性能预算只覆盖视觉实体 | 新增音频预算：单帧 ≤4 次播放、单音效 ≤0.5 s、采样率 22.05 kHz 单声道、内存 PCM；静态发布包仍无音频二进制 |

### 12.1 资源清单

| 编号 | 交付物 | 类型 | 位置 |
|---|---|---|---|
| V4-01 | `NonHeroKind`（`Minion` / `Turret` / `Core` / `Monster`）+ `NonHeroMotion` | 资源类型 | `crates/client/src/entity_visual.rs` |
| V4-02 | `NON_HERO_VISUALS[4]` 造型表 + `non_hero_visual()` | 数据表 | 同上 |
| V4-03 | `NonHeroRig`（3 节点 + 静止姿态）/ `NonHeroAnim` | 组件 | 同上 |
| V4-04 | `sync_monsters` 野怪快照驱动生成回收 | 系统 | 同上 |
| V4-05 | `tick_non_hero_anim` 走路摆动 / 待机呼吸 / 核心脉冲 / 血量损伤 | 系统 | 同上 |
| V4-06 | `spawn_voxel_non_hero` / `spawn_voxel_monster` | 生成函数 | `crates/client/src/voxel.rs` |
| V4-07 | 哨塔 / 源核静态堆叠改父子骨架（不再是死装饰） | 几何改造 | `spawn_voxel_stack` |
| V4-08 | `RIVER_HALF_WIDTH = 6.0` 权威河道半宽 | 地图常量 | `crates/game-core/src/map.rs` |
| V4-09 | `TerrainLayer` + `TERRAIN_LAYERS[7]` 有序高度层表 | 资源类型 / 数据表 | `crates/client/src/voxel.rs` |
| V4-10 | 下沉河床 / 水面 / 河岸三档 + 地面区间减法（水平不重叠） | 地形生成 | `spawn_voxel_terrain` |
| V4-11 | 地面拼贴共享单位 `Cuboid` + `with_scale` 合批 | 性能优化 | 同上 |
| V4-12 | `ChipWave` / `SfxDef` / `SFX_TABLE[7]` / `synthesize()` | 合成器 | `crates/client/src/audio.rs` |
| V4-13 | `ChipSource` + `ChipDecoder`（自定义 `Decodable`，内存 PCM） | 音频资产 | 同上 |
| V4-14 | `SfxQueue` / `play_sfx` / `ChipAudioPlugin` / `SFX_EVENT_MAP` | 事件接线 | 同上 / `rendering.rs` / `main.rs` |
| V4-15 | `Projection::Perspective { near: 1, far: 400 }` + `MARKER_Y` 标记层 | 缺陷修复 | `rendering.rs` / `input.rs` |
| V4-16 | 主相机合并查询、左键当帧重算落点、服务端落点校验 | 缺陷修复 | `input.rs` / `server/src/systems.rs` / `simulation.rs` |
| V4-17 | 17 个新增单测（模型 4 / 地形 3 / 音频 8 / 输入 2） | 验收用例 | 各模块 `tests` |

### 12.2 设计说明

#### 12.2.1 非英雄实体动态模型

| 类别 | 快照来源 | 骨架 | 动画 | 血量反馈 |
|---|---|---|---|---|
| 小兵 | `EntityType::Minion`（`sync_minions`） | 躯干 / 头 / 武器 3 节点 | 移动摆腿摆臂、待机呼吸 | — |
| 野怪 | `EntityType::JungleMonster`（`sync_monsters`，**新增客户端实体**） | 同结构、体型放大 | 同上 | — |
| 哨塔 | 静态地形 `spawn_voxel_stack` | 层级堆叠转为父子 3 节点 | 核心自转 + 脉冲 | 核心缩小、脉动加快 |
| 源核 | 同上 | 同上 | 同上 | 同上 |

- 造型全部读 `NON_HERO_VISUALS`，队伍色仅覆盖躯干；新增类型 = 加一行数据。
- 动画从 `TransformSync.velocity` 读速度、**不读父 `Transform`**，避免同系统 `B0001`；
  每帧基于 `NonHeroRig` 静止姿态重算，最多写 3 个节点，无累积漂移。
- `tick_non_hero_anim` 按 `NetId` 从快照取 `health/max_health` 刷新 `health_ratio`，
  静态建筑无 `NetId` 时恒为 1.0；全程不回写裁决组件（§2.1 隔离）。

#### 12.2.2 地图高度层与河道

| 层 | 中心 y | 厚度 | 顶面 | 用途 |
|---|---|---|---|---|
| `LAYER_RIVER_BED` | -0.55 | 0.30 | -0.40 | 下沉河床 |
| `LAYER_WATER` | -0.30 | 0.10 | -0.25 | 河道水面（`PixelKind::Water`） |
| `LAYER_GROUND` | -0.25 | 0.40 | -0.05 | 12×12 草 / 土棋盘 |
| `LAYER_FOLD` | -0.02 | 0.07 | 0.015 | 折叠线 |
| `LAYER_BANK` | 0.03 | 0.14 | 0.10 | 河岸石条 |
| `LAYER_LANE` | 0.12 | 0.07 | 0.155 | 三路兵线条带 |
| `LAYER_WAYPOINT` | 0.20 | 0.05 | 0.225 | 路点补缝 |

> 相邻层顶面间隔 ≥ 0.05（单测锁定），顶面严格递增。
> 河道 `|x| ≤ 6` 从地面区间中**扣除**后另铺河床 + 水面，陆地与水体水平不重叠；
> 河岸窄条在带两侧，与地面 y 差 0.15。全部层为纯装饰，
> **服务端裁决平面恒为 y=0**，英雄可涉水、不被高地阻挡。
> 地面 144 块改用共享 `Cuboid` + `with_scale`，不再逐块 `meshes.add`。

#### 12.2.3 8-bit 音频

| 层 | 内容 |
|---|---|
| 波形层 | `Square` / `Triangle` / `Noise`（确定性 LCG），带频率滑移 `freq → freq_end` |
| 包络层 | ADSR 四段比例之和恒为 1.0（单测锁定），首尾静音 |
| 量化层 | 4-bit / 16 阶幅度量化，样本恒为 `n/16`，范围 `[-1, 1]` |
| 资产层 | `ChipSource` 自定义 `Decodable` + `ChipDecoder` 实现 `rodio::Source`；内存 PCM 单声道 22.05 kHz，**无文件格式、无解码 feature、无外部字节** |
| 播放层 | `SfxQueue → play_sfx → AudioPlayer(Handle<ChipSource>) + PlaybackSettings::DESPAWN` |
| 降级层 | `Assets<ChipSource>` 未注册（headless / 无声设备）时直接丢弃队列，不 panic |

**事件 → 音效触发时机**（与 §11.2.4 的 seed 段一致，视觉与听觉共用 `spawn_event_fx` 的同一次消费）

| seed | 事件 | 音效 | 时长 |
|---|---|---|---|
| 13 / 110 / 207 / 304 | 四技能 Impact 迸射 | `Cast` | 0.20 s |
| 81 / 94 | 对局结束 / 遭遇结算 | `Victory` | 0.42 s |
| 82 | 英雄阵亡 | `Kill` | 0.28 s |
| 83 / 95 | 哨塔摧毁 / 源核摧毁 | `TurretDown` | 0.36 s |
| 85 | 占领 | `Capture` | 0.24 s |
| 93 | 加时开启 | `Overtime` | 0.34 s |
| 84 / 86~92 / 96+ | 野怪死亡 / 波次 / 护盾 / 状态 | **静默**（高频事件不刷屏） | — |

> `Attack` 已登记在 `SFX_TABLE`，供后续普攻接线使用；当前未接入事件，避免每帧触发。

#### 12.2.4 表现层级（在 §11.2.3 之上补齐）

| 层 | 内容 | 阶段 |
|---|---|---|
| `L0` 常驻 | 地形 7 高度层 / 河道 / 建筑骨架 / 英雄 | P-V1 / P-V2 / P-V4 |
| `L1` 战斗反馈 | 普攻、弹道、受击 | 既有 |
| `L2` 技能序列 | 五阶段主体 + 命中碎片 | P-V3 |
| `L3` 事件广播 | 击杀 / 占领 / 塔毁 / 胜负 | P-V3 |
| `L4` 听觉 | 技能 / 击杀 / 塔毁 / 占领 / 加时 / 胜负 | P-V4 |

#### 12.2.5 性能约束（补充 §7）

| 约束 | 值 | 实现 |
|---|---|---|
| 地形层相邻顶面间隔 | ≥ 0.05 | `TERRAIN_LAYERS` 单测 |
| 地面网格 | 全部共享 1 个 `Cuboid` | `spawn_voxel_terrain` 的 `cube.clone()` |
| 相机 near / far | 1 / 400（比值 4e2，原 1e4） | `RenderingPlugin` |
| 每非英雄实体每帧写入 | ≤ 3 个节点 | `tick_non_hero_anim` |
| 单帧播放音效 | ≤ 4 | `MAX_SFX_PER_FRAME` |
| 单音效时长 / 采样 | ≤ 0.5 s / 22.05 kHz 单声道 | `SFX_TABLE` 单测 |
| 发布树音频二进制 | 0 | 全程内存合成，门禁 `TC-CBT-12` |

### 12.3 复用规则

1. 新增非英雄类型只加 `NON_HERO_VISUALS` 一行 + `NonHeroKind::from_entity_type` 一个分支；**禁止**在 `spawn_voxel_non_hero` 里加 `match kind` 几何分支；
2. 非英雄动画必须经 `NonHeroRig` 的静止姿态重算，禁止逐帧累加；速度只从 `TransformSync` 读；
3. 新增地形层必须登记进有序 `TERRAIN_LAYERS` 并满足间隔单测；**禁止**让两个可能水平重叠的层共用 y；
4. 地面 / 河道 / 河岸只允许共享 `cube` 网格 + `with_scale`，禁止逐块新建 `Mesh`；
5. 新增音效只加 `SfxId` + `SFX_TABLE` 一行；新增触发点只在 `SFX_EVENT_MAP` 登记 seed；
   **禁止**在 `play_sfx` 之外直接 spawn `AudioPlayer`；
6. 高频事件默认静默；每帧播放上限与单音效时长不得在调用点绕过；
7. 音频合成保持纯函数与确定性 LCG，禁止引入系统随机源或外部音频文件；
8. 继续复用 P-V1 `VoxelMaterialPool`、P-V2 `HeroRig`、P-V3 `EventFxQueue`，不新建平行资源库或事件通道。

### 12.4 两个试玩缺陷：根因与护栏

#### 12.4.1 地图随英雄移动异常闪烁

| # | 根因 | 修复 |
|---|---|---|
| 1 | 地面棋盘块宽 `tile + 0.2` **大于**中心距 `tile`，相邻块水平重叠且 y / 厚度完全相同 → 共面 z-fighting | 块宽改为 `b - a`（严格等于区间宽）；河段用区间减法 |
| 2 | 路点补缝与兵线条带同 y（0.05）同厚（0.07）且部分重叠 | 补缝抬到独立 `LAYER_WAYPOINT`（顶 0.225，差 0.07） |
| 3 | 折叠线与地面 y 差仅 0.02 | 归位 `LAYER_FOLD`（顶 0.015，与地面差 0.065） |
| 4 | 基地石座顶 0.35 与地砖顶 0.38 差 0.03 | 石座下移，差扩到 0.11 |
| 5 | 相机默认 `near=0.1 / far=1000`（比值 1e4），深度量化把 0.05 量级层高差压成同一值，镜头平移即闪烁 | 显式 `near=1 / far=400`（比值 4e2） |
| 6 | 范围环 `scale = Vec3::splat(range)` 把 0.12 厚的环体放大到数单位并穿进地形 | 只放大水平 `Vec3::new(range,1,range)`，y 抬到 `MARKER_Y = LAYER_WAYPOINT.top() + 0.075` |
| 7 | 相机 y 分量硬赋值不参与 lerp，英雄高度抖动时纵向跳变 | x/y/z 一起 lerp |
| 8 | 各类光标 / 选中 / 落点标记 y 散落 0.06 / 0.15 / 0.2，与地形层近共面 | 统一 `MARKER_Y`，与层表联动 |

> 护栏单测：`terrain_layers_are_ordered_and_separated`、`ground_tiles_do_not_overlap`、
> `water_palette_is_enabled`、`marker_y_clears_highest_terrain_layer`。

#### 12.4.2 技能不落在鼠标点击处

| # | 根因 | 修复 |
|---|---|---|
| 1 | **决定性主因**：egui 额外插入裸 `Camera`，输入系统用 `Query::single()` 取相机 → 恒失败 → 瞄准点算不出 → 上报 `aim=(0,0)` → 服务端把技能打向地图原点 | `primary_camera()` 用 `Query<(&Camera,&GlobalTransform), With<Camera3d>>` 合并查询，相机与变换来自**同一实体**且排除裸相机 |
| 2 | 旧 `camera_pair` 从两个独立 Query 各取第一个，不保证配对 | 同上，合并查询 |
| 3 | 左键确认使用**上一帧**缓存 `target_point`（瞄准计算在左键之后执行），慢一帧；缓存 `None` 时上报零向量 | 确认当帧用主相机重新求交；无命中**不产生 `PendingCast`**、保持瞄准态 |
| 4 | `viewport_to_world` 命中点 y 残留，下游把 `Vec3` 当下行坐标 | `ground_hit` 显式 `Vec3::new(hit.x, 0.0, hit.z)` |
| 5 | 服务端不校验 `aim_dir`，把 `(0,0)` / NaN / 越界当合法落点 | 新增 `is_finite` + 非零 + `|x|,|z| ≤ MAP_HALF` 三重校验；非法目标落点留在施法者脚下 |
| 6 | 服务端用 3D `length()` 算水平距离，被施法者 y 分量污染 | 改用 `Vec2` 水平距离，方向向量 y 恒 0 |
| 7 | `PendingInputs` 从不清空技能位，同一 `InputCmd` 每 tick 重复触发施法 | 消费后 `ability_mask &= 1`（保留普攻位） |
| 8 | 客户端技能起点锚在落点后方 `range*0.4`，视觉上像「从别处飞来」 | 起点改为落点正上方 `to + Y*2.0`，保持五阶段垂直下落 |
| 9 | 客户端把 `ability_index` 直接当 `ability_id` 查数值表 | 经 `default_hero_abilities()[index]` 换算，越界回退 |

> 护栏单测：`primary_camera_ignores_egui_bare_camera`、`marker_y_clears_highest_terrain_layer`，
> 以及 `entity_visual` / `audio` / `voxel` 三个模块的回归用例。
> 已知限制：点击高塔顶部仍有约 1.25·Δy 的水平视差（裁决平面为 y=0），
> 地面与落点指示已完全对齐；彻底消除需引入几何 raycast，列为后续优化。

### 12.5 阶段验收结论

| 检查项 | 结论 | 证据 |
|---|---|---|
| §8.7 P-V4 三项（非英雄模型 / 高度层与河道 / 8-bit 音频） | 全部落地 | V4-01~V4-14 |
| 造型表 4 类唯一、`EntityType` 映射显式、损伤曲线有界 | 通过 | `entity_visual::tests` 4 例 |
| 高度层有序且间隔 ≥0.05、地面无重叠、`Water` 启用 | 通过 | `voxel::terrain_tests` 3 例 |
| 波形量化 / ADSR / 确定性 / 内存 PCM / headless 静默 / 稀疏映射 | 通过 | `audio::tests` 8 例 |
| 主相机容错与标记层高度 | 通过 | `input::tests` 2 例 |
| 两个试玩缺陷 | **已修复并加回归护栏** | §12.4 |
| 客户端单测 | 42/42 通过 | `cargo test -p client --lib` |
| 合规门禁 | 4/4 通过 | `cargo test -p gx-e2e --test compliance` |
| 工作区编译 | 通过 | `cargo check --workspace --all-targets` |
| 与前阶段口径冲突 | **已先行对齐**（A-05~A-08），未放宽任何验收标准 | §12.0 |
| 零外部资产 | 通过 | 音频内存合成，发布树无音频二进制，资产登记表已同步 |
| P-V 系列是否全部完成 | **是**：P-V0 → P-V4 逐阶段验收通过 | §8.4 / §9.4 / §10.4 / §11.4 / §12.5 |

---

## 13. P-U1 阶段交付：大厅、英雄档案、道具图鉴与模式选择

> 需求：游戏刚进入没有大厅、没有英雄相关信息、商店道具信息不全、缺少 PVP / PVE 可选模式。
> 本阶段沿用 P-V 的「资源清单 / 设计说明 / 复用规则 / 验收结论」四项交付格式。

### 13.0 与前阶段口径对齐（先对齐再继续）

| 编号 | 冲突 | 处置 |
|---|---|---|
| A-09 | P-V0 的 `draw_lobby` 用 `egui::Area` + 手绘 painter 画**静态示意图**，只有一行不可点的模式文字、3 张硬编码英雄卡、一个进入按钮；用户实测「刚进入没有大厅」 | 保留 `AppState` 默认 `Lobby` 与 `OnEnter(InMatch)` 延后连接（不改流程），把大厅换成**锁定视口的顶层 `Window` + 三列布局**；旧函数归档不注册。egui 0.36 的 `CentralPanel::show` 只接受 `&mut Ui`（嵌套语义），顶层必须用 `Window::show(ctx, ..)` |
| A-10 | `README.md` §6.1 长期写「大厅与流程 = `draw_lobby` 全屏入口」，与重构后不符 | 更新该行并新增 `README.md` §6.4 完整说明；§6.1 其余历史描述保留为设计依据 |
| A-11 | 商店协议链（`PurchaseItem` / `SellItem` → `try_buy` / `try_sell` → `ItemPurchased` 等）早已闭环，但客户端**从未发送**，`README` 与 §16 均标为未完成 | 客户端补 `ClientCommandQueue → send_client_commands` 唯一发送入口；文档状态回写。**不改服务端裁决**：距离 / 阵亡 / tick / 返还 / 栏位仍全部由服务端重算 |
| A-12 | `MatchMode::{Pve, Pvp}` 枚举与 PVE bot AI 已实现，但 `Pvp` 从未被赋值，`handle_state_transitions` 恒为「≥1 人即 Running」 | 新增 `ClientMessage::SelectMode`（末尾追加变体，`PROTOCOL_VERSION` 与 `content_digest()` 不变）；服务端只在 `MatchPhase::Warmup` 接受；`required_players()` 规定 PVP 需 2 名真人；`ensure_pve_bot` 依据 `MatchClock.mode` 天然跳过 PVP |
| A-13 | 时序竞争：`ensure_pve_bot` 在 `FixedUpdate` 按默认 PVE 判断，若模式消息晚于首个玩家连接，AI 会在切到 PVP 前被生成 | `SelectMode` 与 `Hello` **同批** `ReliableOrdered` 上行；服务端 `handle_session_messages`（认证前）与 `handle_gameplay_messages`（认证后 / 重连）两处都能处理，覆盖三种到达时机 |
| A-14 | §12.2.2 写「地形在 `spawn_game_scene` 的 Startup 生成」，与「大厅应是独立全屏界面、后景不渲染对局」冲突 | 游戏世界（地形 + 标记）改由 `OnEnter(AppState::InMatch)` 的 `spawn_match_world` 生成，启动期只建相机 + 灯光（供 egui 主上下文）；§12.2.2 改为设计依据，实际以当前代码为准 |

### 13.1 资源清单

| 编号 | 交付物 | 类型 | 位置 |
|---|---|---|---|
| U1-01 | `HeroProfile` + `HERO_PROFILES[6]` + `hero_profile(index)` | 数据表 | `crates/client/src/lobby.rs` |
| U1-02 | `ABILITY_NOTES_ZH/EN[4]` 技能说明（数值仍读 `AbilityDef`） | 数据表 | 同上 |
| U1-03 | `ItemCategory` / `ITEM_CATEGORIES[5]` / `ItemProfile` + `ITEM_PROFILES[12]` / `stats_text()` | 数据表 | 同上 |
| U1-04 | `LobbySelection`（英雄索引 + 模式）/ `ClientCommandQueue` / `ShopToast` / `CodexOpen` | ECS 资源 | 同上 |
| U1-05 | `MatchModeExt` 展示文案扩展 trait | 展示接口 | 同上 |
| U1-06 | `draw_lobby` 三列大厅（模式 / 英雄列表 / 详情） | UI 系统 | `crates/client/src/lobby_ui.rs` |
| U1-07 | `draw_item_codex` 大厅 12 件道具图鉴窗口 | UI 系统 | 同上 |
| U1-08 | `draw_shop` 真实 12 道具 + 分类筛选 + 购买 / 售出 + 回执 | UI 系统 | 同上 |
| U1-09 | `item_card` 图鉴 / 商店共用渲染、`tick_shop_toast` | 复用接口 | 同上 |
| U1-10 | `hero_badge` HUD 徽标 + 技能栏 `on_hover_text` 说明 | 局内信息 | 同上 / `crates/client/src/ui.rs` |
| U1-11 | `MatchMode::code()/from_code()` 权威编解码 | 协议接口 | `crates/game-core/src/gameplay/clock.rs` |
| U1-12 | `ClientMessage::SelectMode { mode: u8 }` | 协议变体 | `crates/game-core/src/net.rs` |
| U1-13 | `apply_match_mode()` 唯一写入口（双系统兜底） | 服务端系统 | `crates/server/src/networking.rs` |
| U1-14 | `required_players()` + PVP 人数门槛 | 状态机 | `crates/server/src/lib.rs` |
| U1-15 | `send_client_commands()` + `Hello` 同批携带模式 | 客户端系统 | `crates/client/src/networking.rs` |
| U1-16 | 11 个新增单测（目录 5 / UI 2 / 服务端 2 / 协议 1 / 渲染 1） | 验收用例 | `lobby.rs`、`lobby_ui.rs`、`server/lib.rs`、`game-core/net.rs`、`rendering.rs` |
| U1-17 | 大厅与对局场景解耦：`spawn_game_scene`(Startup 仅相机 + 灯光) / `spawn_match_world`(`OnEnter(InMatch)` 生成地形 + 标记) | 场景生成 | `crates/client/src/rendering.rs` |

### 13.2 设计说明

#### 13.2.1 大厅结构

| 区 | 内容 | 交互 |
|---|---|---|
| 顶栏 | 标题 / 副标题 / 状态提示 | — |
| 左列 | PVE / PVP 两个 `selectable_value`、模式说明、操作提示、进入对局按钮 | 点选模式；点击或 `Enter` → `AppState::InMatch` |
| 中列 | 6 位英雄档案卡（编号 / 中英名 / 定位） | 点选切换右侧详情 |
| 右列 | 描述、外观、五项定性评分、4 个技能（名称 / CD / 蓝耗 / 射程 / 说明）、推荐出装、打法提示 | 只读 |
| 底栏 | 「道具图鉴」开关、当前档案与模式、连接提示 | 开关图鉴窗口 |

**不伪造数值是本阶段硬约束**：英雄用 1..5 定性评分描述定位，局内真实血量 / 攻击仍只从服务端快照的 `Hero` 组件显示；技能数值一律 `get_ability_def(default_hero_abilities()[i])` 读取，`ABILITY_NOTES_*` 只补文案；道具价格与 8 项属性一律从 `ShopCatalog::placeholder()` 读取，`ItemProfile` 只补中文名 / 分类 / 描述。

#### 13.2.2 道具图鉴与商店

| 渠道 | 展示 | 交互 | 数值来源 |
|---|---|---|---|
| 大厅「道具图鉴」 | 12 件按 5 类分组：中英名、价格、属性加成、合成前置、描述 | 只读 | `ShopCatalog` + `ITEM_PROFILES` |
| 局内商店（`P`） | 同样 12 件，另加分类筛选、金币、提交回执 | 「购买」→ `ClientCommand::Purchase(registry::item_id(id))`；6 个「售出栏位」→ `Sell(slot)` | `Hero.gold` + `ShopCatalog` |

`item_card()` 是两处唯一渲染入口，保证信息一字不差。UI **只入队**：`send_client_commands()` 统一编码为
`ClientMessage::PurchaseItem { item_id }` / `SellItem { slot }`，连接未建立时队列保留；`ShopToast` 显示
「已提交…（等待服务端裁决）」，不冒充购买成功。服务端 `try_buy/try_sell` 重算距离、阵亡、tick、栏位与返还，
客户端不参与判定（§2.1 隔离）。

#### 13.2.3 PVE / PVP 模式链路

| 环节 | 行为 |
|---|---|
| 大厅 | `LobbySelection.mode`，默认 `Pve` |
| 上行 | `Hello` 同批发送 `ClientMessage::SelectMode { mode: MatchMode::code() }`；连接后 `send_client_commands` 再补发一次（重连兜底） |
| 服务端接收 | `handle_session_messages`（未认证，与 Hello 同批）与 `handle_gameplay_messages`（认证后 / 重连）都调用 `apply_match_mode()`；未知码回退 PVE |
| 写入约束 | 仅 `MatchPhase::Warmup` 接受，局中一律忽略，避免中途改变人数门槛与 AI |
| 开局门槛 | `required_players(Pve) = 1`、`required_players(Pvp) = 2`；不足则 `ServerState::WaitingForPlayers` |
| AI 生成 | `ensure_pve_bot` 判据为 `clock.mode == MatchMode::Pve`，因此 PVP 不生成 AI 对手 |
| 英雄外观 | `skin` 仍由服务端 `HeroSpawn` 权威下发；大厅档案是**展示层预览**，不改写裁决组件 |

#### 13.2.4 与零外部资产口径

英雄档案、道具图鉴、模式文案全部是 Rust 常量；UI 面板与字形沿用既有「egui 内置字形 + 运行时借用 OS 中文字形 + ASCII 回退」
（`ASSET_REGISTRY.md` A-005 / A-006）。本阶段未新增任何资源文件、字节内嵌宏或第三方依赖。

### 13.3 复用规则

1. 新增英雄只加 `HERO_PROFILES` 一行（及对应的 `HERO_VISUALS` 造型行）；**禁止**在 `draw_hero_detail` 里加 `index` 分支；
2. 新增道具只加 `ShopCatalog` 一项 + `ITEM_PROFILES` 一行，单测 `item_catalog_exactly_covers_shop_catalog` 会强制一一对应；
3. 技能数值只能从 `get_ability_def()` 读；`ABILITY_NOTES_*` 只放说明文案，**禁止**复制冷却 / 蓝耗 / 数值；
4. 大厅 / 图鉴 / 商店的 UI 只写 `ClientCommandQueue`，**禁止**直接持有 `RenetClientResource` 或 spawn `AudioPlayer`；
5. 购买 / 售出是否成功以服务端事件为准，UI 只能显示「已提交」，`ShopToast` 不得宣称成功；
6. 模式编解码只用 `MatchMode::code()/from_code()`；服务端模式写入只经 `apply_match_mode()`，且保持 Warmup 门禁；
7. 大厅档案与局内 `skin` 解耦是刻意裁决 / 表现隔离，后续若要做「选英雄」必须**先**补服务端选人协议与 `HeroSpawn` 字段，不得在客户端偷偷覆盖权威组件；
8. 顶层 UI 一律用 `Window::show(ctx, ..)`（egui 0.36）；`CentralPanel::show` 只用于嵌套 `&mut Ui`。

### 13.4 阶段验收结论

| 检查项 | 结论 | 证据 |
|---|---|---|
| 进入即见大厅（三列 / 可交互） | 通过 | `lobby_ui::draw_lobby`，旧 `Area` 实现归档 |
| 6 位英雄档案 + 技能 / 出装 / 评分 | 通过 | `HERO_PROFILES`；`lobby::tests` 5 例 |
| 12 件道具完整信息 + 图鉴 / 商店共用渲染 | 通过 | `ITEM_PROFILES` 与 `ShopCatalog` 一一对应单测 |
| 真实购买 / 售出上行 | 通过 | `ClientCommandQueue → send_client_commands` |
| PVE / PVP 可选并贯通开局 | 通过 | `SelectMode` 双系统兜底、`required_players` 单测 2 例 |
| `SelectMode` 协议变体可往返 | 通过 | `game-core/net.rs::select_mode_message_roundtrips` |
| 大厅不渲染对局世界、进入后才生成地形 | 通过 | `rendering::lobby_startup_renders_only_camera_not_match_world` |
| 客户端单测 | 50/50 通过 | `cargo test -p client --lib` |
| 工作区全量测试 | 0 失败 | `cargo test --workspace` |
| 严格静态检查 | 通过 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 合规门禁 | 4/4 通过 | `cargo test -p gx-e2e --test compliance` |
| 与前阶段口径冲突 | **已先行对齐**（A-09~A-14），未放宽任何验收标准 | §13.0 |
| 零外部资产 | 通过 | 全为代码常量，无新增资源文件 |
