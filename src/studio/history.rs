//! The package's undo and redo stacks. One step is one file's text before and after a change, with
//! the label the menu shows (`Undo: move army toll`). A new step clears the redo stack; the undo
//! stack keeps at most [`LIMIT`] steps.

/// The most steps the undo stack keeps.
pub const LIMIT: usize = 200;

/// One change of one file: its text before and after, and the label of the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub path: String,
    pub before: String,
    pub after: String,
    pub label: String,
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
        if step.before == step.after {
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
