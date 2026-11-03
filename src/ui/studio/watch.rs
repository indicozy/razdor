//! Watches the package folder for changes made outside the studio (another editor, an agent, a
//! `git checkout`). The events are only collected here; the screen waits for them to settle and
//! then asks the model what each change means (`studio::Workspace::on_disk_change`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// The changed paths of one batch, as [`Watch::changed`] gives them. An empty path means the
/// watcher lost events (a queue overflow): every open file must be read again.
pub const ALL: &str = "";

pub struct Watch {
    /// Kept alive: the watcher stops when it is dropped.
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<notify::Event>>,
    root: PathBuf,
}

impl Watch {
    /// Starts watching `root` (the package folder, recursively). An error when the platform's
    /// watcher cannot start; the studio then goes on without outside changes.
    pub fn start(root: &Path) -> Result<Watch, String> {
        let root = std::fs::canonicalize(root).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(|e| e.to_string())?;
        watcher.watch(&root, RecursiveMode::Recursive).map_err(|e| e.to_string())?;
        Ok(Watch { _watcher: watcher, rx, root })
    }

    /// The package-relative paths (`/` separators) changed since the last call, sorted and
    /// without repeats; [`ALL`] when events were lost.
    pub fn changed(&self) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Ok(event) => {
                    for path in event.paths {
                        if let Ok(rel) = path.strip_prefix(&self.root) {
                            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                            if !rel.is_empty() {
                                out.push(rel.join("/"));
                            }
                        }
                    }
                }
                Err(_) => out.push(ALL.to_string()),
            }
        }
        out.sort();
        out.dedup();
        out
    }
}
