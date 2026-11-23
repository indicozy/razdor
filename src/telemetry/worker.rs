//! The background sender. [`step`] sends at most one queued item and never holds the queue's lock
//! while the network runs, so a frame never waits for it. [`run`] is the thread's loop.
//!
//! What an answer does: 2xx sends (a crash report's id is kept for its save); 408, 429 and 5xx and
//! any transport failure wait and retry, with a backoff; any other 4xx drops the item, because the
//! server refuses it on purpose (sending it again would only get the install banned). The address
//! list is tried in order; a host the mirror paused is skipped.

use std::time::Duration;

use razdor_wire::{frame, Ack};

use super::jitter::{jittered, seed_of};
use super::queue::Class;
use super::state::Consent;
use super::transport::Transport;
use super::Hub;
use crate::mirror;

/// The stall limit of one request, seconds.
pub const SECONDS: u32 = 30;

/// How long the loop sleeps when nothing is due (it wakes at once on a new item).
const IDLE_SECS: u64 = 300;

/// The outcome of one [`step`].
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// An item was sent or dropped: try the next one at once.
    Progress,
    /// Nothing is due; the next item is due in this many seconds (`None`: the queue has none to send).
    Wait(Option<u64>),
    /// Every address is paused by the mirror; try again shortly.
    Paused,
}

/// The wait before the `tries`-th retry: a minute, doubled each time, at most six hours.
pub fn backoff_secs(tries: u32) -> u64 {
    (60u64 << tries.min(8)).min(6 * 3600)
}

/// The wait before the `tries`-th retry of an item: [`backoff_secs`] with ±25% jitter from the install's
/// seed (`jitter::jittered`). Called with the queue's lock held: the order is queue, then state.
fn retry_wait(hub: &Hub, tries: u32) -> u64 {
    let seed = seed_of(&hub.install_id());
    jittered(backoff_secs(tries) as i64, seed, tries) as u64
}

/// Sends the first due item, if any. `now` is unix seconds. The api addresses come from the config
/// (`remote_config::api`), or the caller's list in tests.
pub fn step(hub: &Hub, transport: &dyn Transport, api: &[String], now: u64) -> Step {
    let consent = hub.consent();
    // Pick the item and read its body under the lock; the lock is released before the network.
    let picked = {
        let mut queue = hub.queue();
        let crash_ids = hub.crash_ids();
        let due = queue
            .metas()
            .filter(|m| m.class == Class::Always || consent == Consent::Yes)
            .filter(|m| m.after.as_ref().is_none_or(|t| crash_ids.contains_key(t)))
            .map(|m| (m.seq, m.not_before))
            .collect::<Vec<_>>();
        match due.iter().find(|(_, at)| *at <= now) {
            Some(&(seq, _)) => {
                let meta = queue.metas().find(|m| m.seq == seq).cloned().expect("the item is queued");
                match queue.body(seq) {
                    Ok(body) => Some((meta, body, crash_ids)),
                    Err(e) => {
                        crate::diag!("telemetry: item {seq} is unreadable, dropped: {e}");
                        queue.remove(seq);
                        return Step::Progress;
                    }
                }
            }
            None => return Step::Wait(due.iter().map(|(_, at)| at.saturating_sub(now)).min()),
        }
    };
    let Some((meta, body, crash_ids)) = picked else { return Step::Wait(None) };

    let Some(base) = api.iter().find(|a| !mirror::paused(a)) else { return Step::Paused };
    let url = format!("{base}{}", meta.path);
    let payload = match &meta.header {
        None => body,
        Some(header) => {
            let mut header = header.clone();
            if let (Some(after), Some(obj)) = (&meta.after, header.as_object_mut()) {
                obj.insert("crash_id".into(), crash_ids.get(after).cloned().unwrap_or_default().into());
            }
            match frame::encode(&header, &body) {
                Ok(bytes) => bytes,
                Err(e) => {
                    crate::diag!("telemetry: item {} does not frame, dropped: {e}", meta.seq);
                    hub.queue().remove(meta.seq);
                    return Step::Progress;
                }
            }
        }
    };

    let answer = transport.post(&url, &payload, SECONDS);
    crate::server_status::note(crate::server_status::reach_of(answer.as_ref().ok().map(|r| r.status)));
    let mut queue = hub.queue();
    match answer {
        Ok(reply) if (200..300).contains(&reply.status) => {
            mirror::answered(&url);
            if let (Some(token), Ok(ack)) = (&meta.token, serde_json::from_slice::<Ack>(&reply.body)) {
                if let Some(id) = ack.id {
                    hub.remember_crash_id(token, id);
                }
            }
            queue.remove(meta.seq);
            Step::Progress
        }
        Ok(reply) if reply.status == 408 || reply.status == 429 || reply.status >= 500 => {
            crate::diag!("telemetry: {url}: HTTP {}, retried later", reply.status);
            if reply.status == 429 || reply.status == 503 {
                mirror::throttled(&url, reply.retry_after);
            }
            let tries = meta.tries + 1;
            // The server's Retry-After, when it is longer than the backoff, is the wait.
            let wait = retry_wait(hub, meta.tries).max(reply.retry_after.unwrap_or(0));
            let _ = queue.reschedule(meta.seq, tries, now + wait);
            Step::Wait(Some(wait))
        }
        Ok(reply) => {
            crate::diag!("telemetry: {url}: HTTP {} refused the item, dropped", reply.status);
            queue.remove(meta.seq);
            Step::Progress
        }
        Err(why) => {
            mirror::note_failure(&url, &why, None);
            let wait = retry_wait(hub, meta.tries);
            let _ = queue.reschedule(meta.seq, meta.tries + 1, now + wait);
            Step::Wait(Some(wait))
        }
    }
}

/// The sender's thread: sends while there is work, sleeps otherwise, and does nothing while the
/// config's kill switch is off. It wakes on a new item ([`Hub::kick`]) and at least once a while.
pub fn run(hub: &Hub, transport: &dyn Transport, api: &dyn Fn() -> Vec<String>, now: &dyn Fn() -> u64) {
    loop {
        if !crate::remote_config::telemetry_allowed() {
            sleep_until_kicked(hub, 60);
            continue;
        }
        let secs = match step(hub, transport, &api(), now()) {
            Step::Progress => continue,
            Step::Wait(due) => due.map_or(IDLE_SECS, |s| s.min(IDLE_SECS)),
            Step::Paused => 30,
        };
        sleep_until_kicked(hub, secs);
    }
}

/// Sleeps `secs` seconds, or until a new item is queued.
fn sleep_until_kicked(hub: &Hub, secs: u64) {
    for _ in 0..secs.max(1) {
        if hub.take_kick() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::transport::Reply;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// An address no test shares with another (the mirror's pauses are per host).
    fn host(tag: &str) -> String {
        format!("https://{tag}.telemetry-test.invalid")
    }

    fn hub(name: &str) -> (Hub, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("razdor-worker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hub = Hub::open(&dir, razdor_wire::limits::CLIENT_QUEUE_BYTES).unwrap();
        (hub, dir)
    }

    struct Fixed(Result<Reply, String>);
    impl Transport for Fixed {
        fn post(&self, _url: &str, _body: &[u8], _seconds: u32) -> Result<Reply, String> {
            self.0.clone()
        }
    }

    /// Hangs for a while before it fails, like a host that never answers.
    struct Hang(Arc<AtomicUsize>);
    impl Transport for Hang {
        fn post(&self, _url: &str, _body: &[u8], _seconds: u32) -> Result<Reply, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1500));
            Err("curl: (7) Failed to connect".into())
        }
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply { status, body: body.as_bytes().to_vec(), retry_after: None }
    }

    #[test]
    fn a_2xx_sends_and_removes_the_item() {
        let (hub, dir) = hub("ok");
        hub.set_consent(Consent::Yes, "1.5.4");
        let a = host("ok");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        assert_eq!(step(&hub, &Fixed(Ok(reply(200, "{}"))), std::slice::from_ref(&a), 1_000), Step::Progress);
        assert!(hub.queue().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_item_is_dropped_and_a_throttle_is_retried_later() {
        let (hub, dir) = hub("codes");
        hub.set_consent(Consent::Yes, "1.5.4");
        let a = host("codes");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        assert_eq!(step(&hub, &Fixed(Ok(reply(400, ""))), std::slice::from_ref(&a), 1_000), Step::Progress);
        assert!(hub.queue().is_empty(), "a 400 is not sent again");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        let waited = step(&hub, &Fixed(Ok(reply(503, ""))), std::slice::from_ref(&a), 1_000);
        let Step::Wait(Some(wait)) = waited else { panic!("{waited:?}") };
        assert!((45..=75).contains(&wait), "a minute with ±25% jitter: {wait}");
        assert_eq!(hub.queue().metas().next().unwrap().not_before, 1_000 + wait);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_retry_after_longer_than_the_backoff_is_the_wait() {
        let (hub, dir) = hub("retry-after");
        hub.set_consent(Consent::Yes, "1.5.4");
        let a = host("retry-after");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        let asked = Reply { status: 429, body: Vec::new(), retry_after: Some(900) };
        let waited = step(&hub, &Fixed(Ok(asked)), std::slice::from_ref(&a), 1_000);
        assert_eq!(waited, Step::Wait(Some(900)), "the server's 900 s beats the minute's backoff");
        assert_eq!(hub.queue().metas().next().unwrap().not_before, 1_900);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_sent_without_consent_except_the_ping() {
        let (hub, dir) = hub("consent");
        let a = host("consent");
        assert!(hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).is_err());
        hub.enqueue_json("/v1/ping", Class::Always, b"{}".to_vec()).unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let t = Hang(hits.clone());
        // The ping goes out; the (never queued) event cannot.
        assert_eq!(step(&hub, &Fixed(Ok(reply(200, ""))), std::slice::from_ref(&a), 1_000), Step::Progress);
        assert_eq!(step(&hub, &t, std::slice::from_ref(&a), 1_000), Step::Wait(None));
        assert_eq!(hits.load(Ordering::SeqCst), 0, "no request for a consent item");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreachable_host_never_blocks_the_game_and_the_queue_caps_at_five_megabytes() {
        let (hub, dir) = hub("blocks");
        hub.set_consent(Consent::Yes, "1.5.4");
        let a = host("blocks");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        let hub = Arc::new(hub);
        let hits = Arc::new(AtomicUsize::new(0));
        let sender = {
            let hub = hub.clone();
            let api = vec![a.clone()];
            let hits = hits.clone();
            std::thread::spawn(move || step(&hub, &Hang(hits), &api, 1_000))
        };
        // The sender now holds its request open; enqueue on this thread meanwhile.
        std::thread::sleep(Duration::from_millis(200));
        let started = std::time::Instant::now();
        let body = vec![b'x'; 256 * 1024];
        for _ in 0..40 {
            let _ = hub.enqueue_json("/v1/perf", Class::Consent, body.clone());
        }
        let took = started.elapsed();
        assert!(took < Duration::from_millis(1000), "enqueue took {took:?}: it must not wait for the network");
        assert!(hub.queue().total() <= razdor_wire::limits::CLIENT_QUEUE_BYTES, "the queue is capped");
        assert!(matches!(sender.join().unwrap(), Step::Wait(Some(45..=75))), "the unreachable send is retried later");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
