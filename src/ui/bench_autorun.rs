//! The performance test started with no one at the screen (the lab's and the owner's runs over adb).
//!
//! A file `bench-autorun.txt` in Razdor's data folder (`platform::app_dir`: the folder with `razdor.log`;
//! on Android the app's own folder) starts a run once the install is loaded and the main menu is up
//! (a first-run game download included: `App::mobile_install_poll` loads it). Its first line is the
//! choice (`mine` or `all`, the windows' «Мои настройки» and «Все пресеты»); an optional `map=<name>`
//! line names the map (default: `bench::MAPS`). Lines starting with `#` are notes.
//!
//! A `profile` line (or `debug`, the same) turns profiling on for that run only: each cell then logs its
//! stage breakdown (`RazdorBenchStages`), and the debug mode's GPU samples run (`diag::set_forced_profile`).
//!
//! The lab's switches (`razdor::lab`, for the run only; `docs/flows/testing-and-tools.md`): `replay-floor`,
//! `gpu-finish`, `clear-targets`, and `swap-interval 0` or `swap-interval 1`. A file with one of them needs a build
//! that knows it; an older one refuses the file (`unknown line`), so the run never starts without its switches.
//!
//! The file is renamed to `bench-autorun.done` at once, so a crash does not start the run again. The run
//! itself is `bench::start_run`, the button's own (`bench::start_on` without profiling). Its end writes `bench-autorun.finished` (the saved
//! `razdor-bench-*.txt`'s name) or, when it cannot start or the run is cancelled, `bench-autorun.error`
//! (why). Both files go in the same folder, and the log gets `bench autorun: …` lines.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use razdor::local;

use razdor::lab::{self, Flags};

use super::bench::{self, Choice};
use super::{App, Screen};

/// The trigger file (read once, renamed to [`DONE`]).
pub const TRIGGER: &str = "bench-autorun.txt";
/// The trigger file once it has been read (a crash does not start the run again).
pub const DONE: &str = "bench-autorun.done";
/// Written at the run's end: the saved results file's name.
pub const FINISHED: &str = "bench-autorun.finished";
/// Written when the run cannot start, or stops before its end: why.
pub const ERROR: &str = "bench-autorun.error";

/// What the trigger file asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub choice: Choice,
    /// The map's file name in the game (`map=<name>`), or None for the default.
    pub map: Option<String>,
    /// The run profiles itself (a `profile` or `debug` line).
    pub profile: bool,
    /// The lab's switches the run has (the lines `replay-floor`, `gpu-finish`, `clear-targets`, `swap-interval 0|1`).
    pub lab: Flags,
}

/// Reads a trigger file's text: the choice on the first line, then optional `map=<name>` and `profile` lines.
/// An empty or unknown line is an error (the run would not be the one the file asks for).
pub fn parse(text: &str) -> Result<Trigger, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let first = lines.next().ok_or("the file has no choice line")?;
    let choice = match first {
        "all" => Choice::All,
        "mine" => Choice::Mine,
        other => return Err(format!("the choice {other:?} is not all or mine")),
    };
    let mut map = None;
    let mut profile = false;
    let mut lab = Flags::default();
    for line in lines {
        if line == "profile" || line == "debug" {
            profile = true;
            continue;
        }
        match line {
            "replay-floor" => lab.replay_floor = true,
            "gpu-finish" => lab.gpu_finish = true,
            "clear-targets" => lab.clear_targets = true,
            _ => {
                if let Some(value) = line.strip_prefix("swap-interval ") {
                    lab.swap_interval = Some(match value.trim() {
                        "0" => 0,
                        "1" => 1,
                        other => return Err(format!("the swap interval {other:?} is not 0 or 1")),
                    });
                    continue;
                }
                let Some(name) = line.strip_prefix("map=") else {
                    return Err(format!("unknown line {line:?}"));
                };
                let name = name.trim();
                if !name.is_empty() {
                    map = Some(name.to_string());
                }
            }
        }
    }
    Ok(Trigger { choice, map, profile, lab })
}

local! {
    /// The trigger was read in this process (renamed or refused): it is not looked at again.
    static CONSUMED: Cell<bool> = const { Cell::new(false) };
    /// The waiting for the install was logged once.
    static WAIT_LOGGED: Cell<bool> = const { Cell::new(false) };
    /// The run in progress was started from the trigger file (its end writes the finished or error file).
    static FROM_FILE: Cell<bool> = const { Cell::new(false) };
}

/// Each frame of the app: starts the trigger file's run when the install is loaded and the main menu is up.
pub fn tick(app: &mut App) {
    if CONSUMED.with(Cell::get) {
        return;
    }
    let Some(dir) = razdor::platform::app_dir() else { return };
    let path = dir.join(TRIGGER);
    if !path.is_file() {
        return;
    }
    if app.dt_content.is_none() {
        if !WAIT_LOGGED.with(|w| w.replace(true)) {
            razdor::diag::log("bench autorun: the trigger file is there; waiting for the game's install");
        }
        return;
    }
    // The menu is quiet: no window, message or dialog over it, and no test already running.
    let quiet = matches!(app.screen, Screen::MainMenu) && app.dialogs.is_empty() && app.message.is_none() && !app.update_open;
    if !quiet || bench::window_open() {
        return;
    }
    CONSUMED.with(|c| c.set(true));
    start_from(app, &dir, &path);
}

/// Reads the trigger, renames it, and starts the run (or writes why it did not start).
fn start_from(app: &mut App, dir: &Path, path: &Path) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => return refuse(dir, &format!("cannot read {}: {e}", path.display())),
    };
    if let Err(e) = std::fs::rename(path, dir.join(DONE)) {
        return refuse(dir, &format!("cannot rename the trigger file: {e}; not started"));
    }
    let trigger = match parse(&text) {
        Ok(t) => t,
        Err(e) => return refuse(dir, &format!("the trigger file: {e}")),
    };
    let choice = if trigger.choice == Choice::Mine { "mine" } else { "all" };
    let map = trigger.map.as_deref().map(|m| format!(" map={m}")).unwrap_or_default();
    let profile = if trigger.profile { " profile" } else { "" };
    let switches = trigger.lab.describe();
    let switches = if switches.is_empty() { String::new() } else { format!(" lab={switches}") };
    razdor::diag::log(&format!("bench autorun: {choice}{map}{profile}{switches}"));
    lab::set(trigger.lab);
    match bench::start_run(app, trigger.choice, trigger.map.as_deref(), trigger.profile) {
        Ok(()) => FROM_FILE.with(|f| f.set(true)),
        Err(e) => refuse(dir, &e),
    }
}

/// Logs why a run did not start, and writes the error file.
fn refuse(dir: &Path, why: &str) {
    razdor::diag::log(&format!("bench autorun: not started: {why}"));
    write(dir, ERROR, &format!("{why}\n"));
}

/// True when the run in progress was started from the trigger file.
pub fn from_file() -> bool {
    FROM_FILE.with(Cell::get)
}

/// The file-started run's end: the finished file (the saved results' name) or the error file. Called once
/// by the run's end and cancel; does nothing for a run the player started.
pub fn finished(result: Result<PathBuf, String>) {
    if !FROM_FILE.with(|f| f.replace(false)) {
        return;
    }
    let Some(dir) = razdor::platform::app_dir() else { return };
    match result {
        Ok(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            razdor::diag::log(&format!("bench autorun: finished {name}"));
            write(&dir, FINISHED, &format!("{name}\n"));
        }
        Err(why) => {
            razdor::diag::log(&format!("bench autorun: no results file: {why}"));
            write(&dir, ERROR, &format!("{why}\n"));
        }
    }
}

fn write(dir: &Path, name: &str, text: &str) {
    if let Err(e) = std::fs::write(dir.join(name), text) {
        razdor::diag::log(&format!("bench autorun: cannot write {name}: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_the_first_line() {
        assert_eq!(parse("all\n").unwrap(), Trigger { choice: Choice::All, map: None, profile: false, lab: Flags::default() });
        assert_eq!(parse("mine").unwrap(), Trigger { choice: Choice::Mine, map: None, profile: false, lab: Flags::default() });
    }

    #[test]
    fn a_map_line_names_the_map_and_crlf_is_fine() {
        let t = parse("all\r\nmap=Другой берег\r\n").unwrap();
        assert_eq!(t, Trigger { choice: Choice::All, map: Some("Другой берег".into()), profile: false, lab: Flags::default() });
        assert_eq!(parse("mine\nmap=  \n").unwrap().map, None);
    }

    #[test]
    fn a_byte_order_mark_and_notes_are_skipped() {
        let t = parse("\u{feff}# the lab's run\n\nall\n# twice\nmap=РК1-Начало пути\n").unwrap();
        assert_eq!(t, Trigger { choice: Choice::All, map: Some("РК1-Начало пути".into()), profile: false, lab: Flags::default() });
    }

    #[test]
    fn a_profile_line_turns_the_profiling_on_for_the_run_in_any_place() {
        let t = parse("all\nprofile\nmap=Другой берег\n").unwrap();
        assert_eq!(t, Trigger { choice: Choice::All, map: Some("Другой берег".into()), profile: true, lab: Flags::default() });
        // `debug` is the same line; the order of the lines does not matter.
        assert_eq!(parse("mine\ndebug").unwrap(), Trigger { choice: Choice::Mine, map: None, profile: true, lab: Flags::default() });
        assert!(parse("# the lab's run, with stages\n\nall\r\nprofile \r\n").unwrap().profile, "a note, a blank line and spaces are fine");
        assert!(!parse("all\nmap=X").unwrap().profile, "no profile line: no profiling");
        assert!(parse("all\nprofile=1\n").is_err(), "the line is `profile`, with no value");
    }

    #[test]
    fn the_lab_lines_set_the_run_switches() {
        let t = parse("all\nreplay-floor\ngpu-finish\nclear-targets\nswap-interval 0\n").unwrap();
        assert_eq!(t.lab, Flags { replay_floor: true, gpu_finish: true, clear_targets: true, swap_interval: Some(0) });
        assert_eq!(parse("mine\nswap-interval 1").unwrap().lab.swap_interval, Some(1));
        assert_eq!(parse("all\nmap=X").unwrap().lab, Flags::default(), "no line: no switch");
        assert!(parse("all\nswap-interval 2\n").is_err(), "only the two intervals the lab compares");
        assert!(parse("all\nswap-interval\n").is_err(), "the interval needs its value");
        assert!(parse("all\nreplay-floor=1\n").is_err(), "a switch line takes no value");
    }

    #[test]
    fn an_empty_or_unknown_file_is_refused() {
        assert!(parse("").is_err());
        assert!(parse("# only a note\n").is_err());
        assert!(parse("everything\n").is_err());
        assert!(parse("all\nspeed=4\n").is_err());
        assert!(parse("all\nmap=X\nlater\n").is_err());
    }
}
