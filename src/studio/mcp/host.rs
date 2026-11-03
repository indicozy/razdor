//! What the MCP tools need from the window: the package as it is in memory, the studio's state,
//! the focus, and the canvas's picture. The window implements it through its UI thread; the tests
//! implement it with a fake. Every method may wait for the window's next frame (a few milliseconds),
//! and fails with a reason the agent can read.

use serde_json::Value;

use crate::studio::build::Job;
use crate::studio::map::Selection;

/// Where a picture is taken from: the Map tab's canvas, or the whole studio window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapTarget {
    Canvas,
    Window,
}

/// A picture of the framebuffer: RGBA, rows from the top, `width` × `height` pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What `focus` points the studio's view at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// A table of a map: its selection, the canvas centred on it, the Script at its table. `map`
    /// is the shown map when `None`.
    Object { map: Option<String>, sel: Selection },
    /// A file opened in the Script, its caret at `line` (1-based) when given.
    File { path: String, line: Option<u32> },
}

/// The window, as the MCP tools see it.
pub trait Host: Send + Sync {
    /// The package as the studio has it now: the folder with the open texts and the painted terrains
    /// over it, the shown map as `map`, and the game's install when there is one. The checks and the
    /// sim are run on this job, off the window's thread.
    fn job(&self) -> Result<Job, String>;

    /// The studio's state (the `get_state` tool's reply): the package, the open map, the selection,
    /// the dirty files and the stale file.
    fn state(&self) -> Result<Value, String>;

    /// Points the studio at a table or a file (the `focus` tool). The reply says what it did.
    fn focus(&self, target: FocusTarget) -> Result<Value, String>;

    /// A picture of the canvas or the window, as the window drew it on its last frame.
    fn snapshot(&self, target: SnapTarget) -> Result<Frame, String>;
}
