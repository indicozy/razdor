//! Parallel byte ranges: a big file is fetched in pieces at once when the server answers Range
//! requests (GitVerse gives about 140-500 KB/s per connection; eight connections add up).
//!
//! The rule: a first request for byte 0 only (the probe) tells the size and whether ranges work.
//! A 206 with a size over [`MIN_SIZE`] (and not over the cap) gets [`PIECES`] pieces at once, each
//! ≥ [`MIN_PIECE`] (fewer for a smaller file). Each piece goes to its own file (`<dest>.piece<i>`)
//! and must come back with its exact byte count; a piece that stalls, fails, comes back short or
//! not as a 206 is tried again, [`TRIES`] times in all, and then the whole address fails. The
//! pieces are joined into `dest` in order and the caller checks the whole file (size, SHA-256),
//! as for a single stream. A 200 to the probe (no ranges), a probe that fails, or a small file
//! gives [`Parted::Single`]: the caller takes its one stream as before.
//!
//! Nothing here needs macroquad or a platform: the network is a [`RangeNet`] (the desktop's
//! `curl`, the phones' downloader, a test's fake). A cancel and the progress are watched by the
//! thread that calls [`fetch`]; the pieces only see a stop flag.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// The pieces a file is fetched in at once.
pub const PIECES: u64 = 8;
/// The pieces at once for a gitverse.ru address: GitVerse answers 429 (rate limit) to eight.
pub const PIECES_GITVERSE: u64 = 4;
/// The waits before a piece is tried again after a 429 or 503 (not counted as a failed try until they
/// are spent): 1 s, 2 s, 4 s, or the server's `Retry-After` (capped at [`MAX_RETRY_AFTER`]).
pub const BACKOFF: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)];
/// The longest wait a `Retry-After` may ask for, in seconds.
pub const MAX_RETRY_AFTER: u64 = 60;
/// The smallest piece: a file splits into fewer pieces when it is smaller than [`PIECES`] of these.
pub const MIN_PIECE: u64 = 512 << 10;
/// A file this small (or smaller) is one stream: no pieces.
pub const MIN_SIZE: u64 = 4 << 20;
/// The tries a piece gets before its address fails (the first try counts).
pub const TRIES: u32 = 3;
/// The reason of a cancelled fetch (the caller reads its own cancel flag as well).
pub const CANCELLED: &str = "cancelled";
/// How often the thread that runs [`fetch`] reads the progress and the cancel.
const POLL: Duration = Duration::from_millis(100);

/// The answer to one range request: its HTTP status (206 when the range was given, 200 when the
/// server ignored it) and the file's whole size from `Content-Range`, when the server says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeReply {
    pub status: u16,
    pub total: Option<u64>,
    /// The server's `Retry-After` in seconds, when it gave one as a number (a 429 or 503).
    pub retry_after: Option<u64>,
}

/// The network the pieces use. `Sync`: the pieces run on threads.
pub trait RangeNet: Sync {
    /// Bytes `start..=end` of `url` into the file `path` (made new or replaced). A 206 writes the
    /// range's body there; a 200 (the server ignored the range) writes nothing the caller needs.
    /// `progress` gets the bytes of this request so far; `cancelled` is asked as it goes, and true
    /// stops the request with an error.
    fn range(&self, url: &str, path: &Path, start: u64, end: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<RangeReply, String>;
    /// Waits `how_long` before the next try of a piece; returns early when `stopped` is true. A test
    /// gives a fake that records the wait instead.
    fn wait(&self, how_long: Duration, stopped: &dyn Fn() -> bool) {
        let until = std::time::Instant::now() + how_long;
        while !stopped() {
            let now = std::time::Instant::now();
            if now >= until {
                break;
            }
            std::thread::sleep((until - now).min(POLL));
        }
    }
}

/// The pieces a file from `url` is fetched in at once: [`PIECES_GITVERSE`] for a gitverse.ru
/// address, else [`PIECES`].
pub fn pieces_for(url: &str) -> u64 {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    if host == "gitverse.ru" || host.ends_with(".gitverse.ru") { PIECES_GITVERSE } else { PIECES }
}

/// A status that asks to wait and try again (rate limit, or the server busy).
fn throttled(status: u16) -> bool {
    matches!(status, 429 | 503)
}

/// What [`fetch`] did: the file is in `dest` (pieces), or the caller takes one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parted {
    Joined,
    Single,
}

/// The byte ranges (first, last: inclusive) of a file of `size` bytes, in order: at most `most`
/// pieces of at least [`MIN_PIECE`] each, the last one taking the rest.
pub fn layout(size: u64, most: u64) -> Vec<(u64, u64)> {
    if size == 0 {
        return Vec::new();
    }
    let n = (size / MIN_PIECE).clamp(1, most.max(1));
    let each = size / n;
    (0..n)
        .map(|i| {
            let start = i * each;
            let end = if i + 1 == n { size - 1 } else { start + each - 1 };
            (start, end)
        })
        .collect()
}

/// `dest` with `.<suffix>` after its name.
fn sibling(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// The file piece `i` of `dest` is fetched into.
pub fn piece_path(dest: &Path, i: usize) -> PathBuf {
    sibling(dest, &format!("piece{i}"))
}

/// Fetches `url` into `dest` in pieces when the server allows it (see the module's rule). `cap` is
/// the most the file may be (a bigger one is one stream, which refuses it). `progress` gets the
/// bytes of all pieces so far; `cancelled` is asked every [`POLL`] and true stops every piece.
/// Errors: a piece that failed all its tries (its reason), or [`CANCELLED`]. On an error, or a
/// cancel, no piece is left and `dest` is not written.
pub fn fetch(net: &dyn RangeNet, url: &str, dest: &Path, cap: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<Parted, String> {
    let probe = sibling(dest, "probe");
    let answer = net.range(url, &probe, 0, 0, &|_| {}, cancelled);
    let _ = fs::remove_file(&probe);
    let total = match answer {
        Ok(RangeReply { status: 206, total: Some(total), .. }) => total,
        Ok(_) => return Ok(Parted::Single),
        Err(why) if cancelled() => return Err(why),
        Err(why) => {
            crate::diag!("download: no range probe, one stream: {why}");
            return Ok(Parted::Single);
        }
    };
    if total <= MIN_SIZE || total > cap {
        return Ok(Parted::Single);
    }
    let pieces = layout(total, pieces_for(url));
    let paths: Vec<PathBuf> = (0..pieces.len()).map(|i| piece_path(dest, i)).collect();
    let fetched = fetch_pieces(net, url, &pieces, &paths, progress, cancelled);
    // A cancel is the cancel, whichever piece's error was the first one given.
    let result = if cancelled() { Err(CANCELLED.to_string()) } else { fetched.and_then(|()| join(dest, &paths, total)) };
    for path in &paths {
        let _ = fs::remove_file(path);
    }
    result?;
    progress(total);
    crate::diag!("download: {total} bytes in {} pieces from {url}", pieces.len());
    Ok(Parted::Joined)
}

/// Every piece on its own thread; this thread reads the progress and the cancel. The first piece
/// that fails all its tries stops the others and is the error.
fn fetch_pieces(net: &dyn RangeNet, url: &str, pieces: &[(u64, u64)], paths: &[PathBuf], progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    for path in paths {
        let _ = fs::remove_file(path);
    }
    let done: Vec<AtomicU64> = pieces.iter().map(|_| AtomicU64::new(0)).collect();
    let stop = AtomicBool::new(false);
    let running = AtomicUsize::new(pieces.len());
    let failure: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        for (i, &(start, end)) in pieces.iter().enumerate() {
            let (done, stop, running, failure, path) = (&done[i], &stop, &running, &failure, &paths[i]);
            s.spawn(move || {
                if let Err(why) = fetch_piece(net, url, path, i, start, end, done, stop) {
                    let mut first = failure.lock().unwrap_or_else(|e| e.into_inner());
                    if first.is_none() {
                        *first = Some(why);
                    }
                    stop.store(true, Ordering::SeqCst);
                }
                running.fetch_sub(1, Ordering::SeqCst);
            });
        }
        while running.load(Ordering::SeqCst) > 0 {
            if cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
            progress(done.iter().map(|d| d.load(Ordering::SeqCst)).sum());
            std::thread::sleep(POLL);
        }
    });
    match failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// The reason a piece gives when it is stopped by another piece's failure or a cancel.
const STOPPED: &str = "stopped";

/// One piece: up to [`TRIES`] tries, each request checked for its 206 and its exact byte count. A 429
/// or 503 is not a try: the piece waits ([`BACKOFF`], or the server's `Retry-After`) and asks again,
/// until the backoff is spent; then it counts as a failed try like any other.
#[allow(clippy::too_many_arguments)]
fn fetch_piece(net: &dyn RangeNet, url: &str, path: &Path, i: usize, start: u64, end: u64, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    let want = end - start + 1;
    let stopped = || stop.load(Ordering::SeqCst);
    let mut tries = 0u32;
    let mut waits = 0usize;
    loop {
        if stopped() {
            return Err(STOPPED.into());
        }
        done.store(0, Ordering::SeqCst);
        let mut hold = None;
        let last = match net.range(url, path, start, end, &|n| done.store(n, Ordering::SeqCst), &stopped) {
            Ok(RangeReply { status: 206, .. }) => {
                let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                if len == want {
                    done.store(want, Ordering::SeqCst);
                    return Ok(());
                }
                format!("piece {i} came back with {len} bytes, not {want}")
            }
            Ok(RangeReply { status, retry_after, .. }) => {
                if throttled(status) && waits < BACKOFF.len() {
                    hold = Some(retry_after.map_or(BACKOFF[waits], |s| Duration::from_secs(s.min(MAX_RETRY_AFTER))));
                    waits += 1;
                }
                format!("the server answered HTTP {status} to a range request")
            }
            Err(why) => why,
        };
        if stopped() {
            return Err(last);
        }
        if let Some(how_long) = hold {
            crate::diag!("download piece {i}: the server is busy, waiting {} s", how_long.as_secs_f64());
            net.wait(how_long, &stopped);
            continue;
        }
        tries += 1;
        crate::diag!("download piece {i}, try {tries} of {TRIES}: {last}");
        if tries == TRIES {
            return Err(last);
        }
    }
}

/// The pieces, in order, into `dest`. The result holds exactly `total` bytes or it is an error.
fn join(dest: &Path, paths: &[PathBuf], total: u64) -> Result<(), String> {
    let mut out = fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut copied = 0u64;
    for path in paths {
        let mut piece = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        copied += io::copy(&mut piece, &mut out).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    out.flush().map_err(|e| format!("{}: {e}", dest.display()))?;
    if copied != total {
        return Err(format!("the pieces hold {copied} bytes, not {total}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool as Flag;
    use std::sync::Arc;

    const MIB: u64 = 1 << 20;

    /// The test's file: `n` bytes of a pattern that differs from byte to byte.
    fn body(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i as u64).wrapping_mul(2654435761) >> 13) as u8).collect()
    }

    /// What the fake does with one request.
    #[derive(Clone, Debug)]
    enum Act {
        /// The range, in full (206).
        Good,
        /// Half of the range, with a matching Content-Length: a piece that comes back short.
        Short,
        /// A transfer error (its reason).
        Fail(&'static str),
        /// An HTTP status with no body.
        Status(u16),
        /// A 429 with a `Retry-After` (seconds), no body.
        Throttle(Option<u64>),
    }

    /// A fake range server. `ranges` false: every request is answered 200 with nothing written.
    /// `script` gives the actions of the requests for one start byte, in order (then `Good`).
    struct Fake {
        body: Vec<u8>,
        ranges: bool,
        script: Mutex<Vec<(u64, VecDeque<Act>)>>,
        /// (start, end) of every request, in order.
        log: Mutex<Vec<(u64, u64)>>,
        /// The bytes a request sends per poll of its cancel (small, so a cancel lands mid-piece).
        chunk: usize,
        /// The waits asked of the network (`RangeNet::wait`), in order; none is slept.
        waits: Mutex<Vec<Duration>>,
        /// Bytes written by all requests so far.
        written: AtomicU64,
        /// The test's cancel flag and the byte count at which the fake sets it (a cancel asked
        /// by the transfer itself, so the test does not depend on timing).
        cancel_after: Option<(Arc<Flag>, u64)>,
    }

    impl Fake {
        fn new(body: Vec<u8>) -> Fake {
            Fake { body, ranges: true, script: Mutex::new(Vec::new()), log: Mutex::new(Vec::new()), chunk: 64 << 10, written: AtomicU64::new(0), cancel_after: None, waits: Mutex::new(Vec::new()) }
        }

        fn with(mut self, start: u64, acts: Vec<Act>) -> Fake {
            self.script.get_mut().unwrap().push((start, acts.into()));
            self
        }

        fn requests_for(&self, start: u64) -> usize {
            self.log.lock().unwrap().iter().filter(|(s, _)| *s == start).count()
        }

        fn act(&self, start: u64) -> Act {
            let mut script = self.script.lock().unwrap();
            match script.iter_mut().find(|(s, _)| *s == start) {
                Some((_, acts)) => acts.pop_front().unwrap_or(Act::Good),
                None => Act::Good,
            }
        }
    }

    impl RangeNet for Fake {
        fn range(&self, _url: &str, path: &Path, start: u64, end: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<RangeReply, String> {
            self.log.lock().unwrap().push((start, end));
            let total = self.body.len() as u64;
            if !self.ranges {
                return Ok(RangeReply { status: 200, total: None, retry_after: None });
            }
            let act = self.act(start);
            let end = end.min(total - 1);
            let mut bytes = self.body[start as usize..=end as usize].to_vec();
            match act {
                Act::Fail(why) => return Err(why.to_string()),
                Act::Status(code) => return Ok(RangeReply { status: code, total: Some(total), retry_after: None }),
                Act::Throttle(after) => return Ok(RangeReply { status: 429, total: Some(total), retry_after: after }),
                Act::Short => bytes.truncate(bytes.len() / 2),
                Act::Good => {}
            }
            let mut out = Vec::new();
            for piece in bytes.chunks(self.chunk) {
                if cancelled() {
                    fs::write(path, &out).map_err(|e| e.to_string())?;
                    return Err("cancelled".into());
                }
                out.extend_from_slice(piece);
                std::thread::sleep(Duration::from_millis(1));
                let written = self.written.fetch_add(piece.len() as u64, Ordering::SeqCst) + piece.len() as u64;
                if let Some((flag, at)) = &self.cancel_after {
                    if written >= *at {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
                progress(out.len() as u64);
            }
            fs::write(path, &out).map_err(|e| e.to_string())?;
            Ok(RangeReply { status: 206, total: Some(total), retry_after: None })
        }

        fn wait(&self, how_long: Duration, _stopped: &dyn Fn() -> bool) {
            self.waits.lock().unwrap().push(how_long);
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-parts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn no_cancel() -> bool {
        false
    }

    const URL: &str = "https://example.org/releases/big.zip";

    #[test]
    fn the_file_is_cut_into_at_most_eight_pieces_of_at_least_512_kib() {
        assert_eq!(layout(0, PIECES), Vec::<(u64, u64)>::new());
        let big = layout(10 * MIB, PIECES);
        assert_eq!(big.len(), 8);
        assert_eq!(big.first().unwrap().0, 0);
        assert_eq!(big.last().unwrap().1, 10 * MIB - 1);
        for w in big.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0, "the pieces are contiguous");
        }
        assert!(big.iter().all(|(s, e)| e - s + 1 >= MIN_PIECE));
        // Fewer pieces for a smaller file: 2 MiB is four pieces of 512 KiB.
        let small = layout(2 * MIB, PIECES);
        assert_eq!(small.len(), 4);
        assert!(small.iter().all(|(s, e)| e - s + 1 == 512 << 10));
        // The sizes add up exactly, also when the size does not divide.
        let odd = layout(9 * MIB + 7, PIECES);
        assert!(layout(10 * MIB, PIECES_GITVERSE).len() == 4, "at most four for GitVerse");
        assert_eq!(odd.iter().map(|(s, e)| e - s + 1).sum::<u64>(), 9 * MIB + 7);
    }

    #[test]
    fn the_parts_are_joined_in_order_and_each_is_one_request() {
        let dir = scratch("joined");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let net = Fake::new(data.clone());
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data);
        let log = net.log.lock().unwrap().clone();
        assert_eq!(log[0], (0, 0), "the first request is the probe");
        assert_eq!(log.len(), 9, "the probe and eight pieces");
        assert!(!dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("piece")), "no piece is left");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_short_piece_is_tried_again_and_the_second_try_is_good() {
        let dir = scratch("short");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let piece3 = layout(10 * MIB, PIECES)[3].0;
        let net = Fake::new(data.clone()).with(piece3, vec![Act::Short]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data, "the short piece was replaced by a whole one");
        assert_eq!(net.requests_for(piece3), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_piece_that_fails_three_times_fails_the_address_and_leaves_nothing() {
        let dir = scratch("fails");
        let dest = dir.join("big.zip");
        let piece5 = layout(10 * MIB, PIECES)[5].0;
        let net = Fake::new(body(10 * MIB as usize)).with(piece5, vec![Act::Fail("stalled"), Act::Status(500), Act::Short]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Err("piece 5 came back with 655360 bytes, not 1310720".to_string()));
        assert_eq!(net.requests_for(piece5), 3, "three tries in all");
        assert!(!dest.exists());
        assert!(!dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("piece")), "no piece is left");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_piece_that_fails_once_then_works_is_fine() {
        let dir = scratch("retry-error");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        // Piece 1 (not piece 0: its start is the probe's, which is asked first).
        let piece1 = layout(10 * MIB, PIECES)[1].0;
        let net = Fake::new(data.clone()).with(piece1, vec![Act::Fail("curl: (56) reset")]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.requests_for(piece1), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_that_ignores_ranges_gives_one_stream() {
        let dir = scratch("no-range");
        let dest = dir.join("big.zip");
        let mut net = Fake::new(body(10 * MIB as usize));
        net.ranges = false;
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Single));
        assert!(!dest.exists(), "the caller takes its one stream");
        assert_eq!(net.log.lock().unwrap().len(), 1, "only the probe was asked");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_small_file_or_one_over_the_cap_is_one_stream() {
        let dir = scratch("small");
        let dest = dir.join("small.zip");
        let net = Fake::new(body(MIB as usize));
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Single));
        assert_eq!(net.log.lock().unwrap().len(), 1, "a small file costs the probe only");
        let net = Fake::new(body(10 * MIB as usize));
        assert_eq!(fetch(&net, URL, &dest, 9 * MIB, &|_| {}, &no_cancel), Ok(Parted::Single), "over the cap");
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_probe_that_fails_is_one_stream_and_a_probe_answer_without_a_size_too() {
        let dir = scratch("probe");
        let dest = dir.join("big.zip");
        let net = Fake::new(body(10 * MIB as usize)).with(0, vec![Act::Fail("could not resolve")]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Single));
        let net = Fake::new(body(10 * MIB as usize)).with(0, vec![Act::Status(416)]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Single));
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_progress_is_the_sum_of_the_pieces_and_ends_at_the_size() {
        let dir = scratch("progress");
        let dest = dir.join("big.zip");
        let size = 10 * MIB;
        let net = Fake::new(body(size as usize));
        let seen = Mutex::new(Vec::new());
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|n| seen.lock().unwrap().push(n), &no_cancel), Ok(Parted::Joined));
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.last(), Some(&size));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "the progress never goes back: {seen:?}");
        assert!(seen.iter().all(|&n| n <= size));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_stops_every_piece_and_leaves_nothing() {
        let dir = scratch("cancel");
        let dest = dir.join("big.zip");
        let mut net = Fake::new(body(10 * MIB as usize));
        // The transfer itself asks for the cancel once 1 MiB has come in, with pieces under way.
        let cancel = Arc::new(Flag::new(false));
        net.cancel_after = Some((cancel.clone(), MIB));
        let result = fetch(&net, URL, &dest, u64::MAX, &|_| {}, &|| cancel.load(Ordering::SeqCst));
        assert_eq!(result, Err(CANCELLED.to_string()));
        assert!(!dest.exists());
        assert!(!dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("piece")), "no piece is left");
        // The cancel reached the pieces: none went on to a retry (the probe and at most eight pieces).
        assert!(net.log.lock().unwrap().len() <= 9, "no piece is tried again after a cancel");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_asked_before_the_probe_stops_at_once() {
        let dir = scratch("cancel-first");
        let dest = dir.join("big.zip");
        let net = Fake::new(body(10 * MIB as usize));
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &|| true), Err("cancelled".to_string()));
        assert_eq!(net.log.lock().unwrap().len(), 1, "the probe only");
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_status_for_a_piece_is_tried_again() {
        let dir = scratch("status");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let piece1 = layout(10 * MIB, PIECES)[1].0;
        let net = Fake::new(data.clone()).with(piece1, vec![Act::Status(200)]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.requests_for(piece1), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_429_waits_one_second_and_the_retry_is_not_a_failed_try() {
        let dir = scratch("throttle-once");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let piece1 = layout(10 * MIB, PIECES)[1].0;
        let net = Fake::new(data.clone()).with(piece1, vec![Act::Status(429)]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.requests_for(piece1), 2);
        assert_eq!(net.waits.lock().unwrap().clone(), vec![Duration::from_secs(1)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_429_backs_off_one_two_four_seconds_then_counts_as_failed_tries() {
        let dir = scratch("throttle-spent");
        let dest = dir.join("big.zip");
        let piece1 = layout(10 * MIB, PIECES)[1].0;
        // Three waits, then three tries that all fail: six requests in all.
        let net = Fake::new(body(10 * MIB as usize)).with(piece1, vec![Act::Status(429); 6]);
        let result = fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel);
        assert_eq!(result, Err("the server answered HTTP 429 to a range request".to_string()));
        assert_eq!(net.requests_for(piece1), 6);
        assert_eq!(net.waits.lock().unwrap().clone(), vec![Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4)]);
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_503_waits_like_a_429_and_retry_after_is_honoured_and_capped() {
        let dir = scratch("throttle-after");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let piece1 = layout(10 * MIB, PIECES)[1].0;
        let net = Fake::new(data.clone()).with(piece1, vec![Act::Throttle(Some(7))]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(net.waits.lock().unwrap().clone(), vec![Duration::from_secs(7)]);
        let piece2 = layout(10 * MIB, PIECES)[2].0;
        let net = Fake::new(data.clone()).with(piece2, vec![Act::Status(503), Act::Throttle(Some(999))]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(net.waits.lock().unwrap().clone(), vec![Duration::from_secs(1), Duration::from_secs(MAX_RETRY_AFTER)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gitverse_gets_four_pieces_and_other_hosts_eight() {
        assert_eq!(pieces_for("https://gitverse.ru/api/attachments/x"), PIECES_GITVERSE);
        assert_eq!(pieces_for("https://GitVerse.ru:443/x"), PIECES_GITVERSE);
        assert_eq!(pieces_for("https://api.gitverse.ru/x"), PIECES_GITVERSE);
        assert_eq!(pieces_for("https://github.com/indicozy/razdor-community/releases/x"), PIECES);
        assert_eq!(pieces_for("https://indicozy.gitverse.site/razdor-community/index.json"), PIECES);
        assert_eq!(pieces_for("https://gitverse.ru.example.org/x"), PIECES);
        assert_eq!(pieces_for("https://evil.example/gitverse.ru/x"), PIECES);
        let dir = scratch("gitverse-pieces");
        let dest = dir.join("big.zip");
        let net = Fake::new(body(10 * MIB as usize));
        assert_eq!(fetch(&net, "https://gitverse.ru/api/attachments/x", &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(net.log.lock().unwrap().len(), 5, "the probe and four pieces");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A local Range server (`tools/range-server/range_server.py`) for the desktop tests: started
    /// on 127.0.0.1 with the given options, stopped when dropped.
    struct Server(std::process::Child, u16);

    impl Server {
        fn start(root: &Path, options: &[String]) -> Server {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/range-server/range_server.py");
            let mut child = Command::new("python3")
                .args(["-I", script, "--root"])
                .arg(root)
                .args(["--port", "0"])
                .args(options)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("python3 runs the range server");
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
            let port = line.strip_prefix("ready ").and_then(|p| p.trim().parse().ok()).expect("the server reports its port");
            Server(child, port)
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// The desktop's real download through the range server: `update::download_to` and
    /// `update::download_watched` (the game's path) into pieces, with one piece cut short and one
    /// failing once on the way, then a cancel. A manual run (`#[ignore]`), against the local server:
    /// `RAZDOR_RANGE_E2E_DIR=<empty dir> [RAZDOR_RANGE_E2E_THROTTLE=300000] [RAZDOR_RANGE_E2E_CLEAN=1]
    /// [RAZDOR_RANGE_E2E_NO_RANGE=1] cargo test --profile fasttest --lib parts::tests::desktop_end_to_end -- --ignored --nocapture`.
    /// `..._THROTTLE` is bytes a second per connection (the GitVerse speed), `..._CLEAN` skips the
    /// faults, `..._NO_RANGE` makes the server ignore Range (one stream for the timing).
    #[test]
    #[ignore = "runs curl against a local Python range server: a manual run"]
    fn desktop_end_to_end() {
        let Some(dir) = std::env::var_os("RAZDOR_RANGE_E2E_DIR") else { return };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let size: u64 = 12 * MIB;
        let data = body(size as usize);
        fs::write(dir.join("blob.bin"), &data).unwrap();
        let mut options: Vec<String> = Vec::new();
        if let Some(rate) = std::env::var("RAZDOR_RANGE_E2E_THROTTLE").ok() {
            options.extend(["--throttle".to_string(), rate]);
        }
        if std::env::var_os("RAZDOR_RANGE_E2E_NO_RANGE").is_some() {
            options.push("--no-range".into());
        } else if std::env::var_os("RAZDOR_RANGE_E2E_CLEAN").is_none() {
            // Piece 3 comes back short once, piece 5 fails once (the pieces are 1.5 MiB each).
            let piece = size / PIECES;
            options.extend(["--cut-once".to_string(), (3 * piece).to_string(), "--fail-once".to_string(), (5 * piece).to_string()]);
        }
        let server = Server::start(&dir, &options);
        let url = format!("http://127.0.0.1:{}/blob.bin", server.1);

        // 1. The hub and the updates' path: `download_to`.
        let first = dir.join("first.bin");
        let started = std::time::Instant::now();
        crate::update::download_to(&url, &first, 60, u64::MAX).expect("the download");
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(fs::read(&first).unwrap(), data, "the joined file is the file");
        println!("e2e: download_to {size} bytes in {secs:.2} s ({:.0} KB/s)", size as f64 / secs / 1024.0);

        // 2. The game's path: `download_watched` with the progress, to the end.
        let second = dir.join("second.bin");
        let last = AtomicU64::new(0);
        let started = std::time::Instant::now();
        let code = crate::update::download_watched(&url, &second, 0, 60, &|n| last.store(n, Ordering::SeqCst), &|| false).expect("the download");
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(code, 200);
        assert_eq!(fs::read(&second).unwrap(), data);
        assert_eq!(last.load(Ordering::SeqCst), size, "the progress ends at the size");
        println!("e2e: download_watched {size} bytes in {secs:.2} s ({:.0} KB/s)", size as f64 / secs / 1024.0);

        // 3. A cancel after 2 MiB, on a server slow enough that the pieces are still running (the
        // local one is done before the first poll): an error, no file, no piece left.
        let slow = Server::start(&dir, &["--throttle".to_string(), "200000".to_string()]);
        let url = format!("http://127.0.0.1:{}/blob.bin", slow.1);
        let third = dir.join("third.bin");
        let seen = AtomicU64::new(0);
        let started = std::time::Instant::now();
        let result = crate::update::download_watched(&url, &third, 0, 60, &|n| seen.store(n, Ordering::SeqCst), &|| seen.load(Ordering::SeqCst) >= 2 * MIB);
        println!("e2e: cancel after {:.2} s: {result:?}", started.elapsed().as_secs_f64());
        assert!(result.is_err());
        assert!(!third.exists());
        assert!(!dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("piece")));
        drop(server);
        let _ = fs::remove_dir_all(&dir);
    }
}
