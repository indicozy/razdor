//! Play events (`docs/superpowers/specs/2026-11-22-analytics-design.md`, section 4): the session,
//! the map runs (start, progress, end, abandoned), older saves, changed settings and the interface
//! counters. A pure tracker: every call takes the time (`now`, seconds since 1970) and the hub, so the
//! tests run without a clock or a network. The UI glue (`ui::analytics`) feeds it.
//!
//! Nothing is recorded unless the player said yes («Помочь Раздору?»): every call checks the consent
//! first and forgets what it holds when the answer is not yes. Events queue as one batch per flush
//! (at most once a minute, or at once at 200 events, and at the session's end) with the consent class,
//! so the hub refuses them without a yes as well.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use razdor_wire::analytics::{BattleOutcome, Event, EventBatch, EventKind, Outcome, SaveSource, Setting};
use razdor_wire::{limits, paths, Validate};
use serde::{Deserialize, Serialize};

use super::queue::Class;
use super::state::Consent;
use super::Hub;

/// A batch goes out at most this often (seconds), unless [`FLUSH_EVENTS`] are pending.
pub const FLUSH_SECS: u64 = 60;
/// Pending events that flush at once.
pub const FLUSH_EVENTS: usize = 200;
/// A progress event every this many game days (and the first one of a run).
pub const PROGRESS_DAYS: u32 = 5;
/// A run with no time in the map for this long (seconds) is abandoned: 7 days.
pub const ABANDONED_SECS: u64 = 7 * 24 * 3600;
/// The most a tick credits to the time in the map (a long stall does not count as play).
const MAX_TICK_SECS: u64 = 60;
/// The run file is rewritten at least this often while a run is open (seconds).
const RUN_SAVE_SECS: u64 = 300;
/// The run file, in the telemetry folder.
const RUNS_FILE: &str = "runs.json";
/// The screen a run starts in, until the glue names one.
const START_SCREEN: &str = "world map";

/// A map run, persisted in `runs.json` so an abandoned map is still reported after a restart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub map: String,
    pub map_hash: String,
    pub started_at: u64,
    pub last_active_at: u64,
    pub last_day: u32,
    pub last_screen: String,
    pub last_battle: Option<BattleOutcome>,
    pub seconds_in_map: u64,
    pub last_progress_day: Option<u32>,
}

/// The FNV-1a 64 hash of `bytes`, as 16 lower-case hex digits (a map's hash in the events).
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x}")
}

/// The settings whose value differs from the default, as catalog-style names and values. Only the
/// names and simple keys go out: a string is sent only when it is a plain identifier (a catalog key),
/// a number or a flag as it is, and a list or an object as «set». Free text is never sent: a string
/// goes out only in lower-case snake case (an enum's variant), so a typed name does not.
pub fn differing_settings(current: &serde_json::Value, default: &serde_json::Value) -> Vec<Setting> {
    let (Some(now), Some(def)) = (current.as_object(), default.as_object()) else { return Vec::new() };
    let mut out = Vec::new();
    for (name, value) in now {
        if def.get(name) == Some(value) {
            continue;
        }
        let shown = match value {
            serde_json::Value::Bool(b) => Some(if *b { "on" } else { "off" }.to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Null => Some("none".to_string()),
            serde_json::Value::String(s) if !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') => Some(s.clone()),
            serde_json::Value::String(_) => None,
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => Some("set".to_string()),
        };
        if let Some(value) = shown {
            out.push(Setting { name: name.clone(), value });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The tracker: the pending events, the open run, the session and the interface counters.
pub struct Tracker {
    dir: PathBuf,
    pending: Vec<Event>,
    last_flush: u64,
    session_start: Option<u64>,
    run: Option<Run>,
    run_saved_at: u64,
    windows: BTreeMap<String, u32>,
    hints: u32,
}

impl Tracker {
    /// A tracker over the telemetry folder `dir`; an open run left in `runs.json` is taken up again.
    /// A missing or unreadable file is no run.
    pub fn new(dir: &Path) -> Tracker {
        let run = std::fs::read(dir.join(RUNS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok());
        Tracker {
            dir: dir.to_path_buf(),
            pending: Vec::new(),
            last_flush: 0,
            session_start: None,
            run,
            run_saved_at: 0,
            windows: BTreeMap::new(),
            hints: 0,
        }
    }

    /// The open run, if any (the tests and the glue read it).
    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    /// Whether the player said yes. Anything else drops what is held (nothing of a no is kept).
    fn gate(&mut self, hub: &Hub) -> bool {
        if hub.consent_now() == Consent::Yes {
            return true;
        }
        self.pending.clear();
        self.windows.clear();
        self.hints = 0;
        self.run = None;
        self.session_start = None;
        let _ = std::fs::remove_file(self.dir.join(RUNS_FILE));
        false
    }

    /// Queues one event. A kind that fails its own checks is dropped alone, so it cannot sink a batch.
    fn push(&mut self, at: u64, kind: EventKind) {
        let event = Event { at, kind };
        if let Err(e) = event.validate() {
            crate::diag!("telemetry: a play event is dropped: {e}");
            return;
        }
        self.pending.push(event);
    }

    /// Flushes at once when the pending events reached [`FLUSH_EVENTS`].
    fn settle(&mut self, hub: &Hub, now: u64) {
        if self.pending.len() >= FLUSH_EVENTS {
            self.flush(hub, now);
        }
    }

    /// Writes the run file (temporary name, then rename) or removes it when there is no run.
    fn save_run(&mut self, now: u64) {
        self.run_saved_at = now;
        let Some(run) = &self.run else {
            let _ = std::fs::remove_file(self.dir.join(RUNS_FILE));
            return;
        };
        let Ok(text) = serde_json::to_vec_pretty(run) else { return };
        let tmp = self.dir.join("runs.json.part");
        let written = std::fs::create_dir_all(&self.dir).and_then(|_| std::fs::write(&tmp, text)).and_then(|_| std::fs::rename(&tmp, self.dir.join(RUNS_FILE)));
        if let Err(e) = written {
            crate::diag!("telemetry: the run is not saved: {e}");
        }
    }

    /// Closes the open run as `outcome`, its end stamped `at`.
    fn close_run(&mut self, at: u64, outcome: Outcome) {
        let Some(run) = self.run.take() else { return };
        self.push(
            at,
            EventKind::MapEnd {
                map: run.map,
                outcome,
                last_day: run.last_day,
                last_screen: run.last_screen,
                last_battle: run.last_battle,
                seconds_in_map: run.seconds_in_map,
            },
        );
    }

    /// A run with no time in the map for 7 days is abandoned, stamped with its last time in the map.
    pub fn sweep(&mut self, hub: &Hub, now: u64) {
        if !self.gate(hub) {
            return;
        }
        let Some(last) = self.run.as_ref().map(|r| r.last_active_at) else { return };
        if now.saturating_sub(last) >= ABANDONED_SECS {
            self.close_run(last, Outcome::Abandoned);
            self.save_run(now);
        }
    }

    /// The session starts: the sweep, then one SessionStart with the window's size and the graphics preset.
    pub fn session_start(&mut self, hub: &Hub, now: u64, screen_w: u32, screen_h: u32, graphics: &str) {
        if !self.gate(hub) {
            return;
        }
        self.sweep(hub, now);
        self.push(now, EventKind::SessionStart { screen_w, screen_h, graphics: graphics.to_string() });
        self.session_start = Some(now);
        self.last_flush = now;
        self.settle(hub, now);
    }

    /// The session ends: one SessionEnd with its length, then everything is flushed at once.
    pub fn session_end(&mut self, hub: &Hub, now: u64) {
        if !self.gate(hub) {
            return;
        }
        let Some(start) = self.session_start.take() else { return };
        self.push(now, EventKind::SessionEnd { seconds: now.saturating_sub(start) });
        self.flush(hub, now);
    }

    /// Called every second by the glue. `in_map`: the player is in a game screen (the time in the map
    /// counts only then). Writes the run file now and then, and flushes once a minute when anything waits.
    pub fn tick(&mut self, hub: &Hub, now: u64, in_map: bool) {
        if !self.gate(hub) {
            return;
        }
        if in_map {
            if let Some(run) = self.run.as_mut() {
                let delta = now.saturating_sub(run.last_active_at).min(MAX_TICK_SECS);
                run.seconds_in_map += delta;
                run.last_active_at = now;
            }
        }
        if self.run.is_some() && now.saturating_sub(self.run_saved_at) >= RUN_SAVE_SECS {
            self.save_run(now);
        }
        let waiting = !self.pending.is_empty() || !self.windows.is_empty() || self.hints > 0;
        if waiting && now.saturating_sub(self.last_flush) >= FLUSH_SECS {
            self.flush(hub, now);
        }
    }

    /// Sends what is pending: the sweep, the interface counters (one event when any), then the
    /// batches (split at [`limits::MAX_EVENTS_PER_BATCH`]) into the queue with the consent class.
    pub fn flush(&mut self, hub: &Hub, now: u64) {
        if !self.gate(hub) {
            return;
        }
        self.sweep(hub, now);
        if !self.windows.is_empty() || self.hints > 0 {
            let windows = std::mem::take(&mut self.windows);
            let hints = std::mem::take(&mut self.hints);
            self.push(now, EventKind::UiCounters { windows, hints });
        }
        self.last_flush = now;
        if self.run.is_some() {
            self.save_run(now);
        }
        let events = std::mem::take(&mut self.pending);
        if events.is_empty() {
            return;
        }
        let Ok(install_id) = razdor_wire::InstallId::parse(&hub.install_id()) else { return };
        for chunk in events.chunks(limits::MAX_EVENTS_PER_BATCH) {
            let batch = EventBatch {
                schema: razdor_wire::SCHEMA,
                install_id: install_id.clone(),
                razdor: env!("CARGO_PKG_VERSION").to_string(),
                events: chunk.to_vec(),
            };
            if let Err(e) = hub.enqueue_checked(paths::EVENTS, Class::Consent, &batch) {
                crate::diag!("telemetry: a play batch is not queued: {e:?}");
            }
        }
    }

    /// A map starts. An open run is closed first as abandoned (at its last time in the map).
    pub fn map_start(&mut self, hub: &Hub, now: u64, map: &str, map_hash: &str, difficulty: &str, ai_strength: &str, experiments: &[String]) {
        if !self.gate(hub) {
            return;
        }
        if let Some(last) = self.run.as_ref().map(|r| r.last_active_at) {
            self.close_run(last, Outcome::Abandoned);
        }
        self.push(
            now,
            EventKind::MapStart {
                map: map.to_string(),
                map_hash: map_hash.to_string(),
                difficulty: difficulty.to_string(),
                ai_strength: ai_strength.to_string(),
                experiments: experiments.to_vec(),
            },
        );
        self.run = Some(Run {
            map: map.to_string(),
            map_hash: map_hash.to_string(),
            started_at: now,
            last_active_at: now,
            last_day: 0,
            last_screen: START_SCREEN.to_string(),
            last_battle: None,
            seconds_in_map: 0,
            last_progress_day: None,
        });
        self.save_run(now);
        self.settle(hub, now);
    }

    /// The day, the hero's level and the army's size. A Progress event on the first call and then
    /// every [`PROGRESS_DAYS`] game days.
    pub fn progress(&mut self, hub: &Hub, now: u64, day: u32, hero_level: u32, army_size: u32) {
        if !self.gate(hub) {
            return;
        }
        let Some(run) = self.run.as_mut() else { return };
        let day_changed = run.last_day != day;
        run.last_day = day;
        let due = run.last_progress_day.is_none_or(|last| day >= last.saturating_add(PROGRESS_DAYS));
        if due {
            run.last_progress_day = Some(day);
            self.push(now, EventKind::Progress { day, hero_level, army_size });
        }
        if due || day_changed {
            self.save_run(now);
        }
        self.settle(hub, now);
    }

    /// The screen the player is on (the run's last screen, named in the end event). Empty names are ignored.
    pub fn set_screen(&mut self, screen: &str) {
        if let (Some(run), false) = (self.run.as_mut(), screen.is_empty()) {
            run.last_screen = screen.to_string();
        }
    }

    /// A battle of the open run ended: its outcome is the run's last battle.
    pub fn battle_ended(&mut self, hub: &Hub, now: u64, outcome: BattleOutcome) {
        if !self.gate(hub) {
            return;
        }
        if let Some(run) = self.run.as_mut() {
            run.last_battle = Some(outcome);
            self.save_run(now);
        }
    }

    /// The open run ends with `outcome` (a win or a loss); nothing happens without an open run.
    pub fn map_end(&mut self, hub: &Hub, now: u64, outcome: Outcome) {
        if !self.gate(hub) {
            return;
        }
        if self.run.is_none() {
            return;
        }
        self.close_run(now, outcome);
        self.save_run(now);
        self.settle(hub, now);
    }

    /// A save is loaded. Recorded only when its format is older than `current`.
    pub fn older_save_loaded(&mut self, hub: &Hub, now: u64, source: SaveSource, save_version: u32, current: u32) {
        if !self.gate(hub) || save_version >= current {
            return;
        }
        self.push(now, EventKind::OlderSaveLoaded { source, save_version });
        self.settle(hub, now);
    }

    /// The settings that differ from their defaults (the glue passes them; see [`differing_settings`]).
    pub fn settings_changed(&mut self, hub: &Hub, now: u64, changed: Vec<Setting>) {
        if !self.gate(hub) || changed.is_empty() {
            return;
        }
        self.push(now, EventKind::SettingsChanged { changed });
        self.settle(hub, now);
    }

    /// A window opened: counted for the session (sent as one UiCounters event, not one per click).
    pub fn count_window(&mut self, hub: &Hub, name: &str) {
        if name.is_empty() || !self.gate(hub) {
            return;
        }
        let n = self.windows.entry(name.to_string()).or_insert(0);
        *n = n.saturating_add(1);
    }

    /// The «?» hint opened: counted for the session.
    pub fn count_hint(&mut self, hub: &Hub) {
        if !self.gate(hub) {
            return;
        }
        self.hints = self.hints.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor_wire::analytics::Side;

    const DAY: u64 = 86_400;
    const T0: u64 = 1_800_000_000;
    const HASH: &str = "0123456789abcdef";

    fn hub(name: &str, consent: Consent) -> (Hub, PathBuf) {
        let dir = std::env::temp_dir().join(format!("razdor-events-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hub = Hub::open(&dir, razdor_wire::limits::CLIENT_QUEUE_BYTES).expect("the hub opens");
        if consent != Consent::Unknown {
            hub.set_consent(consent, "test");
        }
        (hub, dir)
    }

    /// The events of every queued batch, in order.
    fn queued_events(hub: &Hub) -> Vec<Event> {
        let seqs: Vec<(u64, String, Class)> = hub.queue().metas().map(|m| (m.seq, m.path.clone(), m.class)).collect();
        let mut out = Vec::new();
        for (seq, path, class) in seqs {
            if path != paths::EVENTS {
                continue;
            }
            assert_eq!(class, Class::Consent);
            let body = hub.queue().body(seq).expect("the body is read");
            let batch: EventBatch = serde_json::from_slice(&body).expect("a batch decodes");
            batch.validate().expect("the batch validates");
            out.extend(batch.events);
        }
        out
    }

    #[test]
    fn the_hash_is_fnv_1a_64_as_16_hex_digits() {
        assert_eq!(fnv1a64_hex(b""), "cbf29ce484222325");
        assert_eq!(fnv1a64_hex(b"a"), "af63dc4c8601ec8c");
        assert_eq!(fnv1a64_hex(b"a").len(), 16);
    }

    #[test]
    fn without_consent_nothing_is_recorded_or_queued() {
        let (hub, dir) = hub("no-consent", Consent::No);
        let mut t = Tracker::new(&dir);
        t.session_start(&hub, T0, 1280, 720, "high");
        t.map_start(&hub, T0 + 1, "Map", HASH, "100", "original", &[]);
        t.count_window(&hub, "options");
        t.session_end(&hub, T0 + 100);
        assert!(queued_events(&hub).is_empty());
        assert!(t.run().is_none());
        assert!(!dir.join(RUNS_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_consent_a_session_flushes_a_valid_batch() {
        let (hub, dir) = hub("session", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.session_start(&hub, T0, 1280, 720, "medium");
        t.session_end(&hub, T0 + 90);
        let events = queued_events(&hub);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::SessionStart { screen_w: 1280, screen_h: 720, graphics: "medium".into() });
        assert_eq!(events[1].kind, EventKind::SessionEnd { seconds: 90 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_is_abandoned_after_seven_days_and_not_before() {
        let (hub, dir) = hub("abandoned", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "Map", HASH, "100", "original", &[]);
        t.progress(&hub, T0 + 10, 3, 2, 5);
        // The run's last time in the map is its start: only `tick` moves it.
        let last = T0;
        t.sweep(&hub, last + 7 * DAY - 1);
        assert!(t.run().is_some(), "6 days 23 hours keeps the run");
        t.sweep(&hub, last + 7 * DAY);
        assert!(t.run().is_none());
        t.flush(&hub, last + 7 * DAY);
        let events = queued_events(&hub);
        let end = events.iter().find(|e| matches!(e.kind, EventKind::MapEnd { .. })).expect("an end");
        assert_eq!(end.at, last);
        match &end.kind {
            EventKind::MapEnd { map, outcome, last_day, seconds_in_map, .. } => {
                assert_eq!(map, "Map");
                assert_eq!(*outcome, Outcome::Abandoned);
                assert_eq!(*last_day, 3);
                assert_eq!(*seconds_in_map, 0);
            }
            other => panic!("not a map end: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_survives_a_restart() {
        let (hub, dir) = hub("restart", Consent::Yes);
        {
            let mut t = Tracker::new(&dir);
            t.map_start(&hub, T0, "Map", HASH, "100", "max", &["fast_ai".to_string()]);
            t.progress(&hub, T0 + 5, 7, 3, 9);
            t.battle_ended(&hub, T0 + 6, BattleOutcome { winner: Side::Player, turns: 4, player_losses: 1, foe_losses: 2 });
        }
        let mut again = Tracker::new(&dir);
        let run = again.run().expect("the run reloads").clone();
        assert_eq!(run.map, "Map");
        assert_eq!(run.last_day, 7);
        assert_eq!(run.last_progress_day, Some(7));
        assert!(run.last_battle.is_some());
        again.map_end(&hub, T0 + 60, Outcome::Win);
        assert!(again.run().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_is_recorded_on_the_first_call_and_every_five_days() {
        let (hub, dir) = hub("progress", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "Map", HASH, "100", "original", &[]);
        for day in 0..=12 {
            t.progress(&hub, T0 + u64::from(day), day, 1, 4);
        }
        t.flush(&hub, T0 + 100);
        let days: Vec<u32> = queued_events(&hub)
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::Progress { day, .. } => Some(day),
                _ => None,
            })
            .collect();
        assert_eq!(days, vec![0, 5, 10]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_batch_waits_a_minute_unless_it_is_full() {
        let (hub, dir) = hub("cadence", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.session_start(&hub, T0, 800, 600, "low");
        t.tick(&hub, T0 + 30, true);
        assert!(queued_events(&hub).is_empty(), "no flush within 60 s");
        t.tick(&hub, T0 + 61, true);
        assert_eq!(queued_events(&hub).len(), 1, "the first batch after 60 s");
        t.count_hint(&hub);
        t.tick(&hub, T0 + 90, true);
        assert_eq!(queued_events(&hub).len(), 1, "still within a minute of the last flush");
        t.tick(&hub, T0 + 122, true);
        assert_eq!(queued_events(&hub).len(), 2, "the counters follow after a minute");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_hundred_pending_events_flush_at_once() {
        let (hub, dir) = hub("full", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "Map", HASH, "100", "original", &[]);
        for i in 0..FLUSH_EVENTS as u64 {
            t.settings_changed(&hub, T0 + i, vec![Setting { name: "music_volume".into(), value: "0.5".into() }]);
        }
        assert!(queued_events(&hub).len() >= FLUSH_EVENTS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_counted_window_appears_once_per_session_in_one_event() {
        let (hub, dir) = hub("counters", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.session_start(&hub, T0, 800, 600, "low");
        for _ in 0..3 {
            t.count_window(&hub, "options");
        }
        t.count_window(&hub, "save");
        t.count_hint(&hub);
        t.session_end(&hub, T0 + 300);
        let counters: Vec<_> = queued_events(&hub).into_iter().filter(|e| matches!(e.kind, EventKind::UiCounters { .. })).collect();
        assert_eq!(counters.len(), 1);
        match &counters[0].kind {
            EventKind::UiCounters { windows, hints } => {
                assert_eq!(windows.get("options"), Some(&3));
                assert_eq!(windows.get("save"), Some(&1));
                assert_eq!(*hints, 1);
            }
            _ => unreachable!(),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_save_is_recorded_only_when_older() {
        let (hub, dir) = hub("older", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.older_save_loaded(&hub, T0, SaveSource::Manual, 11, 12);
        t.older_save_loaded(&hub, T0 + 1, SaveSource::Auto, 12, 12);
        t.flush(&hub, T0 + 2);
        let events = queued_events(&hub);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::OlderSaveLoaded { source: SaveSource::Manual, save_version: 11 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_differing_settings_go_out_and_free_text_never() {
        let default = serde_json::json!({"music_volume": 0.5, "show_fps": false, "name": "", "keys": {"a": 1}});
        let current = serde_json::json!({"music_volume": 0.25, "show_fps": false, "name": "Vasya", "keys": {"a": 2}});
        let changed = differing_settings(&current, &default);
        assert_eq!(changed, vec![Setting { name: "keys".into(), value: "set".into() }, Setting { name: "music_volume".into(), value: "0.25".into() }]);
        let typed = serde_json::json!({"name": "Vasya Pupkin"});
        assert!(differing_settings(&typed, &serde_json::json!({"name": ""})).is_empty());
    }

    #[test]
    fn a_map_start_closes_the_open_run_as_abandoned() {
        let (hub, dir) = hub("replace", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "First", HASH, "100", "original", &[]);
        t.progress(&hub, T0 + 20, 2, 1, 3);
        t.map_start(&hub, T0 + 500, "Second", HASH, "100", "original", &[]);
        t.flush(&hub, T0 + 600);
        let ends: Vec<Event> = queued_events(&hub).into_iter().filter(|e| matches!(e.kind, EventKind::MapEnd { .. })).collect();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].at, T0, "the first run's last time in the map is its start");
        assert_eq!(t.run().map(|r| r.map.as_str()), Some("Second"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_end_carries_the_run_and_clears_it() {
        let (hub, dir) = hub("end", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "Map", HASH, "100", "original", &[]);
        t.set_screen("battle");
        t.map_end(&hub, T0 + 40, Outcome::Loss);
        assert!(t.run().is_none());
        t.flush(&hub, T0 + 41);
        let ends: Vec<EventKind> = queued_events(&hub).into_iter().map(|e| e.kind).filter(|k| matches!(k, EventKind::MapEnd { .. })).collect();
        assert_eq!(ends.len(), 1);
        match &ends[0] {
            EventKind::MapEnd { outcome, last_screen, .. } => {
                assert_eq!(*outcome, Outcome::Loss);
                assert_eq!(last_screen, "battle");
            }
            _ => unreachable!(),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_time_in_map_counts_only_while_in_the_map() {
        let (hub, dir) = hub("time", Consent::Yes);
        let mut t = Tracker::new(&dir);
        t.map_start(&hub, T0, "Map", HASH, "100", "original", &[]);
        for s in 1..=10 {
            t.tick(&hub, T0 + s, true);
        }
        for s in 11..=20 {
            t.tick(&hub, T0 + s, false);
        }
        assert_eq!(t.run().map(|r| r.seconds_in_map), Some(10));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
