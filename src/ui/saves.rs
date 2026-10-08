//! Saving and loading (video notes §6): the save window with a name, the load window with
//! the original's two tabs (manual saves and autosaves, newest first, with the scenario, the
//! hero and the in-game date), the Esc menu, and the autosaves before every battle and at the
//! 12:00 report. Files live in the player's data folder ([`save::default_dir`]).

use std::path::PathBuf;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::content::Content;
use razdor::rules::game::{Foe, Game};
use razdor::rules::save::{self, SaveEntry, SaveKind};

use super::assets::Assets;
use super::battle_view::BattleView;
use super::widgets::*;
use super::world_view;
use super::Screen;


/// Where a save or load window returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Map,
    Title,
}

impl Back {
    fn screen(self) -> Screen {
        match self {
            Back::Map => Screen::WorldMap,
            Back::Title => Screen::MainMenu,
        }
    }

    /// Where Esc goes: the map in a game (Esc closes every window), else the title.
    fn escape(self) -> Screen {
        match self {
            Back::Title => Screen::MainMenu,
            Back::Map => Screen::WorldMap,
        }
    }
}

pub struct SaveView {
    pub name: String,
    pub entries: Vec<SaveEntry>,
    /// The first row shown (the wheel scrolls the list).
    pub scroll: usize,
    /// Why the last save was refused (shown in the window).
    pub error: Option<String>,
    pub back: Back,
}

impl SaveView {
    pub fn new(game: &Game, back: Back) -> SaveView {
        let name = format!("{} {}", game.world.title.trim(), save::date_name(&game.clock));
        SaveView { name, entries: save::default_dir().map_or_else(Vec::new, |d| save::list(&d, SaveKind::Manual)), scroll: 0, error: None, back }
    }
}

pub struct LoadView {
    /// The row whose deletion is being asked about.
    pub confirm_delete: Option<usize>,
    pub tab: SaveKind,
    pub entries: Vec<SaveEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub back: Back,
}

impl LoadView {
    pub fn new(back: Back) -> LoadView {
        let mut v = LoadView { confirm_delete: None, tab: SaveKind::Manual, entries: Vec::new(), selected: 0, scroll: 0, back };
        v.refresh();
        if v.entries.is_empty() {
            v.tab = SaveKind::Auto;
            v.refresh();
            if v.entries.is_empty() {
                v.tab = SaveKind::Manual;
            }
        }
        v
    }

    fn refresh(&mut self) {
        self.entries = save::default_dir().map_or_else(Vec::new, |d| save::list(&d, self.tab));
        self.selected = 0;
        self.scroll = 0;
    }
}

/// The autosave name before a battle: "Battle - <foe>" (the footage: "Битва - Замок …").
fn battle_name(game: &Game) -> String {
    // The army's or the building's name only, cut at its first `#` (0x4973a0).
    let who = match game.foe {
        Some(Foe::Army(i)) => game.world.armies.get(i).map(|a| a.name.as_str()),
        Some(Foe::Garrison(l)) => game.world.locations.get(l).map(|l| l.name.as_str()),
        None => None,
    };
    match who.map(save::autosave_foe).filter(|w| !w.trim().is_empty()) {
        Some(w) => razdor::trf!("Battle - {foe}", foe = w.trim_start()),
        None => tr("Battle").to_string(),
    }
}

/// Writes an autosave named `name` (`in_battle`: the one before a battle, see
/// [`save::write_autosave`]); a failure is reported on stderr (the game goes on).
pub fn autosave(game: &Game, name: &str, in_battle: bool) {
    if !autosaves_on(super::chrome::has_texts().then(|| super::chrome::options_value("OptValue8")).flatten().as_deref()) {
        return;
    }
    let Some(dir) = save::default_dir() else { return };
    if let Err(e) = save::write_autosave(&dir, name, game, in_battle) {
        razdor::diag!("autosave: {e}");
    }
}

/// Whether autosaves are written, by the install's `[Options] OptValue8` (`None`: no install
/// texts, or no such key): an install that sets it follows it as the original does (on when it
/// reads 1, 0x4b7410); without it Razdor autosaves, as it always did before the parity pass.
fn autosaves_on(opt_value8: Option<&str>) -> bool {
    opt_value8.is_none_or(|v| razdor::dt::ini::loose_int(v) == 1)
}

/// The install's content for the rest of the session after `game` was loaded: a save's row
/// width holds for the session (0x4b771c sets the option in memory, not in the ini), so the
/// next new game, restart or campaign map on the install's maps is played in it too, until
/// Razdor is started again.
pub fn session_content(install: Option<Arc<Content>>, game: &Game) -> Option<Arc<Content>> {
    match install {
        Some(c) if matches!(game.origin, Some(save::ScenarioRef::Map { .. })) && c.formation != game.content.formation => Some(game.content.clone()),
        other => other,
    }
}

/// The battle against the pending foe, after the autosave the original makes before every
/// battle. The autosave is the moment just before it: without the pending foe, so loading
/// it puts the hero on the map next to the enemy (potions, spells and the army can still be
/// seen to) instead of straight into the fight, which starts again when he moves or waits.
pub fn battle(game: &mut Game) -> Screen {
    let name = battle_name(game);
    let foe = game.foe.take();
    autosave(game, &name, true);
    game.foe = foe;
    Screen::Battle(Box::new(BattleView::new(game.start_battle())))
}

/// "2026-09-25 13:08" (UTC) from seconds since 1970.
fn real_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (h, m) = (secs % 86_400 / 3600, secs % 3600 / 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y}-{mo:02}-{d:02} {h:02}:{m:02}")
}

/// One row of the book: the save's name, its scenario and when it was saved.
struct Row {
    name: String,
    scenario: String,
    time: String,
}

fn row_of(e: &SaveEntry) -> Row {
    let m = &e.meta;
    let name = if m.name == save::QUICK_SAVE { tr(save::QUICK_SAVE).to_string() } else { m.name.clone() };
    // A game in which the cheat console worked says so after its scenario.
    let scenario = if m.cheats { razdor::trf!("{title} (cheats)", title = m.title) } else { m.title.clone() };
    Row { name, scenario, time: razdor::trf!("Time: {date}", date = saved_label(m.saved_at)) }
}

/// "2023 год, 1 месяц, 31 день, 13:01": when a file was saved, in the original's words.
fn saved_label(secs: u64) -> String {
    let t = real_time(secs);
    let (date, clock) = t.split_once(' ').unwrap_or((&t, ""));
    let mut p = date.split('-');
    let (y, m, d) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
    let n = |s: &str| s.trim_start_matches('0').to_string();
    razdor::trf!("{year} y, {month} m, {day} d, {clock}", year = y, month = n(m), day = n(d), clock)
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own(section: &str, key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// Layout of the book window: the video's load window (18:12), 594×482 pixels of the
/// 960×720 frame, scaled by `chrome::k`.
struct Book {
    win: Rect,
    k: f32,
}

impl Book {
    const W: f32 = 594.0;
    const H: f32 = 482.0;
    const ROWS: usize = 12;

    fn at(title: &str) -> (Book, bool) {
        let k = super::chrome::k();
        let (w, h) = (Book::W * k, Book::H * k);
        // The video's window stands low (y 190 of 720), clear of the menu's logo.
        let win = Rect::new((screen_width() - w) / 2.0, (190.0 * k).min(screen_height() - h), w, h);
        let (_, closed) = super::chrome::window(win, title, super::chrome::Skin::Marble, true);
        (Book { win, k }, closed)
    }

    /// A point of the window in video pixels.
    fn p(&self, x: f32, y: f32) -> Vec2 {
        vec2(self.win.x + x * self.k, self.win.y + y * self.k)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let p = self.p(x, y);
        Rect::new(p.x, p.y, w * self.k, h * self.k)
    }

    /// The column titles and the open book behind the rows.
    fn page(&self, left: &str, right: &str) {
        let k = self.k;
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            let y = self.p(0.0, 53.0).y;
            super::chrome::shadow_centered(left, self.p(157.0, 0.0).x, y, 16.0 * k, super::chrome::CREAM);
            super::chrome::shadow_centered(right, self.p(437.0, 0.0).x, y, 16.0 * k, super::chrome::CREAM);
        });
        let r = self.rect(17.0, 60.0, 560.0, 350.0);
        match super::chrome::win("S_Save") {
            Some(t) => super::chrome::tex(&t, r, WHITE),
            None => super::chrome::surface(r, super::chrome::Skin::Brown),
        }
        super::chrome::silver_frame(r, k);
    }

    fn row_rect(&self, i: usize) -> Rect {
        self.rect(17.0, 64.0 + i as f32 * 29.0, 560.0, 27.0)
    }

    /// The rows from `scroll` on; returns the row clicked and the delete icon clicked.
    fn rows(&self, rows: &[Row], selected: Option<usize>, scroll: &mut usize, deletable: bool) -> (Option<usize>, Option<usize>) {
        let k = self.k;
        let list = self.rect(17.0, 64.0, 560.0, 29.0 * Book::ROWS as f32);
        if list.contains(crate::ui::widgets::pointer().into()) && !input_blocked() {
            let wh = wheel();
            if wh < 0.0 && *scroll + Book::ROWS < rows.len() {
                *scroll += 1;
            } else if wh > 0.0 && *scroll > 0 {
                *scroll -= 1;
            }
        }
        let (mut picked, mut delete) = (None, None);
        for (i, row) in rows.iter().enumerate().skip(*scroll).take(Book::ROWS) {
            let r = self.row_rect(i - *scroll);
            let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
            if selected == Some(i) {
                // The video's blue bar, the load sign on its left, the delete sign on its right.
                let steps = 8;
                for s in 0..steps {
                    let t = s as f32 / (steps - 1) as f32;
                    let a = 0.85 - 0.5 * (t - 0.5).abs();
                    draw_rectangle(r.x, r.y + r.h * s as f32 / steps as f32, r.w, r.h / steps as f32 + 0.5, Color::new(0.12, 0.2, 0.62, a));
                }
                let sign = 22.0 * k;
                if let Some(t) = super::chrome::win("LSign-Load") {
                    super::chrome::tex(&t, Rect::new(r.x + 4.0 * k, r.y + (r.h - sign) / 2.0, sign, sign), WHITE);
                }
                if deletable {
                    let d = Rect::new(r.x + r.w - sign - 4.0 * k, r.y + (r.h - sign) / 2.0, sign, sign);
                    let over = d.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
                    if let Some(t) = super::chrome::win(if over && is_mouse_button_down(MouseButton::Left) { "LSign-Delete-Down" } else { "LSign-Delete" }) {
                        super::chrome::tex(&t, d, WHITE);
                    }
                    if over {
                        tooltip(&[(own("LoadGame", "DeleteHint", n_("Delete the saved game")), INK)]);
                        if clicked() {
                            delete = Some(i);
                        }
                    }
                }
            } else if hover {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.15, 0.45, 0.35));
            }
            let lh = r.h;
            super::dt_font::with_face(super::dt_font::Face::Title, || {
                let size = fit_size(&row.name, 250.0 * k, 16.0 * k);
                super::chrome::shadow_centered(&ellipsize(&row.name, 250.0 * k, size), self.p(157.0, 0.0).x, r.y + lh * 0.5 + size * 0.36, size, super::chrome::CREAM);
            });
            let cx = self.p(437.0, 0.0).x;
            super::dt_font::with_face(super::dt_font::Face::Bold, || {
                super::chrome::shadow_centered(&ellipsize(&row.scenario, 250.0 * k, 11.0 * k), cx, r.y + lh * 0.42, 11.0 * k, WHITE);
            });
            super::chrome::shadow_centered(&ellipsize(&row.time, 250.0 * k, 11.0 * k), cx, r.y + lh * 0.9, 11.0 * k, Color::new(0.85, 0.75, 0.45, 1.0));
            if hover && clicked() && delete.is_none() {
                picked = Some(i);
            }
        }
        (picked, delete)
    }

    /// A torn-parchment tab hanging under the book (`load-button-1/2.lit`: grey, and orange
    /// for the open one), on a steel line running along the window.
    fn tab(&self, x: f32, label: &str, open: bool) -> bool {
        let k = self.k;
        let r = self.rect(x, 409.0, 133.0, 34.0);
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        match super::chrome::win(if open { "load-button-2" } else { "load-button-1" }) {
            Some(t) => super::chrome::tex(&t, r, if open || hover { WHITE } else { Color::new(0.85, 0.85, 0.85, 1.0) }),
            None => super::chrome::surface(r, super::chrome::Skin::Paper),
        }
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            let size = 15.0 * k;
            super::chrome::shadow_centered(label, r.center().x, r.y + r.h * 0.55, size, if open { super::chrome::CREAM } else { WHITE });
        });
        hover && clicked()
    }

    /// The steel line the tabs hang on.
    fn tab_line(&self) {
        if let Some(t) = super::chrome::win("SteelLine") {
            let r = self.rect(6.0, 418.0, 582.0, 5.0);
            super::chrome::three_slice(&t, r, 40.0, WHITE);
        }
    }

    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, 442.0, w, 28.0);
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        super::chrome::marble_button(r, label, enabled, hover);
        enabled && hover && clicked()
    }
}

/// The save window ("Сохранение активной игры"): the book of manual saves under "(save
/// anew)"; a click on a save takes its name to replace it, typing edits the name.
pub fn save_screen(game: &Game, assets: &Assets, view: &mut SaveView, message: &mut Option<String>) -> Option<Screen> {
    world_view::backdrop(game, assets);
    let (book, closed) = Book::at(&own("SaveGame", "Title", n_("Save the game")));
    book.page(&own("SaveGame", "GameName", n_("Name of the saved game")), &own("SaveGame", "ScenarioName", n_("Name of the scenario")));
    while let Some(c) = get_char_pressed() {
        if !c.is_control() && view.name.chars().count() < 60 {
            view.name.push(c);
        }
    }
    if is_key_pressed(KeyCode::Backspace) {
        view.name.pop();
    }
    let same = view.entries.iter().position(|e| e.meta.name == view.name);
    // The first row is the name being typed (a new save, or the one it replaces).
    let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { " " };
    let typed = if view.name.is_empty() { own("Buttons", "NewSave", n_("(save anew)")) } else { format!("{}{caret}", view.name) };
    let mut rows = vec![Row { name: typed, scenario: game.world.title.clone(), time: razdor::trf!("Time: {date}", date = game.clock.label()) }];
    rows.extend(view.entries.iter().map(row_of));
    if let (Some(k), _) = book.rows(&rows, Some(same.map_or(0, |i| i + 1)), &mut view.scroll, false) {
        view.name = if k == 0 { String::new() } else { view.entries[k - 1].meta.name.clone() };
    }
    let dir = save::default_dir();
    let warning = if dir.is_none() { Some(tr("No data folder for saves: set RAZDOR_SAVE_DIR.")) } else { view.error.as_deref() };
    if let Some(w) = warning {
        let p = book.p(22.0, 435.0);
        text(&ellipsize(w, 330.0 * book.k, 13.0 * book.k), p.x, p.y, 13.0 * book.k, RED);
    }
    let ok = !view.name.trim().is_empty() && dir.is_some();
    if book.button(369.0, 116.0, &own("SaveGame", "Save", n_("Save")), ok) || (ok && key(KeyCode::Enter)) {
        let dir = dir.expect("checked");
        match save::write(&dir, SaveKind::Manual, view.name.trim(), game) {
            Ok(_) => {
                *message = Some(razdor::trf!("Saved: {name}", name = view.name.trim()));
                return Some(Screen::WorldMap);
            }
            // The window stays: a save to replace can be picked.
            Err(e) => view.error = Some(razdor::trf!("Not saved: {e}.", e)),
        }
    }
    if book.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed {
        return Some(view.back.screen());
    }
    if key(KeyCode::Escape) {
        return Some(view.back.escape());
    }
    None
}

/// The load window ("Загрузка сохраненной игры"): the book of saves with the "Личные" and
/// "Авто-сохр." tabs; the selected row can be deleted (after a question). Loading itself is
/// the app's (`pending` receives the file).
pub fn load_screen(game: Option<&Game>, assets: &Assets, view: &mut LoadView, pending: &mut Option<PathBuf>, error: &Option<String>) -> Option<Screen> {
    match game {
        Some(g) if view.back != Back::Title => world_view::backdrop(g, assets),
        _ => super::main_menu::backdrop(),
    }
    let (book, closed) = Book::at(&own("LoadGame", "Title", n_("Load a saved game")));
    book.page(&own("LoadGame", "GameName", n_("Name of the saved game")), &own("LoadGame", "ScenarioName", n_("Name of the scenario")));
    let rows: Vec<Row> = view.entries.iter().map(row_of).collect();
    if rows.is_empty() {
        let p = book.p(297.0, 230.0);
        super::chrome::shadow_centered(tr("No saves here yet."), p.x, p.y, 16.0 * book.k, super::chrome::CREAM);
    }
    let asking = view.confirm_delete.is_some();
    let (picked, delete) = book.rows(&rows, Some(view.selected), &mut view.scroll, true);
    if let Some(k) = picked.filter(|_| !asking) {
        if k == view.selected {
            *pending = view.entries.get(k).map(|e| e.path.clone());
        }
        view.selected = k;
    }
    if let Some(k) = delete.filter(|_| !asking) {
        view.confirm_delete = Some(k);
    }
    book.tab_line();
    for (x, tab, label) in [(17.0, SaveKind::Manual, own("LoadGame", "PrivateSave", n_("Private"))), (172.0, SaveKind::Auto, own("LoadGame", "AutoSave", n_("Autosaves")))] {
        if book.tab(x, &label, view.tab == tab) && view.tab != tab && !asking {
            view.tab = tab;
            view.refresh();
        }
    }
    if let Some(e) = error {
        let p = book.p(22.0, 462.0);
        text(&ellipsize(e, 330.0 * book.k, 12.0 * book.k), p.x, p.y, 12.0 * book.k, Color::new(1.0, 0.45, 0.4, 1.0));
    }
    let chosen = view.entries.get(view.selected);
    if !asking && (book.button(369.0, 116.0, &own("LoadGame", "Load", n_("Load")), chosen.is_some()) || key(KeyCode::Enter)) && chosen.is_some() {
        *pending = chosen.map(|e| e.path.clone());
    }
    if !asking && (book.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed) {
        return Some(view.back.screen());
    }
    // The original's question before deleting: "Удаление сохранения". It opens on the next
    // frame, so the click that asked it cannot also answer it.
    if let Some(k) = view.confirm_delete.filter(|_| asking) {
        match delete_question(view.entries.get(k).map(|e| row_of(e).name).unwrap_or_default()) {
            Some(true) => {
                if let Some(e) = view.entries.get(k) {
                    if let Err(err) = std::fs::remove_file(&e.path) {
                        razdor::diag!("{}: {err}", e.path.display());
                    }
                }
                view.confirm_delete = None;
                view.refresh();
            }
            Some(false) => view.confirm_delete = None,
            None => {}
        }
        return None;
    }
    if key(KeyCode::Escape) {
        return Some(view.back.escape());
    }
    None
}

/// "Удаление сохранения": Yes / No, `None` until answered.
fn delete_question(name: String) -> Option<bool> {
    let title = own("MessageBox", "DeleteSave_Title", n_("Delete the save"));
    let q = (razdor::i18n::lang() == razdor::i18n::Lang::Ru)
        .then(|| super::chrome::ui_text("MessageBox", "DeleteSave_Text"))
        .flatten()
        // 0x4c05ac: `#SAVENAME` filled in (0x471f0c), then read as markup (0x48e438), so
        // the marks of a save's name are read too. Razdor's own text is centred as the
        // install's (`^`).
        .map(|t| t.replace("#SAVENAME", &name))
        .unwrap_or_else(|| format!("^{}", razdor::trf!("Do you really want to delete the saved game \"{name}\"?", name)));
    marked_question(&title, &q)
}

/// What the "Выход из игры" window chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitChoice {
    Quit,
    MainMenu,
    Restart,
}

/// The bar's X button (and Esc on the map): "Выход из игры", the original's warning that the
/// game will end, and "Выйти из игры", "Выйти в меню", "Рестарт" (after a question) or
/// "Отмена". `asking` is true while the restart question is open.
pub fn exit_window(game: &Game, assets: &Assets, asking: &mut bool) -> (Option<ExitChoice>, Option<Screen>) {
    world_view::backdrop_lit(game, assets, Some(super::game_bar::BarButton::Menu));
    let title = own("ExitGame", "Title", n_("Leave the game"));
    let warning = own("ExitGame", "Warning", n_("Attention!\n\nThe game under way will end.\nTo go on with it another time, save it from the map's bar first."));
    match exit_dialog(&title, &warning, asking) {
        (_, true) => (None, Some(Screen::WorldMap)),
        (choice, false) => (choice, None),
    }
}

/// Esc in a battle: "Варианты выхода из битвы", the original's warning that a battle cannot
/// be left for the map, and the same choices as the exit window. Returns the choice and
/// whether it was cancelled.
pub fn battle_exit_dialog(asking: &mut bool) -> (Option<ExitChoice>, bool) {
    let title = own("ExitBattle", "Title", n_("Ways out of the battle"));
    let warning = own(
        "ExitBattle",
        "Warning",
        n_("Attention!\n\nYou cannot go back to the map without finishing the battle!\nTo stop the game under way, leave to the main menu (the game is not saved) or start the scenario again (restart)."),
    );
    exit_dialog(&title, &warning, asking)
}

/// The window of [`exit_window`] and [`battle_exit_dialog`]: the warning and "Выйти из
/// игры", "Выйти в меню", "Рестарт" (after a question), "Отмена".
fn exit_dialog(title: &str, warning: &str, asking: &mut bool) -> (Option<ExitChoice>, bool) {
    let k = super::chrome::k();
    let (w, h) = (560.0 * k, 250.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - super::chrome::bar_height() - h) / 2.0, w, h);
    let (inner, closed) = super::chrome::window(r, title, super::chrome::Skin::Marble, true);
    let size = 13.0 * k;
    let mut y = inner.y + 22.0 * k;
    for line in warning.split('\n').flat_map(|l| if l.is_empty() { vec![String::new()] } else { wrap(l, inner.w - 30.0 * k, size) }) {
        super::chrome::shadow_centered(&line, inner.center().x, y, size, super::chrome::CREAM);
        y += 16.0 * k;
    }
    let labels = [
        (own("ExitGame", "Exit", n_("Quit the game")), Some(ExitChoice::Quit)),
        (own("ExitGame", "ExitToMenu", n_("To the menu")), Some(ExitChoice::MainMenu)),
        (own("ExitGame", "Restart", n_("Restart")), Some(ExitChoice::Restart)),
        (own("Buttons", "Cancel", n_("Cancel")), None),
    ];
    let bw = (inner.w - 50.0 * k) / 4.0;
    let by = inner.y + inner.h - 42.0 * k;
    let mut choice = None;
    let mut cancel = closed;
    // The restart question opens on the next frame, so the click on «Рестарт» cannot also
    // answer it (its «Нет» lies over that button).
    let was_asking = *asking;
    for (i, (label, what)) in labels.iter().enumerate() {
        let b = Rect::new(inner.x + 10.0 * k + i as f32 * (bw + 10.0 * k), by, bw, 30.0 * k);
        let over = !*asking && b.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
        super::chrome::marble_button(b, label, true, over);
        if over && clicked() {
            match what {
                Some(ExitChoice::Restart) => *asking = true,
                Some(c) => choice = Some(*c),
                None => cancel = true,
            }
        }
    }
    if was_asking {
        let t = own("MessageBox", "Restart_Title", n_("Restart the game"));
        // The restart box reads its text as markup (0x4bf748 → 0x48e438); Razdor's own text
        // is centred as the install's (`^`).
        let q = (razdor::i18n::lang() == razdor::i18n::Lang::Ru)
            .then(|| super::chrome::ui_text("MessageBox", "Restart_Text"))
            .flatten()
            .unwrap_or_else(|| format!("^{}", tr(n_("Do you really want to start the scenario under way again from the beginning?"))));
        match marked_question(&t, &q) {
            Some(true) => {
                *asking = false;
                return (Some(ExitChoice::Restart), false);
            }
            Some(false) => *asking = false,
            None => {}
        }
        return (None, false);
    }
    if *asking {
        return (None, false);
    }
    (choice, cancel || key(KeyCode::Escape))
}

/// A question with Yes / No, `None` until answered (Esc or N: no, any other key: yes, as
/// the original's box but for Razdor's N: `answer_key`).
pub(super) fn question(title: &str, text: &str) -> Option<bool> {
    question_box(title, text, false)
}

/// [`question`] for the original's boxes whose text is markup (the restart and delete-save
/// boxes, 0x48e438): each line in its font, centred or justified, no shadow.
fn marked_question(title: &str, text: &str) -> Option<bool> {
    question_box(title, text, true)
}

fn question_box(title: &str, text: &str, marked: bool) -> Option<bool> {
    let k = super::chrome::k();
    let (w, h) = (380.0 * k, 150.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, _) = super::chrome::window(r, title, super::chrome::Skin::Marble, false);
    let size = 13.0 * k;
    let (room, step) = (inner.w - 20.0 * k, 17.0 * k);
    if marked {
        // `ui_text` gave the install's `#\` breaks as new lines: back to a break for 0x48e438.
        let rows = markup_rows(&text.replace('\n', "#\\"), room, size);
        for (i, row) in rows.iter().take(3).enumerate() {
            draw_markup_row(row, inner.x + 10.0 * k, inner.y + 24.0 * k + i as f32 * step, room, size);
        }
    } else {
        for (i, line) in wrap(text, room, size).iter().take(3).enumerate() {
            super::chrome::shadow_centered(line, inner.center().x, inner.y + 24.0 * k + i as f32 * step, size, super::chrome::CREAM);
        }
    }
    let bw = 90.0 * k;
    let by = inner.y + inner.h - 38.0 * k;
    let yes = Rect::new(inner.center().x - bw - 8.0 * k, by, bw, 28.0 * k);
    let no = Rect::new(inner.center().x + 8.0 * k, by, bw, 28.0 * k);
    let over = |r: Rect| r.contains(crate::ui::widgets::pointer().into());
    let (yes_label, no_label) = (own("Buttons", "Yes", n_("Yes")), own("Buttons", "No", n_("No")));
    super::chrome::marble_button(yes, &yes_label, true, over(yes));
    super::chrome::marble_button(no, &no_label, true, over(no));
    let answer = answer_key();
    if (over(yes) && clicked()) || answer == Some(true) {
        return Some(true);
    }
    if (over(no) && clicked()) || answer == Some(false) {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_map_games_row_width_holds_for_the_session() {
        use razdor::rules::content::HeroClass;
        use razdor::rules::formation::Formation;
        let wide = Arc::new(Content::builtin().with_formation(Formation::WIDE));
        let mut game = Game::new(Arc::new(wide.with_formation(Formation::VANILLA)), HeroClass::Knight);
        // The demo is not the install's: its width does not carry.
        assert!(Arc::ptr_eq(&session_content(Some(wide.clone()), &game).unwrap(), &wide));
        game.origin = Some(save::ScenarioRef::Map { file: "m".into(), hash: 0 });
        assert_eq!(session_content(Some(wide.clone()), &game).unwrap().formation, Formation::VANILLA);
        assert!(session_content(None, &game).is_none());
    }

    #[test]
    fn autosaves_are_on_unless_the_install_turns_them_off() {
        assert!(autosaves_on(None), "no install, or no OptValue8: Razdor autosaves");
        assert!(autosaves_on(Some("1")));
        assert!(!autosaves_on(Some("0")));
        assert!(!autosaves_on(Some("")));
    }

    #[test]
    fn real_time_labels() {
        assert_eq!(real_time(0), "1970-01-01 00:00");
        assert_eq!(real_time(1_790_000_000), "2026-09-21 14:13");
    }
}
