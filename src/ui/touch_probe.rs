//! The touch probe (`RAZDOR_TOUCH_PROBE=<file.tsv>`, snapshot runs only): every touch target
//! drawn in the frame the snapshot saves (a button's or a tap's hit rectangle) is listed with its
//! kind, its place in the source and whether it is enabled, for `tools/ui-regress/mobile_ui.py`.
//! Off unless the variable is set; the game never sets it. Like the text probe
//! (`super::text_probe`), it costs nothing when off.

use std::cell::RefCell;
use razdor::local;
use std::sync::OnceLock;

use macroquad::prelude::Rect;

/// One touch target: its kind (`button` or `tap`), its hit rectangle (px), whether it takes
/// input, and the place that asked for it (`file:line`, the screen's call site).
pub struct Touch {
    pub kind: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub enabled: bool,
    pub place: String,
}

local! {
    static LOG: RefCell<Vec<Touch>> = const { RefCell::new(Vec::new()) };
}

/// The file the probe writes, if any.
fn path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("RAZDOR_TOUCH_PROBE").ok().filter(|p| !p.is_empty())).as_deref()
}

/// Whether the probe is on.
pub fn on() -> bool { path().is_some() }

/// A new frame: the list starts again.
pub fn begin_frame() {
    if on() {
        LOG.with(|l| l.borrow_mut().clear())
    }
}

/// Notes one touch target drawn in `r`, from the place that called (the caller's location, through
/// the `#[track_caller]` helpers between it and the screen). Nothing when the probe is off.
#[track_caller]
pub fn record(kind: &'static str, r: Rect, enabled: bool) {
    if !on() {
        return;
    }
    let loc = std::panic::Location::caller();
    let touch = Touch {
        kind,
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        enabled,
        place: format!("{}:{}", loc.file(), loc.line()),
    };
    LOG.with(|l| l.borrow_mut().push(touch));
}

/// One touch target as a line of the file: `kind<TAB>x<TAB>y<TAB>w<TAB>h<TAB>enabled<TAB>place`,
/// the numbers with one decimal, `enabled` 1 or 0. Tabs, CR and LF in the place are replaced by
/// spaces.
pub fn line(t: &Touch) -> String {
    format!(
        "{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}",
        t.kind,
        t.x,
        t.y,
        t.w,
        t.h,
        if t.enabled { 1 } else { 0 },
        t.place.replace(['\t', '\n', '\r'], " ")
    )
}

/// Writes the frame's list: a header line `# dp<TAB><dp or 0><TAB>screen<TAB><w>x<h>` (as the text
/// probe's: dp from `super::display::dp_px()`, screen from `macroquad::prelude::screen_width()/
/// screen_height()` rounded), then one [`line`] per target. Errors are ignored.
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

        for touch in &*logs {
            output.push_str(&line(touch));
            output.push('\n');
        }

        // Write to file (ignoring errors)
        std::fs::write(path, output).ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_formats_a_target() {
        let t = Touch {
            kind: "button",
            x: 10.0,
            y: 20.26,
            w: 300.0,
            h: 44.0,
            enabled: false,
            place: "src/ui/building_view.rs:2210".to_string(),
        };
        assert_eq!(line(&t), "button\t10.0\t20.3\t300.0\t44.0\t0\tsrc/ui/building_view.rs:2210");
        let on = Touch { enabled: true, place: "a\tb".to_string(), ..t };
        assert_eq!(line(&on), "button\t10.0\t20.3\t300.0\t44.0\t1\ta b");
    }

    #[track_caller]
    fn caller_place() -> &'static str {
        std::panic::Location::caller().file()
    }

    #[test]
    fn track_caller_reports_the_test_file() {
        assert_eq!(caller_place(), "src/ui/touch_probe.rs");
    }
}
