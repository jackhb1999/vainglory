//! 大厅 / 英雄档案 / 道具图鉴 / 商店 UI。
//!
//! 与 `ui.rs` 的局内 HUD 分离：大厅用锁定到整个视口的顶层 `Window` 铺满
//! （旧实现用 `Area` + 手绘 painter，只有静态示意、缺少英雄与模式信息），
//! 所有文案与数据来自 `lobby.rs` 目录，数值来自 `ShopCatalog` / `AbilityDef`。
//!
//! UI 只把意图推入 `ClientCommandQueue`，由 `networking` 统一上行；服务端始终权威裁决。

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use game_core::abilities::{default_hero_abilities, get_ability_def};
use game_core::components::{Hero, LocalPlayer};
use game_core::gameplay::clock::MatchMode;
use game_core::gameplay::registry;
use game_core::gameplay::shop::{INVENTORY_SLOTS, ItemDef, ShopCatalog};

use crate::AppState;
use crate::lobby::{
    ABILITY_NOTES_EN, ABILITY_NOTES_ZH, ClientCommand, ClientCommandQueue, CodexOpen,
    HERO_PROFILES, ITEM_CATEGORIES, LobbySelection, MatchModeExt, RATING_LABELS_EN,
    RATING_LABELS_ZH, SKILL_KEYS, ShopToast, hero_profile, item_profile, stats_text,
};
use crate::ui::{UiLanguage, t};

const GOLD: egui::Color32 = egui::Color32::from_rgb(226, 184, 62);
const MUTED: egui::Color32 = egui::Color32::from_rgb(146, 162, 188);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(78, 132, 240);

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(17.0).strong().color(GOLD));
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).size(11.5).color(MUTED));
}

/// 道具卡片：图鉴与商店共用同一份渲染，保证两处信息一字不差。
fn item_card(
    ui: &mut egui::Ui,
    d: &ItemDef,
    cjk: bool,
    lang: &UiLanguage,
    mut queue: Option<&mut ClientCommandQueue>,
) {
    let Some(p) = item_profile(d.id) else {
        return;
    };
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if cjk { p.name_zh } else { p.name_en })
                    .size(14.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(p.category.label(cjk))
                    .size(10.0)
                    .color(ACCENT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(q) = queue.as_mut()
                    && ui.button(t(lang, "购买", "BUY")).clicked()
                {
                    q.0.push(ClientCommand::Purchase(registry::item_id(d.id)));
                }
                ui.label(
                    egui::RichText::new(format!("{} G", d.price))
                        .strong()
                        .color(GOLD),
                );
            });
        });
        let stats = stats_text(&d.stats, cjk);
        if !stats.is_empty() {
            ui.label(egui::RichText::new(stats.join("   ")).size(11.5));
        } else {
            muted(
                ui,
                t(
                    lang,
                    "无属性加成（主动效果）",
                    "No passive stats (active item)",
                ),
            );
        }
        if let Some(from) = d.from {
            let from_name = item_profile(from)
                .map(|f| if cjk { f.name_zh } else { f.name_en })
                .unwrap_or(from);
            muted(
                ui,
                format!("{} {from_name}", t(lang, "合成前置：", "BUILD FROM: ")),
            );
        }
        muted(
            ui,
            if cjk {
                p.description_zh
            } else {
                p.description_en
            },
        );
    });
}

/// 英雄评分条：只做定性展示，不伪造局内数值。
fn rating_rows(ui: &mut egui::Ui, ratings: &[u8; 5], cjk: bool) {
    let labels = if cjk {
        RATING_LABELS_ZH
    } else {
        RATING_LABELS_EN
    };
    for (i, &v) in ratings.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(labels[i]).size(11.0).color(MUTED));
            ui.add(
                egui::ProgressBar::new(v as f32 / 5.0)
                    .desired_width(96.0)
                    .fill(ACCENT),
            );
        });
    }
}

/// 左列：模式选择 + 进入对局 + 操作说明。
fn draw_mode_column(
    ui: &mut egui::Ui,
    lang: &UiLanguage,
    selection: &mut LobbySelection,
    next: &mut NextState<AppState>,
) {
    let cjk = lang.cjk;
    heading(ui, t(lang, "对局模式", "MATCH MODE"));
    ui.add_space(6.0);
    ui.vertical(|ui| {
        for mode in [MatchMode::Pve, MatchMode::Pvp] {
            let resp = ui.selectable_value(&mut selection.mode, mode, mode.label(cjk));
            if resp.clicked() {
                info!("[lobby] mode -> {:?}", mode);
            }
        }
    });
    ui.add_space(4.0);
    muted(ui, selection.mode.detail(cjk));
    ui.add_space(4.0);
    muted(
        ui,
        t(
            lang,
            "PVE 单人可开局；PVP 需两名真人连接后自动开始。",
            "PVE starts solo; PVP waits for 2 human players.",
        ),
    );
    ui.add_space(16.0);
    heading(ui, t(lang, "操作提示", "CONTROLS"));
    muted(
        ui,
        t(
            lang,
            "左键点击地面 移动（自动寻路）· WASD/方向键 直接移动",
            "LMB click ground to move · WASD/Arrows also move",
        ),
    );
    muted(ui, t(lang, "右键  锁定普攻", "RMB  attack"));
    muted(ui, t(lang, "Q / E / R / F  技能", "Q / E / R / F  skills"));
    muted(ui, t(lang, "P  商店 · B  回城", "P  shop · B  recall"));
    ui.add_space(20.0);
    let btn = ui.add_sized(
        [180.0, 44.0],
        egui::Button::new(
            egui::RichText::new(t(lang, "进 入 对 局", "ENTER MATCH"))
                .size(17.0)
                .strong()
                .color(egui::Color32::from_rgb(18, 16, 8)),
        )
        .fill(GOLD),
    );
    if btn.clicked() {
        next.set(AppState::InMatch);
    }
    ui.add_space(8.0);
    muted(
        ui,
        t(
            lang,
            "进入后连接本地服务器 127.0.0.1:5000",
            "Connects to 127.0.0.1:5000 on enter",
        ),
    );
}

/// 中列：6 位英雄的可选卡片。
fn draw_hero_list(ui: &mut egui::Ui, lang: &UiLanguage, selection: &mut LobbySelection) {
    let cjk = lang.cjk;
    heading(ui, t(lang, "英雄档案", "HERO ROSTER"));
    ui.add_space(6.0);
    for h in &HERO_PROFILES {
        let label = format!(
            "{:02}  {}   {}",
            h.index + 1,
            if cjk { h.name_zh } else { h.name_en },
            if cjk { h.role_zh } else { h.role_en }
        );
        if ui
            .selectable_value(&mut selection.hero_index, h.index, label)
            .clicked()
        {
            info!("[lobby] hero -> {}", h.name_en);
        }
        ui.add_space(2.0);
    }
    ui.add_space(12.0);
    muted(
        ui,
        t(
            lang,
            "剪影由造型表驱动；评分为定位示意，局内数值以服务端快照为准。",
            "Silhouettes come from the visual table; ratings are qualitative.",
        ),
    );
}

/// 右列：所选英雄的完整档案（描述 / 评分 / 外观 / 技能 / 出装 / 打法）。
fn draw_hero_detail(ui: &mut egui::Ui, lang: &UiLanguage, hero: &crate::lobby::HeroProfile) {
    let cjk = lang.cjk;
    heading(
        ui,
        &format!(
            "{} / {}",
            if cjk { hero.name_zh } else { hero.name_en },
            if cjk { hero.role_zh } else { hero.role_en }
        ),
    );
    muted(ui, if cjk { hero.name_en } else { hero.name_zh });
    ui.add_space(6.0);
    ui.label(if cjk {
        hero.description_zh
    } else {
        hero.description_en
    });
    ui.add_space(8.0);
    muted(
        ui,
        if cjk {
            hero.appearance_zh
        } else {
            hero.appearance_en
        },
    );
    ui.add_space(8.0);
    rating_rows(ui, &hero.ratings, cjk);
    ui.add_space(10.0);
    heading(ui, t(lang, "技能组（全英雄通用）", "ABILITIES (SHARED)"));
    let ids = default_hero_abilities();
    for (i, key) in SKILL_KEYS.iter().enumerate() {
        let def = get_ability_def(ids[i]);
        ui.label(
            egui::RichText::new(format!(
                "{}  {}    CD {:.0}s    {} {}    {} {:.0}",
                key,
                def.name,
                def.cooldown,
                t(lang, "耗蓝", "MP"),
                def.mana_cost,
                t(lang, "射程", "RANGE"),
                def.range
            ))
            .size(11.5)
            .strong(),
        );
        muted(
            ui,
            if cjk {
                ABILITY_NOTES_ZH[i]
            } else {
                ABILITY_NOTES_EN[i]
            },
        );
    }
    ui.add_space(10.0);
    heading(ui, t(lang, "推荐出装", "RECOMMENDED BUILD"));
    let catalog = ShopCatalog::placeholder();
    for id in hero.build {
        let name = item_profile(id)
            .map(|p| if cjk { p.name_zh } else { p.name_en })
            .unwrap_or(id);
        let price = catalog
            .items
            .iter()
            .find(|d| d.id == *id)
            .map(|d| d.price)
            .unwrap_or(0);
        ui.label(format!("· {name}   {price} G"));
    }
    ui.add_space(10.0);
    heading(ui, t(lang, "打法提示", "PLAYSTYLE"));
    muted(ui, if cjk { hero.tip_zh } else { hero.tip_en });
}

/// 大厅主界面：铺满视口的 `CentralPanel`，三列布局。
pub fn draw_lobby(
    mut contexts: EguiContexts,
    mut next: ResMut<NextState<AppState>>,
    lang: Res<UiLanguage>,
    mut selection: ResMut<LobbySelection>,
    mut codex: ResMut<CodexOpen>,
    mut warned: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        if !*warned {
            warn!("[lobby] egui primary context not ready; waiting for game camera");
            *warned = true;
        }
        return;
    };
    let cjk = lang.cjk;
    let hero = hero_profile(selection.hero_index);
    let screen = ctx.input(|i| i.viewport_rect());
    // egui 0.36 的 `CentralPanel::show` 只接受 `&mut Ui`（嵌套面板语义）；
    // 顶层用无标题栏、锁定到整个视口的 `Window` 才能稳定覆盖 3D 画面。
    egui::Window::new("lobby_root")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .collapsible(false)
        .fixed_rect(screen)
        .frame(egui::Frame::default().fill(egui::Color32::from_rgb(9, 12, 20)))
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(t(&lang, "灰界裂谷", "GREY RIFT"))
                        .size(32.0)
                        .strong()
                        .color(GOLD),
                );
                ui.label(
                    egui::RichText::new(t(
                        &lang,
                        "体素战场 · 选择英雄与对局模式",
                        "VOXEL ARENA - PICK A HERO AND A MODE",
                    ))
                    .size(12.5)
                    .color(MUTED),
                );
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            let available = ui.available_height() - 38.0;
            egui::ScrollArea::vertical()
                .max_height(available.max(160.0))
                .show(ui, |ui| {
                    ui.columns(3, |cols| {
                        draw_mode_column(&mut cols[0], &lang, &mut selection, &mut next);
                        draw_hero_list(&mut cols[1], &lang, &mut selection);
                        draw_hero_detail(&mut cols[2], &lang, &hero);
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(t(&lang, "道具图鉴 / ITEM CODEX", "ITEM CODEX"))
                    .clicked()
                {
                    codex.0 = !codex.0;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{}   {}   {}   {}",
                        t(&lang, "查看档案", "PROFILE"),
                        if cjk { hero.name_zh } else { hero.name_en },
                        "|",
                        selection.mode.label(cjk)
                    ))
                    .color(GOLD),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted(
                        ui,
                        t(
                            &lang,
                            "Enter 快速进入 · 数据全部由代码生成",
                            "ENTER to start - all data code-generated",
                        ),
                    );
                });
            });
        });
    // 保留 Enter 快捷键（旧大厅行为）。
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        next.set(AppState::InMatch);
    }
}

/// 大厅「道具图鉴」窗口：12 件道具的完整信息，按分类分组。
pub fn draw_item_codex(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    mut codex: ResMut<CodexOpen>,
) {
    if !codex.0 {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let cjk = lang.cjk;
    let catalog = ShopCatalog::placeholder();
    egui::Window::new(t(&lang, "道具图鉴 / ITEM CODEX", "ITEM CODEX"))
        .open(&mut codex.0)
        .default_width(560.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(t(
                    &lang,
                    "12 件装备 · 价格与属性来自服务端权威数值表",
                    "12 items - prices and stats from the authoritative catalog",
                ))
                .color(MUTED),
            );
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for cat in ITEM_CATEGORIES {
                    heading(ui, cat.label(cjk));
                    for d in catalog
                        .items
                        .iter()
                        .filter(|d| item_profile(d.id).is_some_and(|p| p.category == cat))
                    {
                        item_card(ui, d, cjk, &lang, None);
                    }
                    ui.add_space(6.0);
                }
            });
        });
}

/// 局内商店（P 开/关）：真实 12 件道具 + 分类筛选 + 购买 / 售出上行。
pub fn draw_shop(
    mut contexts: EguiContexts,
    lang: Res<UiLanguage>,
    mut open: Local<bool>,
    mut filter: Local<u8>,
    mut queue: ResMut<ClientCommandQueue>,
    toast: Res<ShopToast>,
    heroes: Query<&Hero, With<LocalPlayer>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::P)) {
        *open = !*open;
    }
    if !*open {
        return;
    }
    let cjk = lang.cjk;
    let gold = heroes.iter().next().map(|h| h.gold).unwrap_or(0);
    let catalog = ShopCatalog::placeholder();
    egui::Window::new(t(&lang, "商店 / SHOP (P)", "SHOP (P)"))
        .open(&mut open)
        .default_width(520.0)
        .default_height(560.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} {gold} G", t(&lang, "金币", "GOLD")))
                        .size(15.0)
                        .strong()
                        .color(GOLD),
                );
                if toast.ttl > 0.0 {
                    ui.label(
                        egui::RichText::new(if cjk {
                            toast.text_zh.as_str()
                        } else {
                            toast.text_en.as_str()
                        })
                        .size(11.0)
                        .color(ACCENT),
                    );
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut *filter, 0, t(&lang, "全部", "ALL"));
                for (i, cat) in ITEM_CATEGORIES.iter().enumerate() {
                    ui.selectable_value(&mut *filter, i as u8 + 1, cat.label(cjk));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 74.0)
                .show(ui, |ui| {
                    for d in catalog.items.iter().filter(|d| {
                        *filter == 0
                            || item_profile(d.id).is_some_and(|p| {
                                ITEM_CATEGORIES[*filter as usize - 1] == p.category
                            })
                    }) {
                        item_card(ui, d, cjk, &lang, Some(&mut queue));
                    }
                });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(t(&lang, "售出栏位：", "SELL SLOT: "));
                for slot in 0..INVENTORY_SLOTS {
                    if ui.button(format!("{}", slot + 1)).clicked() {
                        queue.0.push(ClientCommand::Sell(slot as u8));
                    }
                }
                muted(
                    ui,
                    t(
                        &lang,
                        "返还与栏位校验由服务端核算",
                        "Refund validated by server",
                    ),
                );
            });
        });
}

/// 商店回执计时（与帧率无关，不依赖 egui 上下文）。
pub fn tick_shop_toast(time: Res<Time>, mut toast: ResMut<ShopToast>) {
    if toast.ttl > 0.0 {
        toast.ttl = (toast.ttl - time.delta_secs()).max(0.0);
    }
}

/// 局内 HUD 顶部的所选英雄标识（由 `ui.rs::draw_hud` 调用）。
pub fn hero_badge(ui: &mut egui::Ui, lang: &UiLanguage, selection: &LobbySelection) {
    let h = hero_profile(selection.hero_index);
    // 明确标注「档案」：外观 skin 仍由服务端权威下发，此处只展示大厅查看的档案。
    ui.label(
        egui::RichText::new(format!(
            "{} {} · {}",
            t(lang, "档案", "PROFILE"),
            if lang.cjk { h.name_zh } else { h.name_en },
            selection.mode.label(lang.cjk)
        ))
        .size(11.0)
        .color(GOLD),
    );
}

/// 技能栏按钮的悬停说明（由 `ui.rs::draw_ability_bar` 调用）。
pub fn ability_tooltip(ui: &mut egui::Ui, lang: &UiLanguage, index: usize) {
    let note = if lang.cjk {
        ABILITY_NOTES_ZH[index]
    } else {
        ABILITY_NOTES_EN[index]
    };
    ui.label(egui::RichText::new(note).size(11.0).color(MUTED));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_skill_note_has_a_matching_ability_def() {
        let ids = default_hero_abilities();
        assert_eq!(ids.len(), SKILL_KEYS.len());
        assert_eq!(ABILITY_NOTES_ZH.len(), ABILITY_NOTES_EN.len());
        for id in ids {
            let d = get_ability_def(id);
            assert!(!d.name.is_empty());
            assert!(d.cooldown >= 0.0);
        }
    }

    #[test]
    fn every_build_item_exists_in_catalog() {
        let catalog = ShopCatalog::placeholder();
        for h in &HERO_PROFILES {
            for id in h.build {
                assert!(catalog.items.iter().any(|d| d.id == *id), "{id} 缺失");
            }
        }
        assert_eq!(catalog.items.len(), INVENTORY_SLOTS * 2);
    }
}
