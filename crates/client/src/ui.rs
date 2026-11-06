//! 客户端 UI 系统（egui）
//!
//! 包含：左上角 HUD（血/蓝/等级/金币）、底部 QWER + 召唤师技能栏、
//! 右上角小地图（绘制河道/兵线/塔/基地/实体）、商店占位窗口、输入调试面板。

use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiGlobalSettings, EguiPrimaryContextPass};

use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::*;
use game_core::map::*;
use game_core::net::EntityType;
use crate::input::{PlayerInputState, SelectedTarget};
use crate::networking::{ClientLobbyInfo, ReceivedSnapshot};

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
        app.add_systems(
            EguiPrimaryContextPass,
            (
                setup_cjk_font,
                draw_hud,
                draw_minimap,
                draw_ability_bar,
                draw_shop,
                draw_debug_panel,
            ),
        );
    }
}

/// 加载系统中文字体。egui 默认字体不含 CJK 字形，否则中文会显示为方块（□□□）。
fn setup_cjk_font(mut contexts: EguiContexts, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // 优先单个 TTF（ab_glyph 对 TTC 合集支持不佳），Windows 常见中文字体
    const CANDIDATES: &[&str] = &[
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/Deng.ttf",
        "C:/Windows/Fonts/simkai.ttf",
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/simsun.ttc",
    ];

    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "cjk".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            // 作为回退字体追加到两个字族末尾（拉丁仍用默认字体，中文回退到 cjk）
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".to_owned());
            }
            ctx.set_fonts(fonts);
            info!("已加载中文字体: {path}");
            break;
        }
    }
    // 无论是否找到，都标记完成，避免每帧重复尝试
    *done = true;
}

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
                        ui.label(format!("💰 {}", hero.gold));
                        ui.label(format!("⚔ {:.0}", hero.attack_damage));
                        ui.label(format!("🛡 {:.0}", hero.armor));
                    });
                });
            });
        });
}

// ============================================================
// 2. 右上角小地图：河道 / 兵线 / 塔 / 基地 / 实体
// ============================================================

/// 世界坐标 (x,z) → 小地图像素坐标。地图范围取 [-64, 64]。
const MINIMAP_HALF: f32 = 64.0;
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
        info!("[UI] draw_minimap 系统已运行（egui 渲染管线正常）");
        *ran = true;
    }
    let size = 190.0;

    egui::Area::new("minimap".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 40.0))
        .show(ctx, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            let painter = ui.painter();

            // 背景
            painter.rect_filled(rect, 4.0, egui::Color32::from_gray(25));

            // 河道（沿 x=z 的对角带，分隔蓝方与红方）
            let r1 = to_minimap(-MINIMAP_HALF, -MINIMAP_HALF, rect);
            let r2 = to_minimap(MINIMAP_HALF, MINIMAP_HALF, rect);
            let river_w = (RIVER_WIDTH / MAP_WIDTH) * rect.width();
            painter.line_segment(
                [r1, r2],
                egui::Stroke::new(river_w, egui::Color32::from_rgb(40, 90, 170)),
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
                let rad = (b.radius / MINIMAP_HALF) * rect.width();
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

            // 基地 / Vain Crystal（菱形）
            for (pos, team) in [
                (BLUE_VAIN_CRYSTAL, Team::Blue),
                (RED_VAIN_CRYSTAL, Team::Red),
            ] {
                let p = to_minimap(pos.x, pos.z, rect);
                painter.circle_filled(p, 6.0, team_color(team));
                painter.circle_stroke(
                    p,
                    7.0,
                    egui::Stroke::new(1.5, egui::Color32::WHITE),
                );
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

                    let resp = ui.add_sized(
                        [64.0, 64.0],
                        egui::Button::new(format!("{}\n{}", keys[i], def.name)),
                    );
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
                ui.add_sized([64.0, 64.0], egui::Button::new("B\n回城"));
                ui.add_sized([64.0, 64.0], egui::Button::new("F\n闪现"));
            });
        });
}

// ============================================================
// 4. 商店占位窗口（按 P 开/关）
// ============================================================
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

    egui::Window::new("商店 (P)")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("🛒 商店（占位）").size(18.0).strong());
            ui.separator();
            ui.label("这里将展示可购买装备列表，点击购买后扣除金币并应用属性。");
            ui.separator();
            // 占位物品格子
            egui::Grid::new("shop_grid")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for i in 0..8 {
                        ui.group(|ui| {
                            ui.allocate_exact_size(
                                egui::vec2(96.0, 96.0),
                                egui::Sense::click(),
                            );
                            ui.label(format!("物品 {}", i + 1));
                        });
                    }
                });
            ui.separator();
            if ui.button("购买选中（占位）").clicked() {
                info!("[SHOP] 购买占位点击");
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
    p.circle_stroke(rect.center(), 22.0, egui::Stroke::new(1.0, egui::Color32::GRAY));
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

    egui::Window::new("输入调试")
        .title_bar(true)
        .anchor(egui::Align2::LEFT_CENTER, [5.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(220.0);

            // 移动 / 瞄准摇杆
            ui.horizontal(|ui| {
                draw_stick(ui, "移动", input.move_dir, egui::Color32::from_rgb(80, 200, 120));
                draw_stick(ui, "瞄准", input.aim_dir, egui::Color32::from_rgb(240, 200, 80));
            });

            ui.separator();

            // ability_mask 位解析
            let m = input.ability_mask;
            ui.label(format!(
                "ability_mask: {:#010b}  (右键攻击:{})",
                m,
                if m & 0b0001 != 0 { "开" } else { "关" }
            ));
            ui.horizontal(|ui| {
                for (i, k) in ['Q', 'E', 'R', 'F'].iter().enumerate() {
                    let on = m & (1 << (i + 1)) != 0;
                    ui.label(format!(
                        "{}:{}",
                        k,
                        if on { "●" } else { "○" }
                    ));
                }
            });

            ui.separator();

            ui.label(format!(
                "锁定普攻目标: {:?}",
                input.target_net_id.map(|n| n.0)
            ));
            ui.label(format!("选中目标(点击): {:?}", selected.0.map(|n| n.0)));
            ui.label(format!("我的 NetId: {:?}", lobby.my_net_id.map(|n| n.0)));

            ui.separator();

            if let Ok(hero) = hero_query.single() {
                ui.label(format!(
                    "HP {:.0}/{:.0}  EN {:.0}/{:.0}",
                    hero.health, hero.max_health, hero.energy, hero.max_energy
                ));
                ui.label(format!(
                    "Lv.{}  💰{}  攻速{:.2}  CD{:.2}s",
                    hero.level, hero.gold, hero.attack_speed, hero.attack_cooldown
                ));
            }

            let fps = 1.0 / time.delta_secs().max(1e-4);
            ui.label(format!("FPS: {:.0}", fps));
            ui.label(format!("快照实体数: {}", received.0.entities.len()));
        });
}
