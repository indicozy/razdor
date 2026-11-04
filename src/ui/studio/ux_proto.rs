//! THROWAWAY PROTOTYPE, not production code. Static egui mockups for the Razdor Studio UX spec
//! (indicozy/razdor#41 Map workspace, #42 Inspector widget set). Snapshot-only:
//! `RAZDOR_SCENE=studio-ux:<option>` with `map`, `map-drawer`, `inspector-army`,
//! `inspector-army-keys`, `inspector-dialog`, `inspector-unit`, `palette`. Fake data, no wiring:
//! every button does nothing. The map picture is the example package's brod terrain.png.
//! The UI font is read at runtime (`RAZDOR_UX_FONT`, else a system Noto Sans); nothing is committed.
//! The icons are egui-phosphor (Regular). Tokens are the `tok` module below.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use egui_macroquad::egui::{
    self, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Id, Layout, Margin,
    Pos2, Rect, RichText, Sense, Shape, Stroke, TextStyle, TextureHandle, TextureOptions, Vec2,
};
use egui_phosphor::regular as ic;

static OPTION: Mutex<Option<String>> = Mutex::new(None);
static FONTS_READY: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TERRAIN: RefCell<Option<TextureHandle>> = const { RefCell::new(None) };
}

/// The example map the mockups show: 50 × 50 cells.
const MAP_CELLS: f32 = 50.0;
const TERRAIN_PNG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/packs/pepelnaya-marka/maps/brod/terrain.png");
const UI_FONT: &str = "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf";

/// Design tokens: one place for colours, sizes and spacing.
mod tok {
    use super::Color32;
    pub const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1b);
    pub const PANEL: Color32 = Color32::from_rgb(0x1f, 0x22, 0x27);
    pub const RAISED: Color32 = Color32::from_rgb(0x29, 0x2d, 0x34);
    pub const HOVER: Color32 = Color32::from_rgb(0x33, 0x38, 0x41);
    pub const WELL: Color32 = Color32::from_rgb(0x13, 0x15, 0x18);
    pub const LINE: Color32 = Color32::from_rgb(0x35, 0x3a, 0x43);
    pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xec);
    pub const DIM: Color32 = Color32::from_rgb(0xa0, 0xa7, 0xb2);
    pub const FAINT: Color32 = Color32::from_rgb(0x6e, 0x75, 0x80);
    /// The one accent: selection, the active tool, changed-field markers, primary buttons.
    pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
    pub const DANGER: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x6c);
    pub const TEXT_SIZE: f32 = 13.5;
    pub const SMALL: f32 = 12.0;
    pub const ICON: f32 = 16.0;
    pub const BTN_H: f32 = 26.0;
    pub const ROW_H: f32 = 24.0;
    pub const LABEL_W: f32 = 112.0;
    pub const STATUS_H: f32 = 24.0;
    pub const TOP_H: f32 = 36.0;
}
use tok::{ACCENT, BG, DANGER, DIM, FAINT, HOVER, LINE, PANEL, RAISED, TEXT, WELL};

const R4: CornerRadius = CornerRadius::same(4);

/// Selects the option to draw (called by the snapshot scene set-up).
pub fn select(option: &str) {
    *OPTION.lock().unwrap_or_else(|e| e.into_inner()) = Some(option.to_string());
}

/// True when a `studio-ux` scene is staged: the app then draws this prototype instead of the studio.
pub fn active() -> bool {
    OPTION.lock().map(|o| o.is_some()).unwrap_or(false)
}

fn option() -> String {
    OPTION.lock().ok().and_then(|o| o.clone()).unwrap_or_default()
}

/// One frame of the prototype.
pub fn frame() {
    macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.086, 0.09, 0.106, 1.0));
    egui_macroquad::ui(|ctx| {
        apply(ctx);
        show(ctx, &option());
    });
    egui_macroquad::draw();
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let path = std::env::var("RAZDOR_UX_FONT").unwrap_or_else(|_| UI_FONT.to_string());
    match std::fs::read(&path) {
        Ok(bytes) => {
            f.font_data.insert("noto".into(), FontData::from_owned(bytes).into());
            f.families.entry(FontFamily::Proportional).or_default().insert(0, "noto".into());
        }
        Err(e) => eprintln!("studio-ux: no UI font at {path} ({e}); egui's default is used"),
    }
    egui_phosphor::add_to_fonts(&mut f, egui_phosphor::Variant::Regular);
    f
}

fn apply(ctx: &egui::Context) {
    if !FONTS_READY.swap(true, Ordering::Relaxed) {
        ctx.set_fonts(fonts());
    }
    ctx.style_mut(|s| {
        s.visuals = egui::Visuals::dark();
        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(6);
        v.extreme_bg_color = WELL;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = R4;
        }
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, FAINT);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.active.weak_bg_fill = HOVER;
        v.widgets.active.bg_fill = HOVER;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        s.spacing.item_spacing = Vec2::new(6.0, 4.0);
        s.spacing.button_padding = Vec2::new(8.0, 3.0);
        s.spacing.interact_size.y = tok::BTN_H;
        s.spacing.scroll.bar_width = 8.0;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(tok::TEXT_SIZE));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(tok::TEXT_SIZE));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(tok::SMALL));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(16.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    });
}

fn load_png(path: &str) -> Option<egui::ColorImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

fn terrain(ctx: &egui::Context) -> Option<TextureHandle> {
    TERRAIN.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            *t = load_png(TERRAIN_PNG).map(|img| ctx.load_texture("ux-brod-terrain", img, TextureOptions::NEAREST));
        }
        t.clone()
    })
}

// ---------------------------------------------------------------- shared pieces

fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(icon).size(tok::ICON)).min_size(Vec2::splat(tok::BTN_H)).selected(on))
        .on_hover_text(tip)
}

fn small_icon(ui: &mut egui::Ui, icon: &str, tip: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(icon).size(14.0)).min_size(Vec2::splat(22.0)).frame(false))
        .on_hover_text(tip)
}

fn text_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let mut b = egui::Button::new(RichText::new(label).color(if primary { Color32::WHITE } else { TEXT }));
    if primary {
        b = b.fill(ACCENT);
    }
    ui.add(b)
}

/// A keycap-style shortcut label.
fn keycap(ui: &mut egui::Ui, text: &str) {
    Frame::NONE
        .fill(WELL)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(11.5).color(DIM));
        });
}

fn tabs(ui: &mut egui::Ui, labels: &[&str], active: usize) {
    ui.horizontal(|ui| {
        for (i, l) in labels.iter().enumerate() {
            let on = i == active;
            let text = RichText::new(*l).color(if on { TEXT } else { DIM });
            let b = egui::Button::new(text).fill(if on { RAISED } else { Color32::TRANSPARENT }).stroke(Stroke::NONE);
            ui.add(b);
        }
    });
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.0).color(FAINT).strong());
}

/// Value well: the look of every text, number and dropdown field.
fn well(ui: &mut egui::Ui, w: f32, text: &str, icon: Option<&str>, mono: bool) {
    Frame::NONE
        .fill(WELL)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(R4)
        .inner_margin(Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_width(w - 12.0);
            ui.horizontal(|ui| {
                let t = if mono { RichText::new(text).monospace().size(12.5) } else { RichText::new(text) };
                ui.label(t);
                if let Some(i) = icon {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(i).color(DIM).size(13.0));
                    });
                }
            });
        });
}

/// One inspector field: label column (fixed width), changed marker, value, reset button.
/// Returns the label's response (the tooltip anchor).
fn field(
    ui: &mut egui::Ui,
    label: &str,
    key: &str,
    keys: bool,
    changed: bool,
    value: impl FnOnce(&mut egui::Ui, f32),
) -> egui::Response {
    let h = if keys { tok::ROW_H + 12.0 } else { tok::ROW_H + 2.0 };
    let mut label_resp = None;
    ui.horizontal(|ui| {
        ui.set_min_height(h);
        // The changed marker: an accent dot before the label (none when unchanged).
        let (dot, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
        if changed {
            ui.painter().circle_filled(dot.center(), 3.0, ACCENT);
        }
        let lab = ui.allocate_ui_with_layout(Vec2::new(tok::LABEL_W - 8.0, h), Layout::top_down(Align::LEFT), |ui| {
            ui.label(RichText::new(label).color(if changed { TEXT } else { DIM }));
            if keys {
                ui.label(RichText::new(key).monospace().size(11.0).color(FAINT));
            }
        });
        label_resp = Some(lab.response);
        let w = (ui.available_width() - 40.0).max(60.0);
        value(ui, w);
        // The reset button: enabled only for a changed field.
        ui.add_enabled(
            changed,
            egui::Button::new(RichText::new(ic::ARROW_COUNTER_CLOCKWISE).size(13.0)).min_size(Vec2::splat(24.0)).frame(false),
        )
        .on_hover_text("Вернуть значение по умолчанию");
    });
    label_resp.unwrap_or_else(|| ui.label(""))
}

fn section(ui: &mut egui::Ui, id: &str, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(id)
        .default_open(open)
        .show(ui, |ui| {
            ui.add_space(2.0);
            body(ui);
            ui.add_space(4.0);
        });
}

/// A list you reorder: drag handle, icon, name, count, and a drop line where an item would land.
/// `selected` is drawn with the accent border; `drop_after` draws the drop line below that row.
fn list(ui: &mut egui::Ui, rows: &[(&str, &str, &str)], selected: Option<usize>, drop_after: Option<usize>, w: f32) {
    for (i, (icon, name, count)) in rows.iter().enumerate() {
        let on = selected == Some(i);
        Frame::NONE
            .fill(if on { HOVER } else { WELL })
            .stroke(Stroke::new(1.0, if on { ACCENT } else { LINE }))
            .corner_radius(R4)
            .inner_margin(Margin::symmetric(4, 2))
            .show(ui, |ui| {
                ui.set_width(w - 12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic::DOTS_SIX_VERTICAL).color(FAINT).size(14.0)).on_hover_text("Тяните, чтобы переставить");
                    ui.label(RichText::new(*icon).color(DIM).size(14.0));
                    ui.label(*name);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(*count).color(DIM).size(12.0));
                    });
                });
            });
        if drop_after == Some(i) {
            let (r, _) = ui.allocate_exact_size(Vec2::new(w - 4.0, 3.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(2), ACCENT);
        } else {
            ui.add_space(2.0);
        }
    }
}

/// Toolbar of a list: add, duplicate, remove (icon buttons, right-aligned).
fn list_tools(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            small_icon(ui, ic::TRASH, "Удалить выбранное", true);
            small_icon(ui, ic::COPY, "Дублировать выбранное", true);
            small_icon(ui, ic::PLUS, "Добавить", true);
        });
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Army,
    Dialog,
    Unit,
}

/// The Godot-style Inspector (#42): header, field search, sections, fields with markers and resets.
fn inspector(ui: &mut egui::Ui, kind: Kind, keys: bool, tip: bool) {
    let (icon, name, kind_line) = match kind {
        Kind::Army => (ic::SWORD, "Отряд Дариуса", "Армия · Игрок 1 · brod"),
        Kind::Dialog => (ic::CHAT_CIRCLE_TEXT, "Приветствие у брода", "Диалог · brod · 3 варианта"),
        Kind::Unit => (ic::SHIELD_STAR, "Копейщик", "Юнит · Пехота · уровень 1"),
    };
    Frame::NONE.fill(RAISED).corner_radius(R4).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            Frame::NONE.fill(WELL).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.label(RichText::new(icon).size(20.0).color(TEXT));
            });
            ui.vertical(|ui| {
                ui.label(RichText::new(name).strong().size(15.0));
                ui.label(RichText::new(kind_line).size(tok::SMALL).color(DIM));
            });
        });
    });
    ui.add_space(4.0);
    let mut query = String::new();
    ui.horizontal(|ui| {
        ui.label(RichText::new(ic::MAGNIFYING_GLASS).color(DIM));
        ui.add(egui::TextEdit::singleline(&mut query).hint_text("Поиск полей").desired_width(ui.available_width()));
    });
    let mut keys_on = keys;
    ui.checkbox(&mut keys_on, RichText::new("Ключи файла в подписях").size(tok::SMALL).color(DIM));
    ui.add_space(2.0);

    let mut tip_anchor: Option<(String, egui::Rect)> = None;
    let mut note = |label: &str, key: &str, rect: egui::Rect| {
        if tip && key == "army.owner" {
            tip_anchor = Some((label.to_string(), rect));
        }
    };

    match kind {
        Kind::Army => {
            section(ui, "army-main", "Основное", true, |ui| {
                let r = field(ui, "Имя", "army.name", keys_on, true, |ui, w| well(ui, w, "Отряд Дариуса", None, false));
                note("Имя", "army.name", r.rect);
                let r = field(ui, "Владелец", "army.owner", keys_on, false, |ui, w| well(ui, w, "Игрок 1 · герой", Some(ic::CARET_DOWN), false));
                note("Владелец", "army.owner", r.rect);
                let r = field(ui, "Клетка", "army.cell", keys_on, false, |ui, w| well(ui, w, "12, 8", None, true));
                note("Клетка", "army.cell", r.rect);
                field(ui, "Ход", "army.moves", keys_on, false, |ui, w| well(ui, w, "5 из 6", None, false));
            });
            section(ui, "army-troops", "Войско", true, |ui| {
                list_tools(ui);
                list(
                    ui,
                    &[(ic::USERS, "Копейщик", "×5"), (ic::USERS, "Лучник", "×3"), (ic::SHIELD_STAR, "Рыцарь", "×2")],
                    Some(1),
                    Some(1),
                    ui.available_width(),
                );
            });
            section(ui, "army-rewards", "Награды", false, |ui| {
                field(ui, "Золото", "army.gold", keys_on, false, |ui, w| well(ui, w, "60", None, false));
                field(ui, "Опыт", "army.xp", keys_on, true, |ui, w| well(ui, w, "120", None, false));
            });
            section(ui, "army-behaviour", "Поведение", true, |ui| {
                field(ui, "Агрессия", "army.aggro", keys_on, false, |ui, w| well(ui, w, "Нападать на видимых", Some(ic::CARET_DOWN), false));
                let mut patrol = true;
                field(ui, "Патруль", "army.patrol", keys_on, false, |ui, _| {
                    ui.checkbox(&mut patrol, "");
                });
                field(ui, "Радиус", "army.radius", keys_on, false, |ui, w| well(ui, w, "4", None, false));
            });
        }
        Kind::Dialog => {
            section(ui, "dlg-main", "Основное", true, |ui| {
                field(ui, "Имя узла", "node.id", keys_on, false, |ui, w| well(ui, w, "brod.greeting", None, true));
                field(ui, "Говорящий", "node.speaker", keys_on, true, |ui, w| well(ui, w, "Гонец из Марки", Some(ic::CARET_DOWN), false));
                field(ui, "Текст", "node.text", keys_on, false, |ui, w| {
                    Frame::NONE
                        .fill(WELL)
                        .stroke(Stroke::new(1.0, LINE))
                        .corner_radius(R4)
                        .inner_margin(Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_width(w - 12.0);
                            ui.label("Вода поднялась за ночь. Мост снесён,\nброд ещё держится — переходите\nсейчас, пока держится.");
                        });
                });
            });
            section(ui, "dlg-choices", "Варианты ответа", true, |ui| {
                list_tools(ui);
                list(
                    ui,
                    &[
                        (ic::ARROW_RIGHT, "Спросить о мосте", "мост"),
                        (ic::ARROW_RIGHT, "Предложить помощь", "помощь"),
                        (ic::ARROW_RIGHT, "Уйти", "конец"),
                    ],
                    Some(0),
                    None,
                    ui.available_width(),
                );
            });
            section(ui, "dlg-when", "Условия и награда", false, |ui| {
                field(ui, "Нужна переменная", "node.need", keys_on, true, |ui, w| well(ui, w, "bridge_open = 0", Some(ic::CARET_DOWN), true));
                field(ui, "Выдать золото", "node.gold", keys_on, false, |ui, w| well(ui, w, "20", None, false));
            });
        }
        Kind::Unit => {
            section(ui, "unit-main", "Основное", true, |ui| {
                field(ui, "Имя", "unit.name", keys_on, false, |ui, w| well(ui, w, "Копейщик", None, false));
                field(ui, "Класс", "unit.class", keys_on, false, |ui, w| well(ui, w, "Пехота", Some(ic::CARET_DOWN), false));
                field(ui, "Уровень", "unit.level", keys_on, false, |ui, w| well(ui, w, "1", None, false));
            });
            section(ui, "unit-stats", "Характеристики", true, |ui| {
                field(ui, "Атака", "unit.attack", keys_on, true, |ui, w| well(ui, w, "5", None, false));
                field(ui, "Защита", "unit.defence", keys_on, false, |ui, w| well(ui, w, "3", None, false));
                field(ui, "Здоровье", "unit.hp", keys_on, false, |ui, w| well(ui, w, "12", None, false));
                field(ui, "Скорость", "unit.speed", keys_on, false, |ui, w| well(ui, w, "4", None, false));
            });
            section(ui, "unit-price", "Цена", true, |ui| {
                field(ui, "Цена", "unit.price", keys_on, true, |ui, w| well(ui, w, "40 золота", None, false));
                Frame::NONE
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(R4)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Предложено по характеристикам").strong());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Сейчас").color(DIM));
                            ui.label("40 золота");
                            ui.label(RichText::new(ic::ARROW_RIGHT).color(DIM));
                            ui.label(RichText::new("45 золота").strong().color(ACCENT));
                        });
                        ui.label(
                            RichText::new("Атака 5 и здоровье 12 дают +5 к цене. Соседний юнит Лучник стоит 45.")
                                .size(tok::SMALL)
                                .color(DIM),
                        );
                        ui.horizontal(|ui| {
                            text_button(ui, "Применить", true);
                            text_button(ui, "Оставить", false);
                        });
                    });
            });
        }
    }

    if let Some((label, rect)) = tip_anchor {
        let pos = Pos2::new(rect.left(), rect.bottom() + 6.0);
        egui::Area::new(Id::new("ux-tip")).order(egui::Order::Tooltip).fixed_pos(pos).show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(format!("{label}: ключ в файле")).size(tok::SMALL).color(DIM));
                ui.label(RichText::new("army.owner").monospace());
            });
        });
    }
}

// ---------------------------------------------------------------- the frame's panels

fn show(ctx: &egui::Context, opt: &str) {
    top_bar(ctx);
    status_bar(ctx);
    let inspector_only = opt.starts_with("inspector");
    let drawer_open = opt == "map-drawer";
    drawer(ctx, drawer_open);
    if inspector_only {
        inspector_side(ctx, opt);
    } else {
        tool_strip(ctx);
        palette_column(ctx);
        right_column(ctx);
    }
    canvas(ctx);
    if opt == "palette" {
        command_palette(ctx);
    }
}

fn top_bar(ctx: &egui::Context) {
    egui::TopBottomPanel::top("ux-top")
        .exact_height(tok::TOP_H)
        .frame(Frame::NONE.fill(PANEL).inner_margin(Margin::symmetric(8, 4)))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Брод · Пепельная Марка").color(DIM).size(tok::SMALL));
                ui.add_space(6.0);
                tabs(ui, &["Карта", "Сюжет", "Данные", "Тест"], 0);
                ui.separator();
                let _ = icon_button(ui, ic::FLOPPY_DISK, "Сохранить (Ctrl+S)", false);
                let _ = icon_button(ui, ic::ARROW_COUNTER_CLOCKWISE, "Отменить (Ctrl+Z)", false);
                let _ = icon_button(ui, ic::ARROW_CLOCKWISE, "Повторить (Ctrl+Shift+Z)", false);
                ui.separator();
                text_button(ui, &format!("{} Проверка", ic::SHIELD_CHECK), false);
                text_button(ui, &format!("{} Играть", ic::PLAY), true);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // The command palette's entry: a field that opens it (Ctrl+Shift+P).
                    Frame::NONE
                        .fill(WELL)
                        .stroke(Stroke::new(1.0, LINE))
                        .corner_radius(R4)
                        .inner_margin(Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.set_width(230.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(ic::MAGNIFYING_GLASS).color(DIM));
                                ui.label(RichText::new("Команды и объекты").color(DIM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    keycap(ui, "Ctrl+Shift+P");
                                });
                            });
                        });
                });
            });
        });
}

fn status_bar(ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("ux-status")
        .exact_height(tok::STATUS_H)
        .frame(Frame::NONE.fill(PANEL).inner_margin(Margin::symmetric(8, 2)))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Клетка (29, 23)").size(tok::SMALL));
                ui.label(RichText::new("·").color(FAINT));
                ui.label(RichText::new("Трава").size(tok::SMALL));
                ui.label(RichText::new("·").color(FAINT));
                ui.label(RichText::new("Дуб").size(tok::SMALL));
                ui.add_space(16.0);
                ui.label(RichText::new("Shift: прямая · Alt: вычесть · [ ]: размер").size(tok::SMALL).color(DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} Сохранено", ic::CHECK)).size(tok::SMALL).color(DIM));
                });
            });
        });
}

fn drawer(ctx: &egui::Context, open: bool) {
    let h = if open { 186.0 } else { 24.0 };
    egui::TopBottomPanel::bottom("ux-drawer")
        .exact_height(h)
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(8, 3)))
        .show(ctx, |ui| {
            if !open {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(ic::CARET_UP).color(DIM).size(12.0));
                    ui.label(RichText::new("Проблемы").size(tok::SMALL));
                    keycap(ui, "1");
                    ui.label(RichText::new("·").color(FAINT));
                    ui.label(RichText::new("Вывод").size(tok::SMALL).color(DIM));
                    ui.label(RichText::new("·").color(FAINT));
                    ui.label(RichText::new("Claude").size(tok::SMALL).color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("Ctrl+J").size(11.0).color(FAINT));
                    });
                });
                return;
            }
            tabs(ui, &["Проблемы 1", "Вывод", "Claude"], 0);
            ui.separator();
            Frame::NONE.fill(WELL).corner_radius(R4).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic::WARNING_CIRCLE).color(DANGER).size(tok::ICON));
                    ui.label("Армия «Отряд Дариуса» стоит на воде");
                    ui.label(RichText::new("brod · клетка (12, 8)").color(DIM).size(tok::SMALL));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        text_button(ui, "Перейти", false);
                    });
                });
            });
        });
}

fn tool_strip(ctx: &egui::Context) {
    egui::SidePanel::left("ux-tools")
        .exact_width(44.0)
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(5, 6)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let groups: [&[(&str, &str, bool)]; 5] = [
                    &[(ic::CURSOR, "Выбор (V)", false), (ic::ARROWS_OUT_CARDINAL, "Перемещение (T)", false)],
                    &[
                        (ic::PAINT_BRUSH, "Кисть (B)", true),
                        (ic::ERASER, "Ластик (E)", false),
                        (ic::PAINT_BUCKET, "Заливка (G)", false),
                        (ic::EYEDROPPER, "Пипетка (I)", false),
                    ],
                    &[
                        (ic::SELECTION, "Прямоугольник (M)", false),
                        (ic::LASSO, "Лассо (L)", false),
                        (ic::MAGIC_WAND, "Волшебная палочка (W)", false),
                    ],
                    &[(ic::STAMP, "Штамп (S)", false), (ic::SWORD, "Армия (A)", false), (ic::BUILDINGS, "Постройка (U)", false)],
                    &[(ic::MAP_PIN, "Точка (P)", false), (ic::POLYGON, "Зона триггера (Z)", false)],
                ];
                for (gi, group) in groups.iter().enumerate() {
                    if gi > 0 {
                        ui.add_space(3.0);
                        ui.separator();
                        ui.add_space(3.0);
                    }
                    for (icon, tip, on) in group.iter() {
                        icon_button(ui, icon, tip, *on);
                    }
                }
            });
        });
}

fn palette_column(ctx: &egui::Context) {
    egui::SidePanel::left("ux-palette")
        .exact_width(200.0)
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            tabs(ui, &["Палитра", "Штампы"], 0);
            ui.add_space(2.0);
            section_title(ui, "Местность");
            let terrains = [
                ("Трава", Color32::from_rgb(0x5f, 0x8a, 0x4a)),
                ("Лес", Color32::from_rgb(0x2f, 0x5a, 0x35)),
                ("Холмы", Color32::from_rgb(0x8a, 0x7d, 0x52)),
                ("Вода", Color32::from_rgb(0x3d, 0x6b, 0x8f)),
                ("Дорога", Color32::from_rgb(0x9c, 0x8a, 0x66)),
                ("Песок", Color32::from_rgb(0xb8, 0xa4, 0x72)),
                ("Болото", Color32::from_rgb(0x4e, 0x5e, 0x45)),
                ("Пепел", Color32::from_rgb(0x55, 0x52, 0x50)),
            ];
            for pair in terrains.chunks(2) {
                ui.horizontal(|ui| {
                    for (name, col) in pair.iter() {
                        swatch(ui, name, *col, *name == "Трава");
                    }
                });
            }
            ui.add_space(6.0);
            section_title(ui, "Объекты");
            let objects = [
                "Дуб", "Ель", "Камень", "Холм", "Мост", "Колодец", "Мельница", "Башня", "Курган", "Деревня", "Ворота", "Могила",
            ];
            egui::Grid::new("ux-objects").num_columns(3).spacing([4.0, 8.0]).show(ui, |ui| {
                for (i, name) in objects.iter().enumerate() {
                    let on = i == 0;
                    ui.vertical(|ui| {
                        ui.set_width(58.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                        let hue = (i as u8).wrapping_mul(23);
                        let fill = Color32::from_rgb(0x30 + hue / 4, 0x34 + hue / 6, 0x3c + hue / 8);
                        ui.painter().rect_filled(r, R4, fill);
                        ui.painter().rect_stroke(
                            r,
                            R4,
                            Stroke::new(if on { 2.0 } else { 1.0 }, if on { ACCENT } else { LINE }),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, "эскиз", FontId::proportional(11.0), FAINT);
                        ui.label(RichText::new(*name).size(tok::SMALL).color(if on { TEXT } else { DIM }));
                    });
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Картинки: эскизы. Со установленной игрой здесь спрайты.").size(11.5).color(FAINT));
        });
}

/// One terrain swatch: a colour chip and its name, in a fixed cell.
fn swatch(ui: &mut egui::Ui, name: &str, col: Color32, on: bool) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(90.0, tok::ROW_H), Sense::click());
    ui.painter().rect_filled(r, R4, if on { HOVER } else { Color32::TRANSPARENT });
    let chip = Rect::from_min_size(r.min + Vec2::new(4.0, 5.0), Vec2::splat(14.0));
    ui.painter().rect_filled(chip, CornerRadius::same(2), col);
    ui.painter().text(r.min + Vec2::new(24.0, tok::ROW_H / 2.0), Align2::LEFT_CENTER, name, FontId::proportional(tok::TEXT_SIZE), TEXT);
}

fn right_column(ctx: &egui::Context) {
    egui::SidePanel::right("ux-right")
        .exact_width(304.0)
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            minimap(ui);
            ui.add_space(4.0);
            tabs(ui, &["Слои", "Сцена"], 0);
            ui.add_space(2.0);
            layers(ui);
            ui.separator();
            ui.label(RichText::new("Инспектор · Армия").size(11.0).color(FAINT).strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                inspector(ui, Kind::Army, false, false);
            });
        });
}

fn minimap(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 132.0), Sense::hover());
    ui.painter().rect_filled(r, R4, WELL);
    let side = 124.0;
    let map = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.top() + 4.0), Vec2::splat(side));
    if let Some(tex) = terrain(ui.ctx()) {
        ui.painter().image(tex.id(), map, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    let c = side / MAP_CELLS;
    let view = Rect::from_min_size(map.min + Vec2::new(18.0 * c, 20.0 * c), Vec2::new(20.0 * c, 14.0 * c));
    ui.painter().rect_stroke(view, CornerRadius::ZERO, Stroke::new(1.5, ACCENT), egui::StrokeKind::Outside);
    ui.painter().text(
        Pos2::new(r.left() + 136.0, r.top() + 10.0),
        Align2::LEFT_TOP,
        "Миникарта",
        FontId::proportional(tok::SMALL),
        DIM,
    );
    ui.painter().text(
        Pos2::new(r.left() + 136.0, r.top() + 28.0),
        Align2::LEFT_TOP,
        "50 × 50",
        FontId::proportional(tok::SMALL),
        FAINT,
    );
}

fn layers(ui: &mut egui::Ui) {
    let rows = [
        ("Местность", true, false, true),
        ("Объекты", false, false, true),
        ("Постройки", false, false, true),
        ("Армии", false, false, true),
        ("Точки", false, true, true),
        ("Зоны триггеров", false, false, false),
    ];
    for (name, active, locked, visible) in rows {
        Frame::NONE
            .fill(if active { HOVER } else { Color32::TRANSPARENT })
            .corner_radius(R4)
            .inner_margin(Margin::symmetric(3, 0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icon_button_small(ui, if visible { ic::EYE } else { ic::EYE_SLASH }, "Показать или скрыть слой");
                    icon_button_small(ui, if locked { ic::LOCK_SIMPLE } else { ic::LOCK_SIMPLE_OPEN }, "Заблокировать слой");
                    ui.add_space(4.0);
                    ui.label(RichText::new(name).color(if active { TEXT } else { DIM }));
                });
            });
    }
    ui.add_space(4.0);
    ui.label(RichText::new("НАЛОЖЕНИЯ").size(11.0).color(FAINT).strong());
    for (name, on) in [
        ("Радиус хода", true),
        ("Сетка", false),
        ("Проходимость", false),
        ("Патрули", false),
        ("Туман войны", false),
    ] {
        let mut v = on;
        ui.checkbox(&mut v, RichText::new(name).size(tok::SMALL).color(DIM));
    }
}

fn icon_button_small(ui: &mut egui::Ui, icon: &str, tip: &str) {
    ui.add(egui::Button::new(RichText::new(icon).size(14.0)).min_size(Vec2::splat(22.0)).frame(false)).on_hover_text(tip);
}

fn inspector_side(ctx: &egui::Context, opt: &str) {
    let kind = match opt {
        "inspector-dialog" => Kind::Dialog,
        "inspector-unit" => Kind::Unit,
        _ => Kind::Army,
    };
    let keys = opt == "inspector-army-keys";
    let tip = opt == "inspector-army";
    egui::SidePanel::right("ux-inspector")
        .exact_width(380.0)
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(10, 8)))
        .show(ctx, |ui| {
            ui.label(RichText::new("ИНСПЕКТОР").size(11.0).color(FAINT).strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                inspector(ui, kind, keys, tip);
            });
        });
}

fn canvas(ctx: &egui::Context) {
    egui::CentralPanel::default().frame(Frame::NONE.fill(BG)).show(ctx, |ui| {
        options_bar(ui);
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
        let area = resp.rect;
        painter.rect_filled(area, 0.0, BG);
        let side = (area.width().min(area.height()) - 24.0).max(50.0);
        let cell = side / MAP_CELLS;
        let origin = area.center() - Vec2::splat(side / 2.0);
        let map = Rect::from_min_size(origin, Vec2::splat(side));
        if let Some(tex) = terrain(ui.ctx()) {
            painter.image(tex.id(), map, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let cr = |x: f32, y: f32| Rect::from_min_size(origin + Vec2::new(x * cell, y * cell), Vec2::splat(cell));
        // Faint cell grid.
        let grid = Stroke::new(1.0, Color32::from_white_alpha(14));
        for i in 0..=50 {
            let d = i as f32 * cell;
            painter.line_segment([origin + Vec2::new(d, 0.0), origin + Vec2::new(d, side)], grid);
            painter.line_segment([origin + Vec2::new(0.0, d), origin + Vec2::new(side, d)], grid);
        }
        // The selected army: an outline and its name.
        let army = cr(12.0, 8.0);
        painter.rect_stroke(army.expand(2.0), CornerRadius::same(2), Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
        let tag = Pos2::new(army.right() + 6.0, army.top() - 2.0);
        painter.text(tag, Align2::LEFT_TOP, "Отряд Дариуса", FontId::proportional(tok::SMALL), TEXT);
        // The brush footprint preview at the cursor cell (29, 23), brush size 3.
        let fp = Rect::from_min_max(cr(28.0, 22.0).min, cr(31.0, 25.0).min);
        painter.rect_filled(fp, 0.0, Color32::from_rgba_unmultiplied(0x4f, 0x8c, 0xff, 46));
        painter.rect_stroke(fp, CornerRadius::ZERO, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
        // A marching-ants rectangle selection over cells x 14..22, y 10..18.
        let sel = Rect::from_min_max(cr(14.0, 10.0).min, cr(22.0, 18.0).min);
        let ring = [sel.left_top(), sel.right_top(), sel.right_bottom(), sel.left_bottom(), sel.left_top()];
        for s in Shape::dashed_line(&ring, Stroke::new(1.0, Color32::BLACK), 5.0, 5.0) {
            painter.add(s);
        }
        for s in Shape::dashed_line_with_offset(&ring, Stroke::new(1.0, Color32::WHITE), &[5.0], &[5.0], 5.0) {
            painter.add(s);
        }
    });
}

fn options_bar(ui: &mut egui::Ui) {
    Frame::NONE
        .fill(PANEL)
        .stroke(Stroke::new(1.0, LINE))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::horizontal()
                .id_salt("ux-options")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, options_row);
        });
}

fn options_row(ui: &mut egui::Ui) {
    {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.spacing_mut().slider_width = 56.0;
                ui.label(RichText::new("Кисть").color(DIM).size(tok::SMALL));
                let mut size = 3.0_f32;
                ui.add(egui::Slider::new(&mut size, 1.0..=9.0).integer());
                ui.add_space(3.0);
                for (i, label) in ["1", "2", "3", "4", "5", "6", "9"].iter().enumerate() {
                    let on = i == 2;
                    let _ = ui.add(egui::Button::new(RichText::new(*label).size(12.0)).min_size(Vec2::new(22.0, tok::BTN_H)).selected(on));
                }
                ui.add_space(3.0);
                let _ = icon_button(ui, ic::SQUARE, "Квадратная кисть", true);
                let _ = icon_button(ui, ic::CIRCLE, "Круглая кисть", false);
                ui.add_space(3.0);
                ui.label(RichText::new("Плотность").color(DIM).size(tok::SMALL));
                let mut density = 60.0_f32;
                ui.add(egui::Slider::new(&mut density, 0.0..=100.0).suffix(" %").integer());
                ui.add_space(3.0);
                let mut only = true;
                ui.checkbox(&mut only, RichText::new("Только в выделении").size(tok::SMALL));
            });
    }
}

fn command_palette(ctx: &egui::Context) {
    // Dim the scene behind the palette.
    let screen = ctx.screen_rect();
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("ux-dim")))
        .rect_filled(screen, 0.0, Color32::from_black_alpha(120));
    egui::Window::new("ux-palette-window")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_TOP, [0.0, 90.0])
        .fixed_size([580.0, 330.0])
        .frame(Frame::window(&ctx.style()).fill(PANEL).inner_margin(Margin::same(8)))
        .show(ctx, |ui| {
            Frame::NONE
                .fill(WELL)
                .stroke(Stroke::new(1.0, ACCENT))
                .corner_radius(R4)
                .inner_margin(Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ic::MAGNIFYING_GLASS).color(DIM).size(tok::ICON));
                        ui.label(RichText::new("лес").size(15.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            keycap(ui, "Esc");
                        });
                    });
                });
            ui.add_space(6.0);
            let rows = [
                (ic::PAINT_BRUSH, "Лес: кисть", "Инструменты", "B", true),
                (ic::PAINT_BUCKET, "Лес: заливка области", "Инструменты", "G", false),
                (ic::SELECTION, "Выделить все леса на карте", "Выделение", "Ctrl+Shift+A", false),
                (ic::TREE, "Заменить деревья лесом", "Правка", "—", false),
                (ic::FLAG, "Показать наложение: леса", "Вид", "F2", false),
                (ic::MAP_PIN, "Перейти к объекту: Лесная сторожка", "Перейти", "Ctrl+P", false),
            ];
            for (icon, name, group, key, on) in rows {
                Frame::NONE
                    .fill(if on { HOVER } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(1.0, if on { ACCENT } else { Color32::TRANSPARENT }))
                    .corner_radius(R4)
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(tok::ICON).color(DIM));
                            ui.label(RichText::new(name).size(14.0));
                            ui.label(RichText::new(group).size(tok::SMALL).color(FAINT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if key == "—" {
                                    ui.label(RichText::new("нет").size(tok::SMALL).color(FAINT));
                                } else {
                                    keycap(ui, key);
                                }
                            });
                        });
                    });
                ui.add_space(2.0);
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Enter: выполнить · стрелки: выбрать · Ctrl+P: объекты, Ctrl+Shift+P: команды")
                    .size(11.5)
                    .color(FAINT),
            );
        });
}
