//! The cloud's requests: any method, an optional account key header, an optional body. Desktop
//! runs the system's `curl` (like `update.rs`, with the window hidden on Windows). Android runs the
//! app's own `HttpURLConnection` through the JNI (`razdorRequest` in `android/MainActivityInject.java`).
//! iOS has no transport yet: its requests fail as offline, so the game stays playable and the sync waits.
//! [`platform_net`] picks the one for the platform the game runs on.

use std::sync::Arc;

/// The header that carries the account key ([`razdor_wire::paths::KEY_HEADER`]).
pub const KEY_HEADER: &str = razdor_wire::paths::KEY_HEADER;

/// The headers of every request to Razdor's own API (cloud saves, telemetry, the server's health):
/// the app version ([`razdor_wire::paths::VERSION_HEADER`]) and the commit it was built from
/// ([`razdor_wire::paths::COMMIT_HEADER`], `unknown` without git). The server filters clients by build.
/// Only the API's transports send them: the downloads and the update feed do not.
pub fn build_headers() -> [(&'static str, String); 2] {
    [
        (razdor_wire::paths::VERSION_HEADER, env!("CARGO_PKG_VERSION").to_string()),
        (razdor_wire::paths::COMMIT_HEADER, option_env!("RAZDOR_GIT").unwrap_or("unknown").to_string()),
    ]
}

/// The longest wait a server's `Retry-After` can ask for, in seconds (a day). A bigger number is cut to
/// it: a huge value would overflow the worker's clock, and no server means a day's silence.
pub const MAX_RETRY_AFTER_SECS: u64 = 24 * 3600;

/// An HTTP answer: its status, its body and the `Retry-After` the server sent, in seconds (only the
/// integer form is read; an HTTP date gives `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
}

/// The `Retry-After` of the last response in a header dump (`curl -D`, which keeps every response's
/// block, redirects included): the seconds, cut to [`MAX_RETRY_AFTER_SECS`], or `None`.
pub fn retry_after_of(dump: &str) -> Option<u64> {
    let mut found = None;
    for line in dump.lines() {
        if line.starts_with("HTTP/") {
            found = None;
        } else if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("retry-after") {
                found = value.trim().parse::<u64>().ok().map(|s| s.min(MAX_RETRY_AFTER_SECS));
            }
        }
    }
    found
}

/// The transport. The sync code takes one, so the tests can run it against the real server or
/// against a stub that never answers.
pub trait Net: Send + Sync {
    /// One request. `Err` is a failure to get an answer (no network, a timeout, no transport).
    fn send(&self, method: &str, url: &str, key: Option<&str>, body: Option<&[u8]>) -> Result<Reply, String>;
}

/// A header line in a file only this user can read (0600 on unix), removed when dropped: how the
/// account key reaches curl without appearing in its arguments.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct HeaderFile(pub(crate) std::path::PathBuf);

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl HeaderFile {
    /// A new private file holding `line` (`""` for an empty one, such as a header dump).
    pub(crate) fn new(line: &str) -> std::io::Result<HeaderFile> {
        use std::io::Write;
        // A counter in the name: the clock alone is not unique (macOS counts microseconds, so two requests in the
        // same microsecond, e.g. two threads, got one name and the second failed with "File exists").
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!("razdor-cloud-{}-{nanos}-{n}.h", std::process::id()));
        let mut file = super::account::open_private(&path)?;
        let file = HeaderFile(path).with(|| file.write_all(line.as_bytes()))?;
        Ok(file)
    }

    /// Runs `write`; on an error the file is removed (by the drop) and the error returned.
    fn with(self, write: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<HeaderFile> {
        write()?;
        Ok(self)
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Drop for HeaderFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The desktop transport: `curl`, which gives up after `seconds`.
pub struct Curl {
    pub seconds: u32,
}

impl Net for Curl {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn send(&self, method: &str, url: &str, key: Option<&str>, body: Option<&[u8]>) -> Result<Reply, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-X", method, "--max-time"])
            .arg(self.seconds.to_string())
            .args(["-H", "Content-Type: application/octet-stream"]);
        for (name, value) in build_headers() {
            cmd.args(["-H", &format!("{name}: {value}")]);
        }
        // The key goes to curl through a private file (`-H @file`, curl 7.55+), never on the command
        // line, where other local users could read it from the process list. Removed when `_header` drops.
        let _header = match key {
            Some(key) => {
                let file = HeaderFile::new(&format!("{KEY_HEADER}: {key}\n")).map_err(|e| format!("curl: key header: {e}"))?;
                cmd.args(["-H", &format!("@{}", file.0.display())]);
                Some(file)
            }
            None => None,
        };
        // The response headers go to a private file (`-D`): the Retry-After is read from it.
        let dump = HeaderFile::new("").map_err(|e| format!("curl: header dump: {e}"))?;
        cmd.arg("-D").arg(&dump.0);
        cmd.args(["-o", "-", "-w", "\n%{http_code}"]);
        if body.is_some() {
            cmd.args(["--data-binary", "@-"]).stdin(Stdio::piped());
        }
        cmd.arg(url).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("curl: {e}"))?;
        if let (Some(body), Some(mut stdin)) = (body, child.stdin.take()) {
            // Written from a thread: a reply may start before the body is read.
            let data = body.to_vec();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
        }
        let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!("curl: exit {}", out.status.code().unwrap_or(-1)));
        }
        let mut reply = split_reply(&out.stdout)?;
        reply.retry_after = std::fs::read(&dump.0).ok().and_then(|head| retry_after_of(&String::from_utf8_lossy(&head)));
        Ok(reply)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn send(&self, _method: &str, _url: &str, _key: Option<&str>, _body: Option<&[u8]>) -> Result<Reply, String> {
        Err("no cloud transport on this platform yet".into())
    }
}

/// Android: the app's own `HttpURLConnection`, called through the JNI (`update::phone_request`). `seconds`
/// is the whole request's cap; a stall gives up after `update::STALL_SECONDS` (as the phones' other calls).
#[cfg(target_os = "android")]
pub struct Phone {
    pub seconds: u32,
}

#[cfg(target_os = "android")]
impl Net for Phone {
    fn send(&self, method: &str, url: &str, key: Option<&str>, body: Option<&[u8]>) -> Result<Reply, String> {
        let [(_, version), (_, commit)] = build_headers();
        let (status, body, retry_after) = crate::update::phone_request(method, url, key, body, self.seconds, &version, &commit)?;
        Ok(Reply { status, body, retry_after })
    }
}

/// iOS: no cloud transport yet, so every request is `Offline` (the sync waits and retries; the game
/// plays on). When iOS gets a request call, it replaces this, as [`Phone`] did for Android.
#[cfg(target_os = "ios")]
pub struct Offline;

#[cfg(target_os = "ios")]
impl Net for Offline {
    fn send(&self, _method: &str, _url: &str, _key: Option<&str>, _body: Option<&[u8]>) -> Result<Reply, String> {
        Err("no cloud transport on iOS yet".into())
    }
}

/// The transport for the platform the game runs on, with `seconds` as its stall limit: [`Curl`] on
/// desktop, [`Phone`] on Android, [`Offline`] on iOS. Whatever starts the cloud worker takes this one
/// (no production code starts it yet; the tests use [`Curl`] directly).
pub fn platform_net(seconds: u32) -> Arc<dyn Net> {
    #[cfg(target_os = "android")]
    {
        Arc::new(Phone { seconds })
    }
    #[cfg(target_os = "ios")]
    {
        let _ = seconds;
        Arc::new(Offline)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Arc::new(Curl { seconds })
    }
}

/// Splits curl's output (the body, then `\n` and the status code) into the reply.
#[cfg_attr(any(target_os = "android", target_os = "ios"), allow(dead_code))]
fn split_reply(out: &[u8]) -> Result<Reply, String> {
    let cut = out.iter().rposition(|&b| b == b'\n').ok_or("curl: no status code")?;
    let status: u16 = std::str::from_utf8(&out[cut + 1..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or("curl: bad status code")?;
    Ok(Reply { status, body: out[..cut].to_vec(), retry_after: None })
}

/// A one-shot local HTTP server for the transports' tests: it answers the first request with 200
/// and `ok`, and the thread returns the request's head (its request line and headers) when done.
#[cfg(test)]
pub(crate) fn one_request_server() -> (u16, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let thread = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept");
        let mut seen = Vec::new();
        let mut buf = [0u8; 4096];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = conn.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&buf[..n]);
        }
        conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").expect("write");
        String::from_utf8_lossy(&seen).into_owned()
    });
    (port, thread)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn header_files_made_at_once_on_many_threads_get_their_own_names() {
        let files: Vec<HeaderFile> = std::thread::scope(|s| {
            let hands: Vec<_> = (0..16).map(|_| s.spawn(|| (0..50).map(|_| HeaderFile::new("x").expect("a new file")).collect::<Vec<_>>())).collect();
            hands.into_iter().flat_map(|h| h.join().expect("joins")).collect()
        });
        let names: std::collections::HashSet<_> = files.iter().map(|f| f.0.clone()).collect();
        assert_eq!(names.len(), files.len(), "every header file has its own name");
    }

    #[test]
    fn the_api_requests_carry_the_version_and_the_commit() {
        let (port, server) = one_request_server();
        let url = format!("http://127.0.0.1:{port}/v1/saves");
        let key = "k".repeat(32);
        let reply = Curl { seconds: 10 }.send("GET", &url, Some(&key), None).expect("curl answers");
        assert_eq!(reply.status, 200);
        let head = server.join().expect("server");
        assert!(head.contains(&format!("{KEY_HEADER}: ")), "the key header: {head}");
        assert!(head.contains(&format!("x-razdor-version: {}", env!("CARGO_PKG_VERSION"))), "the version header: {head}");
        let commit = option_env!("RAZDOR_GIT").unwrap_or("unknown");
        assert!(head.contains(&format!("x-razdor-commit: {commit}")), "the commit header: {head}");
        let names: Vec<&str> = build_headers().iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["x-razdor-version", "x-razdor-commit"]);
    }

    #[test]
    fn retry_after_is_read_from_the_last_response_in_the_dump() {
        assert_eq!(retry_after_of("HTTP/2 429\r\nRetry-After: 120\r\n\r\n"), Some(120));
        // A redirect's block does not count: the last response is the answer.
        assert_eq!(retry_after_of("HTTP/1.1 302 Found\nRetry-After: 5\n\nHTTP/2 200\n"), None);
        assert_eq!(retry_after_of("HTTP/2 503\nretry-after:  7 \n"), Some(7), "name case and spaces");
        // An HTTP date is no number, so no wait.
        assert_eq!(retry_after_of("HTTP/2 503\nRetry-After: Wed, 21 Oct 2026 07:28:00 GMT\n"), None);
        assert_eq!(retry_after_of("HTTP/2 503\n"), None);
        // A number past a day is cut to a day.
        assert_eq!(retry_after_of("HTTP/2 429\nRetry-After: 999999999\n"), Some(MAX_RETRY_AFTER_SECS));
    }

    #[test]
    fn the_status_comes_after_the_body() {
        let r = split_reply(b"{\"error\":\"x\"}\n409").expect("splits");
        assert_eq!(r.status, 409);
        assert_eq!(r.body, b"{\"error\":\"x\"}");
        let empty = split_reply(b"\n200").expect("splits an empty body");
        assert_eq!((empty.status, empty.body.len()), (200, 0));
        assert!(split_reply(b"no newline").is_err());
    }
}
