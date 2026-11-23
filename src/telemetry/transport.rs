//! How the sender posts a request. The live one goes through the app's own HTTP (`update::post`:
//! curl on the desktop, the app's Java connection on Android); the tests inject their own.

/// The server's answer: the status and the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Posts `body` (JSON, or a frame) to `url`. `Err` is a transport failure (no answer at all).
pub trait Transport: Send + Sync {
    fn post(&self, url: &str, body: &[u8], seconds: u32) -> Result<Reply, String>;
}

/// The app's HTTP.
pub struct Live;

impl Transport for Live {
    fn post(&self, url: &str, body: &[u8], seconds: u32) -> Result<Reply, String> {
        let [(_, version), (_, commit)] = crate::cloud::http::build_headers();
        crate::update::post(url, body, seconds, &version, &commit).map(|(status, body)| Reply { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_analytics_posts_carry_the_version_and_the_commit() {
        let (port, server) = crate::cloud::http::one_request_server();
        let reply = Live.post(&format!("http://127.0.0.1:{port}/v1/telemetry"), b"{}", 10).expect("curl answers");
        assert_eq!(reply.status, 200);
        let head = server.join().expect("server");
        assert!(head.starts_with("POST /v1/telemetry"), "the request: {head}");
        assert!(head.contains(&format!("x-razdor-version: {}", env!("CARGO_PKG_VERSION"))), "the version header: {head}");
        let commit = option_env!("RAZDOR_GIT").unwrap_or("unknown");
        assert!(head.contains(&format!("x-razdor-commit: {commit}")), "the commit header: {head}");
    }
}
