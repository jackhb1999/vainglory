//! 客户端 UI 系统（egui）
//!
//! 包含：左上角 HUD（血/蓝/等级/金币）、底部 QWER + 召唤师技能栏、
//! 右上角小地图（绘制折叠线/单条兵线/塔/基地/金矿/实体）、商店占位窗口、输入调试面板。

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPrimaryContextPass, egui};

use crate::AppState;
use crate::input::{PlayerInputState, SelectedTarget};
use crate::networking::{ClientLobbyInfo, ReceivedSnapshot};
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::*;
use game_core::map::*;
use game_core::net::EntityType;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // 关闭 bevy_egui 的“自动为第一个摄像机创建主 Egui 上下文”行为。
        // 项目里存在一个依赖插件在游戏摄像机之前就生成了一个没有渲染图的裸 Camera，
        // 自动创建会把它当成主上下文载体（无 CameraRenderGraph，UI 永远渲染不出来），
        // 而我们又在游戏摄像机上显式挂了 PrimaryEguiContext，导致出现两个共享同一
        // EguiPrimaryContextPass 调度的上下文 → panic。
        // 因此改为手动接管：仅在游戏摄像机上挂载 PrimaryEguiContext（见 rendering.rs）。
        app.insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        });

        // bevy_egui 0.43 要求 UI 系统注册在 EguiPrimaryContextPass 调度下，
        // egui 会在该调度的 run_ui 中初始化字体，否则渲染线程会因字体未初始化而 panic
        // 大厅共享资源与网络插件幂等注册，保证任一插件单独加载时资源都存在。
        app.init_resource::<UiLanguage>()
            .init_resource::<crate::lobby::LobbySelection>()
            .init_resource::<crate::lobby::ClientCommandQueue>()
            .init_resource::<crate::lobby::ShopToast>()
            .init_resource::<crate::lobby::CodexOpen>()
            .add_systems(
                EguiPrimaryContextPass,
                (
                    setup_cjk_font,
                    crate::lobby_ui::tick_shop_toast,
                    // 大厅：CentralPanel 铺满视口 + 道具图鉴窗口
                    crate::lobby_ui::draw_lobby.run_if(in_state(AppState::Lobby)),
                    crate::lobby_ui::draw_item_codex.run_if(in_state(AppState::Lobby)),
                    // 局内 HUD：仅在 InMatch 状态显示
                    draw_hud.run_if(in_state(AppState::InMatch)),
                    draw_minimap.run_if(in_state(AppState::InMatch)),
                    draw_hero_tags.run_if(in_state(AppState::InMatch)),
                    draw_ability_bar.run_if(in_state(AppState::InMatch)),
                    crate::lobby_ui::draw_shop.run_if(in_state(AppState::InMatch)),
                    draw_debug_panel.run_if(in_state(AppState::InMatch)),
                )
                    .chain(),
            );
    }
}

/// 当前 UI 是否具备中文字形。无 CJK 字体时所有文案自动回退到 ASCII，
/// 保证任何平台都不会出现「□□□」方块字。
#[derive(Resource, Default)]
pub struct UiLanguage {
    pub cjk: bool,
}

/// 按当前字形能力选择文案：`t(lang, "中文", "ASCII")`。
pub fn t(lang: &UiLanguage, zh: &'static str, en: &'static str) -> &'static str {
    if lang.cjk { zh } else { en }
}

/// 运行时探测操作系统自带的中文字体（**不随发布包分发任何字体文件**）。
/// 这是 PRES-3 v1.2 的唯一例外：几何/纹理/图标/特效仍全部由代码生成，
/// 只有字形借用宿主 OS 已有的字体，加载失败则回退到 egui 内置拉丁字形 + ASCII 文案。
fn setup_cjk_font(mut contexts: EguiContexts, mut done: Local<bool>, mut lang: ResMut<UiLanguage>) {
    if *done {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    *done = true;

    const CANDIDATES: &[&str] = &[
        // Windows
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/Deng.ttf",
        "C:/Windows/Fonts/simsun.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ];

    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        lang.cjk = true;
        info!("[UI] CJK glyphs enabled from OS font: {path}");
        return;
    }
    lang.cjk = false;
    info!("[UI] no OS CJK font found, falling back to ASCII glyphs");
}

/// 旧大厅实现已归档：`Area` + 手绘 painter 只有静态展示，
/// 无英雄档案与模式选择。现由 `lobby_ui::draw_lobby`（锁定视口的顶层
/// `Window` + 三列布局）取代，保留此函数仅供布局对照，不再注册为系统。
#[allow(dead_code)]
fn draw_lobby(
    mut contexts: EguiContexts,
    mut next: ResMut<NextState<AppState>>,
    lang: Res<UiLanguage>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let screen = ctx.input(|i| i.viewport_rect());
    let w = screen.width();
    let h = screen.height();
    let zh = lang.cjk;

    egui::Area::new("lobby_fullscreen".into())
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            // 取出 painter 的克隆，避免与下方 `ui.put`（需要 &mut ui）的借用冲突
            let p = ui.painter().clone();

            // 深色底 + 3v3 竞技场示意：单条 L 形兵线 + 野区折叠线
            p.rect_filled(screen, 0.0, egui::Color32::from_rgb(8, 10, 18));
            let lane_stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(26, 38, 60));
            // 单条兵线：左下 → 左侧边线 → 顶部边线 → 右上（与游戏内路点同构）
            let lane_pts = vec![
                egui::pos2(screen.left() + w * 0.10, screen.top() + h * 0.90),
                egui::pos2(screen.left() + w * 0.10, screen.top() + h * 0.18),
                egui::pos2(screen.right() - w * 0.10, screen.top() + h * 0.18),
                egui::pos2(screen.right() - w * 0.08, screen.top() + h * 0.08),
            ];
            p.add(egui::Shape::line(lane_pts, lane_stroke));
            // 野区折叠线（左上 → 右下的镜像对称轴）
            p.line_segment(
                [
                    egui::pos2(screen.left() + w * 0.2, screen.top() + h * 0.22),
                    egui::pos2(screen.right() - w * 0.2, screen.bottom() - h * 0.08),
                ],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(18, 42, 78)),
            );
            // 双方基地示意
            p.circle_filled(
                egui::pos2(screen.left() + w * 0.12, screen.top() + h * 0.86),
                w.min(h) * 0.035,
                egui::Color32::from_rgb(46, 96, 214),
            );
            p.circle_filled(
                egui::pos2(screen.left() + w * 0.88, screen.top() + h * 0.14),
                w.min(h) * 0.035,
                egui::Color32::from_rgb(200, 56, 52),
            );
            // 地图中心金矿 / 海妖坑
            p.circle_stroke(
                egui::pos2(screen.center().x, screen.top() + h * 0.62),
                w.min(h) * 0.03,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(226, 184, 62)),
            );

            let title_pos = egui::pos2(screen.center().x, screen.top() + h * 0.14);
            p.text(
                title_pos,
                egui::Align2::CENTER_CENTER,
                if zh { "灰界裂谷" } else { "GREY RIFT" },
                egui::FontId::proportional(58.0),
                egui::Color32::from_rgb(236, 214, 148),
            );
            p.text(
                egui::pos2(screen.center().x, screen.top() + h * 0.205),
                egui::Align2::CENTER_CENTER,
                if zh {
                    "体素战场 · 3v3 单路 + 野区"
                } else {
                    "VOXEL ARENA - 3V3 SINGLE LANE"
                },
                egui::FontId::proportional(18.0),
                egui::Color32::from_rgb(150, 170, 200),
            );

            // 模式卡片
            let mode_rect = egui::Rect::from_center_size(
                egui::pos2(screen.center().x, screen.top() + h * 0.30),
                egui::vec2(520.0, 56.0),
            );
            p.rect_filled(
                mode_rect,
                6.0,
                egui::Color32::from_rgba_unmultiplied(30, 40, 62, 220),
            );
            p.rect_stroke(
                mode_rect,
                6.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 110, 160)),
                egui::StrokeKind::Inside,
            );
            p.text(
                mode_rect.center(),
                egui::Align2::CENTER_CENTER,
                if zh {
                    "模式：PvE 训练 · 单路兵线 + 野区 · 单人可进入"
                } else {
                    "MODE: PVE TRAINING - SINGLE LANE + JUNGLE - SOLO READY"
                },
                egui::FontId::proportional(17.0),
                egui::Color32::from_rgb(210, 225, 245),
            );

            // 英雄阵容卡（突出“不是只靠血条辨认”的外观差异）
            let cards = [
                (
                    if zh { "剑盾先锋" } else { "VANGUARD" },
                    if zh {
                        "金冠 / 长剑 / 厚甲"
                    } else {
                        "CROWN / SWORD / PLATE"
                    },
                    egui::Color32::from_rgb(78, 132, 240),
                ),
                (
                    if zh { "赤角狂战" } else { "BERSERKER" },
                    if zh {
                        "双角 / 战斧 / 红甲"
                    } else {
                        "HORNS / AXE / RED"
                    },
                    egui::Color32::from_rgb(232, 84, 76),
                ),
                (
                    if zh { "紫帽术士" } else { "WARLOCK" },
                    if zh {
                        "紫帽 / 法杖 / 披风"
                    } else {
                        "HAT / STAFF / CAPE"
                    },
                    egui::Color32::from_rgb(182, 130, 236),
                ),
            ];
            let card_w = 176.0;
            let card_h = 150.0;
            let gap = 22.0;
            let total_w = cards.len() as f32 * card_w + (cards.len() - 1) as f32 * gap;
            let start_x = screen.center().x - total_w * 0.5;
            for (i, (name, desc, color)) in cards.iter().enumerate() {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(start_x + i as f32 * (card_w + gap), screen.top() + h * 0.40),
                    egui::vec2(card_w, card_h),
                );
                p.rect_filled(rect, 8.0, egui::Color32::from_rgb(20, 25, 40));
                p.rect_stroke(
                    rect,
                    8.0,
                    egui::Stroke::new(1.5, *color),
                    egui::StrokeKind::Inside,
                );
                // 方块英雄剪影
                let cx = rect.center().x;
                let base_y = rect.top() + 44.0;
                p.rect_filled(
                    egui::Rect::from_center_size(egui::pos2(cx, base_y), egui::vec2(54.0, 54.0)),
                    0.0,
                    *color,
                );
                p.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(cx, base_y - 42.0),
                        egui::vec2(34.0, 34.0),
                    ),
                    0.0,
                    egui::Color32::from_rgb(226, 184, 62),
                );
                p.text(
                    egui::pos2(cx, rect.top() + 98.0),
                    egui::Align2::CENTER_CENTER,
                    *name,
                    egui::FontId::proportional(18.0),
                    egui::Color32::WHITE,
                );
                p.text(
                    egui::pos2(cx, rect.top() + 122.0),
                    egui::Align2::CENTER_CENTER,
                    *desc,
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_rgb(150, 165, 190),
                );
            }

            // 进入对局按钮
            let btn_rect = egui::Rect::from_center_size(
                egui::pos2(screen.center().x, screen.top() + h * 0.76),
                egui::vec2(260.0, 56.0),
            );
            let resp = ui.put(
                btn_rect,
                egui::Button::new(
                    egui::RichText::new(if zh { "进 入 对 局" } else { "ENTER MATCH" })
                        .size(20.0)
                        .strong()
                        .color(egui::Color32::from_rgb(18, 16, 8)),
                )
                .fill(egui::Color32::from_rgb(226, 184, 62)),
            );
            if resp.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                next.set(AppState::InMatch);
            }

            p.text(
                egui::pos2(screen.center().x, screen.top() + h * 0.845),
                egui::Align2::CENTER_CENTER,
                if zh {
                    "左键点击地面移动 · WASD/方向键也移动 · 右键锁定攻击 · Q/E/R/F 技能 · P 商店"
                } else {
                    "LMB click: move - WASD: move - RMB: lock - Q/E/R/F skills - P shop"
                },
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(130, 145, 170),
            );
            p.text(
                egui::pos2(screen.center().x, screen.top() + h * 0.88),
                egui::Align2::CENTER_CENTER,
                if zh {
                    "进入后自动连接本地服务器 127.0.0.1:5000"
                } else {
                    "Auto-connect to 127.0.0.1:5000 after entering"
                },
                egui::FontId::proportional(13.0),
                egui::Color32::from_rgb(96, 112, 140),
            );
        });
}

/// 字体探测候选是否覆盖了三个主流平台（供门禁与自检使用）。
#[cfg(test)]
pub const CJK_FONT_PLATFORMS: usize = 3;

/// 团队颜色（用于 HUD 头像与小地图）。
fn team_color(team: Team) -> egui::Color32 {
    match team {
        Team::Blue => egui::Color32::from_rgb(60, 130, 240),
        Team::Red => egui::Color32::from_rgb(240, 70, 70),
        Team::Neutral => egui::Color32::from_rgb(220, 200, 60),
    }
}

// ============================================================
// 1. 左上角 HUD：头像 / 血条 / 蓝条 / 等级 / 金币
// ============================================================
fn draw_hud(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    selection: Res<crate::lobby::LobbySelection>,
    hero_query: Query<(&Hero, &Team), With<LocalPlayer>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok((hero, team)) = hero_query.single() else {
        return;
    };

    let portrait = team_color(*team);

    egui::Window::new("top_hud")
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_TOP, [5.0, 5.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 头像（队伍色圆 + 首字母）
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(54.0, 54.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 26.0, portrait);
                ui.painter().circle_stroke(
                    rect.center(),
                    26.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    hero.name.chars().next().unwrap_or('?').to_string(),
                    egui::FontId::proportional(22.0),
                    egui::Color32::WHITE,
                );

                ui.vertical(|ui| {
                    // 大厅所选英雄档案标识（名字 / 定位 / 模式）
                    crate::lobby_ui::hero_badge(ui, &lang, &selection);
                    // 血条
                    let hp_pct = (hero.health / hero.max_health).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(hp_pct)
                            .fill(egui::Color32::from_rgb(200, 50, 50))
                            .text(format!(
                                "{:.0} / {:.0}",
                                hero.health.max(0.0),
                                hero.max_health
                            ))
                            .desired_width(220.0),
                    );
                    // 蓝条
                    let en_pct = (hero.energy / hero.max_energy).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(en_pct)
                            .fill(egui::Color32::from_rgb(50, 100, 220))
                            .text(format!(
                                "{:.0} / {:.0}",
                                hero.energy.max(0.0),
                                hero.max_energy
                            ))
                            .desired_width(220.0),
                    );
                    // 等级 / 金币 / 攻击力
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Lv.{}", hero.level))
                                .color(egui::Color32::YELLOW)
                                .strong(),
                        );
                        ui.label(format!("{} {}", t(&lang, "金币", "GOLD"), hero.gold));
                        ui.label(format!(
                            "{} {:.0}",
                            t(&lang, "攻击", "AD"),
                            hero.attack_damage
                        ));
                        ui.label(format!("{} {:.0}", t(&lang, "护甲", "ARM"), hero.armor));
                    });
                });
            });
        });
}

// ============================================================
// 1.5 世界空间英雄姓名 / 血量标签
// ============================================================
/// 在所有英雄头顶绘制姓名 + 等级 + 血量，配合体素外观（头饰/武器/配色/脚环）
/// 让玩家不必只依赖血条就能快速认出不同角色。
fn draw_hero_tags(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    heroes: Query<(&Hero, &Team, &GlobalTransform, Option<&LocalPlayer>)>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok((camera, cam_xform)) = camera.single() else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("hero_tags"),
    ));
    let screen = ctx.input(|i| i.viewport_rect());

    for (hero, team, gt, is_local) in heroes.iter() {
        if hero.health <= 0.0 {
            continue;
        }
        let world = gt.translation() + Vec3::Y * 5.15;
        let Ok(pos) = camera.world_to_viewport(cam_xform, world) else {
            continue;
        };
        let pos = egui::pos2(pos.x, pos.y);
        if !screen.expand(40.0).contains(pos) {
            continue;
        }
        let color = team_color(*team);
        let star = if is_local.is_some() {
            if lang.cjk { "★ " } else { "* " }
        } else {
            ""
        };
        let hp_ratio = if hero.max_health > 0.0 {
            (hero.health / hero.max_health).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let rect = egui::Rect::from_center_size(pos, egui::vec2(142.0, 36.0));
        painter.rect_filled(
            rect,
            3.0,
            egui::Color32::from_rgba_unmultiplied(10, 12, 18, 200),
        );
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
        // 顶部细血条
        let inner = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 4.0, rect.top() + 3.0),
            egui::vec2((rect.width() - 8.0) * hp_ratio, 3.0),
        );
        painter.rect_filled(inner, 1.0, color);
        painter.text(
            egui::pos2(pos.x, pos.y + 4.0),
            egui::Align2::CENTER_CENTER,
            format!(
                "{}{}  Lv.{}\n{:.0} / {:.0}",
                star,
                hero.name,
                hero.level,
                hero.health.max(0.0),
                hero.max_health
            ),
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(232, 238, 248),
        );
    }
}

// ============================================================
// 2. 右上角小地图：折叠线 / 单条兵线 / 塔 / 基地 / 金矿 / 实体
// ============================================================

/// 世界坐标 (x,z) → 小地图像素坐标。地图范围与 `game_core::map` 保持一致。
const MINIMAP_HALF: f32 = MAP_WIDTH * 0.5;
fn to_minimap(x: f32, z: f32, rect: egui::Rect) -> egui::Pos2 {
    let nx = ((x / MINIMAP_HALF) * 0.5 + 0.5).clamp(0.0, 1.0);
    let nz = ((z / MINIMAP_HALF) * 0.5 + 0.5).clamp(0.0, 1.0);
    egui::pos2(
        rect.left() + nx * rect.width(),
        rect.top() + nz * rect.height(),
    )
}

fn draw_minimap(
    mut contexts: EguiContexts,
    received: Res<ReceivedSnapshot>,
    lobby: Res<ClientLobbyInfo>,
    mut ran: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !*ran {
        info!("[UI] minimap draw system running");
        *ran = true;
    }
    let size = 190.0;

    egui::Area::new("minimap".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 40.0))
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            let painter = ui.painter();

            // 背景
            painter.rect_filled(rect, 4.0, egui::Color32::from_gray(25));

            // 野区折叠线（3v3 竞技场的镜像对称轴，`z = x` 对角线），与 3D 地图同源
            let (fold_a, fold_b) = fold_line_endpoints();
            painter.line_segment(
                [
                    to_minimap(fold_a.x, fold_a.z, rect),
                    to_minimap(fold_b.x, fold_b.z, rect),
                ],
                egui::Stroke::new(1.2, egui::Color32::from_rgb(70, 80, 110)),
            );

            // 金矿 / 海妖坑（地图正中心，金色菱形）
            let gm = to_minimap(GOLD_MINE_POS.x, GOLD_MINE_POS.z, rect);
            painter.circle_filled(gm, 5.0, egui::Color32::from_rgb(226, 184, 62));
            painter.circle_stroke(
                gm,
                7.5,
                egui::Stroke::new(1.5, egui::Color32::from_gray(60)),
            );

            // 兵线（来自 minion_paths）
            let lane = egui::Stroke::new(1.5, egui::Color32::from_gray(90));
            for path in minion_paths() {
                let pts: Vec<egui::Pos2> = path
                    .waypoints
                    .iter()
                    .map(|w| to_minimap(w.x, w.z, rect))
                    .collect();
                painter.line(pts, lane);
            }

            // 草丛（淡绿圈）
            for b in BUSHES {
                let c = to_minimap(b.center.x, b.center.z, rect);
                let rad = (b.radius / MAP_WIDTH) * rect.width();
                painter.circle_stroke(
                    c,
                    rad,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 160, 70)),
                );
            }

            // 野怪营地（紫点）
            for c in JUNGLE_CAMPS {
                let p = to_minimap(c.position.x, c.position.z, rect);
                painter.circle_filled(p, 2.5, egui::Color32::from_rgb(150, 90, 200));
            }

            // 防御塔（按队伍上色的小方块）
            for t in TURRET_POSITIONS {
                let p = to_minimap(t.position.x, t.position.z, rect);
                painter.rect_filled(
                    egui::Rect::from_center_size(p, egui::vec2(6.0, 6.0)),
                    1.0,
                    team_color(t.team),
                );
            }

            // 基地 / 源核（菱形）
            for (pos, team) in [(BLUE_SOURCE_CORE, Team::Blue), (RED_SOURCE_CORE, Team::Red)] {
                let p = to_minimap(pos.x, pos.z, rect);
                painter.circle_filled(p, 6.0, team_color(team));
                painter.circle_stroke(p, 7.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
            }

            // 实体（英雄/小兵/塔等），按队伍上色
            let my_id = lobby.my_net_id;
            for e in &received.0.entities {
                let p = to_minimap(e.position.x, e.position.z, rect);
                let color = match e.team {
                    Team::Blue => egui::Color32::from_rgb(60, 130, 240),
                    Team::Red => egui::Color32::from_rgb(240, 70, 70),
                    Team::Neutral => egui::Color32::from_rgb(220, 200, 60),
                };
                match e.entity_type {
                    EntityType::Hero => {
                        painter.circle_filled(p, 4.0, color);
                        if my_id == Some(e.net_id) {
                            painter.circle_stroke(
                                p,
                                7.0,
                                egui::Stroke::new(2.0, egui::Color32::WHITE),
                            );
                        }
                    }
                    EntityType::Minion => {
                        painter.circle_filled(p, 2.0, color);
                    }
                    EntityType::Turret => {
                        painter.rect_filled(
                            egui::Rect::from_center_size(p, egui::vec2(5.0, 5.0)),
                            1.0,
                            color,
                        );
                    }
                    _ => {
                        painter.circle_filled(p, 1.5, color);
                    }
                }
            }
        });
}

// ============================================================
// 3. 底部 QWER + 回城/召唤师技能栏
// ============================================================
fn draw_ability_bar(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    received: Res<ReceivedSnapshot>,
    lobby: Res<ClientLobbyInfo>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Some(my_id) = lobby.my_net_id else {
        return;
    };

    // 取本地英雄在最新快照中的冷却数据
    let cooldowns = received
        .0
        .entities
        .iter()
        .find(|e| e.net_id == my_id && e.entity_type == EntityType::Hero)
        .map(|e| e.ability_cooldowns)
        .unwrap_or([0.0; 4]);
    let ability_ids = default_hero_abilities();
    let keys = ['Q', 'E', 'R', 'F'];

    egui::Window::new("ability_bar")
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -10.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for i in 0..4usize {
                    let def = get_ability_def(ability_ids[i]);
                    let cd = cooldowns[i];
                    let ratio = if def.cooldown > 0.0 {
                        (cd / def.cooldown).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    let note = if lang.cjk {
                        crate::lobby::ABILITY_NOTES_ZH[i]
                    } else {
                        crate::lobby::ABILITY_NOTES_EN[i]
                    };
                    let resp = ui
                        .add_sized(
                            [64.0, 64.0],
                            egui::Button::new(format!("{}\n{}", keys[i], def.name)),
                        )
                        .on_hover_text(format!(
                            "{} · CD {:.0}s · {} {:.0}\n{note}",
                            def.name,
                            def.cooldown,
                            t(&lang, "耗蓝", "MP"),
                            def.mana_cost
                        ));
                    if cd > 0.0 {
                        // 冷却遮罩（自上而下变亮表示冷却结束）
                        let r = resp.rect;
                        ui.painter().rect_filled(
                            r,
                            0.0,
                            egui::Color32::from_black_alpha((ratio * 180.0) as u8),
                        );
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{:.1}", cd),
                            egui::FontId::default(),
                            egui::Color32::WHITE,
                        );
                    }
                }

                ui.separator();

                // 召唤师技能栏：回城(B) + 闪现
                ui.add_sized([64.0, 64.0], egui::Button::new("B\nRECALL"));
                ui.add_sized([64.0, 64.0], egui::Button::new("F\nBLINK"));
            });
        });
}

// ============================================================
// 4. 旧商店占位窗口已归档，由 `lobby_ui::draw_shop` 取代（真实 12 件道具 + 购买/售出）
// ============================================================
#[allow(dead_code)]
fn draw_shop(mut contexts: EguiContexts, mut open: Local<bool>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    if ctx.input(|i| i.key_pressed(egui::Key::P)) {
        *open = !*open;
    }
    if !*open {
        return;
    }

    egui::Window::new("SHOP (P)")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("SHOP (PLACEHOLDER)")
                    .size(18.0)
                    .strong(),
            );
            ui.separator();
            ui.label("Buy items with gold. Full catalog: GX-CBT-2026-001 section 10.");
            ui.separator();
            egui::Grid::new("shop_grid")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for i in 0..8 {
                        ui.group(|ui| {
                            ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::click());
                            ui.label(format!("ITEM {}", i + 1));
                        });
                    }
                });
            ui.separator();
            if ui.button("BUY (PLACEHOLDER)").clicked() {
                info!("[SHOP] placeholder buy clicked");
            }
        });
}

// ============================================================
// 5. 输入调试面板（左侧中部）
// ============================================================

/// 画一个小摇杆指示（中心圆 + 方向点）。dir 为归一化方向。
fn draw_stick(ui: &mut egui::Ui, label: &str, dir: Vec2, color: egui::Color32) {
    ui.label(label);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
    let p = ui.painter();
    p.circle_stroke(
        rect.center(),
        22.0,
        egui::Stroke::new(1.0, egui::Color32::GRAY),
    );
    let r = 18.0;
    let dx = dir.x.clamp(-1.0, 1.0) * r;
    let dy = -dir.y.clamp(-1.0, 1.0) * r; // 屏幕 y 轴向下，取反
    p.circle_filled(
        egui::pos2(rect.center().x + dx, rect.center().y + dy),
        4.0,
        color,
    );
}

fn draw_debug_panel(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    input: Res<PlayerInputState>,
    selected: Res<SelectedTarget>,
    lobby: Res<ClientLobbyInfo>,
    received: Res<ReceivedSnapshot>,
    hero_query: Query<&Hero, With<LocalPlayer>>,
    time: Res<Time>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mode = t(&lang, "PvE 训练", "PVE TRAINING");
    egui::Window::new(t(&lang, "输入调试", "INPUT DEBUG"))
        .title_bar(true)
        .anchor(egui::Align2::LEFT_CENTER, [5.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(220.0);
            ui.label(egui::RichText::new(mode).strong());

            ui.horizontal(|ui| {
                draw_stick(
                    ui,
                    "PATH",
                    input.move_dir,
                    egui::Color32::from_rgb(80, 200, 120),
                );
                draw_stick(
                    ui,
                    "AIM",
                    input.aim_dir,
                    egui::Color32::from_rgb(240, 200, 80),
                );
            });

            ui.separator();

            let m = input.ability_mask;
            ui.label(format!(
                "ability_mask: {:#010b}  (RMB attack:{})",
                m,
                if m & 0b0001 != 0 { "ON" } else { "OFF" }
            ));
            ui.horizontal(|ui| {
                for (i, k) in ['Q', 'E', 'R', 'F'].iter().enumerate() {
                    let on = m & (1 << (i + 1)) != 0;
                    ui.label(format!("{}:{}", k, if on { "1" } else { "0" }));
                }
            });

            ui.separator();

            ui.label(format!(
                "LOCKED TARGET: {:?}",
                input.target_net_id.map(|n| n.0)
            ));
            ui.label(format!("SELECTED: {:?}", selected.0.map(|n| n.0)));
            ui.label(format!("MY NetId: {:?}", lobby.my_net_id.map(|n| n.0)));

            ui.separator();

            if let Ok(hero) = hero_query.single() {
                ui.label(format!(
                    "HP {:.0}/{:.0}  EN {:.0}/{:.0}",
                    hero.health, hero.max_health, hero.energy, hero.max_energy
                ));
                ui.label(format!(
                    "Lv.{}  GOLD {}  AS {:.2}  CD {:.2}s",
                    hero.level, hero.gold, hero.attack_speed, hero.attack_cooldown
                ));
            }

            let fps = 1.0 / time.delta_secs().max(1e-4);
            ui.label(format!("FPS: {:.0}", fps));
            ui.label(format!("SNAPSHOT ENTITIES: {}", received.0.entities.len()));
        });
}
