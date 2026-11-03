//! A file changed on disk while it is open: what the studio does with it. The decision is a
//! function of the document's state and the disk text the caller read; it does no I/O, so the
//! cases are unit-tested without a watcher (`studio::tests`).
//!
//! - The disk text is what the studio saved (its own write, seen again by the watcher): nothing.
//! - A clean document (its text is what it last saved or read) takes the disk text, and the
//!   change is one undo step labelled `outside change: <path>`.
//! - A dirty document keeps its text: the caller asks, and then [`Workspace::take_disk`] (reload)
//!   or [`Workspace::keep_mine`] (keep, the disk text becomes the saved baseline) runs.

use super::history::Step;
use super::Workspace;

/// What [`Workspace::on_disk_change`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reconcile {
    /// Not open, or the disk text is the saved text (or the document's own text).
    Ignored,
    /// A clean document took the disk text; one undo step records it.
    Reloaded,
    /// A dirty document differs from the disk: the user chooses.
    Asks { path: String, disk: String },
}

/// A unified diff from `mine` (the document's text) to `disk`, three lines of context, the
/// `@@` headers and the `+`/`-` lines as `diff -u` writes them.
pub fn unified(mine: &str, disk: &str) -> String {
    similar::TextDiff::from_lines(mine, disk).unified_diff().context_radius(3).header("studio", "disk").to_string()
}

impl Workspace {
    /// Decides what a change of the open file `path` on disk (now reading `disk`) means. See the
    /// module's rules. A file that is not open is ignored (the caller refreshes the file list).
    pub fn on_disk_change(&mut self, path: &str, disk: &str) -> Reconcile {
        let Some(i) = self.docs.iter().position(|d| d.path == path) else {
            return Reconcile::Ignored;
        };
        if self.docs[i].saved == disk || self.docs[i].text == disk {
            // Our own save, or a write that brought the same text: the baseline follows the disk.
            self.docs[i].saved = disk.to_string();
            return Reconcile::Ignored;
        }
        // An open text burst is recorded before the file's text is replaced.
        if self.burst_path() == Some(path) {
            self.end_text_edit();
        }
        if self.docs[i].is_dirty() {
            return Reconcile::Asks { path: path.to_string(), disk: disk.to_string() };
        }
        self.take(i, disk);
        Reconcile::Reloaded
    }

    /// The user chose «Reload»: the document takes the disk text, as an undo step.
    pub fn take_disk(&mut self, path: &str, disk: &str) -> Result<(), String> {
        let i = self.open_doc(path)?;
        self.end_text_edit();
        self.take(i, disk);
        Ok(())
    }

    /// The user chose «Keep mine»: the document keeps its text and stays dirty; the disk text is
    /// the saved baseline from now on.
    pub fn keep_mine(&mut self, path: &str, disk: &str) -> Result<(), String> {
        let i = self.open_doc(path)?;
        self.docs[i].saved = disk.to_string();
        Ok(())
    }

    /// Replaces document `i`'s text with the disk text and records it as one undo step.
    fn take(&mut self, i: usize, disk: &str) {
        let before = std::mem::replace(&mut self.docs[i].text, disk.to_string());
        self.docs[i].saved = disk.to_string();
        let path = self.docs[i].path.clone();
        self.history.record(Step::text(path.clone(), before, disk.to_string(), format!("outside change: {path}")));
    }
}
