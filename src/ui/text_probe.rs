//! The text-size probe (`RAZDOR_TEXT_PROBE=<file.tsv>`, snapshot runs only): every text drawn in
//! the frame the snapshot saves is listed with its place and size, for
//! `tools/ui-regress/text_size.py`. Off unless the variable is set; the game never sets it.

use std::cell::RefCell;
use razdor::local;
use std::sync::OnceLock;

/// One text drawn: its words, the pen's x and baseline y, its width, the size asked for and the
/// height of a capital at that size (px).
pub struct Drawn {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub size: f32,
    pub cap: f32,
}

local! {
    static LOG: RefCell<Vec<Drawn>> = const { RefCell::new(Vec::new()) };
}

/// The file the probe writes, if any.
fn path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("RAZDOR_TEXT_PROBE").ok().filter(|p| !p.is_empty())).as_deref()
}

/// Whether the probe is on.
pub fn on() -> bool { path().is_some() }

/// A new frame: the list starts again.
pub fn begin_frame() {
    if on() {
        LOG.with(|l| l.borrow_mut().clear())
    }
}

/// Notes one text drawn (whitespace-only texts are skipped).
pub fn record(text: &str, x: f32, y: f32, w: f32, size: f32, cap: f32) {
    if !text.trim().is_empty() {
        LOG.with(|l| {
            l.borrow_mut().push(Drawn {
                text: text.to_string(),
                x,
                y,
                w,
                size,
                cap,
            });
        });
    }
}

/// Writes the frame's list: a header line `# dp<TAB><dp or 0><TAB>screen<TAB><w>x<h>` (dp from
/// `super::display::dp_px()`, screen from `macroquad::prelude::screen_width()/screen_height()`
/// rounded), then one line per text: `size<TAB>cap<TAB>x<TAB>y<TAB>w<TAB>text` (numbers with one
/// decimal; tabs, CR and LF in the text replaced by spaces). Errors are ignored.
pub fn write() {
    let Some(path) = path() else { return };
    let screen_w = macroquad::prelude::screen_width().round() as i32;
    let screen_h = macroquad::prelude::screen_height().round() as i32;
    let dp = super::display::dp_px();

    LOG.with(|l| {
        let logs = l.borrow();
        let mut output = String::new();

        // Header line
        output.push_str(&format!("# dp\t{}\tscreen\t{}x{}\n", dp.unwrap_or(0.0), screen_w, screen_h));

        for drawn in &*logs {
            let text = drawn.text.replace(['\t', '\n', '\r'], " ");
            output.push_str(&format!(
                "{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\n",
                drawn.size,
                drawn.cap,
                drawn.x,
                drawn.y,
                drawn.w,
                text
            ));
        }

        // Write to file (ignoring errors)
        std::fs::write(path, output).ok();
    });
}