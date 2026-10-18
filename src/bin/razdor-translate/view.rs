//! The translator's window: the install's maps and campaigns on the left; the model's
//! settings, the chosen map's progress, the buttons and the log on the right. The work runs
//! on a thread ([`razdor::translate::run`]); the window shows its progress.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use macroquad::prelude::*;
use razdor::dt::dtm::Scenario;
use razdor::i18n::tr;
use razdor::translate::job::{self, Job};
use razdor::translate::llm::Settings;
use razdor::translate::{self, Event};
use razdor::trf;

use crate::ui::widgets::{self, button, red_button, text, text_fit, text_field, ACCENT, DIM, FIELD_BG, INK, PANEL};

const RED: Color = Color::new(0.95, 0.45, 0.40, 1.0);
const GREEN: Color = Color::new(0.55, 0.85, 0.50, 1.0);
const ROW_H: f32 = 46.0;
const LIST_W: f32 = 430.0;

/// The server's answer to "Test": its models, or why not.
type ModelList = Result<Vec<String>, String>;

/// A line of the list: one map, or a whole campaign.
struct Item {
    title: String,
    /// The install's own texts rather than maps (`chapters` is empty).
    data: bool,
    /// Indexes into [`View::maps`], in play order.
    chapters: Vec<usize>,
}

/// What the work thread shares with the window.
#[derive(Default)]
struct Shared {
    job: Option<Job>,
    log: Vec<(String, Color)>,
    running: bool,
}

pub struct View {
    install: Option<PathBuf>,
    maps_dir: Option<PathBuf>,
    /// Where the chosen item's English goes.
    target: Option<translate::Target>,
    maps: Vec<(String, Scenario)>,
    items: Vec<Item>,
    selected: Option<usize>,
    scroll: f32,
    settings: Settings,
    saved: Settings,
    /// The chosen item's job and its file.
    job: Option<(Job, Option<PathBuf>)>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    check: Arc<Mutex<Option<ModelList>>>,
    confirm_replace: bool,
    /// "Translate again" was pressed once: the next press forgets the translations.
    confirm_fresh: bool,
    /// The warning over the window: any script is Razdor only.
    script_popup: bool,
}

impl View {
    pub fn new() -> View {
        let install = razdor::dt::install::locate();
        let maps_dir = install.as_deref().and_then(translate::maps_dir);
        let maps: Vec<(String, Scenario)> =
            install.as_deref().map(translate::install_maps).unwrap_or_default().into_iter().map(|(f, _, s)| (f, s)).collect();
        let items = items(&maps);
        let settings = Settings::load();
        if let Some(dir) = install.as_deref() {
            crate::ui::language::set_install(dir);
        }
        let v = View {
            install: install.clone(),
            maps_dir,
            target: None,
            maps,
            items,
            selected: None,
            scroll: 0.0,
            saved: settings.clone(),
            settings,
            job: None,
            shared: Arc::new(Mutex::new(Shared::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            check: Arc::new(Mutex::new(None)),
            confirm_replace: false,
            confirm_fresh: false,
            script_popup: false,
        };
        if install.is_none() {
            v.log(tr("No Discord Times install found: start Razdor once to choose it."), RED);
        }
        v
    }

    fn log(&self, line: impl Into<String>, color: Color) {
        let mut sh = self.shared.lock().unwrap();
        sh.log.push((line.into(), color));
        let n = sh.log.len();
        if n > 200 {
            sh.log.drain(..n - 200);
        }
    }

    fn running(&self) -> bool {
        self.shared.lock().unwrap().running
    }

    /// Stops the work and waits for it (the job is saved after every answer).
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }

    /// Chooses the first item with a map whose file starts with `name`.
    pub fn pick(&mut self, name: &str) {
        if name == "data" {
            if let Some(i) = self.items.iter().position(|it| it.data) {
                self.select(i);
            }
            return;
        }
        let want = name.to_lowercase();
        if let Some(i) = self.items.iter().position(|it| it.chapters.iter().any(|&k| self.maps[k].0.to_lowercase().starts_with(&want))) {
            self.select(i);
        }
    }

    fn select(&mut self, i: usize) {
        if self.running() {
            return;
        }
        self.selected = Some(i);
        self.confirm_replace = false;
        self.confirm_fresh = false;
        if self.items[i].data {
            let Some(install) = self.install.clone() else { return };
            let data = translate::data::install_texts(&install);
            self.job = Some(translate::data::open_job(&data, &self.settings.language, self.settings.any_script));
            self.target = Some(translate::Target::Data { install, data });
            return;
        }
        let chapters: Vec<(String, Scenario)> = self.items[i].chapters.iter().map(|&k| self.maps[k].clone()).collect();
        self.job = Some(translate::open_job(&chapters, &self.settings.language, self.settings.any_script));
        self.target = self.maps_dir.clone().map(translate::Target::Maps);
    }

    pub fn frame(&mut self) {
        crate::ui::input::begin_frame();
        widgets::track_held_key();
        widgets::fields_begin_frame();
        clear_background(Color::from_rgba(12, 12, 11, 255));
        // The thread's latest job.
        if let (Some(j), Some((job, _))) = (self.shared.lock().unwrap().job.take(), self.job.as_mut()) {
            *job = j;
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().map(|w| w.join());
        }
        let (sw, sh) = (screen_width(), screen_height());
        text(tr("Map translator"), 16.0, 30.0, 24.0, ACCENT);
        text_fit(tr("Translated copies of the install's maps, made by a language model"), 230.0, 29.0, sw - 360.0, 15.0, DIM);
        // An open language list takes the window's input.
        widgets::set_input_blocked(widgets::popup_open());
        crate::ui::language::picker_box(Rect::new(sw - 170.0, 8.0, 154.0, 28.0));
        self.list(Rect::new(12.0, 48.0, LIST_W, sh - 60.0));
        let right = Rect::new(LIST_W + 24.0, 48.0, sw - LIST_W - 36.0, sh - 60.0);
        let y = self.settings_panel(right);
        let y = self.job_panel(Rect::new(right.x, y + 10.0, right.w, 186.0));
        self.log_panel(Rect::new(right.x, y + 10.0, right.w, right.y + right.h - y - 10.0));
        self.script_dialog();
        widgets::set_input_blocked(false);
        widgets::draw_popup();
        widgets::fields_end_frame();
        // Another language: the chosen maps' job in it (its own progress and copies).
        if let (Some(i), Some((job, _))) = (self.selected, self.job.as_ref()) {
            let want = self.settings.language.trim();
            if !widgets::typing() && !self.running() && (job.language != want || job.any_script != self.settings.any_script) && !want.is_empty() {
                self.select(i);
            }
        }
        // Settings are kept once typing ends.
        if !widgets::typing() && self.settings != self.saved {
            if let Err(e) = self.settings.save() {
                self.log(trf!("Settings not saved: {e}", e), RED);
            }
            self.saved = self.settings.clone();
        }
    }

    fn list(&mut self, r: Rect) {
        draw_rectangle(r.x, r.y, r.w, r.h, PANEL);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, DIM);
        if self.items.is_empty() {
            text(tr("No maps"), r.x + 12.0, r.y + 28.0, 17.0, DIM);
            return;
        }
        let content = self.items.len() as f32 * ROW_H;
        if widgets::mouse_in(r.x, r.y, r.w, r.h) {
            self.scroll = (self.scroll - widgets::wheel() * ROW_H).clamp(0.0, (content - r.h + 8.0).max(0.0));
        }
        let mut clicked = None;
        for (i, item) in self.items.iter().enumerate() {
            let y = r.y + 4.0 + i as f32 * ROW_H - self.scroll;
            if y + ROW_H < r.y || y > r.y + r.h - ROW_H + 4.0 {
                continue;
            }
            let row = Rect::new(r.x + 4.0, y, r.w - 8.0, ROW_H - 4.0);
            let hover = widgets::mouse_in(row.x, row.y, row.w, row.h);
            if self.selected == Some(i) {
                draw_rectangle(row.x, row.y, row.w, row.h, Color::new(0.30, 0.24, 0.12, 0.9));
            } else if hover {
                draw_rectangle(row.x, row.y, row.w, row.h, Color::new(1.0, 1.0, 1.0, 0.06));
            }
            if hover && widgets::clicked() {
                clicked = Some(i);
            }
            let suffix = job::copy_suffix(&self.settings.language, self.settings.any_script);
            let english = if item.data {
                self.install.as_ref().is_some_and(|d| translate::data::translated(d, &self.settings.language, self.settings.any_script))
            } else {
                self.maps_dir.as_ref().is_some_and(|d| item.chapters.iter().all(|&k| d.join(job::copy_file(&self.maps[k].0, &suffix)).exists()))
            };
            let tag = suffix;
            let title = if english { format!("{}  [{}]", item.title, tag.to_uppercase()) } else { item.title.clone() };
            text_fit(&title, row.x + 8.0, row.y + 19.0, row.w - 16.0, 17.0, if english { GREEN } else { INK });
            let sub = if item.data {
                tr("Units, items, spells and the game's texts: for Razdor in that language").to_string()
            } else if item.chapters.len() > 1 {
                trf!("Campaign: {n} chapters", n = item.chapters.len())
            } else {
                self.maps[item.chapters[0]].0.clone()
            };
            text_fit(&sub, row.x + 8.0, row.y + 37.0, row.w - 16.0, 14.0, DIM);
        }
        if let Some(i) = clicked {
            self.select(i);
        }
    }

    /// The model's settings; returns the panel's bottom.
    fn settings_panel(&mut self, r: Rect) -> f32 {
        let h = 468.0;
        draw_rectangle(r.x, r.y, r.w, h, PANEL);
        draw_rectangle_lines(r.x, r.y, r.w, h, 1.0, DIM);
        text(tr("Language model (OpenAI-compatible API)"), r.x + 12.0, r.y + 24.0, 18.0, ACCENT);
        let lx = r.x + 12.0;
        let fx = r.x + 190.0;
        let fw = r.w - 190.0 - 132.0;
        let bx = fx + fw + 10.0;
        let busy = self.running();
        let mut y = r.y + 38.0;
        text(tr("Address"), lx, y + 18.0, 16.0, INK);
        text_field("tr:endpoint", fx, y, fw, 26.0, &mut self.settings.endpoint, false);
        if button(bx, y, 110.0, 26.0, tr("Test"), !busy) {
            self.check_connection();
        }
        y += 34.0;
        text(tr("Model"), lx, y + 18.0, 16.0, INK);
        text_field("tr:model", fx, y, fw, 26.0, &mut self.settings.model, false);
        y += 34.0;
        text(tr("API key"), lx, y + 18.0, 16.0, INK);
        text_field("tr:key", fx, y, fw, 26.0, &mut self.settings.api_key, false);
        // The key is hidden unless it is being typed.
        if !widgets::has_focus("tr:key") {
            draw_rectangle(fx + 2.0, y + 2.0, fw - 4.0, 22.0, FIELD_BG);
            let shown = if self.settings.api_key.is_empty() { tr("none (a local server)").to_string() } else { "\u{2022}".repeat(self.settings.api_key.chars().count().min(24)) };
            text(&shown, fx + 5.0, y + 19.0, 17.0, DIM);
        }
        if button(bx, y, 110.0, 26.0, tr("Paste"), true) {
            if let Some(k) = macroquad::miniquad::window::clipboard_get() {
                self.settings.api_key = k.trim().to_string();
            }
        }
        y += 34.0;
        text(tr("Context (tokens)"), lx, y + 18.0, 16.0, INK);
        if let Some(v) = widgets::number_field("tr:context", fx, y, 200.0, self.settings.context as i64, 512, 1_048_576) {
            self.settings.context = v as u32;
        }
        y += 34.0;
        text(tr("Translate into"), lx, y + 18.0, 16.0, INK);
        text_field("tr:language", fx, y, 200.0, 26.0, &mut self.settings.language, false);
        text_fit(tr("any language: English, German, Hindi..."), fx + 212.0, y + 18.0, r.x + r.w - fx - 224.0, 14.0, DIM);
        y += 34.0;
        // The letters: what the original holds, or the language's own (Razdor only).
        text(tr("Letters"), lx, y + 18.0, 16.0, INK);
        let half = ((r.x + r.w - fx - 22.0) / 2.0).max(120.0);
        let (latin, native) = (tr("Latin or Cyrillic: the original too"), tr("Any script: Razdor only"));
        if button(fx, y, half, 26.0, latin, true) {
            self.settings.any_script = false;
        }
        if button(fx + half + 10.0, y, half, 26.0, native, true) && !self.settings.any_script {
            self.script_popup = true;
        }
        let lit = if self.settings.any_script { Rect::new(fx + half + 10.0, y, half, 26.0) } else { Rect::new(fx, y, half, 26.0) };
        draw_rectangle_lines(lit.x - 2.0, lit.y - 2.0, lit.w + 4.0, lit.h + 4.0, 2.0, ACCENT);
        y += 34.0;
        text(tr("Reasoning"), lx, y + 18.0, 16.0, INK);
        text_field("tr:effort", fx, y, 200.0, 26.0, &mut self.settings.reasoning_effort, false);
        text_fit(tr("none: thinking off (Ollama); empty: the server's default"), fx + 212.0, y + 18.0, r.x + r.w - fx - 224.0, 14.0, DIM);
        y += 34.0;
        // The player's own wishes for the style, added to the fixed rules.
        text(tr("Translation style"), lx, y + 18.0, 16.0, INK);
        text_field("tr:style", fx, y, r.x + r.w - fx - 12.0, 56.0, &mut self.settings.style, true);
        if self.settings.style.is_empty() && !widgets::has_focus("tr:style") {
            text_fit(tr("e.g. \"archaic, like an old chronicle; keep names Slavic\""), fx + 5.0, y + 19.0, r.x + r.w - fx - 22.0, 15.0, DIM);
        }
        y += 64.0;
        // What the player tells the model about the maps: who is who, the lore, names to keep.
        text(tr("Context"), lx, y + 18.0, 16.0, INK);
        text_field("tr:notes", fx, y, r.x + r.w - fx - 12.0, 80.0, &mut self.settings.notes, true);
        if self.settings.notes.is_empty() && !widgets::has_focus("tr:notes") {
            text_fit(tr("what the model should know: who is who, the story, names to keep as they are"), fx + 5.0, y + 19.0, r.x + r.w - fx - 22.0, 15.0, DIM);
        }
        y += 88.0;
        let check = self.check.lock().unwrap().clone();
        match check {
            Some(Ok(models)) => {
                let has = models.iter().any(|m| m == self.settings.model.trim() || m.trim_end_matches(":latest") == self.settings.model.trim());
                let line = if has {
                    tr("Connected; the model is there.").to_string()
                } else {
                    trf!("Connected, but no model {m}. There are: {list}", m = self.settings.model.trim(), list = models.join(", "))
                };
                text_fit(&line, lx, y + 16.0, r.w - 24.0, 15.0, if has { GREEN } else { RED });
            }
            Some(Err(why)) => text_fit(&trf!("No connection: {why}", why), lx, y + 16.0, r.w - 24.0, 15.0, RED),
            None => text_fit(tr("Ollama: http://localhost:11434/v1, no key. OpenAI: https://api.openai.com/v1 with a key."), lx, y + 16.0, r.w - 24.0, 15.0, DIM),
        }
        r.y + h
    }

    /// Puts into the log what the model is told for the chosen item: the instructions (rules,
    /// context, style) and the start of the first batch.
    fn show_request(&mut self) {
        let Some((job, _)) = self.job.as_ref() else { return };
        let s = translate::llm::Settings { language: job.language.clone(), any_script: job.any_script, ..self.settings.clone() };
        let rules = translate::llm::system_prompt(&s);
        let batch = job.next_batch(s.batch_chars(), s.batch_items());
        let texts: Vec<&str> = batch.iter().take(3).map(|&i| job.entries[i].src.as_str()).collect();
        let user = translate::llm::user_prompt(&texts, &job.glossary(&batch, 40), &job.language, &s.style);
        let mut lines = vec![(tr("— the instructions sent with every batch —").to_string(), ACCENT)];
        let width = screen_width() - LIST_W - 60.0;
        for l in rules.lines().chain(std::iter::once("")).chain(user.lines()) {
            for w in widgets::wrap(l, width, 15.0) {
                lines.push((w, INK));
            }
        }
        let mut sh = self.shared.lock().unwrap();
        sh.log.extend(lines);
    }

    /// The warning before the language's own letters are chosen: the original cannot show them.
    fn script_dialog(&mut self) {
        if !self.script_popup {
            return;
        }
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.6));
        let (w, h) = (560.0f32.min(sw - 40.0), 230.0);
        let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
        draw_rectangle(x, y, w, h, Color::from_rgba(24, 22, 20, 255));
        draw_rectangle_lines(x, y, w, h, 2.0, ACCENT);
        text(tr("The original game cannot show these letters"), x + 20.0, y + 34.0, 20.0, ACCENT);
        let body = tr("Discord Times itself shows only Latin letters without accents and Cyrillic. Maps translated in the language's own letters (umlauts, Devanagari, Chinese characters...) are for Razdor only: the original shows their texts garbled. The Latin or Cyrillic choice works in both.");
        for (i, line) in widgets::wrap(body, w - 40.0, 16.0).iter().enumerate() {
            text(line, x + 20.0, y + 66.0 + i as f32 * 21.0, 16.0, INK);
        }
        if button(x + 20.0, y + h - 50.0, 230.0, 34.0, tr("Use them: Razdor only"), true) {
            self.settings.any_script = true;
            self.script_popup = false;
        }
        if button(x + w - 170.0, y + h - 50.0, 150.0, 34.0, tr("Cancel"), true) || is_key_pressed(KeyCode::Escape) {
            self.script_popup = false;
        }
    }

    fn check_connection(&mut self) {
        let s = self.settings.clone();
        let out = self.check.clone();
        *out.lock().unwrap() = None;
        std::thread::spawn(move || {
            let got = translate::llm::models(&s);
            *out.lock().unwrap() = Some(got);
        });
    }

    /// The chosen map's progress and the buttons; returns the panel's bottom.
    fn job_panel(&mut self, r: Rect) -> f32 {
        draw_rectangle(r.x, r.y, r.w, r.h, PANEL);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, DIM);
        let (Some(i), Some((job, _))) = (self.selected, self.job.as_ref()) else {
            text(tr("Choose a map or a campaign on the left."), r.x + 12.0, r.y + 28.0, 17.0, DIM);
            return r.y + r.h;
        };
        let (total, done, failed, names, chars) = (job.entries.len(), job.done(), job.failed(), job.entries.iter().filter(|e| e.name).count(), job.pending_chars());
        let (complete, open) = (job.complete(), job.entries.iter().any(|e| e.en.is_none() && e.tries < job::MAX_TRIES));
        let busy = self.running();
        text_fit(&self.items[i].title, r.x + 12.0, r.y + 26.0, r.w - 24.0, 20.0, ACCENT);
        let line = trf!("{total} texts ({names} names), {done} translated, {failed} given up; {chars} characters left", total, names, done, failed, chars);
        text_fit(&line, r.x + 12.0, r.y + 50.0, r.w - 24.0, 16.0, INK);
        // Progress bar.
        let (bx, by, bw) = (r.x + 12.0, r.y + 62.0, r.w - 24.0);
        draw_rectangle(bx, by, bw, 14.0, FIELD_BG);
        if total > 0 {
            draw_rectangle(bx, by, bw * done as f32 / total as f32, 14.0, if failed > 0 { RED } else { GREEN });
        }
        draw_rectangle_lines(bx, by, bw, 14.0, 1.0, DIM);
        let y = r.y + 90.0;
        let w = ((r.w - 24.0 - 30.0) / 4.0).min(200.0);
        if busy {
            if button(r.x + 12.0, y, w, 32.0, tr("Stop"), true) {
                self.stop.store(true, Ordering::Relaxed);
                self.log(tr("Stopping after this answer..."), DIM);
            }
        } else if button(r.x + 12.0, y, w, 32.0, if done == 0 { tr("Translate") } else { tr("Go on") }, open) {
            self.start(false);
        }
        if button(r.x + 22.0 + w, y, w, 32.0, tr("Retry given up"), !busy && failed > 0) {
            self.start(true);
        }
        let can_write = !busy && done > 0 && self.target.is_some();
        let write_x = r.x + 32.0 + 2.0 * w;
        if self.confirm_replace {
            if red_button(write_x, y, w, 32.0, tr("Replace translated maps"), can_write) {
                self.write(true);
            }
        } else if button(write_x, y, w, 32.0, tr("Write translated maps"), can_write) {
            self.write(false);
        }
        // Everything again (another model or style): a second press confirms.
        let fresh_x = r.x + 42.0 + 3.0 * w;
        if self.confirm_fresh {
            if red_button(fresh_x, y, w, 32.0, tr("Forget and translate"), !busy) {
                self.confirm_fresh = false;
                if let Some((job, _)) = self.job.as_mut() {
                    job.forget_all();
                }
                self.start(false);
            }
        } else if button(fresh_x, y, w, 32.0, tr("Translate again"), !busy && done > 0) {
            self.confirm_fresh = true;
            self.log(tr("Press again to translate everything anew (with the style and model set now)."), ACCENT);
        }
        // Debugging: what is sent, and the log of every request and raw answer.
        let dy = y + 42.0;
        if button(r.x + 12.0, dy, w, 26.0, tr("Show the request"), true) {
            self.show_request();
        }
        let log = self.job.as_ref().and_then(|(_, p)| p.as_deref()).map(translate::request_log);
        if button(r.x + 22.0 + w, dy, w, 26.0, tr("Request log"), log.as_ref().is_some_and(|l| l.exists())) {
            if let Some(l) = log {
                razdor::update::open_in_browser(&l.to_string_lossy());
            }
        }
        let otherwise = self.job.as_ref().is_some_and(|(j, _)| j.made_otherwise(&self.settings));
        if otherwise && !busy {
            text_fit(tr("The model, style or context changed after these texts were translated: they stay as they were until «Translate again»."), r.x + 12.0, dy + 44.0, r.w - 24.0, 14.0, ACCENT);
        } else if !complete && done > 0 && !busy {
            text_fit(tr("Untranslated texts stay in Russian in the written maps."), r.x + 12.0, dy + 44.0, r.w - 24.0, 14.0, DIM);
        }
        r.y + r.h
    }

    fn start(&mut self, retry: bool) {
        let Some((job, path)) = self.job.as_mut() else { return };
        if retry {
            job.retry_failed();
        }
        let (mut job, path) = (job.clone(), path.clone());
        let s = self.settings.clone();
        let target = self.target.clone();
        let shared = self.shared.clone();
        self.stop.store(false, Ordering::Relaxed);
        let stop = self.stop.clone();
        shared.lock().unwrap().running = true;
        self.log(trf!("Started: model {m}, {n} characters per batch", m = s.model, n = s.batch_chars()), INK);
        self.worker = Some(std::thread::spawn(move || {
            let push = |line: String, color: Color| {
                let mut sh = shared.lock().unwrap();
                sh.log.push((line, color));
            };
            let result = translate::run(&mut job, path.as_deref(), &s, &stop, |job, e| {
                let lines = match e {
                    Event::Sending { items, first } => vec![(trf!("Sending {items}: {first}...", items, first), DIM)],
                    Event::Answer { taken, of, seconds } => {
                        vec![(trf!("Taken {taken} of {of} in {s} s", taken, of, s = format!("{seconds:.1}")), if taken == of { GREEN } else { RED })]
                    }
                    Event::Failed { why, request } => vec![(trf!("Failed: {why} (request {n} in the Request log)", why, n = request), RED)],
                    // Each refused text: why, with what was expected and what came; then the
                    // start of the refused translation and where the whole answer is.
                    Event::Refused { request, texts } => texts.into_iter().flat_map(|r| refused_lines(&r, request)).collect(),
                };
                let mut sh = shared.lock().unwrap();
                sh.log.extend(lines);
                sh.job = Some(job.clone());
            });
            match result {
                // Done: the English copies go straight into the game's maps folder, replacing
                // older copies made by this tool.
                Ok(()) if job.complete() => match target.as_ref().map(|t| t.write(&job, true)) {
                    Some(Ok(files)) => {
                        push(tr("All texts translated.").to_string(), GREEN);
                        for f in files {
                            push(trf!("Written: {f}", f = f.display()), GREEN);
                        }
                    }
                    Some(Err(why)) => push(trf!("Not written: {why}", why), RED),
                    None => push(tr("All texts translated, but the game has no maps folder.").to_string(), RED),
                },
                Ok(()) if stop.load(Ordering::Relaxed) => push(tr("Stopped.").to_string(), DIM),
                Ok(()) => push(trf!("Done; {n} texts given up (Retry given up).", n = job.failed()), RED),
                Err(why) => push(trf!("Stopped: {why}", why), RED),
            }
            let mut sh = shared.lock().unwrap();
            sh.job = Some(job);
            sh.running = false;
        }));
    }

    fn write(&mut self, overwrite: bool) {
        let (Some((job, _)), Some(target)) = (self.job.as_ref(), self.target.as_ref()) else { return };
        match target.write(job, overwrite) {
            Ok(files) => {
                self.confirm_replace = false;
                for f in files {
                    self.log(trf!("Written: {f}", f = f.display()), GREEN);
                }
            }
            Err(_) if !overwrite => {
                self.confirm_replace = true;
                self.log(tr("Translated copies exist already: press again to replace them."), ACCENT);
            }
            Err(why) => self.log(trf!("Not written: {why}", why), RED),
        }
    }

    fn log_panel(&mut self, r: Rect) {
        if r.h < 30.0 {
            return;
        }
        draw_rectangle(r.x, r.y, r.w, r.h, FIELD_BG);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, DIM);
        let sh = self.shared.lock().unwrap();
        let fit = ((r.h - 10.0) / 19.0).floor().max(0.0) as usize;
        let start = sh.log.len().saturating_sub(fit);
        for (k, (line, color)) in sh.log[start..].iter().enumerate() {
            text_fit(line, r.x + 8.0, r.y + 20.0 + k as f32 * 19.0, r.w - 16.0, 15.0, *color);
        }
    }
}

/// A refused text in the log, kept short: the start of the original and why, then the start
/// of the refused translation and the request whose raw answer and whole texts the request
/// log holds.
fn refused_lines(r: &translate::Refused, request: usize) -> [(String, Color); 2] {
    let (text, why) = (translate::text::excerpt(&r.src, 40), &r.why);
    let detail = match &r.answer {
        Some(a) => trf!("translation «{answer}»; request {n} in the Request log", answer = translate::text::excerpt(a, 50), n = request),
        None => trf!("the raw answer: request {n} in the Request log", n = request),
    };
    [(trf!("Refused «{text}»: {why}", text, why), RED), (format!("    {detail}"), DIM)]
}

/// The list: each campaign once (by its first chapter), then the maps alone; English copies
/// made by this tool left out.
fn items(maps: &[(String, Scenario)]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for k in 0..maps.len() {
        if job::is_translated_file(&maps[k].0) {
            continue;
        }
        let chapters = job::campaign(maps, k);
        if out.iter().any(|it| it.chapters.contains(&k)) {
            continue;
        }
        let first = &maps[chapters[0]].1;
        let title = if chapters.len() > 1 && !first.campaign_name.trim().is_empty() { first.campaign_name.trim() } else { first.title.trim() };
        let title = if title.is_empty() { maps[chapters[0]].0.clone() } else { title.to_string() };
        out.push(Item { title, data: false, chapters });
    }
    out.sort_by_key(|it| (it.chapters.len() == 1, it.title.clone()));
    // The install's own texts first: maps translated after them use their names.
    if !maps.is_empty() {
        out.insert(0, Item { title: tr("Game data").to_string(), data: true, chapters: Vec::new() });
    }
    out
}
