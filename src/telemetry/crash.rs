//! Crash reports (the design's What is sent, 2). A panic writes a pending file from the panic hook
//! (`diag`; plain file writes only). On the next start, with the consent yes, the pending report is
//! queued with the cleaned last lines of the crashed run's log (`razdor.previous.log`); the player is
//! then asked whether the newest save goes with it. The save is sent only on «Отправить», after the
//! server has given the report its id (the save item waits for it, `worker::step`).
//!
//! Native crashes (a signal, a Windows access violation) leave no pending file: they are not
//! reported yet (open question in `docs/OPEN_QUESTIONS.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use razdor_wire::analytics::CrashReport;
use razdor_wire::{limits, paths, SCHEMA};
use serde::{Deserialize, Serialize};

use super::queue::Class;
use super::sanitize::Paths;
use super::state::{self, Consent};
use super::{platform, Hub, Refused};

const PENDING: &str = "crash-pending.json";

/// The log lines a report carries (the design: about 200).
pub const LOG_LINES: usize = 200;

#[derive(Serialize, Deserialize)]
struct Pending {
    at: u64,
    screen: String,
    panic_text: String,
}

/// The screen the player has open, for a report (`set_screen`, from the app's frame loop).
static SCREEN: Mutex<String> = Mutex::new(String::new());

/// The question shown after a crash: whether the save goes with the report. `token` names the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub token: String,
    /// The newest save, when there is one. It is read and sent only on «Отправить».
    pub save: Option<PathBuf>,
}

static ASK: Mutex<Option<Ask>> = Mutex::new(None);

/// The open question, if a crash report was queued at this start.
pub fn question() -> Option<Ask> {
    ASK.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Records the screen the player is on (the app calls it when the screen changes).
pub fn set_screen(name: &str) {
    if let Ok(mut s) = SCREEN.lock() {
        if s.as_str() != name {
            *s = name.to_string();
        }
    }
}

/// The screen as last recorded; empty when unknown or when the lock is busy (a panic may hold it).
fn screen() -> String {
    SCREEN.try_lock().map(|s| s.clone()).unwrap_or_default()
}

/// The first words of the sound device's failures: quad-snd's ALSA panics (`quad-snd/src/alsa_snd.rs`)
/// and Razdor's own «no sound device». A player with no sound device is not a crash, so these are
/// not reported. Matched on the panic's first line (its message), never on the backtrace.
const SOUND_FAILURES: [&str; 11] = [
    "PCM",
    "ALSA",
    "no sound device",
    "software parameters",
    "harware parameters",
    "Can't set rate.",
    "channels number",
    "Cant's set buffer size",
    "interleaved mode",
    "start mode",
    "audio interface",
];

/// Whether a panic text is a sound device's failure (see [`SOUND_FAILURES`]).
pub fn is_sound_failure(text: &str) -> bool {
    let message = text.lines().next().unwrap_or_default();
    SOUND_FAILURES.iter().any(|s| message.contains(s))
}

/// Called by the panic hook: writes the pending report. Does nothing when the analytics are off, and
/// for a sound device's failure ([`is_sound_failure`]).
pub fn note_panic(text: &str) {
    if !super::active() || is_sound_failure(text) {
        return;
    }
    let Some(dir) = state::telemetry_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let pending = Pending { at: super::now_secs(), screen: screen(), panic_text: text.to_string() };
    if let Ok(json) = serde_json::to_vec(&pending) {
        let _ = std::fs::write(dir.join(PENDING), json);
    }
}

/// How many folders down [`newest_save`] looks (the saves' kinds: manual, quick, auto, maps).
const SAVE_DEPTH: u32 = 3;

/// The newest save file in the folder `dir` (`.rzsave`, by modification time), looking at most
/// [`SAVE_DEPTH`] folders down. `None` when there is none, or the folder is missing.
pub fn newest_save(dir: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    collect_saves(dir, SAVE_DEPTH, &mut found);
    found.into_iter().max_by_key(|(time, _)| *time).map(|(_, path)| path)
}

fn collect_saves(dir: &Path, depth: u32, out: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                collect_saves(&path, depth - 1, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(crate::rules::save::EXTENSION) {
            if let Ok(time) = entry.metadata().and_then(|m| m.modified()) {
                out.push((time, path));
            }
        }
    }
}

/// The last `n` lines of `text`, in order.
pub fn tail_lines(text: &str, n: usize) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().rev().take(n).collect();
    lines.reverse();
    lines
}

/// `s` cut to `n` characters (the wire limits count characters).
fn cut(s: String, n: usize) -> String {
    if s.chars().count() <= n {
        s
    } else {
        s.chars().take(n).collect()
    }
}

/// At start, after the hub is open. With the consent yes: queues the pending report (its log from
/// `previous_log`, cleaned with `paths`) and returns the question. With no: drops the pending file.
/// With no answer yet: keeps the file for a later start. `save` is the newest save, if any.
pub fn collect(
    hub: &Hub,
    previous_log: Option<&Path>,
    paths: &Paths,
    game: &str,
    settings: BTreeMap<String, String>,
    save: Option<PathBuf>,
) -> Option<Ask> {
    let file = hub.dir().join(PENDING);
    let bytes = std::fs::read(&file).ok()?;
    match hub.consent() {
        Consent::Unknown => return None,
        Consent::No => {
            let _ = std::fs::remove_file(&file);
            return None;
        }
        Consent::Yes => {}
    }
    let _ = std::fs::remove_file(&file);
    let pending: Pending = serde_json::from_slice(&bytes).ok()?;
    let log: Vec<String> = previous_log
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| tail_lines(&t, LOG_LINES).into_iter().map(|l| cut(paths.clean(l), limits::MAX_LOG_LINE_CHARS)).collect())
        .unwrap_or_default();
    let install_id = razdor_wire::InstallId::parse(&hub.install_id()).ok()?;
    let report = CrashReport {
        schema: SCHEMA,
        install_id,
        razdor: env!("CARGO_PKG_VERSION").to_string(),
        platform: platform(),
        game: game.to_string(),
        screen: if pending.screen.is_empty() { "unknown".to_string() } else { pending.screen.clone() },
        at: pending.at,
        settings,
        panic_text: cut(paths.clean(&pending.panic_text), limits::MAX_PANIC_CHARS),
        log,
        has_save: save.is_some(),
    };
    let token = state::new_install_id();
    if let Err(e) = hub.enqueue_token(paths::CRASH, Class::Consent, &report, Some(token.clone())) {
        crate::diag!("telemetry: the crash report is not queued: {e:?}");
        return None;
    }
    let ask = Ask { token, save };
    *ASK.lock().unwrap_or_else(|e| e.into_inner()) = Some(ask.clone());
    Some(ask)
}

/// The player's answer to the question: «Отправить» queues the save (framed, after the report's id).
/// Any other answer forgets it. Returns the queued seq, or why not.
pub fn answer(hub: &Hub, send_save: bool) -> Option<Result<u64, Refused>> {
    let ask = ASK.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    if !send_save {
        return None;
    }
    let path = ask.save?;
    Some(queue_save(hub, &ask.token, &path))
}

/// Queues the save file `path` for the crash report `token`: the body is the file, the header its
/// checksum and length (the worker adds the report's `crash_id` when it sends).
pub fn queue_save(hub: &Hub, token: &str, path: &Path) -> Result<u64, Refused> {
    let bytes = std::fs::read(path).map_err(|e| Refused::Invalid(format!("the save: {e}")))?;
    if bytes.len() > limits::MAX_SAVE_BODY {
        return Err(Refused::Invalid("the save is too big".into()));
    }
    let header = serde_json::json!({
        "schema": SCHEMA,
        "install_id": hub.install_id(),
        "crash_id": "",
        "sha256": crate::update::sha256_hex(&bytes),
        "len": bytes.len(),
    });
    hub.enqueue_frame(paths::CRASH_SAVE, Class::Consent, header, bytes, None, Some(token.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The question slot is one per process: the tests that use it run one at a time.
    static ASK_TESTS: Mutex<()> = Mutex::new(());

    fn hub(name: &str) -> (Hub, PathBuf) {
        let dir = std::env::temp_dir().join(format!("razdor-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Hub::open(&dir, limits::CLIENT_QUEUE_BYTES).unwrap(), dir)
    }

    fn write_pending(dir: &Path, panic_text: &str) {
        let p = Pending { at: 1_700_000_000, screen: "map".into(), panic_text: panic_text.into() };
        std::fs::write(dir.join(PENDING), serde_json::to_vec(&p).unwrap()).unwrap();
    }

    fn previous_log(dir: &Path, lines: usize) -> PathBuf {
        let mut text = String::new();
        text.push_str("[1.0] start from /home/alice/Games/DT\n");
        for i in 0..lines {
            text.push_str(&format!("[{i}.0] line {i} /home/alice/Games/DT/maps/a.DTm\n"));
        }
        let path = dir.join("razdor.previous.log");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn the_log_tail_is_the_last_lines_cleaned_of_the_paths() {
        let dir = std::env::temp_dir().join(format!("razdor-crash-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = previous_log(&dir, 300);
        let text = std::fs::read_to_string(&log).unwrap();
        let last = tail_lines(&text, LOG_LINES);
        assert_eq!(last.len(), LOG_LINES);
        assert!(last[0].starts_with("[100.0] line 100"), "{}", last[0]);
        let paths = Paths::new(Some(Path::new("/home/alice")), Some(Path::new("/home/alice/Games/DT")), None, &[]);
        assert_eq!(paths.clean(last[0]), "[100.0] line 100 <dt>/maps/a.DTm");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_consent_the_report_is_queued_with_its_clean_log_and_the_question_follows() {
        let _one = ASK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let (hub, dir) = hub("yes");
        std::fs::create_dir_all(&dir).unwrap();
        hub.set_consent(Consent::Yes, "1.5.4");
        write_pending(&dir, "boom at /home/alice/Games/DT/src/x.rs");
        let log = previous_log(&dir, 3);
        let paths = Paths::new(Some(Path::new("/home/alice")), Some(Path::new("/home/alice/Games/DT")), None, &[]);
        let ask = collect(&hub, Some(&log), &paths, "dt", BTreeMap::new(), None).expect("a question");
        assert_eq!(ask.save, None);
        let item = hub.queue().metas().find(|m| m.path == paths::CRASH).cloned().expect("queued");
        assert_eq!(item.token.as_deref(), Some(ask.token.as_str()));
        let body = hub.queue().body(item.seq).unwrap();
        let report: CrashReport = serde_json::from_slice(&body).unwrap();
        assert_eq!(report.panic_text, "boom at <dt>/src/x.rs");
        assert_eq!(report.screen, "map");
        assert!(report.log.last().unwrap().contains("<dt>/maps/a.DTm"));
        assert!(!report.log.iter().any(|l| l.contains("/home/alice")), "no home path in the log");
        assert!(!dir.join(PENDING).exists(), "the pending file is taken");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_consent_the_pending_report_is_dropped_and_with_no_answer_it_waits() {
        let (hub, dir) = hub("no");
        std::fs::create_dir_all(&dir).unwrap();
        write_pending(&dir, "boom");
        assert!(collect(&hub, None, &Paths::default(), "dt", BTreeMap::new(), None).is_none());
        assert!(dir.join(PENDING).exists(), "unknown consent keeps the file for the next start");
        hub.set_consent(Consent::No, "1.5.4");
        assert!(collect(&hub, None, &Paths::default(), "dt", BTreeMap::new(), None).is_none());
        assert!(!dir.join(PENDING).exists(), "a no drops it");
        assert!(hub.queue().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_save_is_queued_only_on_a_yes_and_waits_for_the_report_id() {
        let _one = ASK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let (hub, dir) = hub("save");
        std::fs::create_dir_all(&dir).unwrap();
        hub.set_consent(Consent::Yes, "1.5.4");
        write_pending(&dir, "boom");
        let save = dir.join("game.rzsave");
        std::fs::write(&save, b"SAVEBYTES").unwrap();
        let ask = collect(&hub, None, &Paths::default(), "dt", BTreeMap::new(), Some(save.clone())).unwrap();
        // The question is answered no: nothing more is queued.
        assert!(answer(&hub, false).is_none());
        assert!(question().is_none());
        // A second question, answered yes: the save is queued, waiting for the report's id.
        *ASK.lock().unwrap() = Some(ask.clone());
        let queued = answer(&hub, true).expect("answered").expect("queued");
        let item = hub.queue().metas().find(|m| m.seq == queued).cloned().unwrap();
        assert_eq!(item.path, paths::CRASH_SAVE);
        assert_eq!(item.after.as_deref(), Some(ask.token.as_str()));
        assert_eq!(hub.queue().body(queued).unwrap(), b"SAVEBYTES");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sound_device_failure_is_not_a_crash_and_other_panics_are() {
        // quad-snd's ALSA panic, as the audio thread reports it (with its place and a backtrace).
        let alsa = "Can't open PCM device.\nat quad-snd-0.2.8/src/alsa_snd.rs:30:9\nbacktrace: snd_pcm_open";
        assert!(is_sound_failure(alsa));
        assert!(is_sound_failure("no sound device\nat src/ui/audio.rs:1"));
        assert!(!is_sound_failure("index out of bounds: the len is 3 but the index is 7\nat src/rules/battle.rs:10"));
        // A normal panic whose backtrace mentions the audio code is still recorded.
        let normal = "called `Option::unwrap()` on a `None` value\nat src/ui/audio.rs:90\nbacktrace: razdor::ui::audio::play";
        assert!(!is_sound_failure(normal));
    }

    #[test]
    fn the_newest_save_is_the_one_modified_last_in_any_saves_folder() {
        let dir = std::env::temp_dir().join(format!("razdor-newest-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("manual")).unwrap();
        std::fs::create_dir_all(dir.join("auto")).unwrap();
        let at = |secs: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let put = |path: PathBuf, secs: u64| {
            std::fs::write(&path, b"save").unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(at(secs)).unwrap();
        };
        put(dir.join("manual").join("old.rzsave"), 1_000);
        put(dir.join("auto").join("session.rzsave"), 3_000);
        put(dir.join("manual").join("middle.rzsave"), 2_000);
        put(dir.join("manual").join("newer.txt"), 9_000);
        assert_eq!(newest_save(&dir), Some(dir.join("auto").join("session.rzsave")));
        assert_eq!(newest_save(&dir.join("missing")), None, "no folder, no save");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
