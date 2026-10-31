//! 客户端 UI 系统

use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};


use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::*;
use game_core::net::EntityType;
use crate::networking::{ClientLobbyInfo, ReceivedSnapshot};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // bevy_egui 0.43 要求 UI 系统注册在 EguiPrimaryContextPass 调度下，
        // egui 会在该调度的 run_ui 中初始化字体，否则渲染线程会因字体未初始化而 panic
        app.add_systems(EguiPrimaryContextPass, (
            setup_cjk_font,
            draw_hud,
            draw_minimap,
            draw_ability_bar,
            draw_shop,
        ).chain());
    }
}

/// 加载系统中文字体。egui 默认字体不含 CJK 字形，否则中文会显示为方块（□□□）。
fn setup_cjk_font(mut contexts: EguiContexts, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };

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
            fonts
                .font_data
                .insert("cjk".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
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

fn draw_hud(
    mut contexts: EguiContexts,
    hero_query: Query<&Hero, With<LocalPlayer>>,
) {
    let ctx = contexts.ctx_mut().expect("no egui context");
    let Ok(hero) = hero_query.single() else { return };
    
    // 顶部血条/蓝条
    egui::Window::new("top_hud")
        .title_bar(false)
        .anchor(egui::Align2::LEFT_TOP, [5.0, 5.0])
        .show(ctx, |ui| {
        ui.horizontal(|ui| {
            // 左侧：己方血蓝条
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    // 血条
                    let health_pct = hero.health / hero.max_health;
                    ui.add(egui::ProgressBar::new(health_pct)
                        .fill(egui::Color32::from_rgb(200, 50, 50))
                        .text(format!("{:.0}/{:.0}", hero.health, hero.max_health))
                        .desired_width(200.0));
                    
                    // 蓝条
                    let energy_pct = hero.energy / hero.max_energy;
                    ui.add(egui::ProgressBar::new(energy_pct)
                        .fill(egui::Color32::from_rgb(50, 100, 200))
                        .text(format!("{:.0}/{:.0}", hero.energy, hero.max_energy))
                        .desired_width(150.0));
                    
                    // 等级和金币
                    ui.label(format!("{} (Lv.{})", hero.name, hero.level));
                    ui.label(format!("💰 {}", hero.gold));
                });
            });
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 右侧：小地图占位
                ui.group(|ui| {
                    ui.set_min_size(egui::vec2(150.0, 150.0));
                    ui.label("MINIMAP");
                });
            });
        });
    });
    
    // 战局开始提示
    egui::Area::new("match_banner".into())
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("战局开始！左键点地移动 · WASD 移动 · 鼠标瞄准 · 数字键释放技能 · 右键普攻")
                    .color(egui::Color32::WHITE)
                    .size(16.0),
            );
        });
}

fn draw_minimap(
    mut contexts: EguiContexts,
) {
    let ctx = contexts.ctx_mut().expect("no egui context");
    
    egui::Area::new("minimap".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 40.0))
        .show(ctx, |ui| {
            ui.group(|ui| {
                ui.set_min_size(egui::vec2(180.0, 180.0));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 180.0), egui::Sense::hover());
                let painter = ui.painter();
                
                // 绘制小地图背景
                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));

                // 绘制河道（居中竖直，位于面板内部，避免越界斜线）
                let cx = rect.center().x;
                painter.line_segment(
                    [egui::pos2(cx, rect.top()), egui::pos2(cx, rect.bottom())],
                    egui::Stroke::new(3.0, egui::Color32::from_rgb(50, 100, 200)),
                );

                // TODO: 绘制实体位置
            });
        });
}

fn draw_ability_bar(
    mut contexts: EguiContexts,
    received: Res<ReceivedSnapshot>,
    lobby: Res<ClientLobbyInfo>,
) {
    let ctx = contexts.ctx_mut().expect("no egui context");
    let Some(my_id) = lobby.my_net_id else { return };

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
                        let rect = resp.rect;
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            egui::Color32::from_black_alpha((ratio * 180.0) as u8),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{:.1}", cd),
                            egui::FontId::default(),
                            egui::Color32::WHITE,
                        );
                    }
                }
                // 召唤师技能占位
                ui.add_sized([64.0, 64.0], egui::Button::new("B\n回城"));
                ui.add_sized([64.0, 64.0], egui::Button::new("F\n闪现"));
            });
        });
}

fn draw_shop(
    mut contexts: EguiContexts,
    hero_query: Query<&Hero, With<LocalPlayer>>,
) {
    let ctx = contexts.ctx_mut().expect("no egui context");
    let Ok(hero) = hero_query.single() else { return };
    
    // 按 'P' 打开商店
    // TODO: 实现商店 UI
    egui::Window::new("Shop (Press P)")
        .show(ctx, |ui| {
            ui.label(format!("Gold: {}", hero.gold));
            ui.separator();
            ui.label("Items:");
            // TODO: 物品列表
        });
}
