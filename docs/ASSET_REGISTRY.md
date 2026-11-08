# GX-01 资产来源登记表

| 项 | 内容 |
|---|---|
| 文档编号 | `GX-ASSET-REG-2026-001` |
| 版本 | v1.2（本轮需求变更：表现层零外部资源） |
| 关联 | `CR-01` / `CR-08` / `FR-85` / `FR-86` / `FR-96` / `DN-31` / `DN-32` / `TC-COMPLIANCE-02` / `TC-CBT-12` |
| 规则 | 每个进入发布包的可见表现资产必须登记作者、来源、日期与许可；**表现层一律由代码生成**，不引入外部资源文件；未登记资产不得合并 |

## 1. 表现层：零外部资产（v1.2）

本轮需求明确：**表现层全部使用代码生成，不依赖任何外部资源，采用 Minecraft 风格**。因此本次交付不再引入美术、字体或音频文件；所有几何（轴对齐立方体）、低分辨率像素纹理、材质、图标、特效与 UI 字形均由源码在运行时生成。约束 PRES-1~9 见 `GX-CBT-2026-001` §16。

| 资产 ID | 类型 | 作者 / 来源 | 日期 | 许可 | 状态 |
|---|---|---|---|---|---|
| A-001 | 英雄体素模型（7 节点分层骨架，约 13~19 `Cuboid`）、造型表与 7 态动画 | 本项目代码程序化生成（`client/src/voxel.rs::spawn_voxel_hero`、`visual.rs::HERO_VISUALS`、`hero_anim.rs`） | 2026-11-07 | 自有 | 已提供；P-V2 落地 6 剪影原型与待机/移动/攻击/技能/受击/死亡/重生动画，见 `docs/06_VISUAL_SPEC.md` §10 |
| A-002 | 像素纹理（16×16，`Nearest`，12 种 `PixelKind`）与材质池 | 本项目代码程序化生成（`visual::pattern_texture_data` / `VoxelMaterialPool` / `voxel::voxel_material_keyed`） | 2026-11-07 | 自有 | 已提供；P-V1 已落地逐像素噪点 / 5 种图案 / 1px 描边 / 伪 AO 明暗与资源复用，规范见 `docs/06_VISUAL_SPEC.md` §9 |
| A-003 | 体素地形 7 高度层 / 河道 / 兵线 / 草丛 / 基地 / 野区营地 / 建筑动态骨架 / 小兵与野怪模型 | 本项目代码程序化生成（`voxel::TERRAIN_LAYERS` / `spawn_voxel_terrain` / `spawn_voxel_stack` / `spawn_voxel_non_hero`） | 2026-11-07 | 自有 | 已提供；P-V4 落地河道与高度分层（`PixelKind::Water` 已启用）、共享方块合批、非英雄实体动态骨架与野怪快照实体，见 `docs/06_VISUAL_SPEC.md` §12 |
| A-004 | 命中闪光 / 挥击刀光 / 弹道 / 瞄准标记 / 技能五阶段序列 / 关键事件方块碎片 | 本项目代码程序化生成（`client/src/rendering.rs` 的 `CombatFxQueue` / `EventFxQueue` / `tick_cast_effect` / `spawn_event_fx`） | 2026-11-07 | 自有 | 已提供；P-V3 补齐 4 技能差异化与 14 类事件视觉，全部经材质池复用，见 `docs/06_VISUAL_SPEC.md` §11 |
| A-005 | UI 字形、面板与大厅 / 英雄档案 / 道具图鉴 / 商店界面 | 引擎内置字形（egui crate 内嵌数据）+ ASCII 文案 + 方块化面板；运行时可借用宿主 OS 中文字形注入 `FontDefinitions`（**不随包分发**），探测失败回退 ASCII；P-U1 新增三列大厅、图鉴与商店面板（`client/src/lobby_ui.rs`） | 2026-11-08 | 引擎依赖自带 | 已提供；大厅用锁定视口的顶层 `Window`（egui 0.36 的 `CentralPanel::show` 只接受嵌套 `&mut Ui`） |
| A-006 | 音效（8-bit 程序化合成，7 种） | 本项目代码程序化生成（`client/src/audio.rs` 的 `SFX_TABLE` / `synthesize` / `ChipSource` 自定义解码资产，内存 PCM） | 2026-11-08 | 自有 | 已提供；方波 / 三角 / 噪声 + ADSR + 4-bit 量化，零音频文件、零外部依赖；headless 静默降级，规范见 `docs/06_VISUAL_SPEC.md` §12.2.3 |
| A-007 | 配置表数值与地图数据 | 本项目自研占位（`gx-proto-content`、`game-core/src/map.rs`） | 2026-11-07 | 自有 | 已提供；数值需平衡验证；**禁止引用任何第三方数值表或遥测字典**（`DN-23` / `DN-29`） |
| A-008 | 英雄档案 / 道具展示元数据 / 模式选择展示数据 | 本项目代码程序化生成（`client/src/lobby.rs` 的 `HERO_PROFILES[6]` / `ITEM_PROFILES[12]` / `MatchModeExt`；数值派生自 `AbilityDef` 与 `ShopCatalog`） | 2026-11-08 | 自有 | 已提供；P-U1 落地，与数值表一一对应由单测锁定，零外部资源，见 `docs/06_VISUAL_SPEC.md` §13 |

> `TC-COMPLIANCE-02` 的「资产登记 100%」判定：A-001~A-008 全部为**已提供**，音频在 P-V4、大厅与信息层在 P-U1 由代码补齐；唯一**外部依赖**是运行时借用宿主 OS 中文字形（A-005，不随包分发，不构成发布资产）。`TC-CBT-12` 额外扫描发布树，断言不含任何外部字体/图片/音频二进制、根无资源目录、无字节内嵌宏，并要求 `ui.rs` 具备 ASCII 回退。注意：根 `ASSET_REGISTRY.md` 使用另一套编号，其 A-006（中文字形）仍标注**待资产 / 外部依赖**、A-007 为 8-bit 音频（已提供），编号差异在规格升版时统一。

## 2. 隔离的第三方资料（不进入发布包）

按 `DN-29` / `DEV-01`，以下目录已移入 `LEGAL/`，仅为合规留档与可区分性审查用途，**不得据此生成商品化数值表**，**不得进入发布包**，发布流水线须显式排除：

| 目录 | 内容 | 权属 |
|---|---|---|
| `LEGAL/assets/` | 第三方游戏截图/图标/资源映射、遥测样本、以及第三方《Fan Content Policy》文本 | 第三方权利人，版权声明见原目录 README |
| `LEGAL/dictionaries/` | 10 个遥测数据字典 JSON | 第三方权利人 |
| `LEGAL/schemas/` | 遥测 schema | 第三方权利人 |

`LEGAL/**` 与 `docs/**` 为禁词扫描白名单（经澄清 Q-10），扫描器见 `crates/gx-e2e/tests/compliance.rs`。

## 3. 后续新增资产的登记要求

1. 提交时同步更新本表，字段：资产 ID、类型、作者/来源、取得日期、许可文件位置、SHA-256；
2. 若引入字体或音频，须附许可文件到 `LEGAL/licenses/` 并在本表引用；**表现层默认仍须保持代码生成**；
3. CI 的 `TC-COMPLIANCE-02` 校验「登记表中标为已提供的资产」与「发布包内容」一致，`TC-CBT-12` 校验零外部资源路径；
4. 任何资产不得使用禁用词命名（见 `GX-ARCH-2026-001` §2.2）。
