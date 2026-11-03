//! The Claude dock: the author's own `claude` in a pseudo-terminal, in the package folder, with the
//! studio's MCP server registered (`studio::claude`, `studio::terminal`). It starts only when the author
//! presses «Запустить Claude», stops with «Остановить» or when the studio closes. While the terminal has
//! the keyboard (a click in it), the keys go to the program: egui's key events are taken off the frame
//! before the other widgets see them. The screen is drawn from the VT100 model in egui's monospace font.

use std::path::{Path, PathBuf};

use egui_macroquad::egui;
use egui::{Align2, Color32, CornerRadius, Event, FontId, Key, Modifiers, Rect, Sense, Ui};
use razdor::i18n::tr;
use razdor::studio::claude::{self, INSTALL_URL};
use razdor::studio::terminal::{key_bytes, text_bytes, Command, Key as TermKey, Terminal};

const TEXT: Color32 = Color32::from_rgb(0xd8, 0xdb, 0xe0);
const SCREEN_BG: Color32 = Color32::from_rgb(0x12, 0x15, 0x1c);
const ERROR_RED: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x5a);
const FONT_SIZE: f32 = 13.0;

/// The dock's state: the program running (if any), its focus, and the last error.
pub struct ClaudeDock {
    term: Option<Terminal>,
    /// The `claude` program found on PATH when the dock was last looked at.
    program: Option<PathBuf>,
    /// The terminal has the keyboard (a click in it; a click elsewhere takes it back).
    focused: bool,
    /// Why the program did not start.
    error: Option<String>,
    /// The tab was drawn in this frame, and in the frame before (the keys go to the program only while it
    /// is on screen; leaving the tab takes the focus).
    shown_now: bool,
    shown_before: bool,
}

impl ClaudeDock {
    pub fn new() -> ClaudeDock {
        ClaudeDock { term: None, program: claude::find_program(), focused: false, error: None, shown_now: false, shown_before: false }
    }

    /// Starts the program in `root` with the studio's MCP server attached. Nothing else starts it.
    pub fn start(&mut self, root: &Path) {
        self.error = None;
        let Some(program) = claude::find_program() else {
            self.error = Some(tr("Claude Code is not installed: no `claude` program on PATH.").to_string());
            return;
        };
        let started = (|| -> Result<Terminal, String> {
            let exe = std::env::current_exe().map_err(|e| format!("cannot find Razdor: {e}"))?;
            let launch = claude::launch(program, &claude::mcp_config(&exe));
            let command = Command {
                program: launch.program,
                args: launch.args,
                cwd: root.to_path_buf(),
                env: vec![("RAZDOR_STUDIO".into(), "1".into())],
            };
            Terminal::spawn(&command, 24, 80)
        })();
        match started {
            Ok(term) => {
                self.term = Some(term);
                self.focused = true;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Stops the program («Остановить», or the studio closing).
    pub fn stop(&mut self) {
        if let Some(mut term) = self.term.take() {
            term.kill();
        }
        self.focused = false;
    }

    /// The frame's start: the tab's state of the frame before. A tab that was not drawn loses the focus.
    pub fn begin_frame(&mut self) {
        self.shown_before = std::mem::take(&mut self.shown_now);
        if !self.shown_before {
            self.focused = false;
        }
    }

    /// Takes the program's output that came in (every frame). True when the screen changed.
    pub fn pump(&mut self) -> bool {
        self.term.as_mut().is_some_and(Terminal::pump)
    }

    /// Sends the keys, the text, the copy and the paste of this frame to the program while the terminal
    /// has the keyboard. The events it takes are removed from the frame, so no other widget sees them.
    pub fn take_input(&mut self, ctx: &egui::Context) {
        if !self.focused || !self.shown_before {
            return;
        }
        let Some(term) = self.term.as_mut() else { return };
        if !term.running() {
            return;
        }
        let app_cursor = term.screen().application_cursor();
        let bracketed = term.screen().bracketed_paste();
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let mut keep = Vec::with_capacity(events.len());
        for event in events {
            match event {
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(k) = term_key(key, modifiers) {
                        term.write(&key_bytes(k, app_cursor));
                    }
                }
                Event::Key { .. } => {}
                Event::Text(text) | Event::Paste(text) => term.write(&text_bytes(&text, bracketed)),
                // Ctrl+C is the program's interrupt (the window's copy event; the terminal copies nothing).
                Event::Copy => term.write(&[0x03]),
                Event::Cut => term.write(&[0x18]),
                other => keep.push(other),
            }
        }
        ctx.input_mut(|i| i.events = keep);
    }

    /// The dock's tab: the start page when no program runs, else the screen with its header.
    pub fn show(&mut self, ui: &mut Ui, root: &Path) {
        self.shown_now = true;
        if self.term.is_none() {
            self.show_start(ui, root);
            return;
        }
        // A program that has ended keeps its last screen until the author starts it again.
        let running = self.term.as_mut().is_some_and(Terminal::running);
        let status = self.term.as_ref().and_then(|t| t.exit_status().map(str::to_string)).unwrap_or_default();
        let mut restart = false;
        let mut stop = false;
        ui.horizontal(|ui| {
            if running {
                ui.weak(trf_running(root));
                stop = ui.button(tr("Stop Claude")).clicked();
            } else {
                ui.weak(format!("{} ({status})", tr("Claude has ended")));
                restart = ui.button(tr("Start Claude")).clicked();
            }
        });
        if stop {
            self.stop();
            return;
        }
        if restart {
            self.stop();
            self.start(root);
            return;
        }
        self.draw_screen(ui);
    }

    /// No program running: the start button, or what to install when `claude` is not on PATH.
    fn show_start(&mut self, ui: &mut Ui, root: &Path) {
        self.program = claude::find_program();
        ui.add_space(8.0);
        if self.program.is_some() {
            ui.label(tr("Claude runs here, in the package folder, with the studio's tools attached (razdor-studio)."));
            ui.add_space(6.0);
            if ui.button(tr("Start Claude")).clicked() {
                self.start(root);
            }
        } else {
            ui.label(tr("Claude Code is not installed: no `claude` program on PATH."));
            ui.add_space(4.0);
            ui.hyperlink_to(tr("Install Claude Code"), INSTALL_URL);
        }
        if let Some(e) = &self.error {
            ui.add_space(6.0);
            ui.colored_label(ERROR_RED, e);
        }
    }

    /// The screen: its cells in the monospace font, with the colours, the cursor and the scrollback.
    /// The grid is resized to the tab (the program gets the size of its screen in cells).
    fn draw_screen(&mut self, ui: &mut Ui) {
        let font = FontId::monospace(FONT_SIZE);
        let (cw, ch) = ui.fonts(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail.x.max(cw * 2.0), avail.y.max(ch * 2.0)), Sense::click_and_drag());
        let Some(term) = self.term.as_mut() else { return };
        let cols = ((rect.width() / cw).floor() as u16).max(2);
        let rows = ((rect.height() / ch).floor() as u16).max(2);
        term.resize(rows, cols);

        if resp.clicked() || resp.drag_started() {
            self.focused = true;
        }
        if ui.input(|i| i.pointer.primary_pressed()) && !ui.rect_contains_pointer(rect) {
            self.focused = false;
        }
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy.abs() >= ch / 2.0 {
                term.scroll((dy / ch).round() as isize * 3);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, SCREEN_BG);
        let screen = term.screen();
        for row in 0..rows {
            let y = rect.top() + f32::from(row) * ch;
            let mut run = Run::default();
            for col in 0..cols {
                let x = rect.left() + f32::from(col) * cw;
                let Some(cell) = screen.cell(row, col) else {
                    run.flush(&painter, &font, rect.left(), y, cw, ch);
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let mut fg = colour(cell.fgcolor(), TEXT);
                let mut bg = colour_opt(cell.bgcolor());
                if cell.bold() {
                    fg = brighter(fg);
                }
                if cell.inverse() {
                    let back = bg.unwrap_or(SCREEN_BG);
                    bg = Some(fg);
                    fg = back;
                }
                if let Some(bg) = bg {
                    let w = if cell.is_wide() { cw * 2.0 } else { cw };
                    painter.rect_filled(Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, ch)), CornerRadius::ZERO, bg);
                }
                let text = if cell.has_contents() { cell.contents() } else { " " };
                let style = (fg, cell.underline());
                if run.style != Some(style) {
                    run.flush(&painter, &font, rect.left(), y, cw, ch);
                    run.style = Some(style);
                    run.start = col;
                }
                run.text.push_str(text);
            }
            run.flush(&painter, &font, rect.left(), y, cw, ch);
        }

        // The cursor, on the live screen only, drawn solid when the terminal has the keyboard.
        if !screen.hide_cursor() && term.scrolled() == 0 {
            let (r, c) = screen.cursor_position();
            let at = egui::pos2(rect.left() + f32::from(c) * cw, rect.top() + f32::from(r) * ch);
            let cursor = Rect::from_min_size(at, egui::vec2(cw, ch));
            if self.focused {
                painter.rect_filled(cursor, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0xd8, 0xdb, 0xe0, 0xa0));
            } else {
                painter.rect_stroke(cursor, CornerRadius::ZERO, egui::Stroke::new(1.0, TEXT), egui::StrokeKind::Inside);
            }
        }
        if term.scrolled() > 0 {
            painter.text(rect.right_bottom() - egui::vec2(6.0, 6.0), Align2::RIGHT_BOTTOM, tr("scrollback"), FontId::proportional(11.0), Color32::from_gray(0x90));
        }
        if let Some(status) = term.exit_status() {
            painter.text(rect.left_bottom() + egui::vec2(6.0, -6.0), Align2::LEFT_BOTTOM, status, FontId::monospace(11.0), ERROR_RED);
        }
    }
}

/// A run of cells with one style, drawn as one piece of text.
#[derive(Default)]
struct Run {
    text: String,
    style: Option<(Color32, bool)>,
    start: u16,
}

impl Run {
    fn flush(&mut self, painter: &egui::Painter, font: &FontId, left: f32, y: f32, cw: f32, ch: f32) {
        let Some((fg, underline)) = self.style.take() else {
            self.text.clear();
            return;
        };
        if !self.text.trim().is_empty() {
            let x = left + f32::from(self.start) * cw;
            let galley = painter.layout_no_wrap(std::mem::take(&mut self.text), font.clone(), fg);
            let width = galley.size().x;
            painter.galley(egui::pos2(x, y), galley, fg);
            if underline {
                painter.line_segment([egui::pos2(x, y + ch - 2.0), egui::pos2(x + width, y + ch - 2.0)], egui::Stroke::new(1.0, fg));
            }
        }
        self.text.clear();
    }
}

/// The text key of the dock's header line: the folder it runs in.
fn trf_running(root: &Path) -> String {
    format!("{}: {}", tr("Claude runs in"), root.display())
}

/// The egui key of a terminal key, for a key press. Printable keys are not here: the text is.
fn term_key(key: Key, modifiers: Modifiers) -> Option<TermKey> {
    let plain = match key {
        Key::Enter => Some(TermKey::Enter),
        Key::Tab if modifiers.shift => Some(TermKey::BackTab),
        Key::Tab => Some(TermKey::Tab),
        Key::Backspace => Some(TermKey::Backspace),
        Key::Escape => Some(TermKey::Escape),
        Key::ArrowUp => Some(TermKey::Up),
        Key::ArrowDown => Some(TermKey::Down),
        Key::ArrowLeft => Some(TermKey::Left),
        Key::ArrowRight => Some(TermKey::Right),
        Key::Home => Some(TermKey::Home),
        Key::End => Some(TermKey::End),
        Key::PageUp => Some(TermKey::PageUp),
        Key::PageDown => Some(TermKey::PageDown),
        Key::Delete => Some(TermKey::Delete),
        Key::Insert => Some(TermKey::Insert),
        _ => None,
    };
    if plain.is_some() {
        return plain;
    }
    if modifiers.ctrl {
        let letter = match key {
            Key::A => 'a', Key::B => 'b', Key::C => 'c', Key::D => 'd', Key::E => 'e', Key::F => 'f',
            Key::G => 'g', Key::H => 'h', Key::I => 'i', Key::J => 'j', Key::K => 'k', Key::L => 'l',
            Key::M => 'm', Key::N => 'n', Key::O => 'o', Key::P => 'p', Key::Q => 'q', Key::R => 'r',
            Key::S => 's', Key::T => 't', Key::U => 'u', Key::V => 'v', Key::W => 'w', Key::X => 'x',
            Key::Y => 'y', Key::Z => 'z',
            Key::OpenBracket => '[',
            _ => return None,
        };
        return Some(TermKey::Ctrl(letter));
    }
    None
}

/// A VT colour as egui's: the default one, an indexed one (the 16 basic, the 6×6×6 cube, the greys),
/// or an RGB one.
fn colour(c: vt100::Color, default: Color32) -> Color32 {
    colour_opt(c).unwrap_or(default)
}

fn colour_opt(c: vt100::Color) -> Option<Color32> {
    const BASIC: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x26), (0xcd, 0x4a, 0x4a), (0x7f, 0xc8, 0x6a), (0xe0, 0xc0, 0x5a),
        (0x4f, 0x8e, 0xdc), (0xc0, 0x7a, 0xd8), (0x5a, 0xc8, 0xc8), (0xd8, 0xdb, 0xe0),
        (0x6a, 0x70, 0x80), (0xf0, 0x70, 0x70), (0xa0, 0xe8, 0x8a), (0xf5, 0xdc, 0x80),
        (0x80, 0xb0, 0xf5), (0xe0, 0x9a, 0xf0), (0x90, 0xe8, 0xe8), (0xf4, 0xf6, 0xfa),
    ];
    match c {
        vt100::Color::Default => None,
        vt100::Color::Rgb(r, g, b) => Some(Color32::from_rgb(r, g, b)),
        vt100::Color::Idx(n) => {
            let (r, g, b) = match n {
                0..=15 => BASIC[n as usize],
                16..=231 => {
                    let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                    let i = n - 16;
                    (level(i / 36), level(i / 6 % 6), level(i % 6))
                }
                _ => {
                    let v = 8 + 10 * (n - 232);
                    (v, v, v)
                }
            };
            Some(Color32::from_rgb(r, g, b))
        }
    }
}

/// A bold colour: a little lighter (the monospace font has no bold face).
fn brighter(c: Color32) -> Color32 {
    let lift = |v: u8| v.saturating_add(40);
    Color32::from_rgb(lift(c.r()), lift(c.g()), lift(c.b()))
}
