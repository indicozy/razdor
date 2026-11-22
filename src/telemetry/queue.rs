//! The on-disk queue of the analytics: one file per item (`<seq>.item`), so it survives a restart.
//! An item file is its [`Meta`] as one JSON line, then the body's bytes. The total is capped
//! (`razdor_wire::limits::CLIENT_QUEUE_BYTES`); the oldest items go first. Only the file names and
//! the metas live in memory, never the bodies.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which items wait for the consent: the install's ping and its delete request go always, the rest
/// only while the player has said yes (`telemetry::Hub`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Always,
    Consent,
}

/// One queued request: where it goes, how it is framed, and when it may be tried next.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub seq: u64,
    /// The API path (`razdor_wire::paths`), added to the address.
    pub path: String,
    pub class: Class,
    /// The frame's JSON header (`razdor_wire::frame`). `None`: the body is the JSON text itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<serde_json::Value>,
    /// This item's own token: a crash report's id on the server is stored under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The token of the crash report whose server id this item needs (a crash's save).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default)]
    pub tries: u32,
    /// Unix seconds before which the item is not tried.
    #[serde(default)]
    pub not_before: u64,
}

/// Why an item is not queued.
#[derive(Debug, PartialEq, Eq)]
pub enum PushError {
    /// The item alone is bigger than the whole cap: it is refused, never kept.
    TooBig { len: u64, cap: u64 },
    Io(String),
}

pub struct Queue {
    dir: PathBuf,
    cap: u64,
    /// The items in order, oldest first, with each file's size.
    items: VecDeque<(Meta, u64)>,
    next_seq: u64,
}

impl Queue {
    /// Opens the queue in `dir` (made if missing), reading the items already there. A file that does
    /// not read back is removed.
    pub fn open(dir: &Path, cap: u64) -> io::Result<Queue> {
        std::fs::create_dir_all(dir)?;
        let mut found: Vec<(Meta, u64)> = Vec::new();
        for entry in std::fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "item") {
                continue;
            }
            match read_item(&path) {
                Ok(meta) => found.push((meta, entry.metadata().map_or(0, |m| m.len()))),
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        found.sort_by_key(|(m, _)| m.seq);
        let next_seq = found.last().map_or(1, |(m, _)| m.seq + 1);
        Ok(Queue { dir: dir.to_path_buf(), cap, items: found.into(), next_seq })
    }

    fn file(&self, seq: u64) -> PathBuf {
        self.dir.join(format!("{seq:020}.item"))
    }

    /// Adds an item. Its body is `body`. Oldest items are dropped until the total fits the cap.
    /// Returns the item's seq.
    pub fn push(&mut self, mut meta: Meta, body: &[u8]) -> Result<u64, PushError> {
        let mut head = serde_json::to_vec(&meta).map_err(|e| PushError::Io(e.to_string()))?;
        head.push(b'\n');
        let len = (head.len() + body.len()) as u64;
        if len > self.cap {
            return Err(PushError::TooBig { len, cap: self.cap });
        }
        meta.seq = self.next_seq;
        self.next_seq += 1;
        // The seq is in the header too: rewrite the head with it.
        let mut head = serde_json::to_vec(&meta).map_err(|e| PushError::Io(e.to_string()))?;
        head.push(b'\n');
        let len = (head.len() + body.len()) as u64;
        let path = self.file(meta.seq);
        let written = std::fs::File::create(&path).and_then(|mut f| {
            f.write_all(&head)?;
            f.write_all(body)
        });
        if let Err(e) = written {
            let _ = std::fs::remove_file(&path);
            return Err(PushError::Io(e.to_string()));
        }
        let seq = meta.seq;
        self.items.push_back((meta, len));
        while self.total() > self.cap {
            let Some((old, _)) = self.items.pop_front() else { break };
            let _ = std::fs::remove_file(self.file(old.seq));
        }
        Ok(seq)
    }

    /// The bytes of all queued files (what the cap counts).
    pub fn total(&self) -> u64 {
        self.items.iter().map(|(_, n)| n).sum()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The items, oldest first.
    pub fn metas(&self) -> impl Iterator<Item = &Meta> {
        self.items.iter().map(|(m, _)| m)
    }

    /// The body of the item `seq` (its bytes after the header line).
    pub fn body(&self, seq: u64) -> io::Result<Vec<u8>> {
        let bytes = std::fs::read(self.file(seq))?;
        let at = bytes.iter().position(|&b| b == b'\n').ok_or_else(|| io::Error::other("no header"))?;
        Ok(bytes[at + 1..].to_vec())
    }

    /// Removes the item `seq` (sent, refused or dropped).
    pub fn remove(&mut self, seq: u64) {
        if let Some(pos) = self.items.iter().position(|(m, _)| m.seq == seq) {
            self.items.remove(pos);
            let _ = std::fs::remove_file(self.file(seq));
        }
    }

    /// Sets when the item `seq` may be tried next, and how many times it was tried. Rewrites its header.
    pub fn reschedule(&mut self, seq: u64, tries: u32, not_before: u64) -> io::Result<()> {
        let Some(pos) = self.items.iter().position(|(m, _)| m.seq == seq) else { return Ok(()) };
        let body = self.body(seq)?;
        let mut meta = self.items[pos].0.clone();
        meta.tries = tries;
        meta.not_before = not_before;
        let mut head = serde_json::to_vec(&meta).map_err(io::Error::other)?;
        head.push(b'\n');
        let mut out = head;
        out.extend_from_slice(&body);
        std::fs::write(self.file(seq), &out)?;
        self.items[pos] = (meta, out.len() as u64);
        Ok(())
    }

    /// Removes every item of the `Consent` class (the player said no, or deleted the data).
    pub fn purge_consent(&mut self) {
        let doomed: Vec<u64> = self.items.iter().filter(|(m, _)| m.class == Class::Consent).map(|(m, _)| m.seq).collect();
        for seq in doomed {
            self.remove(seq);
        }
    }

    /// Removes every item.
    pub fn purge_all(&mut self) {
        let all: Vec<u64> = self.items.iter().map(|(m, _)| m.seq).collect();
        for seq in all {
            self.remove(seq);
        }
    }
}

fn read_item(path: &Path) -> io::Result<Meta> {
    let bytes = std::fs::read(path)?;
    let at = bytes.iter().position(|&b| b == b'\n').ok_or_else(|| io::Error::other("no header"))?;
    serde_json::from_slice(&bytes[..at]).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(path: &str, class: Class) -> Meta {
        Meta { seq: 0, path: path.into(), class, header: None, token: None, after: None, tries: 0, not_before: 0 }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-queue-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn items_survive_a_reopen_in_order_with_their_bodies() {
        let d = dir("reopen");
        {
            let mut q = Queue::open(&d, 1 << 20).unwrap();
            q.push(meta("/v1/ping", Class::Always), b"{\"a\":1}").unwrap();
            q.push(meta("/v1/events", Class::Consent), b"{\"b\":2}").unwrap();
        }
        let q = Queue::open(&d, 1 << 20).unwrap();
        let paths: Vec<&str> = q.metas().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["/v1/ping", "/v1/events"]);
        let first = q.metas().next().unwrap().seq;
        assert_eq!(q.body(first).unwrap(), b"{\"a\":1}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_cap_drops_the_oldest_items_and_keeps_the_total_under_it() {
        let d = dir("cap");
        let cap = 5 * 1024 * 1024;
        let mut q = Queue::open(&d, cap).unwrap();
        let body = vec![b'x'; 1024 * 1024];
        for _ in 0..8 {
            q.push(meta("/v1/perf", Class::Consent), &body).unwrap();
        }
        assert!(q.total() <= cap, "total {}", q.total());
        assert!(q.len() <= 5 && q.len() >= 4, "len {}", q.len());
        let newest = q.metas().last().unwrap().seq;
        assert_eq!(newest, 8, "the newest item is kept");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_item_bigger_than_the_cap_is_refused_and_nothing_is_dropped_for_it() {
        let d = dir("big");
        let mut q = Queue::open(&d, 1024).unwrap();
        q.push(meta("/v1/ping", Class::Always), b"small").unwrap();
        let err = q.push(meta("/v1/crash/save", Class::Consent), &vec![0u8; 4096]).unwrap_err();
        assert!(matches!(err, PushError::TooBig { .. }));
        assert_eq!(q.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn purging_consent_items_keeps_the_always_ones() {
        let d = dir("purge");
        let mut q = Queue::open(&d, 1 << 20).unwrap();
        q.push(meta("/v1/ping", Class::Always), b"p").unwrap();
        q.push(meta("/v1/events", Class::Consent), b"e").unwrap();
        q.purge_consent();
        let paths: Vec<&str> = q.metas().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["/v1/ping"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
