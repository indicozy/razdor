//! The cloud's requests: any method, an optional account key header, an optional body. Desktop
//! runs the system's `curl` (like `update.rs`, with the window hidden on Windows). Android and iOS
//! have no transport here yet (the phones' `razdorPost` takes POST without headers): their requests
//! fail as offline, so the game stays playable and the sync waits.

/// The header that carries the account key ([`razdor_wire::paths::KEY_HEADER`]).
pub const KEY_HEADER: &str = razdor_wire::paths::KEY_HEADER;

/// An HTTP answer: its status and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// The transport. The sync code takes one, so the tests can run it against the real server or
/// against a stub that never answers.
pub trait Net: Send + Sync {
    /// One request. `Err` is a failure to get an answer (no network, a timeout, no transport).
    fn send(&self, method: &str, url: &str, key: Option<&str>, body: Option<&[u8]>) -> Result<Reply, String>;
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
        if let Some(key) = key {
            cmd.args(["-H", &format!("{KEY_HEADER}: {key}")]);
        }
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
        split_reply(&out.stdout)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn send(&self, _method: &str, _url: &str, _key: Option<&str>, _body: Option<&[u8]>) -> Result<Reply, String> {
        Err("no cloud transport on this platform yet".into())
    }
}

/// Splits curl's output (the body, then `\n` and the status code) into the reply.
fn split_reply(out: &[u8]) -> Result<Reply, String> {
    let cut = out.iter().rposition(|&b| b == b'\n').ok_or("curl: no status code")?;
    let status: u16 = std::str::from_utf8(&out[cut + 1..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or("curl: bad status code")?;
    Ok(Reply { status, body: out[..cut].to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

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
