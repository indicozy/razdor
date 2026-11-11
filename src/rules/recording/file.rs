//! The replay file `.rzreplay`: a magic, the format's version, the header (`ReplayHeader`, JSON),
//! then blocks `[tag u8][length u32 LE][payload]`:
//!
//! - tag 1, the header (JSON);
//! - tag 2, a keyframe (the state's hash at a stamp, with the state's packed snapshot): a
//!   checkpoint inside a segment, every 10 game days; playback checks it;
//! - tag 3, a segment's start (a keyframe that begins a new segment: a new game, or a load);
//! - tag 4, entries (the stream's encoding of the day's entries, `stream::encode`);
//! - tag 5, a battle's start (`BattleStart`, JSON), for a battle's cut-out.
//!
//! A segment is a start keyframe and the entries after it up to the next start. Keyframes are
//! packed (bzip2) and written on a writer thread, in the order they were sent; the frame only
//! serialises them. Truncation (a load of an earlier save) cuts the file at the save's length.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use super::battle_start::BattleStart;
use super::header::ReplayHeader;
use super::stream::{self, Entry, EngineInput, Input, Stamp};

pub const MAGIC: &[u8; 4] = b"RZRP";
/// The file's format version (after the magic).
pub const FILE_FORMAT: u32 = 1;

const TAG_HEADER: u8 = 1;
const TAG_KEYFRAME: u8 = 2;
const TAG_SEGMENT: u8 = 3;
const TAG_ENTRIES: u8 = 4;
const TAG_BATTLE: u8 = 5;
/// A keyframe's payload before its packed snapshot: the stamp (12), the game day (8), the hash (8).
const KEYFRAME_HEAD: usize = 28;

/// The bzip2 level of a keyframe's packed snapshot (1 to 9): keyframes are many, so a small block packs fast. The reader
/// unpacks any level.
const KEYFRAME_LEVEL: u32 = 2;

/// A job for the writer thread.
enum Job {
    /// A block with its payload as it is written.
    Block(u8, Vec<u8>),
    /// A keyframe: its stamp, the game day, the state's hash, and the state's JSON (packed by
    /// the thread).
    Keyframe { tag: u8, at: Stamp, day: u64, hash: u64, json: Vec<u8> },
    /// Everything before is written; the file's length goes back.
    Sync,
}

/// The file's writer: appends blocks on a thread.
pub struct Writer {
    tx: Sender<Job>,
    lengths: Receiver<u64>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

/// Where a save's position in a replay file is: the file's name and its length then. The name is
/// untrusted (a save is shared): only [`replay_name_ok`] names are ever opened, cut or removed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReplayRef {
    pub file: String,
    pub pos: u64,
}

/// Whether `name` is a replay file name the game makes: `replay-<digits>.rzreplay`, a bare name.
/// Separators, `..`, absolute and drive paths, and anything else are refused, so a name read from
/// a save can name nothing outside the replays folder.
pub fn replay_name_ok(name: &str) -> bool {
    let Some(digits) = name.strip_prefix("replay-").and_then(|r| r.strip_suffix(".rzreplay")) else { return false };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Deserializes a save's replay place: a place whose name the game could make; any other place
/// is none, and the save loads with no replay.
pub fn replay_place<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<ReplayRef>, D::Error> {
    use serde::Deserialize;
    Ok(Option::<ReplayRef>::deserialize(d)?.filter(|r| replay_name_ok(&r.file)))
}

fn put_block(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

fn put_stamp(out: &mut Vec<u8>, at: Stamp) {
    out.extend_from_slice(&at.stretch.to_le_bytes());
    out.extend_from_slice(&at.ask.to_le_bytes());
}

impl Writer {
    /// Opens (or creates) the file: `truncate_to` cuts it first (a resume at a save's length), and a
    /// new file gets its magic and header.
    pub fn open(path: &Path, header: &ReplayHeader, truncate_to: Option<u64>) -> Result<Writer, String> {
        let io = |e: std::io::Error| e.to_string();
        // A file is cut only when it is a replay (its magic): anything else is not touched.
        if truncate_to.is_some() {
            let mut head = [0u8; 4];
            let is_replay = File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == MAGIC;
            if !is_replay {
                return Err("not a replay file to resume".into());
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(io)?;
        let len = match truncate_to {
            Some(pos) => {
                let have = file.metadata().map_err(io)?.len();
                if pos > have {
                    return Err(format!("the replay ends at {have}, not at {pos}"));
                }
                file.set_len(pos).map_err(io)?;
                pos
            }
            None => 0,
        };
        file.seek(SeekFrom::Start(len)).map_err(io)?;
        let mut len = len;
        if len == 0 {
            let mut head = MAGIC.to_vec();
            head.extend_from_slice(&FILE_FORMAT.to_le_bytes());
            put_block(&mut head, TAG_HEADER, &serde_json::to_vec(header).map_err(|e| e.to_string())?);
            file.write_all(&head).map_err(io)?;
            len = head.len() as u64;
        }
        let (tx, rx) = channel::<Job>();
        let (ltx, lrx) = channel::<u64>();
        let thread = std::thread::Builder::new()
            .name("replay-writer".into())
            .spawn(move || {
                let mut len = len;
                for job in rx {
                    let block = match job {
                        Job::Block(tag, payload) => {
                            let mut out = Vec::new();
                            put_block(&mut out, tag, &payload);
                            Some(out)
                        }
                        Job::Keyframe { tag, at, day, hash, json } => {
                            let packed = match crate::rules::save::pack_with(&json, bzip2::Compression::new(KEYFRAME_LEVEL)) {
                                Ok(p) => p,
                                Err(_) => continue,
                            };
                            let mut payload = Vec::with_capacity(packed.len() + KEYFRAME_HEAD);
                            put_stamp(&mut payload, at);
                            payload.extend_from_slice(&day.to_le_bytes());
                            payload.extend_from_slice(&hash.to_le_bytes());
                            payload.extend_from_slice(&packed);
                            let mut out = Vec::new();
                            put_block(&mut out, tag, &payload);
                            Some(out)
                        }
                        Job::Sync => {
                            let _ = ltx.send(len);
                            None
                        }
                    };
                    if let Some(out) = block {
                        if file.write_all(&out).is_ok() {
                            len += out.len() as u64;
                        }
                    }
                }
                let _ = file.sync_data();
            })
            .map_err(io)?;
        Ok(Writer { tx, lengths: lrx, thread: Some(thread), path: path.to_path_buf() })
    }

    /// Entries (one day's, or any) as a block.
    pub fn entries(&self, entries: &[Entry]) {
        if !entries.is_empty() {
            let _ = self.tx.send(Job::Block(TAG_ENTRIES, stream::encode(entries)));
        }
    }

    /// A battle's start as a block.
    pub fn battle(&self, start: &BattleStart) {
        if let Ok(json) = serde_json::to_vec(start) {
            let _ = self.tx.send(Job::Block(TAG_BATTLE, json));
        }
    }

    /// A keyframe (`segment` for a segment's start), packed on the thread.
    pub fn keyframe(&self, segment: bool, at: Stamp, day: u64, hash: u64, json: Vec<u8>) {
        let tag = if segment { TAG_SEGMENT } else { TAG_KEYFRAME };
        let _ = self.tx.send(Job::Keyframe { tag, at, day, hash, json });
    }

    /// Waits until everything sent is written; the file's length then.
    pub fn sync(&self) -> u64 {
        if self.tx.send(Job::Sync).is_err() {
            return 0;
        }
        self.lengths.recv().unwrap_or(0)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let (tx, _) = channel::<Job>();
        let old = std::mem::replace(&mut self.tx, tx);
        drop(old);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A segment as it is read: its start (the packed snapshot), its entries (checks included), and
/// the battles begun in it.
#[derive(Debug, Clone)]
pub struct Segment {
    pub start_at: Stamp,
    pub start_day: u64,
    pub start_hash: u64,
    /// The packed snapshot the segment starts from (`Snapshot::from_bytes`).
    pub snapshot: Vec<u8>,
    pub entries: Vec<Entry>,
    /// The battles begun in it, each with the number of entries before its start (`after`).
    pub battles: Vec<(usize, BattleStart)>,
    /// The checkpoints after the start, in order (each one's `after` counts the entries before it).
    pub keyframes: Vec<Keyframe>,
}

/// A checkpoint inside a segment: the state (packed) at a place of the stream. Playback that
/// starts from it applies `entries[after..]`.
#[derive(Debug, Clone)]
pub struct Keyframe {
    pub at: Stamp,
    /// The game day the state is in (`Clock::day_index`).
    pub day: u64,
    pub hash: u64,
    pub after: usize,
    /// The packed snapshot (`Snapshot::from_bytes`).
    pub snapshot: Vec<u8>,
}

/// A replay file as it is read.
#[derive(Debug, Clone)]
pub struct ReplayFile {
    pub header: ReplayHeader,
    pub segments: Vec<Segment>,
    /// The file's length.
    pub len: u64,
}

fn get_stamp(b: &[u8]) -> Result<Stamp, String> {
    if b.len() < 12 {
        return Err("a keyframe without its place".into());
    }
    Ok(Stamp { stretch: u64::from_le_bytes(b[0..8].try_into().unwrap()), ask: u32::from_le_bytes(b[8..12].try_into().unwrap()) })
}

/// Reads a replay file: its header and segments. A damaged block ends the file there (a crash
/// while writing), and everything before it is kept.
pub fn read(path: &Path) -> Result<ReplayFile, String> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|mut f| f.read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if bytes.len() < 8 || &bytes[0..4] != MAGIC {
        return Err("not a replay file".into());
    }
    let format = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if format != FILE_FORMAT {
        return Err(format!("a replay of format {format}"));
    }
    let mut at = 8usize;
    let mut header: Option<ReplayHeader> = None;
    let mut segments: Vec<Segment> = Vec::new();
    while at + 5 <= bytes.len() {
        let tag = bytes[at];
        let n = u32::from_le_bytes(bytes[at + 1..at + 5].try_into().unwrap()) as usize;
        let start = at + 5;
        if start + n > bytes.len() {
            break;
        }
        let payload = &bytes[start..start + n];
        match tag {
            TAG_HEADER => header = Some(serde_json::from_slice(payload).map_err(|e| e.to_string())?),
            TAG_SEGMENT | TAG_KEYFRAME => {
                if payload.len() < KEYFRAME_HEAD {
                    return Err("a keyframe without its head".into());
                }
                let at_stamp = get_stamp(payload)?;
                let day = u64::from_le_bytes(payload[12..20].try_into().unwrap());
                let hash = u64::from_le_bytes(payload[20..28].try_into().unwrap());
                let snapshot = payload[KEYFRAME_HEAD..].to_vec();
                if tag == TAG_SEGMENT {
                    segments.push(Segment {
                        start_at: at_stamp,
                        start_day: day,
                        start_hash: hash,
                        snapshot,
                        entries: Vec::new(),
                        battles: Vec::new(),
                        keyframes: Vec::new(),
                    });
                } else {
                    let seg = segments.last_mut().ok_or("a checkpoint before any segment")?;
                    seg.entries.push(Entry { at: at_stamp, input: Input::Engine(EngineInput::Check { hash }) });
                    let after = seg.entries.len();
                    seg.keyframes.push(Keyframe { at: at_stamp, day, hash, after, snapshot });
                }
            }
            TAG_ENTRIES => {
                let seg = segments.last_mut().ok_or("entries before any segment")?;
                seg.entries.extend(stream::decode(payload)?);
            }
            TAG_BATTLE => {
                let seg = segments.last_mut().ok_or("a battle before any segment")?;
                let start: BattleStart = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
                seg.battles.push((seg.entries.len(), start));
            }
            _ => break,
        }
        at = start + n;
    }
    let header = header.ok_or("a replay without its header")?;
    Ok(ReplayFile { header, segments, len: at as u64 })
}
