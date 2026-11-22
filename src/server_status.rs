//! The server's status mark on the main menu («Сервер: в сети» / «нет связи» / «выключено»).
//!
//! No traffic of its own: the state comes from the answers to requests the game makes anyway, the
//! telemetry sender's posts (`telemetry::worker::step`, [`note`]) and the cloud's sync rounds (the
//! cloud phase, read by the menu). Only when nothing has been answered yet and something is enabled
//! (the send switch of the remote config, or a cloud account), one light `GET /healthz` goes out at
//! start-up (`start_probe`), once per launch. A 429 counts as «в сети»: the server answered.
//! Nothing here needs macroquad.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::cloud::Phase;

/// What the mark shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The server answered: green «в сети».
    Online,
    /// No answer from the server: grey «нет связи».
    Offline,
    /// Nothing is enabled (sending is off by the remote config and there is no cloud account): grey «выключено».
    Off,
    /// Enabled, but nothing has been answered yet: no mark.
    Unknown,
}

/// One request's outcome: the server answered, or it did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Answered,
    Silent,
}

/// The seconds the start-up probe waits for its answer.
const PROBE_SECONDS: u32 = 10;

/// The last request's outcome of this launch (any source).
static LAST: Mutex<Option<Reach>> = Mutex::new(None);

/// Set once the start-up probe has run (or been skipped): it never repeats within the launch.
static PROBED: AtomicBool = AtomicBool::new(false);

/// The outcome of a request that got `status` (`None`: no answer at all). A server error (5xx) or no
/// answer is silent; any other answer, a 429 included, means the server is there.
pub fn reach_of(status: Option<u16>) -> Reach {
    match status {
        Some(s) if s < 500 => Reach::Answered,
        _ => Reach::Silent,
    }
}

/// The reach a cloud phase tells: an answered sync round, or a silent one. Phases in the middle of a
/// round (`Working`) and with no account (`Off`) tell nothing.
pub fn cloud_reach(phase: Phase) -> Option<Reach> {
    match phase {
        Phase::Idle | Phase::Full | Phase::Failed => Some(Reach::Answered),
        Phase::Offline => Some(Reach::Silent),
        Phase::Working | Phase::Off => None,
    }
}

/// The mark for the given switches and the freshest reach: the cloud's decisive phase when it has one,
/// else the last request's outcome of this launch.
pub fn state(telemetry_on: bool, linked: bool, decisive: Option<Reach>, last: Option<Reach>) -> Mark {
    if !telemetry_on && !linked {
        return Mark::Off;
    }
    match decisive.or(last) {
        Some(Reach::Answered) => Mark::Online,
        Some(Reach::Silent) => Mark::Offline,
        None => Mark::Unknown,
    }
}

/// The mark the menu draws now: the switches from the remote config and the cloud's phase, and this
/// launch's last outcome.
pub fn mark(telemetry_on: bool, linked: bool, phase: Phase) -> Mark {
    state(telemetry_on, linked, cloud_reach(phase), last())
}

/// Records a request's outcome (the telemetry sender calls it after each post).
pub fn note(reach: Reach) {
    if let Ok(mut last) = LAST.lock() {
        *last = Some(reach);
    }
}

/// The last outcome of this launch, if any request has been answered or has failed.
pub fn last() -> Option<Reach> {
    LAST.lock().ok().and_then(|l| *l)
}

/// The health address of an API base (`remote_config::api`): `{base}/healthz`.
pub fn healthz_url(base: &str) -> String {
    format!("{}/healthz", base.trim_end_matches('/'))
}

/// The mark forced by `RAZDOR_SERVER_STATUS` (`online`, `offline`, `off` or `unknown`): for the
/// off-screen screenshots of each state. `None` when unset or not one of those.
pub fn forced() -> Option<Mark> {
    match std::env::var("RAZDOR_SERVER_STATUS").ok()?.as_str() {
        "online" => Some(Mark::Online),
        "offline" => Some(Mark::Offline),
        "off" => Some(Mark::Off),
        "unknown" => Some(Mark::Unknown),
        _ => None,
    }
}

/// The start-up probe: when nothing is enabled, nothing is answered yet and the analytics run, one
/// `GET {api}/healthz` goes out in the background. `cloud_linked`: the device has a cloud account.
/// Called once the game files are ready; it does nothing in the snapshots and when the analytics are
/// off (`RAZDOR_TELEMETRY=off`), and never repeats within the launch.
pub fn start_probe(cloud_linked: bool) {
    if !crate::telemetry::active() || !(crate::remote_config::telemetry_allowed() || cloud_linked) {
        return;
    }
    if last().is_some() || PROBED.swap(true, Ordering::SeqCst) {
        return;
    }
    let api = crate::remote_config::api();
    let Some(base) = api.iter().find(|a| !crate::mirror::paused(a)).cloned() else { return };
    let spawned = std::thread::Builder::new().name("server status probe".into()).spawn(move || {
        let net = crate::cloud::http::platform_net(PROBE_SECONDS);
        let status = net.send("GET", &healthz_url(&base), None, None).ok().map(|r| r.status);
        note(reach_of(status));
    });
    if let Err(e) = spawned {
        crate::diag!("server status: the probe does not start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_online_and_a_429_is_too() {
        assert_eq!(reach_of(Some(200)), Reach::Answered);
        assert_eq!(reach_of(Some(204)), Reach::Answered);
        assert_eq!(reach_of(Some(400)), Reach::Answered);
        assert_eq!(reach_of(Some(429)), Reach::Answered);
        assert_eq!(reach_of(Some(499)), Reach::Answered);
        assert_eq!(reach_of(Some(500)), Reach::Silent);
        assert_eq!(reach_of(Some(503)), Reach::Silent);
        assert_eq!(reach_of(None), Reach::Silent);
    }

    #[test]
    fn the_cloud_phases_tell_the_reach() {
        assert_eq!(cloud_reach(Phase::Idle), Some(Reach::Answered));
        assert_eq!(cloud_reach(Phase::Full), Some(Reach::Answered));
        assert_eq!(cloud_reach(Phase::Failed), Some(Reach::Answered));
        assert_eq!(cloud_reach(Phase::Offline), Some(Reach::Silent));
        assert_eq!(cloud_reach(Phase::Working), None);
        assert_eq!(cloud_reach(Phase::Off), None);
    }

    #[test]
    fn nothing_enabled_is_off_whatever_was_answered() {
        assert_eq!(state(false, false, None, None), Mark::Off);
        assert_eq!(state(false, false, Some(Reach::Answered), Some(Reach::Answered)), Mark::Off);
    }

    #[test]
    fn enabled_and_nothing_yet_is_unknown() {
        assert_eq!(state(true, false, None, None), Mark::Unknown);
        assert_eq!(state(false, true, None, None), Mark::Unknown);
    }

    #[test]
    fn the_cloud_phase_wins_over_the_last_request() {
        assert_eq!(state(true, true, Some(Reach::Silent), Some(Reach::Answered)), Mark::Offline);
        assert_eq!(state(true, true, Some(Reach::Answered), Some(Reach::Silent)), Mark::Online);
        assert_eq!(state(true, false, None, Some(Reach::Answered)), Mark::Online);
        assert_eq!(state(true, false, None, Some(Reach::Silent)), Mark::Offline);
    }

    #[test]
    fn the_health_address_joins_the_api_base() {
        assert_eq!(healthz_url("https://razdor.indicozy.dev/api"), "https://razdor.indicozy.dev/api/healthz");
        assert_eq!(healthz_url("https://example.org/api/"), "https://example.org/api/healthz");
    }
}
