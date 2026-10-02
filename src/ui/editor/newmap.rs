//! The new-map dialog (the original's TMakeMap, docs/reference/editor/newmap.md §1): size,
//! map type and orientation, the seven share bars, blur, the seed with its "keep" and
//! "rebuild relief" boxes, and a preview of the last run. "Create" runs the generator in
//! the background (the button breaks it while it runs); "Exit" takes the last run's map, as
//! the original's exit button does after a generation; Cancel leaves the open map as it is.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use macroquad::prelude::*;

use razdor::editor::newmap::{Cells, Generator, Job, Options, Outcome, Shares, Sprites, Stage, Stop, BARS, KINDS, MAX_BLUR, ORIENTATIONS, SIZES};
use razdor::i18n::{n_, tr};
use razdor::trf;

use crate::ui::widgets::*;
use crate::ui::world_view::surface_color;

/// The bars' labels: panel names left to right, split at " | ".
const BAR_LABELS: [&str; 7] = [
    n_("Water | land"),
    n_("Shallows | coastal water | deep sea"),
    n_("Coast band | inland"),
    n_("Plain | hills | mountains | stony soil"),
    n_("Sand | lowland | marsh (of the band)"),
    n_("Marsh | impassable swamp (of the marsh)"),
    n_("No forest | forest | dense forest"),
];

const BAR_COLORS: [&[(f32, f32, f32)]; 7] = [
    &[(0.20, 0.35, 0.65), (0.35, 0.55, 0.25)],
    &[(0.45, 0.65, 0.85), (0.25, 0.45, 0.75), (0.12, 0.22, 0.50)],
    &[(0.50, 0.50, 0.25), (0.35, 0.55, 0.25)],
    &[(0.40, 0.62, 0.30), (0.62, 0.50, 0.30), (0.50, 0.50, 0.50), (0.35, 0.33, 0.30)],
    &[(0.85, 0.78, 0.45), (0.50, 0.70, 0.35), (0.45, 0.48, 0.25)],
    &[(0.45, 0.48, 0.25), (0.28, 0.30, 0.15)],
    &[(0.65, 0.70, 0.50), (0.25, 0.50, 0.20), (0.10, 0.30, 0.10)],
];

/// Pixels per original pixel of a bar.
const SCALE: f32 = 3.0;

pub enum NewMapAction {
    None,
    /// A run ended: the editor's generator is where it left it.
    Ran(razdor::rules::rng::Rng),
    /// Exit after a run: the outcome and the widths to save.
    Accept(Box<Outcome>, Shares),
    /// Cancel; a run broken off on the way leaves the editor's generator here.
    Close(Option<razdor::rules::rng::Rng>),
}

/// A run in the background and its break flag.
type Running = (JoinHandle<(Generator, Outcome)>, Arc<AtomicBool>);

/// The dialog, kept while it is open (its generator holds the relief for "keep").
pub struct NewMapState {
    pub options: Options,
    seed: i64,
    generator: Option<Generator>,
    running: Option<Running>,
    last: Option<Outcome>,
    preview: Option<Texture2D>,
    /// The splitter held: bar, splitter, where in it the press was (original pixels).
    drag: Option<(usize, usize, f32)>,
}

impl NewMapState {
    /// A new dialog as the original's opens: 50 cells, land facing north, blur 2, seed 0,
    /// both boxes off, and the widths of the editor ini.
    pub fn new(shares: Shares) -> NewMapState {
        NewMapState {
            options: Options { shares, ..Options::default() },
            seed: 0,
            generator: Some(Generator::new()),
            running: None,
            last: None,
            preview: None,
            drag: None,
        }
    }

    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// Asks a running generation to stop and waits for it.
    pub fn stop(&mut self) -> Option<razdor::rules::rng::Rng> {
        let (handle, brk) = self.running.take()?;
        brk.store(true, Ordering::Relaxed);
        let (g, out) = handle.join().ok()?;
        self.generator = Some(g);
        Some(out.rng)
    }

    /// Starts a run on the cells of `start` (the open map, or the last run's).
    pub fn start(&mut self, sprites: Sprites, open_map: Cells) {
        let Some(mut g) = self.generator.take() else { return };
        let start = self.last.as_ref().map(|o| o.cells.clone()).unwrap_or(open_map);
        let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        let job = Job { options: self.options, seed: self.seed as i32, sprites, start, clock };
        let brk = Arc::new(AtomicBool::new(false));
        let flag = brk.clone();
        self.running = Some((std::thread::spawn(move || {
            let out = g.run(job, &flag);
            (g, out)
        }), brk));
    }

    /// Picks up a finished run.
    fn poll(&mut self) -> Option<razdor::rules::rng::Rng> {
        if !self.running.as_ref().is_some_and(|(h, _)| h.is_finished()) {
            return None;
        }
        let (handle, _) = self.running.take()?;
        let (g, out) = handle.join().ok()?;
        self.generator = Some(g);
        self.seed = out.seed as i64;
        self.preview = Some(preview(&out.cells));
        let rng = out.rng.clone();
        self.last = Some(out);
        Some(rng)
    }
}

/// The run's map as a picture: terrain colours, darker where trees, hills and mountains are.
fn preview(c: &Cells) -> Texture2D {
    let mut rgba = Vec::with_capacity(c.mark.len() * 4);
    for i in 0..c.mark.len() {
        let col = match c.mark[i] {
            -9 => Color::new(0.16, 0.40, 0.14, 1.0),
            -11 => Color::new(0.08, 0.25, 0.08, 1.0),
            -1 => Color::new(0.58, 0.50, 0.30, 1.0),
            -5 => Color::new(0.45, 0.43, 0.42, 1.0),
            _ => surface_color(c.ground[i]),
        };
        let b: [u8; 4] = col.into();
        rgba.extend_from_slice(&b);
    }
    let tex = Texture2D::from_rgba8(c.w as u16, c.h as u16, &rgba);
    tex.set_filter(FilterMode::Nearest);
    tex
}

/// Why a run stopped, for the status line.
pub fn stop_text(s: Stop) -> String {
    match s {
        Stop::Break => tr("Stopped by the break button: the map is half made.").into(),
        Stop::FlatField => tr("The relief is flat, so the run stopped (the original divides 0 by 0 there).").into(),
        Stop::NoShares => tr("Every share of a bar is 0, so the run stopped (a division by zero in the original).").into(),
        Stop::RangeError => tr("A list outgrew the original's limit, so the run stopped.").into(),
        Stop::SmallForestPool => tr("Too few forest cells for the original's progress bar: it stopped after the first cluster of trees, as the original does.").into(),
        Stop::Hang(stage) => {
            let what = tr(match stage {
                Stage::MoreLowland => n_("growing the lowland"),
                Stage::Sand => n_("taking back sand"),
                Stage::Marsh => n_("placing marsh"),
                Stage::Swamp => n_("placing swamp"),
                Stage::Forest => n_("placing trees (some have no picture)"),
            });
            trf!("The original would never finish {what}; Razdor stopped the run there.", what)
        }
    }
}

/// The dialog in rectangle `r`. `sprites` and `open_map` are built only when a run starts.
pub fn window(st: &mut NewMapState, r: Rect, sprites: impl FnOnce() -> Sprites, open_map: impl FnOnce(u32) -> Cells) -> NewMapAction {
    let mut action = NewMapAction::None;
    if let Some(rng) = st.poll() {
        action = NewMapAction::Ran(rng);
    }
    let busy = st.running();
    let (x, mut y) = (r.x + 20.0, r.y + 32.0);
    text(tr("New map"), x, y, 22.0, ACCENT);
    y += 20.0;
    let o = &mut st.options;
    // Left column: the options.
    text_fit(tr("Size"), x, y + 18.0, 86.0, 17.0, INK);
    for (k, n) in SIZES.iter().enumerate() {
        if toggle_button(x + 90.0 + k as f32 * 66.0, y, 62.0, 28.0, &n.to_string(), o.size == *n) && !busy {
            o.size = *n;
        }
    }
    y += 38.0;
    text_fit(tr("Map type"), x, y + 18.0, 86.0, 17.0, INK);
    let kinds: Vec<(i64, String)> = KINDS.iter().enumerate().map(|(i, l)| (i as i64, tr(l).to_string())).collect();
    if let Some(v) = dropdown("newmap:kind", x + 90.0, y, 160.0, o.kind as i64, &kinds) {
        o.kind = v as u8;
    }
    let orients: Vec<(i64, String)> = ORIENTATIONS.iter().enumerate().map(|(i, l)| (i as i64, tr(l).to_string())).collect();
    if let Some(v) = dropdown("newmap:orient", x + 260.0, y, 100.0, o.orient as i64, &orients) {
        o.orient = v as u8;
    }
    y += 38.0;
    text_fit(tr("Blur"), x, y + 18.0, 86.0, 17.0, INK);
    for b in 0..=MAX_BLUR {
        if toggle_button(x + 90.0 + b as f32 * 45.0, y, 41.0, 28.0, &b.to_string(), o.blur == b) {
            o.blur = b;
        }
    }
    y += 38.0;
    text_fit(tr("Seed"), x, y + 18.0, 86.0, 17.0, INK);
    if let Some(v) = number_field("newmap:seed", x + 90.0, y, 150.0, st.seed, i32::MIN as i64, i32::MAX as i64) {
        st.seed = v;
    }
    y += 34.0;
    if let Some(v) = checkbox(x, y, 360.0, tr("Keep: use this seed (0 takes the clock) and the last relief"), o.keep) {
        o.keep = v;
    }
    y += 28.0;
    if let Some(v) = checkbox(x + 24.0, y, 336.0, tr("Rebuild the relief"), o.rebuild) {
        o.rebuild = v;
    }
    y += 40.0;
    // The share bars.
    for (bar, label) in BAR_LABELS.iter().enumerate() {
        let names: Vec<&str> = tr(label).split(" | ").collect();
        text_fit(&names.join(" / "), x, y + 14.0, 360.0, 15.0, DIM);
        share_bar(st, bar, x, y + 20.0, &names);
        y += 46.0;
    }
    // Right: the preview and what the last run did.
    let side = (r.right() - 20.0 - (x + 440.0)).min(r.bottom() - 120.0 - (r.y + 60.0)).max(80.0);
    let (px, py) = (r.right() - 20.0 - side, r.y + 60.0);
    draw_rectangle(px, py, side, side, FIELD_BG);
    draw_rectangle_lines(px, py, side, side, 1.0, DIM);
    if let Some(t) = &st.preview {
        draw_texture_ex(t, px, py, WHITE, DrawTextureParams { dest_size: Some(vec2(side, side)), ..Default::default() });
    }
    let note = if busy {
        tr("Generating...").to_string()
    } else if let Some(out) = &st.last {
        let mut s = trf!("{w} x {h}, seed {seed}.", w = out.cells.w, h = out.cells.h, seed = out.seed);
        if let Some(stop) = out.stop {
            s.push(' ');
            s.push_str(&stop_text(stop));
        }
        s
    } else {
        tr("Create makes a map; Exit then takes it.").to_string()
    };
    for (i, line) in wrap(&note, side, 15.0).iter().take(5).enumerate() {
        text(line, px, py + side + 20.0 + i as f32 * 18.0, 15.0, INK);
    }
    // Buttons: create (break while running), exit after a run, cancel.
    let (bx, by) = (r.right() - 400.0, r.bottom() - 54.0);
    if busy {
        if button(bx, by, 120.0, 40.0, tr("Break"), true) {
            if let Some((_, brk)) = &st.running {
                brk.store(true, Ordering::Relaxed);
            }
        }
    } else if button(bx, by, 120.0, 40.0, tr("Create"), true) {
        let cells = open_map(st.options.size);
        st.start(sprites(), cells);
    }
    // The original's exit button does nothing before the first run.
    if button(bx + 130.0, by, 120.0, 40.0, tr("Exit"), !busy && st.last.is_some()) {
        if let Some(out) = st.last.take() {
            return NewMapAction::Accept(Box::new(out), st.options.shares);
        }
    }
    let esc = !typing() && is_key_pressed(KeyCode::Escape);
    if button(bx + 260.0, by, 120.0, 40.0, tr("Cancel"), true) || esc {
        return NewMapAction::Close(st.stop());
    }
    action
}

/// One bar: its panels in their colours, the splitters draggable.
fn share_bar(st: &mut NewMapState, bar: usize, x: f32, y: f32, names: &[&str]) {
    let s = st.options.shares;
    let panels = s.panels(bar);
    let colors = BAR_COLORS[bar];
    let h = 22.0;
    draw_rectangle(x - 1.0, y - 1.0, 106.0 * SCALE + 2.0, h + 2.0, FIELD_BG);
    // Units: left panels each with its splitter, the stretched one, then the right panel.
    let lefts = BARS[bar].iter().filter(|s| !s.right).count();
    let mut u = 0.0;
    let mut splitters = Vec::new();
    for (i, w) in panels.iter().enumerate() {
        let (r, g, b) = colors[i.min(colors.len() - 1)];
        let pw = *w as f32 * SCALE;
        draw_rectangle(x + u, y, pw, h, Color::new(r, g, b, 1.0));
        if mouse_in(x + u, y, pw, h) {
            let name = names.get(i).copied().unwrap_or("");
            tooltip(&[(format!("{name}: {w}"), INK)]);
        }
        u += pw;
        // A splitter after each left panel and before the right one.
        let k = if i < lefts { Some(i) } else if i == lefts && BARS[bar].len() > lefts { Some(lefts) } else { None };
        if let Some(k) = k {
            splitters.push((k, u / SCALE));
            draw_rectangle(x + u, y, 3.0 * SCALE, h, if st.drag.is_some_and(|d| d.0 == bar && d.1 == k) { ACCENT } else { DIM });
            u += 3.0 * SCALE;
        }
    }
    let (mx, _) = pointer();
    let at = (mx - x) / SCALE;
    if st.drag.is_none() && is_mouse_button_pressed(MouseButton::Left) {
        for &(k, start) in &splitters {
            if mouse_in(x + start * SCALE - 2.0, y, 3.0 * SCALE + 4.0, h) {
                st.drag = Some((bar, k, at - start));
            }
        }
    }
    if let Some((b, k, grab)) = st.drag {
        if b != bar {
            return;
        }
        if !is_mouse_button_down(MouseButton::Left) {
            st.drag = None;
            return;
        }
        let split_at = at - grab;
        let spl = BARS[bar][k];
        let size = if spl.right {
            106.0 - (split_at + 3.0)
        } else {
            // The panel starts after the left panels and splitters before it.
            let before: i32 = BARS[bar][..k].iter().map(|o| s.get(o.field) + 3).sum();
            split_at - before as f32
        };
        st.options.shares.drag(bar, k, size.round() as i32);
    }
}

/// The share widths a new dialog starts with: Razdor's editor ini, else the install's.
pub fn load_shares(install: Option<&std::path::Path>) -> Shares {
    Shares::load(razdor::editor::options::editor_dir().as_deref(), install)
}
