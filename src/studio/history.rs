//! The package's undo and redo stacks. One step is one change with the label the menu shows
//! (`Undo: move army toll`): a text file's text before and after, or a rectangle of a terrain's
//! cells before and after a paint stroke. A new step clears the redo stack; the undo stack keeps
//! at most [`LIMIT`] steps.

/// The most steps the undo stack keeps.
pub const LIMIT: usize = 200;

/// What a step changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A text file's text before and after (a visual edit, a burst of typing, an outside change).
    Text { path: String, before: String, after: String },
    /// A terrain's cells in a rectangle (`x`, `y`, `w`, `h`, row-major), before and after a stroke.
    Cells { path: String, x: u32, y: u32, w: u32, h: u32, before: Vec<u8>, after: Vec<u8> },
    /// Several changes made by one operation (a burn, a paste, a forest stroke): one undo step.
    /// Undo puts the members back in reverse order, redo applies them in order.
    Group(Vec<Change>),
    /// A package file made or removed as a whole (a generated map's files): its bytes before and
    /// after, `None` where the file is not there (undo removes a file that was made).
    File { path: String, before: Option<Vec<u8>>, after: Option<Vec<u8>> },
}

impl Change {
    /// The file the change is of (a group's first member's; empty for an empty group).
    pub fn path(&self) -> &str {
        match self {
            Change::Text { path, .. } | Change::Cells { path, .. } | Change::File { path, .. } => path,
            Change::Group(v) => v.first().map_or("", Change::path),
        }
    }

    /// Whether the change leaves its files as they were.
    pub fn is_noop(&self) -> bool {
        match self {
            Change::Text { before, after, .. } => before == after,
            Change::Cells { before, after, .. } => before == after,
            Change::File { before, after, .. } => before == after,
            Change::Group(v) => v.iter().all(Change::is_noop),
        }
    }
}

/// One change and the label of the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub label: String,
    pub change: Change,
}

impl Step {
    /// A text change of `path`.
    pub fn text(path: impl Into<String>, before: impl Into<String>, after: impl Into<String>, label: impl Into<String>) -> Step {
        Step { label: label.into(), change: Change::Text { path: path.into(), before: before.into(), after: after.into() } }
    }

    /// The file the change is of.
    pub fn path(&self) -> &str {
        self.change.path()
    }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Records a new change: it goes on the undo stack and the redo stack is cleared. A step that
    /// changes nothing is not recorded.
    pub fn record(&mut self, step: Step) {
        if step.change.is_noop() {
            return;
        }
        self.undo.push(step);
        if self.undo.len() > LIMIT {
            let excess = self.undo.len() - LIMIT;
            self.undo.drain(..excess);
        }
        self.redo.clear();
    }

    /// The label of the step Ctrl+Z undoes.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// The label of the step Ctrl+Shift+Z redoes.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn pop_undo(&mut self) -> Option<Step> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<Step> {
        self.redo.pop()
    }

    /// Puts a step back on the undo stack after an undo or a redo (the redo stack is kept).
    pub fn push_undo(&mut self, step: Step) {
        self.undo.push(step);
    }

    /// Puts a step back on the redo stack after an undo (the undo stack is kept).
    pub fn push_redo(&mut self, step: Step) {
        self.redo.push(step);
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
}
