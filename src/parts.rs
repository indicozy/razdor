//! Parallel byte ranges: a big file is fetched in pieces at once when the server answers Range
//! requests (GitVerse gives about 140-500 KB/s per connection; eight connections add up).
//!
//! The rule: a first request for byte 0 only (the probe) tells the size and whether ranges work.
//! A 206 with a size over [`MIN_SIZE`] (and not over the cap) gets [`PIECES`] pieces at once, each
//! ≥ [`MIN_PIECE`] (fewer for a smaller file).
//!
//! A piece RESUMES. Its requests write their bytes into their own files; when one stalls, fails or
//! comes back short, the bytes it has are kept and the next request asks for the rest of the range
//! (a Range request from the first byte not yet had). A piece gets [`TRIES`] tries in all, a 429 or
//! 503 waits first (see [`BACKOFF`]) and is not a try; then the whole address fails.
//!
//! Tail hedging: when every other piece is whole, a piece that is slow gets ONE more request, for
//! the rest of its bytes (from the first byte its primary request has not yet had). The first of the
//! two to make the piece whole wins; the other is cancelled. "Slow" is no new byte for
//! [`Tuning::stall`], or a rate under the finished pieces' average divided by [`Tuning::slow_ratio`]
//! once the piece is [`Tuning::min_age`] old ([`TUNING`] in the app).
//!
//! The pieces are joined into `dest` in order and the caller checks the whole file (size, SHA-256),
//! as for a single stream. A 200 to the probe (no ranges), a probe that fails, or a small file gives
//! [`Parted::Single`]: the caller takes its one stream as before.
//!
//! The network contract ([`RangeNet::range`]): a request writes its body into its file only when the
//! reply is a 206; a request that breaks off inside the body leaves the bytes that came (a prefix of
//! its range); any other reply leaves no file. That is what makes a partial file a resumable prefix.
//!
//! Nothing here needs macroquad or a platform: the network is a [`RangeNet`] (the desktop's
//! `curl`, the phones' downloader, a test's fake). A cancel and the progress are watched by the
//! thread that calls [`fetch`]; the pieces only see a stop flag.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
/// How often the thread that runs [`fetch`] reads the progress and the cancel, and how often a piece
/// waits on its hedge.
const POLL: Duration = Duration::from_millis(100);
/// The reason a piece gives when it is stopped by another piece's failure or a cancel.
const STOPPED: &str = "stopped";

/// When a running piece is hedged (see the module's rule): a piece with no new byte for `stall`, or
/// whose rate is under the finished pieces' average divided by `slow_ratio` once it is `min_age` old.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub stall: Duration,
    pub min_age: Duration,
    pub slow_ratio: f64,
}

/// The app's hedging: a piece with no byte for 8 s, or 3 s old and at a third of the others' rate.
pub const TUNING: Tuning = Tuning { stall: Duration::from_secs(8), min_age: Duration::from_secs(3), slow_ratio: 3.0 };

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
    /// range's body there, and a request that breaks off inside it leaves the bytes that came; any
    /// other reply (a 200, a 429, a refused status) leaves no file. `progress` gets the bytes of this
    /// request so far (written to the file); `cancelled` is asked as it goes, and true stops the
    /// request with an error.
    fn range(&self, url: &str, path: &Path, start: u64, end: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<RangeReply, String>;
    /// Waits `how_long` before the next try of a piece; returns early when `stopped` is true. A test
    /// gives a fake that records the wait instead.
    fn wait(&self, how_long: Duration, stopped: &dyn Fn() -> bool) {
        let until = Instant::now() + how_long;
        while !stopped() {
            let now = Instant::now();
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

/// A new file of piece `piece` for one request (`what` says which). Each name is used once in the
/// process, so a retry, a hedge or a later download never shares a file with another request.
fn new_file(dest: &Path, piece: usize, what: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    sibling(dest, &format!("piece{piece}.{what}{seq}"))
}

/// The size of the file at `path` (0 when there is none).
fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// A piece's primary request in flight: its first byte, its file, the bytes it has so far.
struct Req {
    from: u64,
    file: PathBuf,
    got: u64,
}

/// The piece's hedge (at most one per piece): none yet, running (its first byte and its bytes so
/// far; its file is in `made`), or done (won, failed or cancelled).
#[derive(Default)]
enum Hedge {
    #[default]
    None,
    Running { from: u64, got: u64 },
    Done,
}

/// A piece's state, under its lock: the threads of the piece and the coordinator read and write it.
#[derive(Default)]
struct Life {
    /// The primary's first request started (the piece's clock for its rate).
    started: Option<Instant>,
    /// The piece became whole.
    finished: Option<Instant>,
    /// Whole: `segments` hold its bytes, in order.
    won: bool,
    /// The bytes of the primary's finished requests that count, in order from the piece's first byte:
    /// each file and its bytes. They cover the piece up to the primary's next request.
    kept: Vec<(PathBuf, u64)>,
    /// The primary's request in flight.
    cur: Option<Req>,
    hedge: Hedge,
    /// The files that make the piece whole, in order, each with the bytes that count.
    segments: Vec<(PathBuf, u64)>,
    /// Every file the piece made (removed when the fetch ends).
    made: Vec<PathBuf>,
}

impl Life {
    fn kept_bytes(&self) -> u64 {
        self.kept.iter().map(|(_, n)| n).sum()
    }

    /// The bytes on hand from the piece's first byte: the kept ones with the primary's, or the
    /// hedge's when that reaches further.
    fn on_hand(&self, start: u64) -> u64 {
        let mut have = self.kept_bytes() + self.cur.as_ref().map_or(0, |c| c.got);
        if let Hedge::Running { from, got, .. } = self.hedge {
            have = have.max(from - start + got);
        }
        have
    }

    /// The files that hold the piece's bytes from its first byte up to `to` (exclusive), in order,
    /// each with the bytes that count; None when they do not reach `to`.
    fn cover(&self, start: u64, to: u64) -> Option<Vec<(PathBuf, u64)>> {
        let mut out = Vec::new();
        let mut at = start;
        for (file, n) in &self.kept {
            if at >= to {
                break;
            }
            let take = (*n).min(to - at);
            out.push((file.clone(), take));
            at += take;
        }
        if at < to {
            let cur = self.cur.as_ref()?;
            if cur.from != at || file_len(&cur.file) < to - at {
                return None;
            }
            out.push((cur.file.clone(), to - at));
            at = to;
        }
        (at == to).then_some(out)
    }

    /// The piece is whole, from these files.
    fn win(&mut self, segments: Vec<(PathBuf, u64)>) {
        self.segments = segments;
        self.won = true;
        self.cur = None;
        self.finished = Some(Instant::now());
    }
}

/// One piece: its byte range and its state.
struct Piece {
    start: u64,
    end: u64,
    life: Mutex<Life>,
    /// Set when the hedge won: the primary's request is cancelled.
    primary_stop: AtomicBool,
    /// Set when the primary won: the hedge is cancelled.
    hedge_stop: AtomicBool,
}

impl Piece {
    fn new(start: u64, end: u64) -> Piece {
        Piece { start, end, life: Mutex::new(Life::default()), primary_stop: AtomicBool::new(false), hedge_stop: AtomicBool::new(false) }
    }

    fn size(&self) -> u64 {
        self.end - self.start + 1
    }

    fn lock(&self) -> MutexGuard<'_, Life> {
        self.life.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn won(&self) -> bool {
        self.lock().won
    }
}

/// One fetch: the network, the address, the pieces, and the stop and the first failure they share.
struct Ctx<'a> {
    net: &'a dyn RangeNet,
    url: &'a str,
    dest: &'a Path,
    pieces: Vec<Piece>,
    /// Set by a cancel or a failed piece: every request stops.
    stop: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl Ctx<'_> {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// A piece failed for good: the first reason is the fetch's, and every other piece stops.
    fn fail(&self, why: String) {
        let mut first = self.failure.lock().unwrap_or_else(|e| e.into_inner());
        if first.is_none() {
            *first = Some(why);
        }
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Fetches `url` into `dest` in pieces when the server allows it (see the module's rule). `cap` is
/// the most the file may be (a bigger one is one stream, which refuses it). `progress` gets the
/// bytes of all pieces so far; `cancelled` is asked every [`POLL`] and true stops every piece.
/// Errors: a piece that failed all its tries (its reason), or [`CANCELLED`]. On an error, or a
/// cancel, no piece is left and `dest` is not written.
pub fn fetch(net: &dyn RangeNet, url: &str, dest: &Path, cap: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<Parted, String> {
    fetch_tuned(net, url, dest, cap, progress, cancelled, tuning())
}

/// The hedging the app uses. Tests turn it off for the baseline measurement (`HEDGE_OFF`).
#[cfg(not(test))]
fn tuning() -> Tuning {
    TUNING
}

#[cfg(test)]
static HEDGE_OFF: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn tuning() -> Tuning {
    if HEDGE_OFF.load(Ordering::SeqCst) {
        return Tuning { stall: Duration::from_secs(1 << 40), min_age: Duration::from_secs(1 << 40), slow_ratio: 3.0 };
    }
    TUNING
}

/// [`fetch`] with the hedging given (the tests use shorter times).
pub fn fetch_tuned(net: &dyn RangeNet, url: &str, dest: &Path, cap: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool, tune: Tuning) -> Result<Parted, String> {
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
    let ranges = layout(total, pieces_for(url));
    let ctx = Ctx {
        net,
        url,
        dest,
        pieces: ranges.iter().map(|&(start, end)| Piece::new(start, end)).collect(),
        stop: AtomicBool::new(false),
        failure: Mutex::new(None),
    };
    let ran = run(&ctx, progress, cancelled, tune);
    // A cancel is the cancel, whichever piece's error was the first one given.
    let result = if cancelled() {
        Err(CANCELLED.to_string())
    } else {
        ran.and_then(|()| join(dest, &segments(&ctx), total))
    };
    for piece in &ctx.pieces {
        for path in &piece.lock().made {
            let _ = fs::remove_file(path);
        }
    }
    result?;
    progress(total);
    crate::diag!("download: {total} bytes in {} pieces from {url}", ranges.len());
    Ok(Parted::Joined)
}

/// Every piece's files that make it whole, in order.
fn segments(ctx: &Ctx) -> Vec<(PathBuf, u64)> {
    ctx.pieces.iter().flat_map(|p| p.lock().segments.clone()).collect()
}

/// Every piece on its own thread (the primary requests), and the coordinator here: it reads the
/// progress and the cancel, and starts a hedge when a piece is slow and every other piece is whole.
/// The first piece that fails for good stops the others and is the error.
fn run(ctx: &Ctx, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool, tune: Tuning) -> Result<(), String> {
    let n = ctx.pieces.len();
    let running = AtomicUsize::new(n);
    let begun = Instant::now();
    let mut high = vec![0u64; n];
    let mut seen = vec![0u64; n];
    let mut changed = vec![begun; n];
    std::thread::scope(|s| {
        for i in 0..n {
            let running = &running;
            s.spawn(move || {
                if let Err(why) = primary(ctx, i) {
                    ctx.fail(why);
                }
                running.fetch_sub(1, Ordering::SeqCst);
            });
        }
        while running.load(Ordering::SeqCst) > 0 {
            if cancelled() {
                ctx.stop.store(true, Ordering::SeqCst);
            }
            let now = Instant::now();
            let mut sum = 0;
            let mut ons = Vec::with_capacity(n);
            for (i, p) in ctx.pieces.iter().enumerate() {
                let on = p.lock().on_hand(p.start);
                if on > seen[i] {
                    seen[i] = on;
                    changed[i] = now;
                }
                high[i] = high[i].max(on);
                sum += high[i];
                ons.push(on);
            }
            progress(sum);
            if !ctx.stopped() {
                let avg = average_rate(ctx);
                for (i, &on) in ons.iter().enumerate() {
                    if !hedge_due(ctx, i, now, changed[i], on, avg, tune) {
                        continue;
                    }
                    if let Some((from, file)) = start_hedge(ctx, i) {
                        running.fetch_add(1, Ordering::SeqCst);
                        let running = &running;
                        s.spawn(move || {
                            hedge_run(ctx, i, from, &file);
                            running.fetch_sub(1, Ordering::SeqCst);
                        });
                        crate::diag!("download piece {i}: a slow tail, hedged from byte {from}");
                    }
                }
            }
            std::thread::sleep(POLL);
        }
    });
    if let Some(why) = ctx.failure.lock().unwrap_or_else(|e| e.into_inner()).take() {
        return Err(why);
    }
    if ctx.pieces.iter().any(|p| !p.won()) {
        return Err("a piece is not whole".into());
    }
    Ok(())
}

/// The average rate (bytes a second) of the whole pieces, each over its own time from its first
/// request to its end; None while no piece is whole.
fn average_rate(ctx: &Ctx) -> Option<f64> {
    let rates: Vec<f64> = ctx
        .pieces
        .iter()
        .filter_map(|p| {
            let life = p.lock();
            let (from, to) = (life.started?, life.finished?);
            life.won.then(|| p.size() as f64 / to.duration_since(from).as_secs_f64().max(1e-3))
        })
        .collect();
    (!rates.is_empty()).then(|| rates.iter().sum::<f64>() / rates.len() as f64)
}

/// Whether piece `i` should get its hedge now: it has none, it is not whole, every other piece is
/// whole, and it is slow (see the module's rule). `on` is its bytes on hand, `changed` the last time
/// they grew, `avg` the whole pieces' average rate.
fn hedge_due(ctx: &Ctx, i: usize, now: Instant, changed: Instant, on: u64, avg: Option<f64>, tune: Tuning) -> bool {
    let p = &ctx.pieces[i];
    let started = {
        let life = p.lock();
        if life.won || !matches!(life.hedge, Hedge::None) {
            return false;
        }
        match life.started {
            Some(started) => started,
            None => return false,
        }
    };
    if !ctx.pieces.iter().enumerate().all(|(j, q)| j == i || q.won()) {
        return false;
    }
    let quiet = now.duration_since(changed);
    let elapsed = now.duration_since(started).as_secs_f64().max(1e-3);
    let slow_rate = avg.is_some_and(|a| elapsed >= tune.min_age.as_secs_f64() && (on as f64 / elapsed) < a / tune.slow_ratio);
    quiet >= tune.stall || slow_rate
}

/// Marks piece `i`'s hedge as running, from the first byte its primary has not yet had, with a new
/// file. Returns that byte and the file; None when the piece is whole or the rest is empty.
fn start_hedge(ctx: &Ctx, i: usize) -> Option<(u64, PathBuf)> {
    let p = &ctx.pieces[i];
    let mut life = p.lock();
    if life.won || !matches!(life.hedge, Hedge::None) {
        return None;
    }
    let from = match &life.cur {
        Some(c) => c.from + c.got,
        None => p.start + life.kept_bytes(),
    };
    if from > p.end {
        return None;
    }
    let file = new_file(ctx.dest, i, "hedge");
    life.made.push(file.clone());
    life.hedge = Hedge::Running { from, got: 0 };
    Some((from, file))
}

/// The hedge of piece `i`: the bytes `from..=end` into `file`, one request. It wins the piece when
/// the primary's bytes before `from` are on hand (see [`Life::cover`]); the primary is then cancelled.
fn hedge_run(ctx: &Ctx, i: usize, from: u64, file: &Path) {
    let p = &ctx.pieces[i];
    let want = p.end - from + 1;
    let stopped = || ctx.stopped() || p.hedge_stop.load(Ordering::SeqCst);
    let result = ctx.net.range(ctx.url, file, from, p.end, &|n| set_hedge_got(p, n), &stopped);
    let whole = matches!(result, Ok(RangeReply { status: 206, .. })) && file_len(file) == want;
    let mut life = p.lock();
    life.hedge = Hedge::Done;
    if !whole || life.won {
        return;
    }
    match life.cover(p.start, from) {
        Some(mut segments) => {
            segments.push((file.to_path_buf(), want));
            life.win(segments);
            p.primary_stop.store(true, Ordering::SeqCst);
            crate::diag!("download piece {i}: the hedge from byte {from} finished first");
        }
        None => crate::diag!("download piece {i}: the hedge cannot join the primary's bytes; dropped"),
    }
}

fn set_hedge_got(p: &Piece, n: u64) {
    if let Hedge::Running { got, .. } = &mut p.lock().hedge {
        *got = n;
    }
}

fn set_primary_got(p: &Piece, n: u64) {
    if let Some(cur) = p.lock().cur.as_mut() {
        cur.got = n;
    }
}

/// The primary's request of a piece has ended without the piece: its bytes that count are kept (the
/// next request resumes after them), or, when it brought none, its file goes. A piece that its kept
/// bytes now make whole is whole.
fn end_request(p: &Piece, file: &Path) {
    let mut life = p.lock();
    life.cur = None;
    if life.won {
        return;
    }
    let room = p.size() - life.kept_bytes();
    let len = file_len(file).min(room);
    if len > 0 {
        life.kept.push((file.to_path_buf(), len));
    } else {
        let _ = fs::remove_file(file);
    }
    if life.kept_bytes() == p.size() {
        let segments = life.kept.clone();
        life.win(segments);
        p.hedge_stop.store(true, Ordering::SeqCst);
    }
}

/// The primary's request brought the piece's last bytes (`want`, in `file`): the piece is whole, and
/// a hedge still running is cancelled. A piece that the hedge made whole first stays as it is.
fn finish_primary(p: &Piece, file: &Path, want: u64) {
    let mut life = p.lock();
    if life.won {
        return;
    }
    let mut segments = life.kept.clone();
    segments.push((file.to_path_buf(), want));
    life.win(segments);
    p.hedge_stop.store(true, Ordering::SeqCst);
}

/// Waits while the piece's hedge is running (the primary is out of tries): the hedge may yet make
/// the piece whole.
fn wait_hedge(ctx: &Ctx, p: &Piece) {
    loop {
        {
            let life = p.lock();
            if life.won || ctx.stopped() || !matches!(life.hedge, Hedge::Running { .. }) {
                return;
            }
        }
        std::thread::sleep(POLL);
    }
}

/// The primary of piece `i`: up to [`TRIES`] tries, each a request for the piece's bytes from the
/// first byte not yet had, checked for its 206 and its exact byte count. A 429 or 503 is not a try:
/// the piece waits ([`BACKOFF`], or the server's `Retry-After`) and asks again, until the backoff is
/// spent; then it counts as a failed try like any other.
fn primary(ctx: &Ctx, i: usize) -> Result<(), String> {
    let p = &ctx.pieces[i];
    let stopped = || ctx.stopped() || p.primary_stop.load(Ordering::SeqCst);
    p.lock().started.get_or_insert_with(Instant::now);
    let mut tries = 0u32;
    let mut waits = 0usize;
    loop {
        if ctx.stopped() {
            return Err(STOPPED.into());
        }
        if p.won() {
            return Ok(());
        }
        let (from, file) = {
            let mut life = p.lock();
            let from = p.start + life.kept_bytes();
            let file = new_file(ctx.dest, i, "try");
            life.made.push(file.clone());
            life.cur = Some(Req { from, file: file.clone(), got: 0 });
            (from, file)
        };
        let want = p.end - from + 1;
        let result = ctx.net.range(ctx.url, &file, from, p.end, &|n| set_primary_got(p, n), &stopped);
        let last = match result {
            Ok(RangeReply { status: 206, .. }) => {
                let len = file_len(&file);
                if len == want {
                    finish_primary(p, &file, want);
                    return Ok(());
                }
                format!("piece {i} came back with {len} bytes, not {want}")
            }
            Ok(RangeReply { status, retry_after, .. }) => {
                if throttled(status) && waits < BACKOFF.len() {
                    let how_long = retry_after.map_or(BACKOFF[waits], |s| Duration::from_secs(s.min(MAX_RETRY_AFTER)));
                    waits += 1;
                    end_request(p, &file);
                    crate::diag!("download piece {i}: the server is busy, waiting {} s", how_long.as_secs_f64());
                    ctx.net.wait(how_long, &stopped);
                    continue;
                }
                format!("the server answered HTTP {status} to a range request")
            }
            Err(why) => why,
        };
        end_request(p, &file);
        if ctx.stopped() {
            return Err(last);
        }
        if p.won() {
            return Ok(());
        }
        tries += 1;
        crate::diag!("download piece {i}, try {tries} of {TRIES}: {last}");
        if tries == TRIES {
            wait_hedge(ctx, p);
            return if p.won() { Ok(()) } else { Err(last) };
        }
    }
}

/// The pieces, in order, into `dest`: each segment's first bytes (`len`). The result holds exactly
/// `total` bytes or it is an error.
fn join(dest: &Path, segments: &[(PathBuf, u64)], total: u64) -> Result<(), String> {
    let mut out = fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut copied = 0u64;
    for (path, len) in segments {
        let piece = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        copied += io::copy(&mut piece.take(*len), &mut out).map_err(|e| format!("{}: {e}", dest.display()))?;
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
    /// The longest a fake hang waits for its cancel before it gives up (a test that would hang fails).
    const HANG_LIMIT: Duration = Duration::from_secs(10);

    /// The test's file: `n` bytes of a pattern that differs from byte to byte.
    fn body(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i as u64).wrapping_mul(2654435761) >> 13) as u8).collect()
    }

    /// The hedging of the tests: no piece is slow for the fetch's own timing, a slow piece is hedged
    /// after 300 ms of its own clock.
    const QUICK: Tuning = Tuning { stall: Duration::from_secs(60), min_age: Duration::from_millis(300), slow_ratio: 3.0 };

    /// What the fake does with one request.
    #[derive(Clone, Debug)]
    enum Act {
        /// The range, in full (206).
        Good,
        /// Half of the range, with a matching Content-Length: a piece that comes back short.
        Short,
        /// A transfer error before any byte (its reason).
        Fail(&'static str),
        /// An HTTP status with no body.
        Status(u16),
        /// A 429 with a `Retry-After` (seconds), no body.
        Throttle(Option<u64>),
        /// The first `n` bytes of the range, then a transfer error (a stall that breaks off).
        Stall(usize),
        /// The range, a chunk at a time with this many milliseconds between chunks (a slow connection).
        Slow(u64),
        /// No byte: the request waits until it is cancelled (a connection that stalls for good).
        Hang,
    }

    /// A fake range server. `ranges` false: every request is answered 200 with nothing written.
    /// `script` gives the actions of the requests for one start byte, in order; then `rules` (every
    /// request whose start is in the rule's range, each time); then `Good`.
    struct Fake {
        body: Vec<u8>,
        ranges: bool,
        script: Mutex<Vec<(u64, VecDeque<Act>)>>,
        rules: Vec<(u64, u64, Act)>,
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
            Fake {
                body,
                ranges: true,
                script: Mutex::new(Vec::new()),
                rules: Vec::new(),
                log: Mutex::new(Vec::new()),
                chunk: 64 << 10,
                waits: Mutex::new(Vec::new()),
                written: AtomicU64::new(0),
                cancel_after: None,
            }
        }

        fn with(mut self, start: u64, acts: Vec<Act>) -> Fake {
            self.script.get_mut().unwrap().push((start, acts.into()));
            self
        }

        fn rule(mut self, lo: u64, hi: u64, act: Act) -> Fake {
            self.rules.push((lo, hi, act));
            self
        }

        fn requests_for(&self, start: u64) -> usize {
            self.log.lock().unwrap().iter().filter(|(s, _)| *s == start).count()
        }

        /// The requests that start after `piece_start` and no later than `piece_end`: the hedges.
        fn hedges_in(&self, piece_start: u64, piece_end: u64) -> Vec<(u64, u64)> {
            self.log.lock().unwrap().iter().filter(|(s, _)| *s > piece_start && *s <= piece_end).copied().collect()
        }

        /// The requests that start in `lo..=hi`: a piece's first request and its hedge (the hedge may
        /// start at the piece's first byte when the primary had no byte yet).
        fn requests_within(&self, lo: u64, hi: u64) -> usize {
            self.log.lock().unwrap().iter().filter(|(s, _)| (lo..=hi).contains(s)).count()
        }

        fn act(&self, start: u64) -> Act {
            let mut script = self.script.lock().unwrap();
            if let Some((_, acts)) = script.iter_mut().find(|(s, _)| *s == start) {
                if let Some(act) = acts.pop_front() {
                    return act;
                }
            }
            self.rules.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&start)).map_or(Act::Good, |(_, _, act)| act.clone())
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
            let began = Instant::now();
            let end = end.min(total - 1);
            let body = &self.body[start as usize..=end as usize];
            match act {
                Act::Fail(why) => return Err(why.to_string()),
                Act::Status(code) => return Ok(RangeReply { status: code, total: Some(total), retry_after: None }),
                Act::Throttle(after) => return Ok(RangeReply { status: 429, total: Some(total), retry_after: after }),
                Act::Hang => {
                    while !cancelled() {
                        if began.elapsed() > HANG_LIMIT {
                            return Err("hung: no cancel came".into());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    return Err("cancelled".into());
                }
                _ => {}
            }
            let limit = match act {
                Act::Short => body.len() / 2,
                Act::Stall(n) => n.min(body.len()),
                _ => body.len(),
            };
            let mut file = fs::File::create(path).map_err(|e| e.to_string())?;
            let mut out = 0usize;
            for piece in body[..limit].chunks(self.chunk) {
                if cancelled() {
                    return Err("cancelled".into());
                }
                file.write_all(piece).map_err(|e| e.to_string())?;
                out += piece.len();
                let pause = match act {
                    Act::Slow(ms) => Duration::from_millis(ms),
                    _ => Duration::from_millis(1),
                };
                std::thread::sleep(pause);
                let written = self.written.fetch_add(piece.len() as u64, Ordering::SeqCst) + piece.len() as u64;
                if let Some((flag, at)) = &self.cancel_after {
                    if written >= *at {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
                progress(out as u64);
            }
            if let Act::Stall(_) = act {
                return Err("stalled".into());
            }
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

    /// No file of the pieces is left in `dir`.
    fn no_piece_left(dir: &Path) -> bool {
        !dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("piece"))
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
        assert!(no_piece_left(&dir), "no piece is left");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_short_piece_resumes_from_the_bytes_that_came() {
        let dir = scratch("short");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let (s3, e3) = layout(10 * MIB, PIECES)[3];
        let net = Fake::new(data.clone()).with(s3, vec![Act::Short]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data, "the short piece was completed, not replaced");
        let len = e3 - s3 + 1;
        assert!(net.log.lock().unwrap().contains(&(s3 + len / 2, e3)), "the next request asks for the rest");
        assert_eq!(net.requests_for(s3), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_piece_that_stalls_resumes_from_its_offset_and_the_data_is_whole() {
        let dir = scratch("stall");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let (s1, e1) = layout(10 * MIB, PIECES)[1];
        let net = Fake::new(data.clone()).with(s1, vec![Act::Stall(300_000)]);
        assert_eq!(fetch(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel), Ok(Parted::Joined));
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.requests_for(s1), 1, "the stalled request was not started again from the piece's first byte");
        let log = net.log.lock().unwrap().clone();
        assert!(log.contains(&(s1 + 300_000, e1)), "the resumed request starts where the bytes ran out: {log:?}");
        assert_eq!(log.len(), 10, "the probe, eight pieces and the one resumed request");
        assert!(no_piece_left(&dir));
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
        assert!(no_piece_left(&dir), "no piece is left");
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
        // A stall and a short piece on the way: the progress still never goes back.
        let (s2, _) = layout(size, PIECES)[2];
        let (s6, _) = layout(size, PIECES)[6];
        let net = Fake::new(body(size as usize)).with(s2, vec![Act::Stall(100_000)]).with(s6, vec![Act::Short]);
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
        assert!(no_piece_left(&dir), "no piece is left");
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
    fn a_slow_tail_piece_is_hedged_and_the_faster_copy_wins() {
        let dir = scratch("hedge");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let (s3, e3) = layout(10 * MIB, PIECES)[3];
        // The piece's own request is slow (100 ms a chunk, about 2 s in all); the hedge is not.
        let net = Fake::new(data.clone()).with(s3, vec![Act::Slow(100)]);
        let started = Instant::now();
        assert_eq!(fetch_tuned(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel, QUICK), Ok(Parted::Joined));
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(fs::read(&dest).unwrap(), data);
        let hedges = net.hedges_in(s3, e3);
        assert_eq!(hedges.len(), 1, "one hedge, at most one per piece: {hedges:?}");
        assert_eq!(hedges[0].1, e3, "the hedge asks for the piece's rest");
        assert_eq!(net.requests_for(s3), 1, "the slow request is not tried again");
        assert!(secs < 1.5, "the slow request alone takes about 2 s; the hedge finished the piece in {secs:.2} s");
        assert!(no_piece_left(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_piece_with_no_byte_for_the_stall_time_is_hedged() {
        let dir = scratch("hedge-stall");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let (s5, e5) = layout(10 * MIB, PIECES)[5];
        // The piece's first request hangs (no byte) until cancelled; its hedge, from the same byte, is fast.
        let net = Fake::new(data.clone()).with(s5, vec![Act::Hang]);
        let tune = Tuning { stall: Duration::from_millis(200), min_age: Duration::from_secs(3600), slow_ratio: 3.0 };
        let started = Instant::now();
        assert_eq!(fetch_tuned(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel, tune), Ok(Parted::Joined));
        assert!(started.elapsed().as_secs_f64() < 3.0, "the hedge, not the hang, finished the piece");
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.requests_within(s5, e5), 2, "the hanging request and its hedge");
        assert!(no_piece_left(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_copy_that_finishes_first_wins_and_the_other_is_cancelled() {
        let dir = scratch("hedge-loser");
        let dest = dir.join("big.zip");
        let data = body(10 * MIB as usize);
        let (s3, e3) = layout(10 * MIB, PIECES)[3];
        // The piece's own request finishes (about 2 s); its hedge hangs until cancelled.
        let net = Fake::new(data.clone()).with(s3, vec![Act::Slow(100)]).rule(s3 + 1, e3, Act::Hang);
        let started = Instant::now();
        assert_eq!(fetch_tuned(&net, URL, &dest, u64::MAX, &|_| {}, &no_cancel, QUICK), Ok(Parted::Joined));
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(fs::read(&dest).unwrap(), data);
        assert_eq!(net.hedges_in(s3, e3).len(), 1);
        assert!((1.5..6.0).contains(&secs), "the primary finished in {secs:.2} s, and the hedge was cancelled (not waited for)");
        assert!(no_piece_left(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_while_a_piece_is_hedged_stops_everything_and_leaves_nothing() {
        let dir = scratch("hedge-cancel");
        let dest = dir.join("big.zip");
        let (s3, e3) = layout(10 * MIB, PIECES)[3];
        // The piece and its hedge both hang; the fetch is cancelled a second in.
        let net = Fake::new(body(10 * MIB as usize)).rule(s3, e3, Act::Hang);
        let tune = Tuning { stall: Duration::from_millis(100), min_age: Duration::from_secs(3600), slow_ratio: 3.0 };
        let began = Instant::now();
        let result = fetch_tuned(&net, URL, &dest, u64::MAX, &|_| {}, &|| began.elapsed() >= Duration::from_secs(1), tune);
        assert_eq!(result, Err(CANCELLED.to_string()));
        assert!(began.elapsed() < Duration::from_secs(5), "the hangs ended with the cancel");
        assert!(!dest.exists());
        assert!(no_piece_left(&dir), "no piece or hedge file is left");
        assert_eq!(net.requests_within(s3, e3), 2, "the piece's request and its hedge both started before the cancel");
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
        if let Ok(rate) = std::env::var("RAZDOR_RANGE_E2E_THROTTLE") {
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
        let started = Instant::now();
        crate::update::download_to(&url, &first, 60, u64::MAX).expect("the download");
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(fs::read(&first).unwrap(), data, "the joined file is the file");
        println!("e2e: download_to {size} bytes in {secs:.2} s ({:.0} KB/s)", size as f64 / secs / 1024.0);

        // 2. The game's path: `download_watched` with the progress, to the end.
        let second = dir.join("second.bin");
        let last = AtomicU64::new(0);
        let started = Instant::now();
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
        let started = Instant::now();
        let result = crate::update::download_watched(&url, &third, 0, 60, &|n| seen.store(n, Ordering::SeqCst), &|| seen.load(Ordering::SeqCst) >= 2 * MIB);
        println!("e2e: cancel after {:.2} s: {result:?}", started.elapsed().as_secs_f64());
        assert!(result.is_err());
        assert!(!third.exists());
        assert!(no_piece_left(&dir));
        drop(server);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The slow tail on the desktop (the measurement of the hedge): 12 MiB from the local server at
    /// `RAZDOR_RANGE_E2E_THROTTLE` bytes a second per connection, and the connection of the last piece
    /// at `RAZDOR_RANGE_E2E_SLOW_TAIL` bytes a second. `RAZDOR_RANGE_E2E_NO_HEDGE=1` turns the hedge
    /// off (the baseline: the same code without hedging). Prints the seconds and the checks.
    /// `RAZDOR_RANGE_E2E_DIR=<empty dir> RAZDOR_RANGE_E2E_THROTTLE=300000 RAZDOR_RANGE_E2E_SLOW_TAIL=40000
    /// cargo test --profile fasttest --lib parts::tests::slow_tail_end_to_end -- --ignored --nocapture`.
    #[test]
    #[ignore = "a manual measurement against the local Python range server"]
    fn slow_tail_end_to_end() {
        let Some(dir) = std::env::var_os("RAZDOR_RANGE_E2E_DIR") else { return };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let size: u64 = 12 * MIB;
        let data = body(size as usize);
        fs::write(dir.join("blob.bin"), &data).unwrap();
        let mut options: Vec<String> = Vec::new();
        if let Ok(rate) = std::env::var("RAZDOR_RANGE_E2E_THROTTLE") {
            options.extend(["--throttle".to_string(), rate]);
        }
        if let Ok(rate) = std::env::var("RAZDOR_RANGE_E2E_SLOW_TAIL") {
            // The last piece starts at 7/8 of the file (the pieces are equal: 12 MiB / 8).
            options.extend(["--slow-start".to_string(), (7 * size / PIECES).to_string(), "--slow-rate".to_string(), rate]);
        }
        HEDGE_OFF.store(std::env::var_os("RAZDOR_RANGE_E2E_NO_HEDGE").is_some(), Ordering::SeqCst);
        let server = Server::start(&dir, &options);
        let url = format!("http://127.0.0.1:{}/blob.bin", server.1);
        let out = dir.join("out.bin");
        let started = Instant::now();
        crate::update::download_to(&url, &out, 120, u64::MAX).expect("the download");
        let secs = started.elapsed().as_secs_f64();
        assert_eq!(fs::read(&out).unwrap(), data, "the joined file is the file");
        println!("e2e: slow tail, hedge {}: {size} bytes in {secs:.2} s", if HEDGE_OFF.load(Ordering::SeqCst) { "off" } else { "on" });
        HEDGE_OFF.store(false, Ordering::SeqCst);
        drop(server);
        let _ = fs::remove_dir_all(&dir);
    }
}
