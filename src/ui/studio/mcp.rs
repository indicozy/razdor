//! The window's side of the studio's MCP server (`studio::mcp`): the requests of the server's threads
//! are answered on the window's frames. The job, the state and the focus are answered at once; a
//! picture is taken from the framebuffer after the frame is drawn (`macroquad`'s screen grab), so it is
//! what the author sees.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

use macroquad::prelude as mq;
use macroquad::texture::{get_screen_data, Image};
use razdor::pack::load::{line_of, Package};
use razdor::pack::open::lang;
use razdor::studio::build::Job;
use razdor::studio::map::{self, Selection};
use razdor::studio::mcp::{FocusTarget, Frame, Host, SnapTarget};
use serde_json::{json, Value};

use super::{Jump, StudioScreen, jump_for};

/// How long a tool waits for the window's next frames before it gives up.
const ANSWER_WAIT: Duration = Duration::from_secs(20);

/// A request of the MCP server, to be answered by the window.
pub enum Request {
    Job(Sender<Result<Job, String>>),
    State(Sender<Result<Value, String>>),
    Focus(FocusTarget, Sender<Result<Value, String>>),
    Snapshot(SnapTarget, Sender<Result<Frame, String>>),
}

/// The server's view of the window: each call sends a request and waits for its answer.
pub struct UiHost {
    tx: Mutex<Sender<Request>>,
}

impl UiHost {
    /// The host, and the window's end of its requests.
    pub fn new() -> (UiHost, Receiver<Request>) {
        let (tx, rx) = channel();
        (UiHost { tx: Mutex::new(tx) }, rx)
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<Result<T, String>>) -> Request) -> Result<T, String> {
        let (tx, rx) = channel();
        let sent = self.tx.lock().map_err(|_| "the studio's request line is broken".to_string())?.send(make(tx));
        if sent.is_err() {
            return Err("Razdor Studio is closed".into());
        }
        rx.recv_timeout(ANSWER_WAIT).map_err(|_| "Razdor Studio did not answer in time".to_string())?
    }
}

impl Host for UiHost {
    fn job(&self) -> Result<Job, String> {
        self.ask(Request::Job)
    }

    fn state(&self) -> Result<Value, String> {
        self.ask(Request::State)
    }

    fn focus(&self, target: FocusTarget) -> Result<Value, String> {
        self.ask(|tx| Request::Focus(target, tx))
    }

    fn snapshot(&self, target: SnapTarget) -> Result<Frame, String> {
        self.ask(|tx| Request::Snapshot(target, tx))
    }
}

impl StudioScreen {
    /// Answers the requests that came in since the last frame. A picture waits for the frame's end.
    pub(super) fn serve_mcp(&mut self) {
        while let Ok(req) = self.mcp_rx.try_recv() {
            match req {
                Request::Job(tx) => {
                    let job = self.ws.job(self.maps.map.clone(), lang(), self.install.clone());
                    let _ = tx.send(Ok(job));
                }
                Request::State(tx) => {
                    let _ = tx.send(self.mcp_state());
                }
                Request::Focus(target, tx) => {
                    let _ = tx.send(self.mcp_focus(target));
                }
                Request::Snapshot(target, tx) => self.mcp_snaps.push((target, tx)),
            }
        }
    }

    /// The pictures asked for, taken from the frame just drawn (the screen grab of the framebuffer).
    pub(super) fn answer_snapshots(&mut self) {
        if self.mcp_snaps.is_empty() {
            return;
        }
        let image = get_screen_data();
        let pending: Vec<_> = self.mcp_snaps.drain(..).collect();
        for (target, tx) in pending {
            let answer = match target {
                SnapTarget::Window => whole(&image),
                SnapTarget::Canvas => match self.canvas.shown_view() {
                    Some(view) => crop(&image, view),
                    None => Err("the Map tab is not on screen: show it, then take the snapshot again".into()),
                },
            };
            let _ = tx.send(answer);
        }
    }

    /// The studio's state: the package, the map, the selection, the unsaved files and the stale file.
    fn mcp_state(&self) -> Result<Value, String> {
        let pkg = self.ws.package();
        let dirty: Vec<String> = self
            .ws
            .docs
            .iter()
            .filter(|d| d.is_dirty())
            .map(|d| d.path.clone())
            .chain(self.ws.terrains.iter().filter(|t| t.is_dirty()).map(|t| t.path.clone()))
            .collect();
        let stale = self.ws.stale.first().map(|s| json!({ "file": s.path, "line": s.line, "message": s.message }));
        Ok(json!({
            "package": self.ws.root.display().to_string(),
            "name": self.name,
            "map": self.maps.map,
            "selection": self.selection_json(pkg.as_deref()),
            "dirty": dirty,
            "stale": stale,
        }))
    }

    /// The selection as the agent reads it: its kind and id (or index), and its cell, file and line.
    fn selection_json(&self, pkg: Option<&Package>) -> Value {
        let Some(sel) = self.selection.as_ref() else { return Value::Null };
        let mut out = match sel {
            Selection::Hero => json!({ "kind": "hero" }),
            Selection::Building(id) => json!({ "kind": "building", "id": id }),
            Selection::Army(id) => json!({ "kind": "army", "id": id }),
            Selection::Point(i) => json!({ "kind": "point", "index": i }),
            Selection::Object(i) => json!({ "kind": "object", "index": i }),
        };
        let Some(map_id) = self.maps.map.as_deref() else { return out };
        let file = format!("maps/{map_id}/map.toml");
        out["map"] = json!(map_id);
        out["file"] = json!(file);
        let Some(spec) = pkg.and_then(|p| p.maps.get(map_id)).and_then(|m| m.spec.as_ref()) else { return out };
        if let Some(cell) = map::cell_of(spec, sel) {
            out["cell"] = json!(cell);
        }
        if let Some(loc) = map::locator_of(&file, spec, sel) {
            let at = self.ws.spans(&file).ok().and_then(|spans| spans.into_iter().find(|(l, _)| *l == loc));
            if let (Some((_, range)), Ok(text)) = (at, self.ws.text_of(&file)) {
                out["line"] = json!(line_of(&text, range.start));
            }
        }
        out
    }

    /// Points the studio at a table or a file: the map shown and the selection, the canvas centred on
    /// the table, the Script at its line; or a file opened in the Script at a line.
    fn mcp_focus(&mut self, target: FocusTarget) -> Result<Value, String> {
        match target {
            FocusTarget::File { path, line } => {
                if !self.ws.entries.contains(&path) || !razdor::studio::is_text(&path) {
                    return Err(format!("{path} is not a text file of the package"));
                }
                self.open_file(&path);
                self.jump = line.map(|line| Jump { line, range: None });
                Ok(json!({ "ok": true, "file": path, "line": line }))
            }
            FocusTarget::Object { map: wanted, sel } => {
                let map_id = match wanted {
                    Some(m) => m,
                    None => self.maps.map.clone().ok_or("no map is open on the canvas")?,
                };
                let pkg = self.ws.package().ok_or("the package could not be read: see Output")?;
                let spec = pkg
                    .maps
                    .get(&map_id)
                    .and_then(|m| m.spec.as_ref())
                    .ok_or_else(|| format!("the package has no map `{map_id}`"))?;
                let centre = map::centre_of(spec, &sel).ok_or_else(|| format!("the map `{map_id}` has no such table"))?;
                let file = format!("maps/{map_id}/map.toml");
                let jump = jump_for(&self.ws, Some(&pkg), &file, &sel);
                if self.maps.map.as_deref() != Some(map_id.as_str()) {
                    let selection = &mut self.selection;
                    super::map_tab::switch_map(&mut self.maps, &mut self.canvas, selection, &map_id);
                    self.reload();
                }
                self.selection = Some(sel);
                self.canvas.centre = Some(centre);
                self.jump = jump;
                let shown = self.selection_json(Some(&pkg));
                Ok(json!({ "ok": true, "selection": shown }))
            }
        }
    }
}

/// The whole frame as RGBA, rows from the top. The grab returns the rows bottom first (GL's order).
fn whole(image: &Image) -> Result<Frame, String> {
    let (w, h) = (usize::from(image.width), usize::from(image.height));
    rows_from_top(image, w, h, 0, h, 0, w)
}

/// The part of the frame `view` (in pixels, from the top left) as RGBA, rows from the top.
fn crop(image: &Image, view: mq::Rect) -> Result<Frame, String> {
    let (iw, ih) = (usize::from(image.width), usize::from(image.height));
    let x0 = (view.x.max(0.0).floor() as usize).min(iw);
    let y0 = (view.y.max(0.0).floor() as usize).min(ih);
    let x1 = ((view.x + view.w).ceil().max(0.0) as usize).min(iw);
    let y1 = ((view.y + view.h).ceil().max(0.0) as usize).min(ih);
    if x1 <= x0 || y1 <= y0 {
        return Err("the canvas is off screen".into());
    }
    rows_from_top(image, iw, ih, y0, y1, x0, x1)
}

/// The rectangle `[y0, y1) × [x0, x1)` of the frame, with its rows put from the top: the grab's row
/// `r` is the screen's row `ih - 1 - r`.
fn rows_from_top(image: &Image, iw: usize, ih: usize, y0: usize, y1: usize, x0: usize, x1: usize) -> Result<Frame, String> {
    if image.bytes.len() != iw * ih * 4 {
        return Err(format!("the grab has {} bytes for {iw}x{ih} pixels", image.bytes.len()));
    }
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        let row = (ih - 1 - y) * iw * 4;
        rgba.extend_from_slice(&image.bytes[row + x0 * 4..row + x1 * 4]);
    }
    Ok(Frame { width: (x1 - x0) as u32, height: (y1 - y0) as u32, rgba })
}
